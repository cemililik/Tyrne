---
name: add-bsp
description: Add a new Board Support Package (BSP) crate to the Tyrne workspace — from crate skeleton through boot checklist to first QEMU or hardware boot.
when-to-use: When adding support for a new hardware target (e.g. Raspberry Pi 4, a custom board) or when porting the kernel to a new QEMU machine type.
---

# Add BSP

## Inputs

Before starting, the agent must have:

- **Target name** — a short kebab-case identifier (e.g. `rpi4`, `qemu-virt`).
- **Architecture** — the Rust target triple (e.g. `aarch64-unknown-none`).
- **Boot EL** — the exception level the hardware or QEMU drops the kernel to (EL1, EL2, …). Verify this before writing any boot code; do not assume.
- **Peripheral map** — UART base address, any other peripherals needed for a boot console.
- **ADR for boot flow** — a new ADR covering reset-vector design and memory map for this target (model on [ADR-0012](../../../docs/decisions/0012-boot-flow-qemu-virt.md)).

If the boot EL or peripheral map is unknown, stop and ask the maintainer before proceeding.

## Procedure

### 1. Write the boot-flow ADR

Follow the [write-adr](../write-adr/SKILL.md) skill. The ADR must document:

- Which EL the target enters at.
- Stack address and linker symbol convention.
- Memory map (ROM/RAM ranges visible at reset).
- Any privileged setup required before Rust entry (EL transition, cache init, …).

Do not write boot code before this ADR is Accepted.

### 2. Create the crate skeleton

```
bsp-<target>/
  Cargo.toml
  build.rs          (if a linker script needs to be emitted)
  linker.ld
  src/
    boot.s          (reset vector — assembly only)
    main.rs         (kernel_entry and panic handler)
    console.rs      (Console impl for the target's UART)
    cpu.rs          (Cpu + ContextSwitch impl for the target)
```

`Cargo.toml` must set:
```toml
[package]
name    = "tyrne-bsp-<target>"
edition = "2021"

[[bin]]
name              = "tyrne-bsp-<target>"
path              = "src/main.rs"

[dependencies]
tyrne-hal    = { path = "../hal" }
tyrne-kernel = { path = "../kernel" }
```

Add the crate to `[workspace]` in the root `Cargo.toml`.

### 3. Work through the BSP boot checklist — in order

Read [`docs/standards/bsp-boot-checklist.md`](../../../docs/standards/bsp-boot-checklist.md) in full.
Execute each item before moving to the next. Do **not** assume any item is already satisfied on a new target:

| # | Item | Common mistake |
|---|------|----------------|
| 1 | Exception level confirmed | Assuming EL1 when hardware enters EL2 |
| 2 | CPACR_EL1.FPEN = 0b11 set in `boot.s` | Forgetting → NEON trap, silent hang |
| 3 | VBAR configured before enabling IRQs | Missing → any exception = silent hang |
| 4 | SP 16-byte aligned at first `bl` | Wrong linker alignment → AAPCS64 fault |
| 5 | BSS zeroed before `kernel_entry` | Uninitialised statics → subtle UB |
| 6 | Context-switch fn is `#[unsafe(naked)]` | Using `#[inline(never)]` → sp corruption |

### 4. Write `boot.s`

Minimum content (aarch64 EL1 example):

```asm
    .section .text.boot, "ax"
    .global _start
_start:
    /* 1. Set stack pointer */
    adrp    x0, __stack_top
    add     x0, x0, :lo12:__stack_top
    mov     sp, x0

    /* 2. Enable FP/SIMD — do not rely on CPACR_EL1 reset value */
    mov     x0, #0x300000       // FPEN = 0b11
    msr     cpacr_el1, x0
    isb

    /* 3. Zero BSS */
    adrp    x0, __bss_start
    add     x0, x0, :lo12:__bss_start
    adrp    x1, __bss_end
    add     x1, x1, :lo12:__bss_end
0:  cmp     x0, x1
    b.hs    1f
    str     xzr, [x0], #8
    b       0b
1:
    bl      kernel_entry
2:  wfe
    b       2b
```

Adjust for EL2→EL1 transition if the target enters at EL2 (add `msr hcr_el2, …` / `eret` sequence before step 1).

### 5. Implement `cpu.rs`

Copy the structure from [`bsp-qemu-virt/src/cpu.rs`](../../../bsp-qemu-virt/src/cpu.rs).

- Implement `Cpu` trait: `current_core_id`, `disable_irqs`, `restore_irq_state`, `wait_for_interrupt`, `instruction_barrier`.
- Implement `ContextSwitch` trait with `#[unsafe(naked)]` `context_switch_asm` that saves **all** AAPCS64 callee-saved registers: x19–x28, fp, lr, sp, **and d8–d15**.
- Use the same `Aarch64TaskContext` layout (168 bytes, repr(C)) and the same field offsets.
- Add every `unsafe` block to the audit log per the [justify-unsafe](../justify-unsafe/SKILL.md) skill.

### 6. Implement `console.rs`

