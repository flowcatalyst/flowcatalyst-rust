//! `fc-fnhost`: the FlowCatalyst function host daemon, a drop-in for Java's
//! `FnHostMain`. No sub-commands. Exit codes: 2 on a start-up environment
//! error (one line to stderr naming every missing or invalid variable), 0
//! after `FC_EXIT_AFTER_START` or a clean shutdown (SIGTERM / Ctrl-C), 1 if
//! the host cannot start.

use fc_fnhost_core::env::EnvReader;
use fc_fnhost_core::host::{run, shutdown_signal};
use fc_fnhost_core::loader::Loaders;

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("fn-host")
        .build()
        .expect("the tokio runtime builds");
    // No runtimes are registered yet: the WASM engine lands in H4, and a
    // `jvm` entry is reported RUNTIME_UNSUPPORTED (it stays on JVM hosts).
    let code = runtime.block_on(async {
        let mut stderr = std::io::stderr();
        run(
            EnvReader::system(),
            &mut stderr,
            Loaders::none(),
            None,
            shutdown_signal(),
        )
        .await
    });
    runtime.shutdown_timeout(std::time::Duration::from_secs(5));
    std::process::exit(code);
}
