# 0012 — Boot flow and memory layout for `bsp-qemu-virt`

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

Phase 4c needs the kernel to boot. Getting the CPU from QEMU's entry point into `kernel_main` requires decisions that bake into the build artifact for the lifetime of the BSP:

- **Where is the kernel loaded?** The linker needs a fixed load address.
- **What does the reset vector do?** Set up a stack, zero BSS, hand off to Rust.
- **At which Exception Level do we execute?** QEMU can hand the kernel EL1 or EL2 depending on configuration.
- **What is the memory layout?** Where do `.text`, `.rodata`, `.data`, `.bss`, and the initial stack live?
- **Who provides the initial diagnostic console?** In v1 the UART is hardcoded.

Because the BSP is per-board and already scoped to QEMU `virt` (see [ADR-0004](0004-target-platforms.md) and [ADR-0006](0006-workspace-layout.md)), decisions here apply only to `bsp-qemu-virt`. Each future BSP (`bsp-pi4`, `bsp-pi5`, Jetson, RISC-V boards) will have its own ADR for its boot protocol; the patterns here are a template, not a universal mandate.

## Decision drivers

- **Compatibility with QEMU `-kernel`.** The simplest way to boot our kernel under QEMU is the `-kernel <file>` flag, which loads an ELF at its linked-in addresses and jumps to the ELF's entry point.
- **Minimize privileged-EL plumbing in v1.** QEMU `virt` enters either EL1 or EL2 depending on whether `-machine virtualization=on` is used. Writing robust EL-drop code is real work; v1 avoids it.
- **Single-core.** Consistent with [ADR-0008 (Cpu v1)](0008-cpu-trait.md) and [ADR-0010 (Timer v1)](0010-timer-trait.md). Secondary cores stay offline; no PSCI `CPU_ON` in v1.
- **No device-tree parsing in v1.** QEMU passes the device-tree blob in `x0`; we currently hardcode MMIO addresses (PL011 at `0x0900_0000`, `GICv3` distributor at `0x0800_0000`, etc.) because `virt` is a well-known platform. DTB parsing arrives when the first board with runtime topology lands (Pi 4).
- **Reproducibility.** The boot flow must be deterministic: given the same ELF, the same sequence of operations runs on every boot.
- **Audit-friendly `unsafe`.** Assembly and MMIO are unavoidable; everything outside them stays in safe Rust.
- **Small enough to fit in one BSP crate.** Boot code must not sprawl across many crates; everything boot-related lives under `bsp-qemu-virt/`.

## Considered options

### Option A — drop to EL1 in `_start`

The first thing `_start` does is an EL-switch routine that configures `SCTLR_EL2`, `HCR_EL2`, an `ELR_EL2` / `SPSR_EL2` pair, and issues `ERET` to land in EL1. The kernel body runs at EL1.

### Option B — stay at whichever EL QEMU provides

`_start` sets up the stack and BSS, then branches into Rust. No EL switch. The kernel runs at EL1 *or* EL2 depending on QEMU configuration; since v1 does not access EL-sensitive registers, this is safe.

### Option C — use `-device loader` with a custom entry binary

Skip the `-kernel` flag entirely; use `-device loader,file=...,addr=0x...` to place the kernel at a custom address and `-device loader,cpu-num=0,addr=0x...` to set the PC. More control, much more config in the runner.

## Decision outcome

**Chosen: Option B — load the kernel via `-kernel` at `0x40080000`, stay at whichever Exception Level QEMU provides, and defer EL handling and DTB parsing to future ADRs.**

Concretely:

