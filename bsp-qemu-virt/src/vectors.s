/*
 * aarch64 EL1 exception vector table for tyrne-bsp-qemu-virt.
 *
 * See docs/architecture/exceptions.md and ADR-0011 for the design.
 * UNSAFE-2026-0020 is the audit-log entry covering this file.
 *
 * Layout (per ARM ARM, DDI 0487G.b §D1.10): 16 entries × 0x80 bytes
 * each, 2 KiB total, 2 KiB-aligned. The CPU branches to the entry for
 * the relevant exception class; each entry has at most 32 instructions.
 *
 *   +0x000 sync   curr_el_sp0
 *   +0x080 IRQ    curr_el_sp0
 *   +0x100 FIQ    curr_el_sp0
 *   +0x180 SError curr_el_sp0
 *   +0x200 sync   curr_el_spx
 *   +0x280 IRQ    curr_el_spx   ← the only entry that fires in v1
 *   +0x300 FIQ    curr_el_spx
 *   +0x380 SError curr_el_spx
 *   +0x400 sync   lower_el_aarch64
 *   +0x480 IRQ    lower_el_aarch64
 *   +0x500 FIQ    lower_el_aarch64
 *   +0x580 SError lower_el_aarch64
 *   +0x600 sync   lower_el_aarch32
 *   +0x680 IRQ    lower_el_aarch32
 *   +0x700 FIQ    lower_el_aarch32
 *   +0x780 SError lower_el_aarch32
 *
 * Tyrne runs at EL1 with SPSel = 1 (per ADR-0024's EL drop +
 * SPSR_EL2 = 0x3c5 = EL1h). An IRQ taken from kernel code lands at
 * +0x280; userspace doesn't exist in v1 so the lower-EL entries are
 * unreachable. Sync/FIQ/SError on any class trampoline to a panic.
 *
 * Each entry is one `b <label>` instruction; the rest of the 32-slot
 * region pads with .balign back to the next entry boundary so the
 * symbols stay layout-correct against the CPU's expected offsets.
 */

    .section .text.vectors, "ax"
    .balign 2048
    .global tyrne_vectors

tyrne_vectors:
    // ── Current EL with SP_EL0 (unused; SPSel=1 in v1) ────────────────
    .balign 0x80
    b       tyrne_unhandled_exception_trampoline    // +0x000 sync
    .balign 0x80
    b       tyrne_unhandled_irq_trampoline          // +0x080 IRQ
    .balign 0x80
    b       tyrne_unhandled_exception_trampoline    // +0x100 FIQ
    .balign 0x80
    b       tyrne_unhandled_exception_trampoline    // +0x180 SError

    // ── Current EL with SP_ELx (the live category in v1) ──────────────
    .balign 0x80
    b       tyrne_unhandled_exception_trampoline    // +0x200 sync
    .balign 0x80
    b       tyrne_irq_curr_el_trampoline            // +0x280 IRQ ★
    .balign 0x80
    b       tyrne_unhandled_exception_trampoline    // +0x300 FIQ
    .balign 0x80
    b       tyrne_unhandled_exception_trampoline    // +0x380 SError

    // ── Lower EL using AArch64 (no userspace in v1) ──────────────────
    .balign 0x80
    b       tyrne_unhandled_exception_trampoline    // +0x400 sync
    .balign 0x80
    b       tyrne_unhandled_irq_trampoline          // +0x480 IRQ
    .balign 0x80
    b       tyrne_unhandled_exception_trampoline    // +0x500 FIQ
    .balign 0x80
    b       tyrne_unhandled_exception_trampoline    // +0x580 SError

    // ── Lower EL using AArch32 (no userspace, no aarch32 in v1) ──────
    .balign 0x80
    b       tyrne_unhandled_exception_trampoline    // +0x600 sync
    .balign 0x80
    b       tyrne_unhandled_irq_trampoline          // +0x680 IRQ
    .balign 0x80
    b       tyrne_unhandled_exception_trampoline    // +0x700 FIQ
    .balign 0x80
    b       tyrne_unhandled_exception_trampoline    // +0x780 SError
    .balign 0x80
    // (table ends at +0x800 — 2 KiB total)

