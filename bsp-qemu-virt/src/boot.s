/*
 * QEMU virt aarch64 reset entry.
 *
 * See docs/decisions/0012-boot-flow-qemu-virt.md and
 * docs/architecture/boot.md for the design.
 *
 * Responsibilities, in order:
 *   1. Set the stack pointer to __stack_top (from linker.ld).
 *   2. Zero the BSS region [__bss_start, __bss_end), which is 8-byte
 *      aligned at both ends so 8-byte stores are safe.
 *   3. Branch to kernel_entry (a Rust function marked extern "C").
 *   4. If kernel_entry ever returns (it should not), halt defensively.
 *
 * The Exception Level is whatever QEMU hands us at entry; no EL
 * manipulation here (per ADR-0012 v1). The DTB pointer in x0 is
 * currently ignored.
 */

    .section .text.boot, "ax"
    .global _start

_start:
    adrp    x0, __stack_top
    add     x0, x0, :lo12:__stack_top
    mov     sp, x0

    adrp    x0, __bss_start
    add     x0, x0, :lo12:__bss_start
    adrp    x1, __bss_end
    add     x1, x1, :lo12:__bss_end
0:
    cmp     x0, x1
    b.hs    1f
    str     xzr, [x0], #8
    b       0b

1:
    bl      kernel_entry

2:
    wfe
    b       2b
