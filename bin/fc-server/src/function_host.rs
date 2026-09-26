//! The function-host role (`FC_FUNCTION_HOST_ENABLED`): the WASM function
//! host (`crates/fc-fnhost-core`, a drop-in for Java's `fc-fnhost`) that
//! polls the platform's `/control/functions/*` for its pool's desired
//! state, loads the functions and serves them. It reads its own `FC_FN_*`
//! environment (`fc_fnhost_core::env::HostEnv`): `FC_FN_PLATFORM_URL`,
//! `FC_FN_CLIENT_ID` and `FC_FN_CLIENT_SECRET` are required, `FC_FN_POOL`,
//! `FC_FN_SIGNATURES`, `FC_FN_TRUST_ROOT`, `FC_FN_CACHE_DIR`,
//! `FC_FN_MAX_*`, `FC_FN_TRUSTED_PROXIES`, `FC_DRAIN_TIMEOUT_SECONDS` … as
//! the former `fc-fnhost` binary did.
//!
//! Two shapes:
//!
//! - **Host only** (this flag on, every other role off, the platform
//!   included): `fc-server` *is* the former `fc-fnhost` daemon — the
//!   process environment read as-is, the function listeners on `FC_FN_PORT`
//!   (8080) and `FC_FN_PUBLIC_PORT` (8081), `/health`, `/ready` and
//!   `/metrics` on `FC_METRICS_PORT` (9090), `FC_EXIT_AFTER_START`, exit 2
//!   naming every bad variable. No database, and none of `fc-server`'s own
//!   listeners: the density shape, a node that only hosts functions.
//! - **Beside other roles**: the host runs in this process on ports of its
//!   own, since `fc-server`'s listeners hold 8080 and 9090: `FC_FN_PORT`
//!   (default **8090**), `FC_FN_PUBLIC_PORT` (**8091**) and
//!   `FC_FN_METRICS_PORT` (**9091**) for its observability listener — the
//!   defaults `fc-dev` uses. A port another listener of this process holds
//!   refuses the boot. The host starts once the API listener is bound (its
//!   first reconcile may call this very process) and stops first at
//!   shutdown, so its `DRAINING` heartbeat still reaches the platform.

use anyhow::{anyhow, Result};
use fc_fnhost_core::env::{EnvReader, HostEnv, PublicPort};
use fc_fnhost_core::host::{function_listener, wasm_loaders, FnHost};
use tracing::info;

/// The ports a host beside other roles defaults to (`fc-dev`'s).
const SHARED_FN_PORT: &str = "8090";
const SHARED_FN_PUBLIC_PORT: &str = "8091";
const SHARED_FN_METRICS_PORT: &str = "9091";

/// Host only: the former `fc-fnhost` daemon on the process environment.
/// Returns the exit code.
pub async fn run_host_only() -> i32 {
    let mut stderr = std::io::stderr();
    fc_fnhost_core::host::run_wasm_host(
        EnvReader::system(),
        &mut stderr,
        fc_fnhost_core::host::shutdown_signal(),
    )
    .await
}

/// The host's environment beside other roles: the process environment with
/// the host's own port defaults, and its observability listener on
/// `FC_FN_METRICS_PORT` (the process's `FC_METRICS_PORT` is fc-server's).
fn shared_env_pairs(process: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = process
        .into_iter()
        .filter(|(k, _)| k != "FC_METRICS_PORT")
        .collect();
    let get = |pairs: &[(String, String)], key: &str| {
        pairs
            .iter()
            .find(|(k, v)| k == key && !v.trim().is_empty())
            .map(|(_, v)| v.clone())
    };
    let metrics =
        get(&pairs, "FC_FN_METRICS_PORT").unwrap_or_else(|| SHARED_FN_METRICS_PORT.into());
    for (key, default) in [
        ("FC_FN_PORT", SHARED_FN_PORT),
        ("FC_FN_PUBLIC_PORT", SHARED_FN_PUBLIC_PORT),
    ] {
        if get(&pairs, key).is_none() {
            pairs.retain(|(k, _)| k != key);
            pairs.push((key.to_string(), default.to_string()));
        }
    }
    pairs.push(("FC_METRICS_PORT".to_string(), metrics));
    pairs
}

/// The host beside other roles, configured and not yet started.
pub struct SharedHost {
    env: HostEnv,
}

