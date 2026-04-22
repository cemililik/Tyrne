//! Deterministic fake [`tyrne_hal::Cpu`] for host-side tests.

use std::sync::Mutex;
use tyrne_hal::{CoreId, Cpu, IrqState};

/// A [`Cpu`] that tracks interrupt state and method-call counts for test
/// assertions.
///
/// The fake records:
///
/// - Whether interrupts are currently masked.
/// - How many times [`Cpu::wait_for_interrupt`] has been called.
/// - How many times [`Cpu::instruction_barrier`] has been called.
///
/// Tests can inspect these via the `irqs_enabled`, `wait_for_interrupt_count`,
/// and `instruction_barrier_count` accessors.
pub struct FakeCpu {
    core_id: CoreId,
    state: Mutex<FakeCpuState>,
}

struct FakeCpuState {
    irqs_enabled: bool,
    wait_for_interrupt_count: u64,
    instruction_barrier_count: u64,
}

impl FakeCpu {
    /// Construct a `FakeCpu` reporting core 0 with interrupts initially
    /// enabled.
    #[must_use]
    pub fn new() -> Self {
        Self::with_core_id(0)
    }

    /// Construct a `FakeCpu` reporting the given core id with interrupts
    /// initially enabled.
    #[must_use]
    pub fn with_core_id(core_id: CoreId) -> Self {
        Self {
            core_id,
            state: Mutex::new(FakeCpuState {
                irqs_enabled: true,
                wait_for_interrupt_count: 0,
                instruction_barrier_count: 0,
            }),
        }
    }

    /// Return whether interrupts are currently enabled on this fake core.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned by a prior panic in
    /// another thread.
    #[must_use]
    pub fn irqs_enabled(&self) -> bool {
        self.locked().irqs_enabled
    }

    /// Return the number of times [`Cpu::wait_for_interrupt`] has been
    /// called on this fake.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned.
    #[must_use]
    pub fn wait_for_interrupt_count(&self) -> u64 {
        self.locked().wait_for_interrupt_count
    }

    /// Return the number of times [`Cpu::instruction_barrier`] has been
    /// called on this fake.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned.
    #[must_use]
    pub fn instruction_barrier_count(&self) -> u64 {
        self.locked().instruction_barrier_count
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, FakeCpuState> {
        self.state.lock().expect("FakeCpu mutex poisoned")
    }
}

impl Default for FakeCpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu for FakeCpu {
    fn current_core_id(&self) -> CoreId {
        self.core_id
    }

    fn disable_irqs(&self) -> IrqState {
        let mut state = self.locked();
        let prev = IrqState(usize::from(state.irqs_enabled));
        state.irqs_enabled = false;
        prev
    }

    fn restore_irq_state(&self, state: IrqState) {
        self.locked().irqs_enabled = state.0 != 0;
    }

    fn wait_for_interrupt(&self) {
        self.locked().wait_for_interrupt_count += 1;
    }

    fn instruction_barrier(&self) {
        self.locked().instruction_barrier_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::FakeCpu;
    use tyrne_hal::{Cpu, IrqGuard};

    #[test]
    fn default_cpu_reports_core_zero_with_irqs_enabled() {
        let cpu = FakeCpu::new();
        assert_eq!(cpu.current_core_id(), 0);
        assert!(cpu.irqs_enabled());
    }

    #[test]
    fn with_core_id_sets_reported_id() {
        let cpu = FakeCpu::with_core_id(3);
        assert_eq!(cpu.current_core_id(), 3);
    }

    #[test]
    fn disable_irqs_masks_and_returns_previous_state() {
        let cpu = FakeCpu::new();
        let prev = cpu.disable_irqs();
        assert!(!cpu.irqs_enabled());
        cpu.restore_irq_state(prev);
        assert!(cpu.irqs_enabled());
    }

    #[test]
    fn irq_guard_enters_and_exits_critical_section() {
        let cpu = FakeCpu::new();
        assert!(cpu.irqs_enabled());
        {
            let _g = IrqGuard::new(&cpu);
            assert!(!cpu.irqs_enabled());
        }
        assert!(cpu.irqs_enabled());
    }

    #[test]
    fn nested_irq_guards_restore_outer_state() {
        let cpu = FakeCpu::new();
        {
            let _outer = IrqGuard::new(&cpu);
            assert!(!cpu.irqs_enabled());
            {
                let _inner = IrqGuard::new(&cpu);
                assert!(!cpu.irqs_enabled());
            }
            // Dropping the inner guard must leave us inside the outer
            // critical section, not fully re-enable interrupts.
            assert!(!cpu.irqs_enabled());
        }
        assert!(cpu.irqs_enabled());
    }

    #[test]
    fn wait_for_interrupt_increments_count() {
        let cpu = FakeCpu::new();
        assert_eq!(cpu.wait_for_interrupt_count(), 0);
        cpu.wait_for_interrupt();
        cpu.wait_for_interrupt();
        assert_eq!(cpu.wait_for_interrupt_count(), 2);
    }

    #[test]
    fn instruction_barrier_increments_count() {
        let cpu = FakeCpu::new();
        assert_eq!(cpu.instruction_barrier_count(), 0);
        cpu.instruction_barrier();
        assert_eq!(cpu.instruction_barrier_count(), 1);
    }
}