Implement `tyrne_hal::Console` for the target UART. Follow the existing `Pl011Uart` implementation. Each UART model is different; check the datasheet for the FIFO-full flag and data-register offsets.

### 7. Wire `main.rs`

`kernel_entry` must:
1. Initialise the console and print the greeting (`tyrne: hello from kernel_main`).
2. Call `tyrne_kernel::sched` to register tasks and start the scheduler.
3. Never return (`-> !`).

Provide a `#[panic_handler]` that writes to the console and loops.

### 8. Add a linker script

Model on [`bsp-qemu-virt/linker.ld`](../../../bsp-qemu-virt/linker.ld). At minimum:

- Place `.text.boot` first so `_start` is at the load address.
- Align `__bss_start` and `__bss_end` to 8 bytes.
- Align `__stack_top` to 16 bytes.

### 9. Add a run script

Create `tools/run-<target>.sh`. For QEMU targets, model on [`tools/run-qemu.sh`](../../../tools/run-qemu.sh):

- Include the `--int-log` flag (`-d int -D /tmp/qemu_int.log`) for silent-hang debugging.
- Document the QEMU machine flags in the script header.

For real hardware, document the flashing command (e.g. `openocd`, `rpiboot`, `cargo flash`).

### 10. Verify the smoke test

Boot the kernel and confirm:

```
tyrne: hello from kernel_main
tyrne: starting cooperative scheduler
tyrne: task A — iteration 0
tyrne: task B — iteration 0
...
tyrne: task A done; spinning
```

If the kernel hangs silently, run with `--int-log` (QEMU) or attach a JTAG debugger (real hardware) and check for the following before anything else:

1. Exception log: `grep "Taking exception" /tmp/qemu_int.log`
2. CPACR_EL1 — is FPEN set? (ESR EC=0x07 = FP/SIMD trap)
3. SP misalignment — AAPCS64 stack-alignment fault?
4. BSS not zeroed — statics have garbage values?

See [`docs/standards/bsp-boot-checklist.md`](../../../docs/standards/bsp-boot-checklist.md) for the full diagnostic table.

### 11. Commit

Per [commit-style.md](../../../docs/standards/commit-style.md):

```
feat(bsp-<target>): initial BSP — boot to kernel_entry on <target>
```

Body: one sentence on what the BSP proves (e.g. "boots to kernel_entry on RPi4 CM4 at EL2→EL1; PL011 console confirmed").

Trailer: `Refs: ADR-NNNN` (the boot-flow ADR from step 1).

## Acceptance criteria

- [ ] Boot-flow ADR Accepted before any code lands.
- [ ] All six items in the BSP boot checklist verified.
- [ ] `cargo build --target <triple> -p tyrne-bsp-<target>` succeeds with zero warnings.
- [ ] QEMU or hardware boots to the cooperative scheduler smoke-test output.
- [ ] Every `unsafe` block has a `// SAFETY:` comment with (a) why needed, (b) invariants, (c) why alternatives rejected; audit log updated.
- [ ] `context_switch_asm` is `#[unsafe(naked)]` and saves d8–d15 in addition to x19–x28, fp, lr, sp.
- [ ] Run script exists and `--int-log` (or equivalent) is documented.
- [ ] Commit follows `commit-style.md`.

## Anti-patterns

- **Skipping the boot-flow ADR.** Every BSP has target-specific boot decisions; writing code before documenting them produces undocumented assumptions.
- **Copying `boot.s` without checking the EL.** QEMU `virt` enters EL1; RPi4 enters EL2. The CPACR_EL1 sequence is only needed if the kernel runs at EL1; at EL2 you need CPTR_EL2.
- **Using `#[inline(never)]` instead of `#[unsafe(naked)]` for the context switch.** The compiler will still emit a prologue. See `docs/standards/unsafe-policy.md §5a`.
- **Omitting d8–d15 from `Aarch64TaskContext`.** NEON is enabled at boot; the compiler may allocate d8–d15 in any function. Omitting them silently corrupts task state at higher optimisation levels.
- **Not zeroing BSS.** Rust guarantees `.bss` is zero; if the hardware or QEMU does not zero it, every static initialised to zero will have garbage values.

## References

- [`docs/standards/bsp-boot-checklist.md`](../../../docs/standards/bsp-boot-checklist.md) — ordered checklist with diagnostic table.
- [`docs/standards/unsafe-policy.md`](../../../docs/standards/unsafe-policy.md) — `#[unsafe(naked)]` rule (§5a) and general unsafe discipline.
- [`bsp-qemu-virt/`](../../../bsp-qemu-virt/) — reference BSP implementation.
- [ADR-0012](../../../docs/decisions/0012-boot-flow-qemu-virt.md) — boot-flow ADR for the reference BSP.
- [ADR-0020](../../../docs/decisions/0020-cpu-trait-v2-context-switch.md) — `ContextSwitch` trait contract.
- [AAPCS64](https://github.com/ARM-software/abi-aa/releases) — callee-saved register list (x19–x28, x29, x30, d8–d15).
