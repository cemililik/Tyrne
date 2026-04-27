//! `Cpu`, `ContextSwitch`, and `Timer` implementations for the QEMU `virt`
//! aarch64 target.
//!
//! `QemuVirtCpu` implements:
//! - [`tyrne_hal::Cpu`] — interrupt masking and core identity (object-safe).
//! - [`tyrne_hal::ContextSwitch`] — cooperative register-state save/restore
//!   (generic; see [ADR-0020]).
//! - [`tyrne_hal::Timer`] — monotonic time via the ARM Generic Timer's
//!   **virtual** counter (`CNTVCT_EL0`) and frequency register
//!   (`CNTFRQ_EL0`); see [ADR-0010]. The deadline-arming half
//!   (`arm_deadline` / `cancel_deadline`) is intentionally
//!   `unimplemented!()` until GIC + interrupt-vector-table wiring lands —
//!   see T-009 task notes. Reading the virtual counter (rather than the
//!   physical `CNTPCT_EL0`) keeps the read side aligned with the
//!   deferred deadline-arming side, which programs `CNTV_CVAL_EL0` /
//!   `CNTV_CTL_EL0` per ADR-0010's references and ADR-0022's first-
//!   rider sub-rider.
//!
//! # Safety overview
//!
//! The context-switch assembly (`context_switch_asm`) is the only intrinsically
//! unsafe operation in this file. Every other `unsafe impl` is a marker that
//! follows from the struct's invariants, and every inline-asm system-register
//! read is a non-mutating MRS at EL1. See the individual `// SAFETY:` comments
//! and the audit entries `UNSAFE-2026-0006` through `UNSAFE-2026-0009`, plus
//! `UNSAFE-2026-0015` for the Timer-trait additions.
//!
//! [ADR-0010]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0010-timer-trait.md
//! [ADR-0020]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0020-cpu-trait-v2-context-switch.md

use core::arch::{asm, naked_asm};

use tyrne_hal::timer::{resolution_ns_for_freq, ticks_to_ns};
use tyrne_hal::{ContextSwitch, CoreId, Cpu, IrqState, Timer};

// ─── QemuVirtCpu ────────────────────────────────────────────────────────────

/// The QEMU `virt` aarch64 CPU implementation.
///
/// Holds the generic-timer parameters read once at construction; all
/// other behaviour comes from DAIF register manipulation and the
/// context-switch assembly stub. Construct via [`QemuVirtCpu::new`].
///
/// # Layout
///
/// Two `u64` fields, both populated from system registers in [`Self::new`]:
///
/// - `frequency_hz` — value of `CNTFRQ_EL0`, the system counter frequency
///   in Hz. ARM ARM treats this as firmware-set; QEMU virt sets it to
///   62.5 MHz. `Timer::now_ns` reads it on every call (along with
///   `CNTVCT_EL0`) and forwards to [`tyrne_hal::timer::ticks_to_ns`],
///   which performs the 128-bit `count * 1e9 / frequency_hz` conversion.
/// - `resolution_ns` — round-to-nearest result of
///   [`tyrne_hal::timer::resolution_ns_for_freq`] computed once at
///   construction. **Cached only to serve the `Timer::resolution_ns`
///   trait method** — `now_ns`'s hot path does NOT use it; the
///   conversion is exact via the `ticks_to_ns` u128 path. Earlier
///   drafts of T-009 used `count * resolution_ns` for `now_ns`'s
///   conversion, but second-read review found that form drifts on
///   non-divisor frequencies (e.g. 19.2 MHz → 0.16 % drift) and was
///   replaced.
pub struct QemuVirtCpu {
    /// Counter frequency from `CNTFRQ_EL0`, in Hz. Read once at construction.
    frequency_hz: u64,
    /// Pre-computed `resolution_ns_for_freq(frequency_hz)`. Used **only**
    /// by the [`Timer::resolution_ns`] trait method; [`Timer::now_ns`]
    /// converts via `ticks_to_ns` and does not consult this field.
    resolution_ns: u64,
}

