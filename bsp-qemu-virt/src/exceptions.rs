//! Rust handlers called by the aarch64 vector trampolines in
//! `src/vectors.s`.
//!
//! Two entry points:
//!
//! - [`irq_entry`] — called from the IRQ trampoline at vector
//!   offset `+0x280` (Current EL with `SP_ELx`). Acknowledges the
//!   pending IRQ at the GIC, dispatches recognised IRQs to their
//!   handlers, signals end-of-interrupt, and returns to the
//!   trampoline which `eret`s back to the interrupted code.
//! - [`panic_entry`] — called from any of the unhandled-class
//!   trampolines (sync, FIQ, `SError` on any mode; IRQ on a
//!   non-`curr_el_spx` mode). Does not return — diverges into
//!   [`panic!`].
//!
//! See [`docs/architecture/exceptions.md`] §"Dispatch flow" for the
//! design. UNSAFE-2026-0020 covers the vector-table install + the
//! trampolines; the Rust functions here run with the GIC's CPU
//! interface in "active" state for the duration of `irq_entry`.
//!
//! [`docs/architecture/exceptions.md`]: https://github.com/cemililik/Tyrne/blob/main/docs/architecture/exceptions.md

use core::arch::asm;
use core::sync::atomic::{compiler_fence, Ordering};

use tyrne_hal::IrqController;

use crate::gic::QemuVirtGic;
use crate::GIC;

/// PPI 27 — the EL1 virtual generic-timer interrupt on QEMU virt's
/// `GICv2`. Mirrors the constant in `cpu.rs::arm_deadline`; kept here
/// because the IRQ-dispatch dispatch table is the natural home for
/// recogniser constants.
const TIMER_IRQ_ID: u32 = 27;

/// Saved-register frame populated by the IRQ trampoline before it
/// branches into [`irq_entry`].
///
/// `#[repr(C)]` is mandatory — the field order and offsets must match
/// the asm `stp` sequence in `src/vectors.s` byte-for-byte. The frame
/// is 192 bytes total; SP alignment is preserved.
#[repr(C)]
#[derive(Debug)]
pub struct TrapFrame {
    /// `x0` and `x1` saved at frame offset 0x00.
    pub x0_x1: [u64; 2],
    /// `x2` and `x3` at offset 0x10.
    pub x2_x3: [u64; 2],
    /// `x4` and `x5` at offset 0x20.
    pub x4_x5: [u64; 2],
    /// `x6` and `x7` at offset 0x30.
    pub x6_x7: [u64; 2],
    /// `x8` and `x9` at offset 0x40.
    pub x8_x9: [u64; 2],
    /// `x10` and `x11` at offset 0x50.
    pub x10_x11: [u64; 2],
    /// `x12` and `x13` at offset 0x60.
    pub x12_x13: [u64; 2],
    /// `x14` and `x15` at offset 0x70.
    pub x14_x15: [u64; 2],
    /// `x16` and `x17` at offset 0x80.
    pub x16_x17: [u64; 2],
    /// `x18` and `x30` (lr) at offset 0x90.
    pub x18_lr: [u64; 2],
    /// `ELR_EL1` (return address) and `SPSR_EL1` (saved PSTATE) at offset 0xA0.
    pub elr_spsr: [u64; 2],
    /// Padding — keeps the frame at 192 bytes total (16-byte SP-aligned).
    pub _reserved: [u64; 2],
}

// The trampoline in `vectors.s` reserves exactly 192 bytes of stack
// for the frame and writes through fixed offsets that mirror the
// field order above. A drift between the asm and the Rust `repr(C)`
// would corrupt saved registers on every IRQ; this compile-time guard
// fails the build before that can ship.
const _: () = assert!(core::mem::size_of::<TrapFrame>() == 192);

/// Class encoding passed by the unhandled-trampoline to [`panic_entry`].
///
/// The asm trampolines pass a small integer constant rather than
/// decoding the exception class fully — the goal is to surface
/// "something hit an unhandled vector" loudly, not to provide rich
/// diagnostics in v1.
#[repr(u64)]
enum PanicClass {
    /// Sync/FIQ/SError on any mode — generic "unhandled exception".
    Generic = 0,
    /// IRQ on `curr_el_sp0` or any lower-EL mode (i.e. not
    /// `curr_el_spx`). v1 has no userspace and runs with `SPSel = 1`,
    /// so an IRQ outside `curr_el_spx` indicates kernel-state corruption.
    UnhandledIrqMode = 1,
}

