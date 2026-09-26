//! `fc-server` in its function-host role (`FC_FUNCTION_HOST_ENABLED`).
//!
//! Host only (every other role off), it is the former `fc-fnhost` daemon
//! (Java `FnHostMainTest`, P8): exit 2 with one line naming every bad
//! variable; `FC_EXIT_AFTER_START` exits 0 after a real start; SIGTERM
//! drains (a `DRAINING` heartbeat) and exits 0.
//!
//! Beside another role (here MCP, so no database is needed), the host runs
//! in the process on its own ports and drains first at shutdown.
//!
//! The fake platform is fc-fnhost-core's test support.

#[path = "../../../crates/fc-fnhost-core/tests/support/fake_platform.rs"]
mod support;

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::json;

/// `fc-server` with only the function host enabled (the platform is on by
/// default, so it is turned off).
fn host() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fc-server"));
    command
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("FC_PLATFORM_ENABLED", "false")
        .env("FC_FUNCTION_HOST_ENABLED", "true");
    command
}

#[test]
fn a_bad_environment_exits_2_naming_every_variable_on_one_line() {
    let output = host()
        .env("FC_FN_POOL", "Not_A_Label")
        .env("FC_FN_PUBLIC_PORT", "nope")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 1, "{stderr}");
    for name in [
        "FC_FN_POOL",
        "FC_FN_PLATFORM_URL",
        "FC_FN_CLIENT_ID",
        "FC_FN_CLIENT_SECRET",
        "FC_FN_PUBLIC_PORT",
    ] {
        assert!(lines[0].contains(name), "{name} missing: {stderr}");
    }
}

#[test]
fn signatures_off_outside_dev_mode_exits_2() {
    let output = host()
        .env("FC_FN_PLATFORM_URL", "http://127.0.0.1:1")
        .env("FC_FN_CLIENT_ID", "id")
        .env("FC_FN_CLIENT_SECRET", "secret")
        .env("FC_FN_SIGNATURES", "off")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("FLOWCATALYST_DEV_MODE"));
}

fn base_env(command: &mut Command, url: &str, cache: &std::path::Path) {
    command
        .env("FC_FN_PLATFORM_URL", url)
        .env("FC_FN_CLIENT_ID", "id")
        .env("FC_FN_CLIENT_SECRET", "secret")
        .env("FC_FN_HOST_ID", "proc-host")
        .env("FC_FN_SIGNATURES", "off")
        .env("FLOWCATALYST_DEV_MODE", "true")
        .env("FC_METRICS_PORT", "0")
        .env("FC_FN_PORT", "0")
        .env("FC_FN_PUBLIC_PORT", "0")
        .env("FC_FN_CACHE_DIR", cache)
        .env("FC_LOG_FORMAT", "json");
}

#[tokio::test]
async fn exit_after_start_runs_a_real_reconcile_then_exits_0() {
    let (platform, url) = support::start().await;
    // A real component (fc-fnhost-core's committed test guest) for `wasm`,
    // and a `jvm` entry, which this host has no runtime for.
    let component = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/fc-fnhost-core/tests/fixtures/wasm/pure.wasm"
    ))
    .unwrap();
    let mut wasm = support::entry(
        "app.svc.fn",
        1,
        "wasm",
        "warm",
        &format!(
            "platform://fnc_1/{}",
            &support::sha256_digest(&component)[7..]
        ),
        &component,
    );
    wasm["manifest"]["entrypoint"] = json!("wasi:http/incoming-handler");
    platform
        .artifacts
        .lock()
        .insert(support::version_id("app.svc.fn", 1), component);
    platform.set_document(json!({"functions": [
        wasm,
        support::entry("app.svc.jvm", 1, "jvm", "warm", "platform://fnc_2/00", b"x")
    ]}));
    let cache = tempfile::tempdir().unwrap();
    let mut command = host();
    base_env(&mut command, &url, cache.path());
    command.env("FC_EXIT_AFTER_START", "true");
    let status = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    assert_eq!(
        status.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let beats = platform.heartbeats.lock().clone();
    let state_of = |address: &str| {
        beats[0]["loaded"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["address"] == address)
            .cloned()
            .unwrap_or_else(|| panic!("{address} missing from {beats:?}"))
    };
    assert_eq!(state_of("app.svc.fn")["state"], "LOADED", "{beats:?}");
    assert_eq!(
        state_of("app.svc.jvm")["error"],
        "RUNTIME_UNSUPPORTED",
        "{beats:?}"
    );
    assert_eq!(beats.last().unwrap()["state"], "DRAINING");
    let stderr = String::from_utf8(status.stderr).unwrap();
    let started = stderr
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|l| l["msg"] == "function host started")
        .expect("a started line");
    assert_eq!(started["host_id"], "proc-host");
    assert!(started["port"].as_u64().is_some_and(|p| p > 0), "{started}");
    assert!(
        started["public_port"].as_u64().is_some_and(|p| p > 0),
        "{started}"
    );
    assert_eq!(started["level"], "INFO");
    assert_eq!(started["logger"], "fc_fnhost_core::host");
}

