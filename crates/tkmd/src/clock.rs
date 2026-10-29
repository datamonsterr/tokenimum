//! Time sources. Production uses the wall clock; tests drive a [`ManualClock`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub use tkm_decide::chain::{Clock, SystemClock};

/// A clock that only moves when told to.
#[derive(Debug)]
pub struct ManualClock {
    ms: AtomicU64,
}

impl ManualClock {
    /// Creates a clock at `ms` Unix milliseconds.
    pub fn new(ms: u64) -> Self {
        Self {
            ms: AtomicU64::new(ms),
        }
    }

    /// Moves the clock forward.
    pub fn advance(&self, by: Duration) {
        let step = u64::try_from(by.as_millis()).unwrap_or(u64::MAX);
        self.ms.fetch_add(step, Ordering::SeqCst);
    }

    /// Sets the absolute time.
    pub fn set(&self, ms: u64) {
        self.ms.store(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.ms.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_advances_only_on_request() {
        let clock = ManualClock::new(10);
        assert_eq!(clock.now_ms(), 10);
        clock.advance(Duration::from_secs(2));
        assert_eq!(clock.now_ms(), 2_010);
        clock.set(5);
        assert_eq!(clock.now_ms(), 5);
    }
}