impl PanicClass {
    fn from_u64(raw: u64) -> Self {
        // Class id 1 is the only non-default; everything else (0 or
        // any unexpected value) maps to `Generic`. Unexpected values
        // are themselves a corruption signal and the raw class id is
        // preserved in the panic message at the call site.
        if raw == 1 {
            Self::UnhandledIrqMode
        } else {
            Self::Generic
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Generic => "unhandled-class exception",
            Self::UnhandledIrqMode => "IRQ taken from unsupported mode (not curr_el_spx)",
        }
    }
}

/// IRQ-dispatch entry point.
///
/// Called by the asm trampoline at vector offset `+0x280` after the
/// caller-saved register frame is on the stack. The frame pointer is
/// the trampoline's `sp` at the time of the `bl irq_entry`; the Rust
/// function may read or modify the frame to alter return-PSTATE if
/// needed (v1 does not).
///
/// # Safety
///
/// `frame` is guaranteed valid by the trampoline (constructed via
/// `stp` immediately before the `bl`); the function dereferences it
/// only inside `unsafe` blocks. The function returns to the
/// trampoline normally — the asm does the `eret`.
///
/// **Why `unsafe` is required:** the function is `extern "C"` so the
/// trampoline can `bl` it; the AAPCS64 contract is upheld by the
/// trampoline's stack-frame discipline. **Invariants upheld:** the
/// function does not modify the frame above `sp` after return; it
/// touches `GIC` only via the `IrqController` trait; it does not
/// take any momentary `&mut Scheduler<C>` because v1's only IRQ
/// (the future timer) does not need scheduler-state mutation.
/// **Rejected alternatives:** routing all IRQs through a Rust
/// `extern "C" fn(usize)` with a numeric class would lose typed
/// dispatch; the typed `IrqController::acknowledge` return value
/// is the safer default.
///
/// Audit: UNSAFE-2026-0020.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn irq_entry(_frame: *mut TrapFrame) {
    // SAFETY: producing a stable `&QemuVirtGic` from the static
    // `MaybeUninit<QemuVirtGic>` in `GIC` requires `unsafe` because Rust
    // cannot statically prove the cell is initialised in IRQ context.
    // Invariants relied on: (a) `kernel_entry` writes the
    // `MaybeUninit` cell *before* unmasking `DAIF.I`, so an IRQ cannot
    // arrive while it is uninit; (b) `GIC` is never moved or
    // reinitialised after that point — the `StaticCell` wrapper in
    // `main.rs` is set-once; (c) no kernel path takes a `&mut
    // QemuVirtGic` aliasing this `&` for the lifetime of the ISR, so
    // `assume_init_ref` produces a reference that is valid and
    // exclusive-of-`&mut` for the duration of `irq_entry`. Rejected
    // alternatives: a `Mutex<QemuVirtGic>` would block the ISR on a
    // contention path that v1 cannot tolerate (single-CPU, IRQ taken
    // with `DAIF.I` masked, no sleep primitive); a `RefCell` is
    // `!Sync` and unusable from a static; an `Option<QemuVirtGic>`
    // wrapped in a `OnceCell`-style guard would still need `unsafe`
    // for the static-mut access pattern Tyrne uses (heap-free, no
    // global allocator), giving identical safety surface for more
    // boilerplate. Audit: UNSAFE-2026-0020.
    let gic: &QemuVirtGic = unsafe { (*GIC.0.get()).assume_init_ref() };

    // Acknowledge the top-pending IRQ. The GIC marks it active and
    // returns its ID; spurious returns yield `None`.
    let Some(irq) = gic.acknowledge() else {
        // Spurious — nothing to dispatch and nothing to EOI per the
        // GICv2 architecture spec (acknowledging a spurious read does
        // not require a paired EOI write).
        compiler_fence(Ordering::SeqCst);
        return;
    };

    // Dispatch on IRQ ID.
    if irq.0 == TIMER_IRQ_ID {
        // EL1 virtual generic timer (PPI 27).
        //
        // Mask the timer at the source so the same deadline does not
        // re-fire before the next `arm_deadline` re-arm. We write
        // `CNTV_CTL_EL0 = 0b10` (ENABLE = 0, IMASK = 1).
        //
        // SAFETY: writing a system register via `msr` is `unsafe`
        // because the compiler cannot reason about its side effects on
        // CPU state. Invariants relied on: (a) `CNTV_CTL_EL0` is the
        // architected EL1 virtual timer control register and is
        // accessible unconditionally at EL1 in the non-VHE
        // configuration Tyrne runs in (per ADR-0024 + UNSAFE-2026-0017
        // — the EL drop runs to completion before any IRQ is unmasked,
        // so we are guaranteed to be at EL1 here); (b) the literal
        // `2u64` is `IMASK = 1, ENABLE = 0`, which is the architected
        // "mask the source" encoding for the timer; (c) the write
        // touches no memory and clobbers no general-purpose register
        // outside the AAPCS64 caller-saved set, so `options(nostack,
        // nomem)` is correct. Rejected alternatives: a typed crate
        // wrapper such as `cortex_a`'s `CNTV_CTL_EL0::set` would still
        // expand to the same `msr` and would still require `unsafe` at
        // the call site (system-register writes are intrinsically
        // unsafe under any wrapper); pulling in another HAL crate for
        // a single ISR-hot register write costs build-graph footprint
        // for no safety gain. Audit: UNSAFE-2026-0021.
        unsafe {
            asm!(
                "msr cntv_ctl_el0, {}",
                in(reg) 2u64,
                options(nostack, nomem),
            );
        }
        // Signal end-of-interrupt to the GIC. v1 has no scheduler-
        // side wake-on-deadline path yet (the timer is wired but no
        // caller arms it in the cooperative IPC demo); future tasks
        // that need preemption / `time_sleep_until` will add a
        // `sched::on_timer_irq` hook here.
        gic.end_of_interrupt(irq);
        return;
    }

    // Any other IRQ — v1's GIC enables only the timer line, so this
    // path is structurally unreachable. If we reach it, kernel state
    // has been corrupted upstream; surface loudly. Acknowledge before
    // panicking so the GIC line stays consistent if a future panic-
    // handler attempts recovery (none currently does).
    let unhandled = irq.0;
    gic.end_of_interrupt(irq);
    panic!("irq_entry: unhandled IRQ {unhandled}");
}

