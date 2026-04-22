//! Deterministic fake [`tyrne_hal::IrqController`] for host-side tests.

use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;
use tyrne_hal::{IrqController, IrqNumber};

/// A [`IrqController`] whose enable set, pending queue, and EOI history
/// are visible to tests.
///
/// Tests populate the pending queue with [`Self::inject`] and then call
/// code under test that uses [`IrqController::acknowledge`] /
/// [`IrqController::end_of_interrupt`]. The fake records each call so
/// tests can assert on the sequence.
pub struct FakeIrqController {
    state: Mutex<FakeIrqState>,
}

struct FakeIrqState {
    enabled: HashSet<IrqNumber>,
    pending: VecDeque<IrqNumber>,
    eoi_history: Vec<IrqNumber>,
}

impl FakeIrqController {
    /// Construct a new `FakeIrqController` with no lines enabled and an
    /// empty pending queue.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(FakeIrqState {
                enabled: HashSet::new(),
                pending: VecDeque::new(),
                eoi_history: Vec::new(),
            }),
        }
    }

    /// Inject a pending interrupt.
    ///
    /// The fake does not check that `irq` is enabled — tests that want
    /// to verify enable-gating can compose `is_enabled` with this.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned.
    pub fn inject(&self, irq: IrqNumber) {
        self.locked().pending.push_back(irq);
    }

    /// Return whether the given line is currently enabled.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned.
    #[must_use]
    pub fn is_enabled(&self, irq: IrqNumber) -> bool {
        self.locked().enabled.contains(&irq)
    }

    /// Return the number of pending interrupts that have not yet been
    /// acknowledged.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.locked().pending.len()
    }

    /// Return a snapshot of every interrupt that has been
    /// end-of-interrupted, in the order `end_of_interrupt` was called.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned.
    #[must_use]
    pub fn eoi_history(&self) -> Vec<IrqNumber> {
        self.locked().eoi_history.clone()
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, FakeIrqState> {
        self.state.lock().expect("FakeIrqController mutex poisoned")
    }
}

impl Default for FakeIrqController {
    fn default() -> Self {
        Self::new()
    }
}

impl IrqController for FakeIrqController {
    fn enable(&self, irq: IrqNumber) {
        self.locked().enabled.insert(irq);
    }

    fn disable(&self, irq: IrqNumber) {
        self.locked().enabled.remove(&irq);
    }

    fn acknowledge(&self) -> Option<IrqNumber> {
        self.locked().pending.pop_front()
    }

    fn end_of_interrupt(&self, irq: IrqNumber) {
        self.locked().eoi_history.push(irq);
    }
}

#[cfg(test)]
mod tests {
    use super::FakeIrqController;
    use tyrne_hal::{IrqController, IrqNumber};

    #[test]
    fn enable_marks_line_as_enabled() {
        let ic = FakeIrqController::new();
        assert!(!ic.is_enabled(IrqNumber(30)));
        ic.enable(IrqNumber(30));
        assert!(ic.is_enabled(IrqNumber(30)));
    }

    #[test]
    fn enable_is_idempotent() {
        let ic = FakeIrqController::new();
        ic.enable(IrqNumber(30));
        ic.enable(IrqNumber(30));
        assert!(ic.is_enabled(IrqNumber(30)));
    }

    #[test]
    fn disable_removes_enabled_state() {
        let ic = FakeIrqController::new();
        ic.enable(IrqNumber(30));
        ic.disable(IrqNumber(30));
        assert!(!ic.is_enabled(IrqNumber(30)));
    }

    #[test]
    fn acknowledge_returns_none_when_queue_empty() {
        let ic = FakeIrqController::new();
        assert_eq!(ic.acknowledge(), None);
    }

    #[test]
    fn acknowledge_returns_pending_fifo() {
        let ic = FakeIrqController::new();
        ic.inject(IrqNumber(30));
        ic.inject(IrqNumber(31));
        assert_eq!(ic.acknowledge(), Some(IrqNumber(30)));
        assert_eq!(ic.acknowledge(), Some(IrqNumber(31)));
        assert_eq!(ic.acknowledge(), None);
    }

    #[test]
    fn end_of_interrupt_records_irq_in_order() {
        let ic = FakeIrqController::new();
        ic.end_of_interrupt(IrqNumber(30));
        ic.end_of_interrupt(IrqNumber(31));
        assert_eq!(ic.eoi_history(), vec![IrqNumber(30), IrqNumber(31)]);
    }

    #[test]
    fn ack_eoi_cycle_leaves_clean_state() {
        let ic = FakeIrqController::new();
        ic.enable(IrqNumber(30));
        ic.inject(IrqNumber(30));
        assert_eq!(ic.pending_count(), 1);

        let irq = ic.acknowledge().expect("pending IRQ must ack");
        assert_eq!(irq, IrqNumber(30));
        assert_eq!(ic.pending_count(), 0);

        ic.end_of_interrupt(irq);
        assert_eq!(ic.eoi_history(), vec![IrqNumber(30)]);
    }

    #[test]
    fn disabled_irq_can_still_be_injected_for_test_purposes() {
        // The fake does not enforce enable-gating on inject; it's up to
        // tests to compose is_enabled with inject behaviour. Documents
        // the intended use.
        let ic = FakeIrqController::new();
        assert!(!ic.is_enabled(IrqNumber(30)));
        ic.inject(IrqNumber(30));
        assert_eq!(ic.acknowledge(), Some(IrqNumber(30)));
    }
}