/*
 * ── IRQ trampoline (curr_el_spx) ──────────────────────────────────────
 *
 * Save the AAPCS64 caller-saved GPRs (x0..x18) plus x30 (LR) plus
 * ELR_EL1 + SPSR_EL1 to the kernel stack, call irq_entry(&mut frame)
 * in Rust, then restore everything and ERET.
 *
 * Frame layout (192 bytes; 16-byte aligned):
 *   [sp + 0x00]  x0,  x1
 *   [sp + 0x10]  x2,  x3
 *   [sp + 0x20]  x4,  x5
 *   [sp + 0x30]  x6,  x7
 *   [sp + 0x40]  x8,  x9
 *   [sp + 0x50]  x10, x11
 *   [sp + 0x60]  x12, x13
 *   [sp + 0x70]  x14, x15
 *   [sp + 0x80]  x16, x17
 *   [sp + 0x90]  x18, x30 (lr)
 *   [sp + 0xA0]  ELR_EL1, SPSR_EL1
 *   [sp + 0xB0]  reserved (alignment to 192 = 0xC0)
 *
 * Rust function signature:
 *   #[no_mangle] extern "C" fn irq_entry(frame: *mut TrapFrame);
 *
 * AAPCS64 callee-saved registers (x19..x28, x29) survive the bl
 * naturally — Rust preserves them across function-call boundary, so
 * the trampoline doesn't need to save them. (If a future preemption
 * scheme needs the full register set, the trampoline grows.)
 */
// The 192-byte stack carve-out and the per-`stp` byte offsets below mirror
// the Rust-side `TrapFrame` `#[repr(C)]` definition; a size or layout drift
// would corrupt every saved register on every IRQ. The compile-time guard
// `const _: () = assert!(core::mem::size_of::<TrapFrame>() == 192)` next to
// the `TrapFrame` definition in `bsp-qemu-virt/src/exceptions.rs` fails the
// build before the drift can reach an IRQ. Update both sides together if
// the frame ever grows.
tyrne_irq_curr_el_trampoline:
    sub     sp, sp, #192
    stp     x0,  x1,  [sp, #0x00]
    stp     x2,  x3,  [sp, #0x10]
    stp     x4,  x5,  [sp, #0x20]
    stp     x6,  x7,  [sp, #0x30]
    stp     x8,  x9,  [sp, #0x40]
    stp     x10, x11, [sp, #0x50]
    stp     x12, x13, [sp, #0x60]
    stp     x14, x15, [sp, #0x70]
    stp     x16, x17, [sp, #0x80]
    stp     x18, x30, [sp, #0x90]
    mrs     x0,  elr_el1
    mrs     x1,  spsr_el1
    stp     x0,  x1,  [sp, #0xA0]

    mov     x0, sp
    bl      irq_entry

    ldp     x0,  x1,  [sp, #0xA0]
    msr     elr_el1, x0
    msr     spsr_el1, x1
    ldp     x18, x30, [sp, #0x90]
    ldp     x16, x17, [sp, #0x80]
    ldp     x14, x15, [sp, #0x70]
    ldp     x12, x13, [sp, #0x60]
    ldp     x10, x11, [sp, #0x50]
    ldp     x8,  x9,  [sp, #0x40]
    ldp     x6,  x7,  [sp, #0x30]
    ldp     x4,  x5,  [sp, #0x20]
    ldp     x2,  x3,  [sp, #0x10]
    ldp     x0,  x1,  [sp, #0x00]
    add     sp, sp, #192
    eret

/*
 * ── Unhandled-exception trampoline (sync/FIQ/SError, any class) ──────
 *
 * Save a minimal frame and call panic_entry. Does not return (panics).
 * Rust signature:
 *   #[no_mangle] extern "C" fn panic_entry(class: u64, esr: u64) -> !;
 *
 * `class` is a fixed integer encoding (0=sync, 1=fiq, 2=serror); the
 * trampolines use a simple constant.
 *
 * `esr` is read from ESR_EL1; helps narrow down "what went wrong" in
 * panic output.
 */
tyrne_unhandled_exception_trampoline:
    // Reserve 16 bytes for SP alignment; we don't return.
    sub     sp, sp, #16
    mov     x0, #0                    // class = 0 (sync/FIQ/SError, generic)
    mrs     x1, esr_el1
    bl      panic_entry
    // panic_entry never returns — defensively halt.
1:  wfe
    b       1b

/*
 * ── Unhandled-IRQ trampoline (lower-EL or curr_el_sp0; should not
 *    fire in v1) ────────────────────────────────────────────────────
 *
 * v1 has no userspace and runs with SPSel=1, so an IRQ taken from
 * SP_EL0 mode or from a lower EL is a kernel-state corruption signal.
 * Halt loudly.
 */
tyrne_unhandled_irq_trampoline:
    sub     sp, sp, #16
    mov     x0, #1                    // class = 1 (unhandled IRQ outside curr_el_spx)
    mrs     x1, esr_el1
    bl      panic_entry
1:  wfe
    b       1b

/*
 * ── Trampoline-end markers ────────────────────────────────────────────
 *
 * Used by the linker (and by the audit log) to bound the vector
 * region. `tyrne_vectors_end` is one past the last byte of the
 * trampolines; `linker.ld` does not currently use these symbols but
 * having them makes the section bounds easy to verify in objdump.
 */
    .global tyrne_vectors_end
tyrne_vectors_end:
