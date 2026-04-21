//! `Cpu` and `ContextSwitch` implementations for the QEMU `virt` aarch64 target.
//!
//! `QemuVirtCpu` implements:
//! - [`umbrix_hal::Cpu`] — interrupt masking and core identity (object-safe).
//! - [`umbrix_hal::ContextSwitch`] — cooperative register-state save/restore
//!   (generic; see [ADR-0020]).
//!
//! # Safety overview
//!
//! The context-switch assembly (`context_switch_asm`) is the only intrinsically
//! unsafe operation in this file. Every other `unsafe impl` is a marker that
//! follows from the struct's invariants. See the individual `// SAFETY:` comments
//! and the audit entries `UNSAFE-2026-0006` through `UNSAFE-2026-0009`.
//!
//! [ADR-0020]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0020-cpu-trait-v2-context-switch.md

use core::arch::{asm, naked_asm};

use umbrix_hal::{ContextSwitch, CoreId, Cpu, IrqState};

// ─── QemuVirtCpu ────────────────────────────────────────────────────────────

/// The QEMU `virt` aarch64 CPU implementation.
///
/// A zero-size type — all behaviour comes from DAIF register manipulation
/// and the context-switch assembly stub. Construct via [`QemuVirtCpu::new`].
pub struct QemuVirtCpu {
    _priv: (),
}

impl QemuVirtCpu {
    /// Construct the CPU handle.
    ///
    /// # Safety
    ///
    /// There must be at most one `QemuVirtCpu` instance driving a given
    /// physical core. Creating a second instance on the same core and calling
    /// `restore_irq_state` on both may produce inconsistent DAIF state —
    /// the second instance's saved `IrqState` would reflect a different DAIF
    /// snapshot and restoring it would silently override the first instance's
    /// state. In v1 (single-core), construct exactly once in `kernel_entry`.
    #[must_use]
    pub const unsafe fn new() -> Self {
        Self { _priv: () }
    }
}

// SAFETY: `QemuVirtCpu` is a zero-size marker; it has no interior mutability
// and holds no pointers. Sending it between threads is safe — the only shared
// hardware resource (DAIF) is accessed via per-core system registers that are
// inherently thread-local in a single-core system.
// Rejected alternatives: wrapping in a `Mutex` or `AtomicUsize` would add
// overhead with no benefit — DAIF is already a per-core register, not shared
// memory; there is nothing to protect with a software lock.
// Audit: UNSAFE-2026-0006.
unsafe impl Send for QemuVirtCpu {}

// SAFETY: Same reasoning as the `Send` impl — no interior mutability; DAIF
// reads/writes are atomic per-core register operations. A `RefCell` or similar
// interior-mutability wrapper would not help because the resource (DAIF) is
// a hardware register, not a Rust data structure; the safe abstraction is
// already the `Cpu` trait methods.
// Audit: UNSAFE-2026-0006.
unsafe impl Sync for QemuVirtCpu {}

impl Cpu for QemuVirtCpu {
    fn current_core_id(&self) -> CoreId {
        let mpidr: u64;
        // SAFETY: `MRS x, MPIDR_EL1` is a non-privileged read of a read-only
        // system register. It does not modify any state and is always available
        // in EL1. Audit: UNSAFE-2026-0007.
        unsafe {
            asm!("mrs {}, mpidr_el1", out(reg) mpidr, options(nostack, nomem));
        }
        // AFF0 (bits 7:0) identifies the core within a cluster. QEMU virt
        // presents a flat topology where AFF0 == core index.
        #[allow(clippy::cast_possible_truncation, reason = "AFF0 fits in u32")]
        let id = (mpidr & 0xFF) as u32;
        id
    }