- **Load address:** `0x40080000`. QEMU `virt` RAM starts at `0x40000000`; `-kernel` conventionally loads aarch64 images at `+0x80000`, matching the Linux kernel convention and the QEMU documentation. The linker places `_start` there.
- **Entry point:** `_start`, declared in `.text.boot` so the linker script forces it to the beginning of `.text`. The ELF's `e_entry` therefore points at `0x40080000`.
- **Exception Level:** whichever QEMU delivers. v1 kernel code does not read or write EL-sensitive system registers, so it runs correctly at either EL1 or EL2. A future ADR (likely paired with multi-core or MMU setup) will drop to EL1 explicitly.
- **Stack:** a 64 KiB region reserved at the end of the image. `__stack_top` is a linker-defined symbol at the high end of that region; `_start` sets `SP` to it.
- **BSS:** zeroed in `_start` before control passes to Rust. The range is bracketed by `__bss_start` / `__bss_end`, both linker-defined.
- **DTB pointer:** QEMU places it in `x0` at entry. v1 ignores it. A future ADR will introduce DTB parsing (probably in `tyrne-dt`) and the kernel will receive a typed `BootInfo` from the BSP.
- **Secondary cores:** QEMU is invoked with `-smp 1`; no PSCI `CPU_ON` calls in v1. Multi-core is a future ADR.
- **Diagnostic console:** PL011 UART at `0x0900_0000`, hardcoded in the BSP. Matches the QEMU `virt` memory map.

Option A (drop to EL1) was rejected for v1 because the EL-switch code is non-trivial and adds `unsafe` surface with no current payoff. It will be written when a kernel subsystem actually needs EL1-specific semantics — likely the MMU setup, which is itself a later ADR. Option C (`-device loader`) was rejected because it adds complexity to the QEMU runner for no functional gain: `-kernel` already does exactly what we need.

## Memory layout

```
0x40080000  _start (.text.boot)
            .text
            .rodata
            .data
            .bss          — zeroed in _start
            (64 KiB)      — initial stack region
            __stack_top
```

The layout is a single contiguous image. RAM at `0x40000000 .. 0x40080000` is left untouched by the kernel image; QEMU uses part of that range for its own firmware when applicable. The 128 MiB RAM size configured in the QEMU runner is more than adequate for v1.

## Linker script structure

The `linker.ld` file lives at `bsp-qemu-virt/linker.ld` and is wired in by `bsp-qemu-virt/build.rs` via `-T<abs-path>/linker.ld`. Its sections:

- `ENTRY(_start)` — declares the ELF entry point.
- `MEMORY` — one region, `RAM (rwx) : ORIGIN = 0x40080000, LENGTH = 128M`.
- `SECTIONS`:
  - `.text`: starts with `KEEP(*(.text.boot))` so `_start` is first, followed by `*(.text .text.*)`.
  - `.rodata`, `.data`: standard, 8-byte aligned.
  - `.bss`: brackets `__bss_start` and `__bss_end`, 8-byte aligned (so the BSS-zeroing loop can use 8-byte stores).
  - Stack region: reserves 64 KiB, defines `__stack_top` at the upper end.
  - `/DISCARD/`: drops `.comment`, `.note.*`, `.eh_frame*`, `.gcc_except_table*` (we use `panic=abort`; unwinding tables are dead weight).

## Reset-vector responsibilities

`_start`, implemented as `core::arch::global_asm!`:

