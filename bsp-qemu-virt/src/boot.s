/*
 * QEMU virt aarch64 reset entry.
 *
 * See docs/decisions/0012-boot-flow-qemu-virt.md, ADR-0024 (EL drop
 * to EL1 policy), and docs/architecture/boot.md for the design.
 *
 * Responsibilities, in order:
 *   1. K3-12: mask all interrupts (DAIF) at the very head of _start
 *      so the reset vector cannot accidentally take an interrupt
 *      before the kernel installs a vector table. (BSP boot-checklist
 *      rule; per ADR-0024 dependency chain item.)
 *   2. EL drop to EL1 per ADR-0024:
 *        - If CurrentEL is EL2 (e.g. -machine virtualization=on, or
 *          most real-hardware boot stacks delivering at EL2),
 *          configure HCR_EL2 (RW=1 so EL1 runs aarch64; E2H=0,
 *          TGE=0 — explicitly non-VHE per ADR-0024), set SPSR_EL2 =
 *          EL1h with DAIF masked, set ELR_EL2 to the post_eret label,
 *          and ERET.
 *        - If CurrentEL is EL1 (QEMU virt's default), skip the drop.
 *        - If CurrentEL is EL3 (or any other unexpected value), halt
 *          via `wfe; b .-`. v1 has no EL3-aware infrastructure;
 *          ADR-0024 §Open questions tracks the future EL3→EL2→EL1
 *          chain for hardware that requires it.
 *   3. Set the stack pointer to __stack_top (from linker.ld).
 *   4. Enable FP/SIMD at EL1: set CPACR_EL1.FPEN = 0b11 so that the
 *      compiler-generated NEON instructions (e.g. movi/stp q-regs for
 *      zero-initialisation) do not trap as Undefined Instruction.
 *   5. Zero the BSS region [__bss_start, __bss_end), which is 8-byte
 *      aligned at both ends so 8-byte stores are safe.
 *   6. Branch to kernel_entry (a Rust function marked extern "C").
 *   7. If kernel_entry ever returns (it should not), halt defensively.
 *
 * After step 2, every later instruction runs at EL1 regardless of
 * the entry EL — the precondition T-009's UNSAFE-2026-0016 runtime
 * `CurrentEL == 1` self-check in QemuVirtCpu::new now relies on as a
 * load-bearing invariant rather than a defensive guard.
 *
 * Audit: UNSAFE-2026-0017.
 */

    .section .text.boot, "ax"
    .global _start

_start:
    /* (1) K3-12: mask DAIF immediately at reset. */
    msr     daifset, #0xf

    /* (2a) Read CurrentEL into x0; mask to bits[3:2] (the EL field). */
    mrs     x0, CurrentEL
    and     x0, x0, #(3 << 2)

    /* (2b) Dispatch on the EL field. */
    cmp     x0, #(2 << 2)
    b.eq    el2_to_el1
    cmp     x0, #(1 << 2)
    b.eq    post_eret              // already at EL1; skip the drop
    /* Anything else (EL3 today; EL0 cannot happen at reset) — halt
     * loudly. Pre-Rust panic infrastructure is unavailable here. */
halt_unsupported_el:
    wfe
    b       halt_unsupported_el

el2_to_el1:
    /* HCR_EL2 controls whether EL1 runs as aarch64 and what gets
     * routed to EL2. Tyrne wants:
     *   RW    = 1  (bit 31)  — EL1 executes aarch64
     *   E2H   = 0  (bit 34)  — non-VHE
     *   TGE   = 0  (bit 27)  — do not trap EL1 traps to EL2
     *   FMO/IMO/AMO = 0       — leave IRQ/FIQ/SError routing at EL1
     * Everything else stays cleared. The reset value is implementation-
     * defined; we set the entire register to a known shape rather than
     * trusting reset state. Per ADR-0024 §Decision outcome,
     * E2H = TGE = 0 is the explicit non-VHE configuration the kernel
     * documented in T-009's UNSAFE-2026-0015 Amendment ("non-VHE EL1"). */
    mov     x0, #(1 << 31)
    msr     hcr_el2, x0

    /* SPSR_EL2 = saved program status the ERET will install at EL1.
     *   M[3:0] = 0b0101  (EL1h — EL1 with sp_el1)
     *   F (bit 6) = 1    (FIQ masked)
     *   I (bit 7) = 1    (IRQ masked)
     *   A (bit 8) = 1    (SError masked)
     *   D (bit 9) = 1    (Debug masked)
     * Total: 0x3c5. The DAIF bits in SPSR_EL2 propagate to PSTATE
     * after the ERET; no second `msr daifset` is needed at EL1. */
    mov     x0, #0x3c5
    msr     spsr_el2, x0

    /* ELR_EL2 = the address ERET should land at (post_eret label).
     * adrp + add :lo12: gives a PC-relative address with full 4 KiB
     * page resolution — works regardless of where the kernel image
     * is loaded. */
    adrp    x0, post_eret
    add     x0, x0, :lo12:post_eret
    msr     elr_el2, x0

    eret

post_eret:
    /* Now at EL1 regardless of where boot started. From this point
     * down, every instruction matches the pre-T-013 boot path. */

    /* (3) Set the stack pointer. */
    adrp    x0, __stack_top
    add     x0, x0, :lo12:__stack_top
    mov     sp, x0

    /* (4) Enable FP/SIMD at EL1 and EL0 (CPACR_EL1.FPEN = 0b11 = bits[21:20]).
     * 0x300000 = 3 << 20.  We do not rely on the reset value of CPACR_EL1;
     * we write FPEN = 0b11 explicitly and leave all other fields zero (no
     * ZEN, no TTA traps).  ISB ensures the write takes effect before the
     * first NEON instruction in BSS zeroing or Rust code. */
    mov     x0, #0x300000
    msr     cpacr_el1, x0
    isb

    /* (5) Zero BSS. */
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
    /* (6) Hand control to Rust. */
    bl      kernel_entry

2:
    /* (7) Defensive halt if kernel_entry ever returns. */
    wfe
    b       2b