    fn disable_irqs(&self) -> IrqState {
        let daif: usize;
        // SAFETY: `MRS x, DAIF` reads the current interrupt mask; `MSR DAIFSet, #0xf`
        // sets all four DAIF mask bits (D, A, I, F) atomically via the write-only
        // DAIFSet encoding — this is distinct from `MSR DAIF, #imm` which would
        // require a 9-bit immediate. Both are EL1-privileged register operations.
        // The returned `IrqState` captures the prior value so that
        // `restore_irq_state` can restore it exactly. Audit: UNSAFE-2026-0007.
        unsafe {
            asm!(
                "mrs {daif}, daif",
                "msr daifset, #0xf",
                daif = out(reg) daif,
                options(nostack, nomem),
            );
        }
        IrqState(daif)
    }

    fn restore_irq_state(&self, state: IrqState) {
        // SAFETY: `MSR DAIF, x` writes the full DAIF register. `state` must be
        // a value previously returned by `disable_irqs`; the caller is
        // contractually bound to pass it unmodified. Writing an arbitrary value
        // could enable or suppress interrupts unexpectedly, but the contract
        // documents this requirement. Audit: UNSAFE-2026-0007.
        unsafe {
            asm!("msr daif, {}", in(reg) state.0, options(nostack, nomem));
        }
    }

    fn wait_for_interrupt(&self) {
        // SAFETY: `WFI` halts the core until an interrupt arrives. It does not
        // modify registers or memory; it only affects CPU power state.
        // Audit: UNSAFE-2026-0007.
        unsafe {
            asm!("wfi", options(nostack, nomem));
        }
    }

    fn instruction_barrier(&self) {
        // SAFETY: `ISB` synchronizes the instruction stream. It is always safe
        // to call; it cannot cause memory corruption. Audit: UNSAFE-2026-0007.
        unsafe {
            asm!("isb", options(nostack, nomem));
        }
    }
}

// ─── Aarch64TaskContext ──────────────────────────────────────────────────────

/// Saved callee-register state for one cooperative task on aarch64.
///
/// Layout must match the field offsets used by [`context_switch_asm`].
/// `#[repr(C)]` prevents field reordering.
///
/// Per AAPCS64, callee-saved registers are:
/// - General-purpose: x19–x28, x29 (fp), x30 (lr), sp
/// - SIMD/FP: the lower 64 bits of v8–v15 (i.e. d8–d15)
///
/// d8–d15 must be saved whenever `CPACR_EL1.FPEN` is non-zero, because the
/// compiler may allocate those registers for any kernel-level task and will
/// not emit callee-save spills across a cooperative yield.
///
/// Total size: (10 + 1 + 1 + 1) × 8 + 8 × 8 = 104 + 64 = 168 bytes.
#[derive(Default)]
#[repr(C)]
pub struct Aarch64TaskContext {
    /// `x19`–`x28`: callee-saved general-purpose registers (10 × u64).
    pub x19_x28: [u64; 10],
    /// `x29` — frame pointer (callee-saved in AAPCS64).
    pub fp: u64,
    /// `x30` — link register / return address (callee-saved in AAPCS64).
    pub lr: u64,
    /// Stack pointer — saved explicitly (not a general-purpose register).
    pub sp: u64,
    /// Lower 64 bits of `v8`–`v15` (`d8`–`d15`): AAPCS64 callee-saved
    /// SIMD/FP registers. Only the lower 8 bytes need to be preserved;
    /// the upper 64 bits are caller-saved.
    pub d8_d15: [u64; 8],
}

// ─── context_switch_asm ──────────────────────────────────────────────────────