/// Panic entry point for unhandled exception classes.
///
/// Called by the unhandled-class trampolines with a small integer
/// `class` and the value of `ESR_EL1` at exception-entry time.
/// Diverges into [`panic!`] — the panic handler in `main.rs` writes
/// a marker line to the Pl011 console and halts.
///
/// # Safety
///
/// Same `extern "C"` ABI as [`irq_entry`]. `class` is a small integer
/// constant set by the trampoline; `esr` is the raw `ESR_EL1` value.
/// The function is `-> !` so the trampoline's post-`bl` code never
/// runs (the `wfe; b 1b` halt loop in the trampoline is defensive
/// against an over-eager linker).
///
/// **Why `unsafe` is required:** `extern "C"` ABI for asm callers.
/// **Invariants upheld:** the function does not return; it does not
/// touch `GIC` or any kernel statics that may be in inconsistent
/// state (a sync exception during `cap_take`, say, leaves the cap
/// table mid-transition; the panic handler does not trust that
/// state). **Rejected alternatives:** decoding `ESR_EL1` into a
/// Rust enum here would add complexity without changing the v1
/// behaviour (always panic). Future work may decode for richer
/// diagnostics.
///
/// Audit: UNSAFE-2026-0020.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn panic_entry(class: u64, esr: u64) -> ! {
    let class_str = PanicClass::from_u64(class).as_str();
    panic!("tyrne: {class_str}; ESR_EL1 = {esr:#018x}; class id = {class}",);
}
