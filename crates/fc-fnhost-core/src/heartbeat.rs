//! The body of `POST /control/functions/heartbeat` (Java
//! `fnhost/reconcile/HeartbeatReport.java`; wire shape from
//! `HttpControlPlane.toWire`): `{hostId, pool, state, loaded: [{address,
//! version, state, error?}]}`, keys in that order, `error` only on `FAILED`.

use fc_function_abi::FunctionAddress;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostState {
    Active,
    Draining,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadState {
    Registered,
    Loaded,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedEntry {
    pub address: FunctionAddress,
    pub version: i32,
    pub state: LoadState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatReport {
    pub host_id: String,
    pub pool: String,
    pub state: HostState,
    pub loaded: Vec<LoadedEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireReport<'a> {
    host_id: &'a str,
    pool: &'a str,
    state: &'static str,
    loaded: Vec<WireEntry<'a>>,
}

#[derive(Serialize)]
struct WireEntry<'a> {
    address: String,
    version: i32,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

impl HeartbeatReport {
    pub fn to_json(&self) -> String {
        let wire = WireReport {
            host_id: &self.host_id,
            pool: &self.pool,
            state: match self.state {
                HostState::Active => "ACTIVE",
                HostState::Draining => "DRAINING",
            },
            loaded: self
                .loaded
                .iter()
                .map(|entry| WireEntry {
                    address: entry.address.render(),
                    version: entry.version,
                    state: match entry.state {
                        LoadState::Registered => "REGISTERED",
                        LoadState::Loaded => "LOADED",
                        LoadState::Failed(_) => "FAILED",
                    },
                    error: match &entry.state {
                        LoadState::Failed(error) => Some(error),
                        _ => None,
                    },
                })
                .collect(),
        };
        serde_json::to_string(&wire).expect("a heartbeat always serialises")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_shape_matches_java() {
        let report = HeartbeatReport {
            host_id: "h-1".into(),
            pool: "default".into(),
            state: HostState::Draining,
            loaded: vec![
                LoadedEntry {
                    address: FunctionAddress::parse("a.b.c").unwrap(),
                    version: 2,
                    state: LoadState::Loaded,
                },
                LoadedEntry {
                    address: FunctionAddress::parse("a.b.d").unwrap(),
                    version: 1,
                    state: LoadState::Failed("RUNTIME_UNSUPPORTED".into()),
                },
            ],
        };
        assert_eq!(
            report.to_json(),
            r#"{"hostId":"h-1","pool":"default","state":"DRAINING","loaded":[{"address":"a.b.c","version":2,"state":"LOADED"},{"address":"a.b.d","version":1,"state":"FAILED","error":"RUNTIME_UNSUPPORTED"}]}"#
        );
    }
}
