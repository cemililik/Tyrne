//! Deterministic fake [`umbrix_hal::Timer`] for host-side tests.

use std::sync::Mutex;
use umbrix_hal::Timer;

/// A [`Timer`] whose clock is set manually and whose armed deadline is
/// visible to tests.
///
/// The fake does **not** implement deadline expiry — advancing the clock
/// past a deadline does not fire any IRQ, because the test environment has
/// no interrupt controller. Tests that need to observe "did the code arm
/// the right deadline?" assert on [`FakeTimer::armed_deadline`] directly.
pub struct FakeTimer {
    resolution_ns: u64,
    state: Mutex<FakeTimerState>,
}

struct FakeTimerState {
    now_ns: u64,
    armed_deadline: Option<u64>,
    cancel_count: u64,
}

impl FakeTimer {
    /// Construct a `FakeTimer` with `now = 0` and the given resolution.
    #[must_use]
    pub fn new(resolution_ns: u64) -> Self {
        Self {
            resolution_ns,
            state: Mutex::new(FakeTimerState {
                now_ns: 0,
                armed_deadline: None,
                cancel_count: 0,
            }),
        }
    }

    /// Advance the fake's clock by `delta_ns`.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned.
    pub fn advance(&self, delta_ns: u64) {
        let mut state = self.locked();
        state.now_ns = state.now_ns.saturating_add(delta_ns);
    }

    /// Set the fake's clock to the given absolute value.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned.
    pub fn set_now(&self, ns: u64) {
        self.locked().now_ns = ns;
    }

    /// Return the currently-armed deadline, if any.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned.
    #[must_use]
    pub fn armed_deadline(&self) -> Option<u64> {
        self.locked().armed_deadline
    }

    /// Return the number of times [`Timer::cancel_deadline`] has been
    /// called on this fake.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned.
    #[must_use]
    pub fn cancel_count(&self) -> u64 {
        self.locked().cancel_count
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, FakeTimerState> {
        self.state.lock().expect("FakeTimer mutex poisoned")
    }
}

impl Default for FakeTimer {
    /// Construct a `FakeTimer` with 1 ns resolution and `now = 0`.
    fn default() -> Self {
        Self::new(1)
    }
}

impl Timer for FakeTimer {
    fn now_ns(&self) -> u64 {
        self.locked().now_ns
    }

    fn arm_deadline(&self, deadline_ns: u64) {
        self.locked().armed_deadline = Some(deadline_ns);
    }

    fn cancel_deadline(&self) {
        let mut state = self.locked();
        state.armed_deadline = None;
        state.cancel_count += 1;
    }

    fn resolution_ns(&self) -> u64 {
        self.resolution_ns
    }
}

#[cfg(test)]
mod tests {
    use super::FakeTimer;
    use umbrix_hal::Timer;

    #[test]
    fn new_starts_at_zero_with_given_resolution() {
        let t = FakeTimer::new(100);
        assert_eq!(t.now_ns(), 0);
        assert_eq!(t.resolution_ns(), 100);
    }

    #[test]
    fn advance_moves_clock_forward() {
        let t = FakeTimer::new(1);
        t.advance(500);
        assert_eq!(t.now_ns(), 500);
        t.advance(250);
        assert_eq!(t.now_ns(), 750);
    }

    #[test]
    fn set_now_overrides_clock() {
        let t = FakeTimer::new(1);
        t.advance(100);
        t.set_now(42);
        assert_eq!(t.now_ns(), 42);
    }

    #[test]
    fn arm_deadline_records_value() {
        let t = FakeTimer::new(1);
        assert_eq!(t.armed_deadline(), None);
        t.arm_deadline(1_000);
        assert_eq!(t.armed_deadline(), Some(1_000));
    }

    #[test]
    fn arm_deadline_replaces_previous() {
        let t = FakeTimer::new(1);
        t.arm_deadline(1_000);
        t.arm_deadline(2_000);
        assert_eq!(t.armed_deadline(), Some(2_000));
    }

    #[test]
    fn cancel_clears_deadline_and_counts() {
        let t = FakeTimer::new(1);
        t.arm_deadline(500);
        assert_eq!(t.cancel_count(), 0);
        t.cancel_deadline();
        assert_eq!(t.armed_deadline(), None);
        assert_eq!(t.cancel_count(), 1);
        // Cancelling when nothing is armed is a no-op for the deadline
        // but still increments the count.
        t.cancel_deadline();
        assert_eq!(t.cancel_count(), 2);
    }

    #[test]
    fn default_has_one_nanosecond_resolution() {
        let t = FakeTimer::default();
        assert_eq!(t.resolution_ns(), 1);
        assert_eq!(t.now_ns(), 0);
    }
}
