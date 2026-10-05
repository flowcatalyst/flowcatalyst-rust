//! The function-host role, compiled out: this binary was built without the
//! `function-host` feature (`--no-default-features`), so it carries no
//! wasmtime and no V8. `FC_FUNCTION_HOST_ENABLED=true` refuses to start,
//! naming the feature, rather than running without the role it asked for.

use anyhow::{anyhow, Result};

const COMPILED_OUT: &str = "FC_FUNCTION_HOST_ENABLED=true, but this fc-server was built without \
     the `function-host` feature (cargo build --no-default-features); rebuild with the default \
     features to host functions";

/// No host can be configured in this build, so none is ever started.
pub enum SharedHost {}

/// A running host; none exists in this build.
pub enum RunningHost {}

impl SharedHost {
    /// Always refuses: the role is not in this build.
    pub fn from_env(_taken: &[(&str, u16)]) -> Result<Self> {
        Err(anyhow!(COMPILED_OUT))
    }

    pub async fn start(self) -> Result<RunningHost> {
        match self {}
    }
}

impl RunningHost {
    pub async fn close(&mut self) {
        match *self {}
    }
}

/// Host only: refuses with exit code 2, the host's code for bad configuration.
pub async fn run_host_only() -> i32 {
    eprintln!("{COMPILED_OUT}");
    2
}
