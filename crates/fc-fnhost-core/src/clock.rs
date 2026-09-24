//! Wall-clock time as a parameter, so token expiry and idle unloading are
//! testable without sleeping.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;

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

/// A clock a test moves by hand.
#[derive(Debug, Clone)]
pub struct ManualClock(Arc<Mutex<DateTime<Utc>>>);

impl ManualClock {
    pub fn new(start: DateTime<Utc>) -> Self {
        Self(Arc::new(Mutex::new(start)))
    }

    pub fn set(&self, time: DateTime<Utc>) {
        *self.0.lock() = time;
    }

    pub fn advance(&self, by: chrono::Duration) {
        let mut now = self.0.lock();
        *now += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock()
    }
}

pub type SharedClock = Arc<dyn Clock>;
