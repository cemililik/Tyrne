# Phase H — Platform expansion

**Exit bar:** `bsp-pi5`, `bsp-jetson` (CPU-only), and one RISC-V BSP each boot and run the Phase A / B / E subset on real hardware.

**Scope:** Prove that the HAL abstraction is real. Each new BSP is mostly additive and stresses the HAL interfaces written in Phase A.

**Out of scope:** Mobile (Phase I); new architectural features; rework of HAL traits (those are a signal that the abstraction was wrong, resolved via ADR).

---

## Milestone H1 — `bsp-pi5`

Raspberry Pi 5 uses BCM2712 (Cortex-A76) with a new RP1 southbridge that handles peripherals differently than Pi 4.

### Sub-breakdown

1. **ADR-0052 — Pi 5 differences.** RP1 southbridge, peripheral topology, console routing, GIC changes.
2. **New BSP** `bsp-pi5/` — mirrors `bsp-pi4`'s shape with Pi 5 specifics.
3. **QEMU parity** — Phase A / B features work on Pi 5.
4. **Additive expectation**: the HAL trait surfaces do not change; any change is a signal for an ADR that reviews whether the HAL was wrong.

### Acceptance criteria

- ADR-0052 Accepted.
- Pi 5 boots and runs the test suite from Phase D's parity list.

## Milestone H2 — `bsp-jetson` (CPU-only)

NVIDIA Jetson Orin Nano / Orin NX / AGX Orin as aarch64 boards. Per [ADR-0004](../../decisions/0004-target-platforms.md), the GPU is out of scope (proprietary blob).

### Sub-breakdown

1. **ADR-0053 — Jetson boot chain.** CBoot / U-Boot sequence, where Umbrix inserts itself.
2. **New BSP** `bsp-jetson/` with the specific Jetson model(s) supported.
3. **`config` documentation** for users setting up Jetson hardware.

### Acceptance criteria

- ADR-0053 Accepted.
- A Jetson board boots Umbrix to the Phase A / B exit bar.
- Release notes are explicit: Jetson's GPU / NPU are inaccessible.

## Milestone H3 — First RISC-V BSP

Candidate: an MMU-capable RISC-V board — e.g., a SiFive HiFive Unmatched / Unleashed or a StarFive VisionFive 2. The first non-aarch64 target — validates that `Cpu`, `Mmu`, `IrqController`, `Timer` abstract correctly across architectures. MMU-less RISC-V microcontrollers (ESP32-C6, ESP32-C3, RP2350-RISCV, etc.) are deliberately out of scope for H3 because they cannot exercise the `Mmu` trait; if a future ADR decides to cover no-MMU targets, that is a separate milestone with its own acceptance criteria.

### Sub-breakdown

1. **ADR-0054 — RISC-V target choice.** Specific board, specific ISA subset (RV32 vs. RV64, extensions).
2. **`Cpu` / `Mmu` / `IrqController` extensions or splits** if needed — e.g., RISC-V's PLIC differs from GIC enough that an adapter or sibling trait may be warranted. If so, an ADR captures the architectural separation.
3. **New BSP** `bsp-<target>/`.
4. **Parity tests** on real hardware for the Phase A / B subset.

### Acceptance criteria

- ADR-0054 Accepted.
- RISC-V BSP boots Umbrix; the test suite runs within the architecture's capabilities.

### Phase H closure

Business review. The HAL abstraction has been tested by three architecturally distinct targets (Pi 5, Jetson, RISC-V); any leaks in the abstraction surface here.

## ADR ledger for Phase H

| ADR | Purpose | Expected state |
|-----|---------|----------------|
| ADR-0052 | Pi 5 differences | H1 |
| ADR-0053 | Jetson boot chain | H2 |
| ADR-0054 | RISC-V target choice | H3 |

## Open questions carried into Phase H

- Whether any HAL trait needs a v2 to accommodate architectural differences that Phase A could not foresee.
- The degree to which BSPs should share helper code (e.g., a `bsp-arm-gic` crate between Pi 4 / Pi 5 / Jetson) vs. remaining independent.
- Whether to target a specific RISC-V profile (e.g., RVA22) or stay minimal.