#[cfg(unix)]
#[tokio::test]
async fn sigterm_drains_and_exits_0() {
    let (platform, url) = support::start().await;
    let cache = tempfile::tempdir().unwrap();
    let mut command = host();
    base_env(&mut command, &url, cache.path());
    let mut child = command.stderr(Stdio::piped()).spawn().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (port, function_port) = tokio::task::spawn_blocking(move || {
        for line in BufReader::new(stderr).lines() {
            let line: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
            if line["msg"] == "function host started" {
                return (
                    line["metrics_port"].as_u64().unwrap(),
                    line["port"].as_u64().unwrap(),
                );
            }
        }
        panic!("no started line");
    })
    .await
    .unwrap();
    let ready = reqwest::get(format!("http://127.0.0.1:{port}/ready"))
        .await
        .unwrap();
    assert_eq!(ready.status(), 200);
    // the function listener answers with Java's contract
    let unknown = reqwest::get(format!(
        "http://127.0.0.1:{function_port}/functions/app.svc.nope/x"
    ))
    .await
    .unwrap();
    assert_eq!(unknown.status(), 404);
    assert_eq!(
        unknown.text().await.unwrap(),
        r#"{"error":"FUNCTION_NOT_FOUND","message":"no such function"}"#
    );
    Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    let status = tokio::task::spawn_blocking(move || child.wait().unwrap())
        .await
        .unwrap();
    assert_eq!(status.code(), Some(0));
    support::wait_for("a DRAINING heartbeat", || {
        platform
            .last_heartbeat()
            .is_some_and(|b| b["state"] == "DRAINING")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(10)).await;
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Beside another role (MCP: no database needed), the host runs in the
/// process on ports of its own, `/ready` reports the role, and at SIGTERM
/// the host drains (a `DRAINING` heartbeat) before the process exits 0.
#[cfg(unix)]
#[tokio::test]
async fn beside_another_role_the_host_runs_on_its_own_ports_and_drains_first() {
    let (platform, url) = support::start().await;
    let cache = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let (api, metrics, mcp) = (free_port(), free_port(), free_port());
    let mut command = host();
    base_env(&mut command, &url, cache.path());
    command
        .env("HOME", home.path())
        .env("FC_MCP_ENABLED", "true")
        .env("FC_MCP_PORT", mcp.to_string())
        .env("FLOWCATALYST_URL", "http://127.0.0.1:9")
        .env("FLOWCATALYST_CLIENT_ID", "mcp-client")
        .env("FLOWCATALYST_CLIENT_SECRET", "mcp-secret")
        .env("FC_API_PORT", api.to_string())
        // fc-server's metrics listener; the host's is FC_FN_METRICS_PORT.
        .env("FC_METRICS_PORT", metrics.to_string())
        .env("FC_FN_METRICS_PORT", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn().unwrap();

    support::wait_for("the host's first heartbeat", || {
        platform.last_heartbeat().is_some()
    })
    .await;
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let ready: serde_json::Value = http
        .get(format!("http://127.0.0.1:{metrics}/ready"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ready["function_host"], true, "{ready}");
    assert_eq!(ready["mcp"], true, "{ready}");
    assert_eq!(platform.last_heartbeat().unwrap()["hostId"], "proc-host");

    Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    let status = tokio::task::spawn_blocking(move || child.wait().unwrap())
        .await
        .unwrap();
    assert_eq!(status.code(), Some(0));
    assert_eq!(platform.last_heartbeat().unwrap()["state"], "DRAINING");
}
