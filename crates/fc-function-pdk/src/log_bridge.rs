//! The `log` crate facade, bridged to the function's logger: `log::info!`
//! in a handler (or in any crate it uses) is a line on `fn.<address>`.

use crate::context::Level;

struct Bridge;

static BRIDGE: Bridge = Bridge;

impl log::Log for Bridge {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true // the host's own logger configuration decides what is kept
    }

    fn log(&self, record: &log::Record<'_>) {
        let level = match record.level() {
            log::Level::Trace => Level::Trace,
            log::Level::Debug => Level::Debug,
            log::Level::Info => Level::Info,
            log::Level::Warn => Level::Warn,
            log::Level::Error => Level::Error,
        };
        crate::wasi::log_line(level, &record.args().to_string());
    }

    fn flush(&self) {}
}

/// Installs the bridge (once per instance, which is once per request). A
/// function that wants fewer lines lowers `log::set_max_level` itself.
pub(crate) fn install() {
    if log::set_logger(&BRIDGE).is_ok() {
        log::set_max_level(log::LevelFilter::Trace);
    }
}