impl QemuVirtCpu {
    /// Construct the CPU handle, sampling `CNTFRQ_EL0` to set up the timer.
    ///
    /// # Safety
    ///
    /// There must be at most one `QemuVirtCpu` instance driving a given
    /// physical core. Creating a second instance on the same core and calling
    /// `restore_irq_state` on both may produce inconsistent DAIF state —
    /// the second instance's saved `IrqState` would reflect a different DAIF
    /// snapshot and restoring it would silently override the first instance's
    /// state. In v1 (single-core), construct exactly once in `kernel_entry`.
    ///
    /// # Panics
    ///
    /// Panics in two boot-time-invariant cases. Both indicate a
    /// misconfigured BSP or a deviation from ADR-0012's boot contract;
    /// failing loudly is preferred to silently producing wrong timer
    /// values:
    ///
    /// - **`CurrentEL` is not EL1.** Tyrne expects `kernel_entry` to run
    ///   at EL1 per [ADR-0012]; the assertion catches a future boot-flow
    ///   change that leaves the kernel at EL2 / EL3 before any
    ///   generic-timer MRS would silently misbehave. Audit: UNSAFE-2026-0016.
    /// - **`CNTFRQ_EL0` reads as zero.** ARM ARM specifies firmware must
    ///   set this register; a zero value would make `now_ns` divide by
    ///   zero and `resolution_ns_for_freq` overflow. Audit: UNSAFE-2026-0015.
    ///
    /// [ADR-0012]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0012-boot-flow-qemu-virt.md
    #[must_use]
    pub unsafe fn new() -> Self {
        // Runtime assertion of the ADR-0012 boot-time precondition: QEMU
        // virt is supposed to deliver `kernel_entry` at EL1, and `boot.s`
        // performs no EL transition. The MRS reads below assume that
        // contract; if a future boot-flow change accidentally leaves us at
        // EL2 or EL3, the timer system-register accesses would either
        // trap or read undefined values. Catching the violation here —
        // before any timer read — turns a subtle hardware-level
        // misbehaviour into a loud, named boot panic.
        let current_el_raw: u64;
        // SAFETY: `MRS x, CurrentEL` is a non-privileged read of a
        // read-only system register, available at every Exception Level.
        // The instruction does not modify any state. `options(nostack,
        // nomem)` is correct. Rejected alternatives: there is no safe-Rust
        // path to read CurrentEL; this assertion is the safe abstraction.
        // Audit: UNSAFE-2026-0016.
        unsafe {
            asm!("mrs {}, CurrentEL", out(reg) current_el_raw, options(nostack, nomem));
        }
        let current_el = (current_el_raw >> 2) & 0b11;
        assert_eq!(
            current_el, 1,
            "QemuVirtCpu::new must run at EL1 per ADR-0012; observed EL{current_el} instead",
        );

        let frequency_hz: u64;
        // SAFETY: `MRS x, CNTFRQ_EL0` is a non-privileged read of a read-only
        // system register. Tyrne enters `kernel_entry` at EL1 per
        // [ADR-0012] (QEMU virt drops the kernel to EL1 before execution;
        // `boot.s` performs no EL transition) — and the assertion above
        // confirms this at runtime, so the EL-precondition reasoning that
        // follows is not just documentation but a checked invariant.
        // At EL1 in the non-VHE configuration the kernel runs in
        // (HCR_EL2.{E2H, TGE} = {0, 0}), CNTFRQ_EL0 is unconditionally
        // readable — the CNTHCTL_EL2.EL1PCTEN gating that exists in VHE
        // mode does not apply here. The instruction does not modify any
        // state; `options(nostack, nomem)` is correct (no stack pointer
        // touch, no memory access). Rejected alternatives: there is no
        // safe-Rust way to read a system register; the HAL `Timer` trait
        // is the safe abstraction wrapping this access.
        // Audit: UNSAFE-2026-0015.
        //
        // [ADR-0012]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0012-boot-flow-qemu-virt.md
        unsafe {
            asm!("mrs {}, cntfrq_el0", out(reg) frequency_hz, options(nostack, nomem));
        }
        assert!(
            frequency_hz > 0,
            "CNTFRQ_EL0 reads as zero — firmware/emulator did not initialise the generic timer",
        );
        let resolution_ns = resolution_ns_for_freq(frequency_hz);
        Self {
            frequency_hz,
            resolution_ns,
        }
    }

    /// Return the cached counter frequency in Hz, as read from `CNTFRQ_EL0`
    /// at construction. Exposed for diagnostic / boot-banner use; not part
    /// of any HAL trait.
    #[must_use]
    pub fn frequency_hz(&self) -> u64 {
        self.frequency_hz
    }
}

