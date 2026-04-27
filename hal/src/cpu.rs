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

/// Read the current Exception Level on aarch64 bare-metal targets.
///
/// Returns the 2-bit `EL` field of the `CurrentEL` system register
/// (0 = EL0, 1 = EL1, 2 = EL2, 3 = EL3). This is the safe-Rust entry
/// point for any code that needs to assert which EL it is currently
/// running at — it lets callers avoid duplicating the inline-asm `MRS`
/// pattern from one site to another.
///
/// Free function rather than a [`Cpu`] trait method per ADR-0024
/// §Open questions: the early-boot path (between `_start` and
/// `kernel_entry`) needs to read the EL before any `Cpu` instance has
/// been constructed, so the helper must be callable without a `&self`
/// receiver. Test mocks (e.g. the `FakeCpu` instances in
/// [`tyrne_test_hal`] and the kernel's `sched` test module) therefore
/// do not need to declare an EL of their own.
///
/// # Availability
///
/// Defined only on `target_arch = "aarch64"` AND `target_os = "none"`
/// (i.e. the bare-metal kernel build). On hosted targets — including
/// `aarch64-apple-darwin` running unit tests, where `CurrentEL` reads
/// would trap with a `SIGILL` because user code is not in EL1 — the
/// function is intentionally absent. Host tests must mock the EL
/// rather than call this helper.
///
/// # Safety
///
/// `MRS x, CurrentEL` is a non-privileged read of a read-only system
/// register. It is callable at every EL ≥ 0, does not modify any
/// state, and `options(nostack, nomem)` is correct — there is no
/// stack-pointer touch and no memory access. The function presents
/// itself as `safe` because it upholds those invariants and returns a
/// plain `u8`. Audit: UNSAFE-2026-0018.
#[cfg(all(target_arch = "aarch64", target_os = "none"))]
#[must_use]
pub fn current_el() -> u8 {
    let raw: u64;
    // SAFETY: see the function-level safety paragraph above. The
    // read of `CurrentEL` is non-privileged, side-effect-free, and
    // available at every EL. Audit: UNSAFE-2026-0018.
    unsafe {
        core::arch::asm!(
            "mrs {}, CurrentEL",
            out(reg) raw,
            options(nostack, nomem),
        );
    }
    // CurrentEL bits [3:2] hold the EL field (0..=3); mask + shift
    // and the result fits in u8 trivially.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "EL field is 2 bits — masked value is in 0..=3, fits in u8"
    )]
    let el = ((raw >> 2) & 0b11) as u8;
    el
}
