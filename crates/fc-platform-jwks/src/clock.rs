//! Wall-clock time as a parameter, so key refetch floors and token expiry
//! are testable without sleeping.

use std::sync::Arc;

use chrono::{DateTime, Utc};

pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Any `Fn() -> DateTime<Utc>` is a clock: a caller with a clock abstraction
/// of its own adapts it with a closure.
impl<F> Clock for F
where
    F: Fn() -> DateTime<Utc> + Send + Sync + 'static,
{
    fn now(&self) -> DateTime<Utc> {
        self()
    }
}

pub type SharedClock = Arc<dyn Clock>;

/// The system clock, shared.
pub fn system() -> SharedClock {
    Arc::new(SystemClock)
}