impl SharedHost {
    /// Reads and checks the host's environment (every bad variable named in
    /// one error), and refuses a port that `taken` (another listener of
    /// this process: `(variable, port)`) already holds.
    pub fn from_env(taken: &[(&str, u16)]) -> Result<Self> {
        Self::from_process_env(std::env::vars(), taken)
    }

    fn from_process_env(
        process: impl IntoIterator<Item = (String, String)>,
        taken: &[(&str, u16)],
    ) -> Result<Self> {
        let pairs = shared_env_pairs(process);
        let env = HostEnv::load(&EnvReader::from_pairs(pairs))
            .map_err(|e| anyhow!("function host: {e}"))?;
        let mut mine = vec![
            ("FC_FN_PORT", env.port),
            ("FC_FN_METRICS_PORT", env.metrics_port),
        ];
        if let PublicPort::Port(port) = env.public_port {
            mine.push(("FC_FN_PUBLIC_PORT", port));
        }
        let all: Vec<(&str, u16)> = mine.iter().chain(taken.iter()).copied().collect();
        for (i, (name, port)) in all.iter().enumerate() {
            if *port == 0 {
                continue;
            }
            if let Some((other, _)) = all[i + 1..].iter().find(|(_, p)| p == port) {
                return Err(anyhow!(
                    "function host: {name} and {other} are both port {port}; give the function \
                     host its own ports (FC_FN_PORT, FC_FN_PUBLIC_PORT, FC_FN_METRICS_PORT)"
                ));
            }
        }
        Ok(Self { env })
    }

    /// Starts the host: the WASM runtime, the first reconcile, the
    /// listeners. A host that cannot start is an error.
    pub async fn start(self) -> Result<FnHost> {
        let loaders = wasm_loaders(&self.env)
            .map_err(|e| anyhow!("cannot start the function runtime: {e}"))?;
        let listener = function_listener(&self.env);
        let mut host = FnHost::new(self.env, loaders, Some(listener))
            .map_err(|e| anyhow!("cannot create the function cache directory: {e}"))?;
        if let Err(e) = host.start().await {
            host.close().await;
            return Err(anyhow!("the function host did not start: {e}"));
        }
        info!(
            pool = %host.env().pool,
            host_id = %host.env().host_id,
            port = host.port(),
            public_port = host.public_port(),
            metrics_port = host.metrics_port(),
            "function host started"
        );
        Ok(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(extra: &[(&str, &str)]) -> Vec<(String, String)> {
        [
            ("FC_FN_PLATFORM_URL", "http://platform"),
            ("FC_FN_CLIENT_ID", "id"),
            ("FC_FN_CLIENT_SECRET", "secret"),
            ("FC_METRICS_PORT", "9090"),
        ]
        .iter()
        .chain(extra.iter())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    fn load(extra: &[(&str, &str)]) -> HostEnv {
        HostEnv::load(&EnvReader::from_pairs(shared_env_pairs(pairs(extra)))).unwrap()
    }

    #[test]
    fn beside_other_roles_the_host_defaults_to_its_own_ports() {
        let env = load(&[]);
        assert_eq!(env.port, 8090);
        assert_eq!(env.public_port, PublicPort::Port(8091));
        // fc-server's FC_METRICS_PORT is not the host's.
        assert_eq!(env.metrics_port, 9091);
    }

    #[test]
    fn a_port_another_listener_holds_refuses_the_boot() {
        let err = SharedHost::from_process_env(pairs(&[]), &[("FC_MCP_PORT", 8090)])
            .err()
            .expect("refused")
            .to_string();
        assert!(
            err.contains("FC_FN_PORT and FC_MCP_PORT are both port 8090"),
            "{err}"
        );
        assert!(SharedHost::from_process_env(pairs(&[]), &[("FC_API_PORT", 8080)]).is_ok());
    }

    #[test]
    fn a_bad_host_environment_names_the_variables() {
        let err = SharedHost::from_process_env(Vec::<(String, String)>::new(), &[])
            .err()
            .expect("refused")
            .to_string();
        assert!(err.contains("FC_FN_PLATFORM_URL"), "{err}");
        assert!(err.contains("FC_FN_CLIENT_SECRET"), "{err}");
    }

    #[test]
    fn the_hosts_ports_are_configurable() {
        let env = load(&[
            ("FC_FN_PORT", "7000"),
            ("FC_FN_PUBLIC_PORT", "off"),
            ("FC_FN_METRICS_PORT", "7001"),
        ]);
        assert_eq!(env.port, 7000);
        assert_eq!(env.public_port, PublicPort::Disabled);
        assert_eq!(env.metrics_port, 7001);
    }
}