1. Load `__stack_top` and set `SP`.
2. Zero the BSS region (`str xzr, [x0], #8` loop over `__bss_start` .. `__bss_end`; 8-byte aligned).
3. Branch to the Rust entry `kernel_entry` (declared `#[no_mangle] pub extern "C" fn kernel_entry() -> !`).
4. If `kernel_entry` ever returns (it shouldn't), halt with `wfe; b .`.

This is the minimum to get from QEMU's entry to Rust; everything else runs in Rust.

## Consequences

### Positive

- **Shortest possible path to Rust.** Under 30 lines of assembly; every Rust lint applies to the rest.
- **Deterministic build.** Linker script, load address, and entry point all pinned.
- **QEMU-native.** Boots with `qemu-system-aarch64 -M virt -kernel <elf>` with no additional flags for image placement.
- **Reusable template.** Pi 4 / Pi 5 / Jetson BSPs can follow the same structure with their own load addresses and linker scripts.

### Negative

- **EL1-vs-EL2 ambiguity.** Running at EL2 is fine for v1, but MMU setup in a later phase will want EL1 explicitly. That work will include the EL-drop code.
- **DTB is ignored.** We do not currently validate that QEMU is passing us the config we expect; we trust `virt` to be the shape we encoded. A future ADR adds DTB parsing and with it a runtime sanity check.
- **Hardcoded MMIO addresses.** The PL011 UART at `0x0900_0000` is a QEMU `virt` convention, not a universal aarch64 address. Each BSP hardcodes its own; this is fine given [architectural principle P6](../standards/architectural-principles.md#p6--hal-separation).
- **No multi-core in v1.** `-smp 1` is required; multi-boot requires an ADR extension.
- **64 KiB stack is modest.** Deep recursion or large stack-allocated structures will overflow. For v1 this is plenty; the scheduler's per-task stacks will be sized separately when tasks arrive.

### Neutral

- Unwinding tables are discarded. `panic=abort` makes them unused; the kernel panic handler does its own minimal reporting and halts.
- The boot flow is explicitly `bsp-qemu-virt`-specific. Universal boot patterns, if any emerge across BSPs, will be noted as prior art in future BSP ADRs, not refactored into a shared crate.

## Pros and cons of the options

### Option A — drop to EL1 in `_start`

- Pro: matches what most "real" ARMv8 kernels do.
- Pro: gives us EL1-specific registers uniformly at entry.
- Con: 40–80 lines of additional assembly to write and audit.
- Con: no current consumer of EL1-specific state; premature.

### Option B — stay at whichever EL QEMU provides (chosen)

- Pro: smallest possible `_start`.
- Pro: all currently-needed functionality works at both EL1 and EL2.
- Con: EL-drop will be required later, adding complexity then instead of now.

### Option C — `-device loader` + custom entry

- Pro: maximum placement control.
- Pro: decouples from `-kernel` conventions.
- Con: runner config grows considerably.
- Con: loses the QEMU path most tutorials and CI examples use.

## Open questions

Each will be resolved by a future ADR or a paired update.

- **EL drop.** When the first EL1-specific register is accessed (likely MMU setup), the ADR that introduces that work also defines the EL-drop routine.
- **DTB parsing.** Introduced when the first runtime-config need arises (typically Pi 4 bring-up). `tyrne-dt` is the tentative crate name.
- **Multi-core start.** Secondary-core bring-up via PSCI `CPU_ON`; pairs with the multi-core extension to `Cpu`.
- **Boot-time MMU activation.** Currently the kernel runs with whatever translation state QEMU provides (typically MMU-off). Turning it on is a future ADR; the linker script may need adjustments for the mapped-vs-identity split.
- **Stack size policy.** 64 KiB is a placeholder; a real kernel will want per-task stacks with guard pages, which requires MMU + scheduler integration.
- **Measured boot.** Hooks for a measurement register to record the boot code. Out of scope until Pi 4 hardware support and a TPM / secure-element substitute.
- **`.init_array` / C++-style static init.** The kernel does not use these today; if a future dependency pulls them in, the linker script needs to call the init array from Rust.

## References

- [ADR-0004: Target platforms and support tiers](0004-target-platforms.md).
- [ADR-0006: Workspace layout](0006-workspace-layout.md).
- [ADR-0008: Cpu trait v1](0008-cpu-trait.md).
- [ADR-0010: Timer trait v1](0010-timer-trait.md).
- [`docs/architecture/hal.md`](../architecture/hal.md) — `bsp-qemu-virt` architectural role.
- QEMU `virt` machine documentation — https://qemu.readthedocs.io/en/latest/system/arm/virt.html
- Linux *aarch64 boot protocol* (Documentation/arm64/booting.rst) — prior art for load-address and register conventions.
- ARM *Architecture Reference Manual* (ARMv8-A) — Exception Levels, `SP_EL0`, `ERET`, `HCR_EL2`.
- PL011 UART documentation — ARM reference for the data and flag registers the `Console` impl writes.