// SAFETY: `QemuVirtCpu` holds two `u64` fields written exactly once at
// construction (`frequency_hz` and `resolution_ns`); afterwards it is
// effectively immutable. It has no interior mutability and holds no
// pointers. Sending it between threads is safe — the only shared hardware
// resources (DAIF, the generic timer's CNTVCT/CNTFRQ) are accessed via
// per-core system registers that are inherently thread-local in a
// single-core system.
// Rejected alternatives: wrapping in a `Mutex` or `AtomicUsize` would add
// overhead with no benefit — the cached fields never change after `new()`,
// and DAIF / CNTVCT are per-core registers, not shared memory; there is
// nothing to protect with a software lock.
// The audit-log entry's body still describes the original zero-size shape
// of `QemuVirtCpu`; the post-T-009 struct shape is recorded under the
// 2026-04-23 Amendment block of UNSAFE-2026-0006 per unsafe-policy §3.
// Audit: UNSAFE-2026-0006.
unsafe impl Send for QemuVirtCpu {}

// SAFETY: Same reasoning as the `Send` impl — no interior mutability after
// construction; DAIF reads/writes and CNTVCT/CNTFRQ reads are atomic
// per-core register operations. A `RefCell` or similar interior-mutability
// wrapper would not help because the resource is a hardware register, not
// a Rust data structure; the safe abstractions are already the `Cpu` and
// `Timer` trait methods.
// Audit: UNSAFE-2026-0006 (post-T-009 scope under the 2026-04-23 Amendment).
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

// ─── Timer impl ──────────────────────────────────────────────────────────────

impl Timer for QemuVirtCpu {
    fn now_ns(&self) -> u64 {
        let count: u64;
        // SAFETY: `MRS x, CNTVCT_EL0` is a non-privileged read of the
        // virtual count register of the ARM Generic Timer. Tyrne reads the
        // **virtual** counter (CNTVCT) rather than the physical one
        // (CNTPCT) so the read side and the deferred deadline-arming side
        // (`CNTV_CVAL_EL0` / `CNTV_CTL_EL0`, see ADR-0010 references and
        // ADR-0022's first-rider sub-rider) live in the same register
        // family. On QEMU virt with `CNTVOFF_EL2 = 0` the two counters
        // coincide; using CNTVCT preserves correctness when a future boot
        // path leaves a non-zero offset.
        //
        // EL access: at EL1 in the non-VHE configuration Tyrne runs in
        // (per ADR-0012 the kernel enters at EL1 and boot.s performs no EL
        // transition), CNTVCT_EL0 is unconditionally readable — the
        // CNTHCTL_EL2.EL1VCTEN gating that exists in VHE mode does not
        // apply here. The instruction does not modify any state;
        // `options(nostack, nomem)` is correct.
        //
        // Rejected alternatives: there is no safe-Rust way to read a
        // system register; the `Timer` trait is the safe abstraction
        // wrapping this MRS. The `cortex-a` / `aarch64-cpu` crates would
        // wrap a single MRS in a dependency, disproportionate per the
        // dependency policy. Audit: UNSAFE-2026-0015.
        unsafe {
            asm!("mrs {}, cntvct_el0", out(reg) count, options(nostack, nomem));
        }
        // Conversion delegated to `ticks_to_ns` (host-testable). u128
        // intermediate arithmetic: overflow-free for any tick count up to
        // u64::MAX at any sane frequency. Saturating cast back to u64
        // preserves ADR-0010's monotonicity at the rare ~584-year extreme
        // (where ns would exceed u64::MAX) instead of wrapping to zero.
        ticks_to_ns(count, self.frequency_hz)
    }

    fn arm_deadline(&self, _deadline_ns: u64) {
        // Deadline arming requires programming the generic-timer compare
        // registers (`CNTV_CVAL_EL0` / `CNTV_CTL_EL0`) **and** routing the
        // resulting IRQ via the GIC + interrupt-vector-table. T-009 scope
        // (phase-b.md §B0 item 5) explicitly excludes IRQ wiring; the
        // follow-up IRQ task will fill this method in. A silent no-op here
        // would make a future caller think the deadline was armed when it
        // was not, so this method panics loudly per unsafe-policy
        // §"unimplemented surfaces".
        unimplemented!(
            "QemuVirtCpu::arm_deadline requires GIC + IVT wiring (a future B0 follow-up task); \
             T-009 implements the measurement half of Timer only"
        );
    }

    fn cancel_deadline(&self) {
        // See `arm_deadline` above; cancellation only makes sense once
        // arming is wired. Deferred to the same follow-up task.
        unimplemented!(
            "QemuVirtCpu::cancel_deadline requires GIC + IVT wiring (a future B0 follow-up task); \
             T-009 implements the measurement half of Timer only"
        );
    }

    fn resolution_ns(&self) -> u64 {
        self.resolution_ns
    }
}