/// Save all AAPCS64 callee-saved registers into `*current` and restore from `*next`.
///
/// Saves: x19–x28, x29 (fp), x30 (lr), sp, d8–d15.
/// Restores: same set from `*next`, then `ret`s to the restored lr.
///
/// # Safety
///
/// - Both pointers must be 8-byte-aligned and valid for the duration of the
///   switch.
/// - `next` must have been written by a prior call to `context_switch_asm`
///   or fully initialised by [`QemuVirtCpu::init_context`].
/// - The caller is responsible for disabling interrupts before calling this
///   function.
///
/// `#[unsafe(naked)]` suppresses the compiler-generated prologue/epilogue so
/// that `sp` is saved and restored exactly, with no hidden adjustment. Without
/// it the compiler pushes a frame onto the stack before our asm runs, causing
/// the saved `sp` to be 16 bytes too low — the caller's epilogue then reads
/// callee-saved registers from the wrong stack addresses after a context switch.
///
/// Registers arrive per AAPCS64: `current` → x0, `next` → x1.
/// x8 is used as a scratch register (caller-saved; clobbered by the asm).
///
/// Audit: UNSAFE-2026-0008.
#[unsafe(naked)]
unsafe extern "C" fn context_switch_asm(
    current: *mut Aarch64TaskContext,
    next: *const Aarch64TaskContext,
) {
    // Field offsets within Aarch64TaskContext (repr(C)):
    //   x19_x28  offset   0  (10 × 8 = 80 bytes)
    //   fp       offset  80
    //   lr       offset  88
    //   sp       offset  96
    //   d8_d15   offset 104  ( 8 × 8 = 64 bytes)
    //   total          168 bytes
    //
    // sp cannot appear as a source operand in most AArch64 store instructions,
    // so we move it through x8 (a caller-saved scratch register).
    //
    // d8–d15 are saved as 64-bit values (lower half of v8–v15). AAPCS64
    // requires preserving only the lower 8 bytes of each v8–v15 register.
    naked_asm!(
        // ── save current (x0) ─────────────────────────────────────────
        "stp x19, x20, [x0,  #0]",
        "stp x21, x22, [x0,  #16]",
        "stp x23, x24, [x0,  #32]",
        "stp x25, x26, [x0,  #48]",
        "stp x27, x28, [x0,  #64]",
        "stp x29, x30, [x0,  #80]", // fp, lr
        "mov x8,  sp",
        "str x8,  [x0,  #96]", // sp
        "stp d8,  d9,  [x0,  #104]",
        "stp d10, d11, [x0,  #120]",
        "stp d12, d13, [x0,  #136]",
        "stp d14, d15, [x0,  #152]",
        // ── restore next (x1) ─────────────────────────────────────────
        "ldp d14, d15, [x1,  #152]",
        "ldp d12, d13, [x1,  #136]",
        "ldp d10, d11, [x1,  #120]",
        "ldp d8,  d9,  [x1,  #104]",
        "ldr x8,  [x1,  #96]", // sp
        "mov sp,  x8",
        "ldp x29, x30, [x1,  #80]", // fp, lr
        "ldp x27, x28, [x1,  #64]",
        "ldp x25, x26, [x1,  #48]",
        "ldp x23, x24, [x1,  #32]",
        "ldp x21, x22, [x1,  #16]",
        "ldp x19, x20, [x1,   #0]",
        // ret jumps to the lr just loaded from `next`.
        // On a task's first run that lr was set by init_context to the
        // entry function; on subsequent runs it is the return address
        // stored by the `bl context_switch_asm` in the previous yield.
        "ret",
    );
}

// ─── ContextSwitch impl ───────────────────────────────────────────────────────

impl ContextSwitch for QemuVirtCpu {
    type TaskContext = Aarch64TaskContext;

    unsafe fn context_switch(&self, current: &mut Self::TaskContext, next: &Self::TaskContext) {
        // SAFETY: caller guarantees interrupts are disabled and both contexts
        // are valid. We forward directly to the assembly stub which upholds
        // the AAPCS64 callee-save contract. Audit: UNSAFE-2026-0008.
        unsafe {
            context_switch_asm(
                core::ptr::from_mut::<Aarch64TaskContext>(current),
                core::ptr::from_ref::<Aarch64TaskContext>(next),
            );
        }
    }

    unsafe fn init_context(
        &self,
        ctx: &mut Self::TaskContext,
        entry: fn() -> !,
        stack_top: *mut u8,
    ) {
        // Set lr to the entry function — the first `ret` in context_switch_asm
        // will jump here to begin the task.
        // Set sp to stack_top — caller must guarantee 16-byte alignment.
        // All other callee-saved registers are zero (from Default), which is
        // safe: the entry function establishes its own frame.
        // Audit: UNSAFE-2026-0009.
        ctx.lr = entry as usize as u64;
        ctx.sp = stack_top as u64;
    }
}
