//! `fc-server` in its MCP role (Go's `FC_MCP_ENABLED`): the read-only MCP
//! server on its own listener, with no platform and no database, and a
//! refusal to start without credentials. No Docker needed.

use std::env;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tokio::time;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A scratch HOME, so fc-dev's MCP credentials file on this machine (if
/// any) is never read.
fn scratch_home(name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("fc-server-mcp-{name}-{}", process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn fc_server(home: &PathBuf, env: &[(&str, String)]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fc-server"));
    command
        .env_clear()
        .env("PATH", env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("FC_PLATFORM_ENABLED", "false")
        .env("FC_MCP_ENABLED", "true")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (k, v) in env {
        command.env(k, v);
    }
    command
}

struct Killed(Child);

impl Drop for Killed {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_role_serves_with_no_platform_and_no_database() {
    let (api, metrics, mcp) = (free_port(), free_port(), free_port());
    let home = scratch_home("serves");
    let _server = Killed(
        fc_server(
            &home,
            &[
                ("FC_API_PORT", api.to_string()),
                ("FC_METRICS_PORT", metrics.to_string()),
                ("FC_MCP_PORT", mcp.to_string()),
                ("FLOWCATALYST_URL", "http://127.0.0.1:9".to_string()),
                ("FLOWCATALYST_CLIENT_ID", "mcp-client".to_string()),
                ("FLOWCATALYST_CLIENT_SECRET", "mcp-secret".to_string()),
            ],
        )
        .spawn()
        .unwrap(),
    );

    let health = format!("http://127.0.0.1:{mcp}/health");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(r) = client().get(&health).send().await {
            if r.status() == 200 {
                break;
            }
        }
        assert!(Instant::now() < deadline, "the MCP listener never answered");
        time::sleep(Duration::from_millis(100)).await;
    }

    // A client's first call: initialize over the streamable HTTP transport.
    let init = client()
        .post(format!("http://127.0.0.1:{mcp}/mcp"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {"name": "fc-server-test", "version": "0"}
                }
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(init.status(), 200);
    assert!(
        init.headers().contains_key("mcp-session-id"),
        "a session is opened"
    );

    // The metrics listener reports the role.
    let ready: serde_json::Value = client()
        .get(format!("http://127.0.0.1:{metrics}/ready"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ready["mcp"], true);
    assert_eq!(ready["platform"], false);
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn mcp_role_refuses_to_start_without_credentials() {
    let home = scratch_home("refuses");
    let output = fc_server(
        &home,
        &[
            ("FC_API_PORT", free_port().to_string()),
            ("FC_METRICS_PORT", free_port().to_string()),
            ("FC_MCP_PORT", free_port().to_string()),
        ],
    )
    .output()
    .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("FLOWCATALYST_CLIENT_ID"), "{stderr}");
    let _ = fs::remove_dir_all(&home);
}
