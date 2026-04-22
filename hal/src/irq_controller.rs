//! Interrupt controller dispatch and control.
//!
//! See [ADR-0011] for the v1 scope and the list of deferred capabilities.
//!
//! [ADR-0011]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0011-irq-controller-trait.md

/// A hardware interrupt line number.
///
/// Wide enough (`u32`) to cover every realistic interrupt controller —
/// `GICv3` addresses up to ~16 million lines. The newtype is there to keep
/// interrupt numbers type-distinct from other `u32` values in the kernel.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct IrqNumber(pub u32);

/// Interrupt controller operations.
///
/// Covers the four primitives the kernel's ISR entry/exit path needs:
/// enable, disable, acknowledge the currently-pending interrupt, and
/// signal end-of-interrupt. Priority configuration, edge/level type
/// selection, per-CPU routing, SGI/IPI, nested interrupts, pending-clear,
/// active/pending queries, LPIs, and MSI are all out of v1 scope and
/// will arrive through their own ADRs.
///
/// # Contract
///
/// - **Object-safe.** The kernel uses `&dyn IrqController`.
/// - **`Send + Sync`.** Enforced at the bound.
/// - **No allocation.** Implementations must not touch the heap.
/// - **Idempotent enable/disable.** Calling `enable` on an already-enabled
///   line (or `disable` on an already-disabled line) is a no-op.
/// - **`acknowledge` returns `None` on spurious.** The GIC's spurious ID
///   (1023) and race-window empty reads are folded into `None`, so the
///   caller handles the "nothing to do" case explicitly.
/// - **`end_of_interrupt` takes the number from `acknowledge`.** The
///   caller must pair each successful `acknowledge` with one
///   `end_of_interrupt` using the returned number.
pub trait IrqController: Send + Sync {
    /// Enable delivery of the given interrupt line to the CPU.
    fn enable(&self, irq: IrqNumber);

    /// Suppress delivery of the given interrupt line.
    fn disable(&self, irq: IrqNumber);

    /// Acknowledge the currently-pending interrupt.
    ///
    /// Called at ISR entry. Reads the controller's acknowledge register,
    /// marks the top-pending interrupt as active, and returns its number.
    /// Returns `None` for a spurious interrupt (the `GIC` returns
    /// `INTID 1023` to signal this) or for a race where the interrupt
    /// disappeared before acknowledgement.
    fn acknowledge(&self) -> Option<IrqNumber>;

    /// Signal end-of-interrupt for the given line.
    ///
    /// Called at ISR exit with the number returned by [`Self::acknowledge`].
    /// On `GICv3` this performs both Priority Drop and Deactivation; on
    /// `GICv2` it is a single EOI. The kernel does not distinguish.
    fn end_of_interrupt(&self, irq: IrqNumber);
}
