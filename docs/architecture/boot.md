# Boot flow

Tyrne boots in four stages: QEMU (or the board firmware) hands control to the ELF entry point, a short assembly stub sets up the runtime environment, a Rust entry function (`kernel_entry`) wires the BSP together, and the portable `tyrne_kernel::run` function takes over. This document is the "how" for Phase 4c on `bsp-qemu-virt`; the "why" for each concrete choice lives in [ADR-0012](../decisions/0012-boot-flow-qemu-virt.md). Each future BSP will follow the same stage structure with its own addresses and peripherals.

## Context

The overall three-layer architecture is described in [`overview.md`](overview.md), and the HAL traits the kernel uses are in [`hal.md`](hal.md). This document focuses specifically on the boot path from reset to `kernel_main` steady state, as implemented for the QEMU `virt` aarch64 target.

## Design

### Stages

The four boot stages, each with a tightly bounded responsibility:

1. **Firmware / loader.** QEMU's `-kernel` flag loads the ELF image at its linked-in load address (`0x40080000` per [ADR-0012](../decisions/0012-boot-flow-qemu-virt.md)), sets the PC to the ELF's entry point (`_start`), and enters at EL1 (default QEMU `virt`) or EL2 (`-machine virtualization=on`, or most real-hardware boot stacks delivering at EL2). The device-tree blob address is placed in `x0`; v1 ignores it.
2. **Assembly stub (`_start`).** Three phases: first, K3-12 (interrupts masked via `MSR DAIFSet, #0xf`) executes at the very head of the reset vector so a spurious interrupt cannot escape into an uninstalled vector table. Second, the EL drop (per [ADR-0024](../decisions/0024-el-drop-policy.md)) reads `CurrentEL`; on EL2 it configures `HCR_EL2` / `SPSR_EL2` / `ELR_EL2` and `eret`s to a post-drop label, on EL1 it falls through, on EL3 (or any unexpected EL) it halts — there is no Rust panic infrastructure pre-`kernel_entry`. Third, the conventional setup: load `__stack_top` into `SP`, enable FP/SIMD via `CPACR_EL1`, zero the BSS range (`__bss_start` .. `__bss_end`) using 8-byte stores, and branch to `kernel_entry`. If `kernel_entry` ever returns (it shouldn't), the stub falls into a `wfe; b .-` halt loop. After phase two, every later instruction runs at EL1 — the precondition T-009's `UNSAFE-2026-0016` runtime check now relies on as a load-bearing invariant rather than a defensive guard.
3. **`kernel_entry` (Rust, in the BSP).** The first Rust code to run. Constructs the BSP's concrete HAL instances (for Phase 4c: the `Pl011Uart` console), then calls the portable [`tyrne_kernel::run`](../../kernel/src/lib.rs) with the console handle. Marked `#[no_mangle] extern "C"` so the assembly stub can find it.
4. **`tyrne_kernel::run` (portable kernel).** Architecture- and board-agnostic. In Phase 4c v0.0.1 it writes a greeting to the console and halts with a `spin_loop` idle. Subsequent phases will bring up the scheduler, IPC, and capability system here before reaching steady state.

### Boot-time sequence

```mermaid
sequenceDiagram
    participant QEMU as QEMU virt
    participant Asm as _start (asm stub)
    participant KE as kernel_entry (BSP, Rust)
    participant K as tyrne_kernel::run
    participant U as PL011 UART

    QEMU->>Asm: PC = _start, DTB in x0 (ignored)
    Asm->>Asm: SP ← __stack_top
    Asm->>Asm: BSS zeroed (__bss_start..__bss_end)
    Asm->>KE: bl kernel_entry
    KE->>KE: construct Pl011Uart at 0x0900_0000
    KE->>K: call run(&console)
    K->>U: write_bytes(b"tyrne: hello from kernel_main\n")
    K->>K: spin_loop() idle
    Note over K: steady state (v0.0.1)
```

### Memory map at boot

The kernel image is a single contiguous block starting at `0x40080000`; RAM below that is reserved for QEMU's internal use. The initial stack is a 64 KiB region reserved at the image's tail.

```
0x4000_0000  ─── RAM start (reserved for QEMU firmware region)
             ...
0x4008_0000  ─── _start (.text.boot) ← ELF entry
             .text
             .rodata
             .data
             .bss              (zeroed by _start)
             [reserved 64 KiB] (initial stack region)
__stack_top  ─── high end of stack
             ...
0x4800_0000  ─── end of 128 MiB RAM region
```

- **Code and read-only data** (`.text`, `.rodata`) are loaded at their linked addresses.
- **Initialized data** (`.data`) is loaded from the ELF.
- **BSS** is zeroed in `_start` before Rust executes, so all `static` items in safe Rust see their declared initial values (zero for BSS-resident statics).
- **Stack** grows downward from `__stack_top`. Nothing enforces that it does not grow into `.bss` — stack overflow is undefined behaviour in v1. Guard pages arrive with MMU setup.

### What `_start` does, line-by-line

```asm
.section .text.boot, "ax"
.global _start
_start:
    /* (1) K3-12: mask DAIF before anything else. */
    msr     daifset, #0xf

    /* (2) EL drop per ADR-0024. Read CurrentEL; mask bits[3:2]. */
    mrs     x0, CurrentEL
    and     x0, x0, #(3 << 2)
    cmp     x0, #(2 << 2)
    b.eq    el2_to_el1                ; EL2 → drop to EL1
    cmp     x0, #(1 << 2)
    b.eq    post_eret                 ; already at EL1 → skip drop
halt_unsupported_el:                  ; EL3 (or anything else) → halt
    wfe
    b       halt_unsupported_el

el2_to_el1:
    mov     x0, #(1 << 31)            ; HCR_EL2.RW = 1 (EL1 = aarch64); E2H/TGE = 0 (non-VHE)
    msr     hcr_el2, x0
    mov     x0, #0x3c5                ; SPSR_EL2 = EL1h | DAIF masked
    msr     spsr_el2, x0
    adrp    x0, post_eret
    add     x0, x0, :lo12:post_eret
    msr     elr_el2, x0
    eret

post_eret:
    /* (3) Conventional setup. From here on, EL is guaranteed = 1. */
    adrp    x0, __stack_top           ; page-aligned base of the symbol
    add     x0, x0, :lo12:__stack_top ; add the low 12 bits
    mov     sp, x0                    ; set SP

    mov     x0, #0x300000             ; CPACR_EL1.FPEN = 0b11
    msr     cpacr_el1, x0
    isb

    adrp    x0, __bss_start
    add     x0, x0, :lo12:__bss_start
    adrp    x1, __bss_end
    add     x1, x1, :lo12:__bss_end
0:  cmp     x0, x1
    b.hs    1f
    str     xzr, [x0], #8
    b       0b

1:  bl      kernel_entry              ; hand off to Rust
2:  wfe                               ; defensive halt if we return
    b       2b
```

`adrp + add` with `:lo12:` is the standard aarch64 idiom for "address of symbol" — PC-relative, handles any static layout the linker picks. `str xzr, [x0], #8` stores the zero register with post-increment. `eret` consumes `SPSR_EL2`'s mode + DAIF + register state and `ELR_EL2`'s target address: after the instruction the CPU runs at EL1 with DAIF still masked (the K3-12 mask propagates via `SPSR_EL2`'s DAIF bits, so no second `msr daifset` is needed at `post_eret`). The full safety argument lives in [`UNSAFE-2026-0017`](../audits/unsafe-log.md).

### Linker script responsibilities

[`bsp-qemu-virt/linker.ld`](../../bsp-qemu-virt/linker.ld) pins the above memory map:

- `ENTRY(_start)` — the ELF's `e_entry` is set to `_start`'s address.
- `MEMORY` — a single `RAM` region: `ORIGIN = 0x40080000, LENGTH = 128M`.
- `.text` starts with `KEEP(*(.text.boot))`, guaranteeing `_start` is at `0x40080000`.
- `.bss` is 8-byte aligned at both ends so the BSS-zero loop can step by 8.
- A 64 KiB stack region is reserved after `.bss`; `__stack_top` names its high end.
- `/DISCARD/` drops `.comment`, `.note.*`, `.eh_frame*`, and `.gcc_except_table*` — unwinding tables are dead weight under [`panic=abort`](../standards/error-handling.md).

### Panic path

When `tyrne_kernel::run` or any later kernel code panics, control reaches the BSP's `#[panic_handler]` function. In Phase 4c, that handler:

1. Reconstructs the `Pl011Uart` (the original instance may not be reachable from the panic context).
2. Writes a short marker (`"\n!! tyrne panic !!\n"`).
3. Writes the panic message using `FmtWriter` adapted onto the `Console`.
4. Halts in a `spin_loop` that never returns.

This is the minimum useful panic reporting. Future revisions will add core id, register state, and a backtrace — each requires additional infrastructure that is not in v1.

## Invariants

Properties the boot flow maintains. These are the claims a reader can rely on and a test can exercise.

- **Entry is deterministic.** `_start` always runs the same sequence of instructions on the same input.
- **Interrupts are masked from the very first instruction.** K3-12: `MSR DAIFSet, #0xf` is the literal first instruction at `_start`. The mask carries through the EL drop via `SPSR_EL2`'s DAIF bits, so it is still in effect at `kernel_entry`. Tasks unmask explicitly via `Cpu::restore_irq_state(IrqState(0))` when they need interrupts.
- **`kernel_entry` runs at EL1 unconditionally.** Per [ADR-0024](../decisions/0024-el-drop-policy.md): if the BSP is delivered at EL2, `_start`'s drop sequence transitions to non-VHE EL1; if delivered at EL1, the drop is a no-op; if delivered at EL3 (no v1 hardware target does), `_start` halts loudly. T-009's `UNSAFE-2026-0016` runtime check inside `QemuVirtCpu::new` is the post-condition that pins this.
- **The stack is set before any Rust code runs.** No Rust code executes with an undefined `SP`.
- **BSS is zero when Rust sees it.** All `static` items in safe Rust have their declared initial values.
- **`kernel_entry` never runs more than once.** There is only one boot CPU in v1; it calls `kernel_entry` once.
- **`kernel_entry` never returns to the asm stub.** It is `-> !`; a return would be a bug and is defensively halted by the stub.
- **Hardware MMIO addresses are hardcoded.** No runtime discovery. BSP-specific; justified because `virt` is a fixed platform.
- **`panic=abort`, not unwind.** No unwinding tables in the binary; panics halt.

## Trade-offs

- **EL drop is `boot.s`-side, not kernel-side.** [ADR-0024 Option A](../decisions/0024-el-drop-policy.md) — the kernel reasons about exactly one EL (EL1, non-VHE) and `boot.s` does the work of getting there. The alternative (multi-EL kernel code) was rejected because the maintenance tax compounds across every later HAL impl. The cost is ~30 lines of asm in `_start`.
- **DTB ignored.** Convenient now; will need explicit parsing when the first board with runtime topology (Pi 4) lands.
- **Stack is a fixed 64 KiB with no guard page.** Overflow is UB. Good enough for v1; per-task stacks with guards come with the scheduler.
- **`_start` is hand-written assembly.** Every BSP will have its own. A shared-boot library would force premature commonality; we accept the duplication to keep each BSP's boot transparent.
- **Hardcoded UART base.** `0x0900_0000` is QEMU `virt` specific. Each BSP carries its own constants; the trade is deliberate (see [P6 — HAL separation](../standards/architectural-principles.md#p6--hal-separation)).

## Open questions

- **EL3 → EL2 → EL1 chain.** v1 hardware targets do not boot at EL3; if a future BSP requires it, a follow-up task adds the EL3→EL2 transition on top of the existing EL2→EL1 logic per ADR-0024 §Open questions.
- **DTB parsing and `BootInfo`.** The kernel's typed boot-info contract, probably introduced with Pi 4 support.
- **Multi-core start.** PSCI `CPU_ON` for secondary cores.
- **MMU activation at boot.** Currently MMU-off; the linker script may need adjustments when the kernel-half mapping is introduced.
- **Guard-page stacks.** Dependent on MMU activation.
- **Measured boot / attestation.** Hardware-dependent; deferred per [ADR-0012](../decisions/0012-boot-flow-qemu-virt.md).

## References

- [ADR-0012: Boot flow and memory layout for `bsp-qemu-virt`](../decisions/0012-boot-flow-qemu-virt.md).
- [ADR-0024: EL drop to EL1 policy](../decisions/0024-el-drop-policy.md).
- [ADR-0004: Target platforms](../decisions/0004-target-platforms.md).
- [ADR-0006: Workspace layout](../decisions/0006-workspace-layout.md).
- [`hal.md`](hal.md) — the HAL traits the BSP implements.
- [`overview.md`](overview.md) — three-layer architecture.
- [`docs/guides/run-under-qemu.md`](../guides/run-under-qemu.md) — how to actually run the kernel.
- QEMU `virt` machine documentation — https://qemu.readthedocs.io/en/latest/system/arm/virt.html
- ARM *Architecture Reference Manual* (ARMv8-A) — `adrp` / `ERET` / EL semantics.
- PL011 UART documentation — for the console implementation.
