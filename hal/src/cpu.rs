//! CPU control primitives and critical-section support.
//!
//! See [ADR-0008] for the trait-signature rationale.
//!
//! [ADR-0008]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0008-cpu-trait.md

/// Identifier for a CPU core.
///
/// On aarch64 this is derived from `MPIDR_EL1`; other architectures define
/// the mapping themselves. Core identifiers are stable for the lifetime of
/// a boot.
pub type CoreId = u32;

/// The state of the CPU's interrupt mask, as saved by
/// [`Cpu::disable_irqs`] and consumed by [`Cpu::restore_irq_state`].
///
/// Callers should treat the inner value as opaque — pass it back
/// unmodified to `restore_irq_state`. The inner field is `pub` so that BSP
/// implementations can construct the value from raw architecture bits;
/// it is not an invitation for callers to inspect or synthesize bits.
#[derive(Copy, Clone)]
pub struct IrqState(pub usize);

/// Privileged CPU state and control.
///
/// Covers the single-core primitives the kernel needs during boot, idle,
/// and critical sections: identifying the running core, masking and
/// restoring interrupts, halting until the next interrupt, and
/// synchronizing the instruction stream.
///
/// Multi-core start, context-switch primitives, and topology queries are
/// deliberately not on this trait; they will arrive through their own ADRs
/// alongside the work that needs them.
///
/// # Contract
///
/// - **Object-safe.** The kernel uses `&dyn Cpu`.
/// - **`Send + Sync`.** Compiler-enforced; the same `Cpu` instance may be
///   called from any online core.
/// - **No allocation.** Implementations must not touch the heap.
/// - **Primitive operations.** Each method corresponds to one
///   architecture-level operation; higher-level helpers (such as
///   [`IrqGuard`]) are layered on top, not into the trait.
pub trait Cpu: Send + Sync {
    /// Return the identifier of the CPU core on which this call is
    /// currently executing.
    fn current_core_id(&self) -> CoreId;

    /// Mask CPU-level interrupts and return the previous mask state.
    ///
    /// Pair with [`Cpu::restore_irq_state`] to form a critical section.
    /// Prefer the [`IrqGuard`] wrapper for RAII-style usage.
    fn disable_irqs(&self) -> IrqState;

    /// Restore the CPU interrupt mask to the given saved state.
    ///
    /// `state` must be a value previously returned by
    /// [`Cpu::disable_irqs`]; passing any other value is a violation of
    /// the calling contract and the resulting behaviour is
    /// implementation-defined.
    fn restore_irq_state(&self, state: IrqState);

    /// Halt the CPU until the next interrupt wakes it.
    ///
    /// On aarch64 this is `WFI`. On other architectures, the equivalent
    /// low-power-halt instruction.
    fn wait_for_interrupt(&self);

    /// Synchronize the instruction stream.
    ///
    /// Required after writing privileged system registers whose effects
    /// may not yet be observed by the CPU's in-flight pipeline. On
    /// aarch64 this is `ISB`. Data memory barriers are covered by
    /// [`core::sync::atomic::fence`] and are not exposed on this trait.
    fn instruction_barrier(&self);
}

/// RAII guard that masks interrupts for its lifetime and restores the
/// previous mask state on drop.
///
/// Construct with [`IrqGuard::new`] to enter a critical section. The
/// guard captures the interrupt state at entry, so nested guards compose
/// correctly — dropping an inner guard leaves the outer section's mask
/// in place rather than fully re-enabling interrupts.
///
/// Generic over the concrete CPU type `C` to avoid fat-pointer vtable
/// dispatch on critical-section paths. Dynamic dispatch (`&dyn Cpu`) is also
/// avoided because coercing a concrete type to a trait object at certain
/// inlining depths can produce vtable references that alias unrelated data
/// in `.rodata`; using a concrete type parameter eliminates the coercion site
/// entirely.
///
/// # Example
///
/// ```ignore
/// use tyrne_hal::IrqGuard;
///
/// let _g = IrqGuard::new(cpu);
/// // critical section: interrupts are masked here
/// // `_g` drops at end of scope; interrupts restored to previous state
/// ```
pub struct IrqGuard<'a, C: Cpu> {
    cpu: &'a C,
    prev: IrqState,
}

impl<'a, C: Cpu> IrqGuard<'a, C> {
    /// Enter a critical section by masking interrupts on the current core.
    ///
    /// The interrupt state at the moment of construction is remembered and
    /// restored when the returned guard is dropped.
    pub fn new(cpu: &'a C) -> Self {
        let prev = cpu.disable_irqs();
        Self { cpu, prev }
    }
}

impl<C: Cpu> Drop for IrqGuard<'_, C> {
    fn drop(&mut self) {
        self.cpu.restore_irq_state(self.prev);
    }
}
