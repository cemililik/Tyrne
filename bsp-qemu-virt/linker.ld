/*
 * Linker script for tyrne-bsp-qemu-virt.
 *
 * See docs/decisions/0012-boot-flow-qemu-virt.md for the memory layout
 * rationale. In short: QEMU virt loads us at 0x40080000 via -kernel;
 * _start must be first in .text; BSS is 8-byte aligned so the BSS-zero
 * loop can use 8-byte stores; 64 KiB stack is reserved after .bss and
 * named __stack_top at its high end.
 *
 * T-016 (ADR-0027) adds the .boot_pt reservation: 4 × 4 KiB page-aligned
 * frames bracketed by __boot_pt_start / __boot_pt_end and individually
 * named __boot_pt_l0 / __boot_pt_l1 / __boot_pt_l2_low / __boot_pt_l2_high
 * for the bootstrap routine to populate. Placed inside the .bss range so
 * the existing BSS-zero loop in boot.s pre-zeros all four frames before
 * mmu_bootstrap runs.
 */

ENTRY(_start)

MEMORY {
    RAM (rwx) : ORIGIN = 0x40080000, LENGTH = 128M
}

SECTIONS {
    . = ORIGIN(RAM);

    .text : {
        KEEP(*(.text.boot))
        /* aarch64 EL1 exception vector table — VBAR_EL1 requires
         * 2 KiB (2048-byte) alignment per ARM ARM §D11.2. The
         * `.text.vectors` section holds the 16-entry table assembled
         * in `src/vectors.s`; KEEP prevents linker GC. */
        . = ALIGN(2048);
        KEEP(*(.text.vectors))
        *(.text .text.*)
    } > RAM

    .rodata : ALIGN(8) {
        *(.rodata .rodata.*)
    } > RAM

    .data : ALIGN(8) {
        *(.data .data.*)
    } > RAM

    .bss : ALIGN(8) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        /* Bootstrap page-table frames (T-016 / ADR-0027).
         *
         * Four 4 KiB-aligned frames live inside the .bss range so the
         * existing BSS-zero loop in boot.s pre-zeros them before
         * mmu_bootstrap populates the descriptors. Order matches the
         * VMSAv8 hierarchy walked by the bootstrap routine:
         *   __boot_pt_l0      — L0 root; entry [0] points at L1
         *   __boot_pt_l1      — L1; entry [0] → L2_low (MMIO range),
         *                              entry [1] → L2_high (RAM range)
         *   __boot_pt_l2_low  — L2 covering 0x0800_0000..0x0920_0000
         *                              (GIC + UART; 9 device blocks)
         *   __boot_pt_l2_high — L2 covering 0x4000_0000..0x4800_0000
         *                              (kernel + RAM; 64 normal blocks)
         */
        . = ALIGN(4096);
        __boot_pt_start = .;
        __boot_pt_l0 = .;
        . = . + 4096;
        __boot_pt_l1 = .;
        . = . + 4096;
        __boot_pt_l2_low = .;
        . = . + 4096;
        __boot_pt_l2_high = .;
        . = . + 4096;
        __boot_pt_end = .;
        . = ALIGN(8);
        __bss_end = .;
    } > RAM

    . = ALIGN(16);
    . = . + 64K;
    __stack_top = .;

    /DISCARD/ : {
        *(.comment)
        *(.note.*)
        *(.eh_frame*)
        *(.gcc_except_table*)
    }
}
