# Phase D — Raspberry Pi 4 (first real hardware)

**Exit bar:** `bsp-pi4` boots on a real Pi 4 at feature parity with `bsp-qemu-virt` — all of Phase A / B / C features work on hardware.

**Scope:** A second BSP (`bsp-pi4`) with its own reset, UART, timer, GIC-400, MMU setup; a DTB parser (`umbrix-dt`) so the kernel learns board topology at runtime; SD-card boot. HAL traits that worked for QEMU continue to work; differences are isolated to the BSP.

**Out of scope:** Pi-specific drivers beyond what the kernel needs to boot (those belong in Phase E's driver model); Wi-Fi (blob-dependent, deferred); multi-core on Pi (may drop in naturally if C was done first).

---

## Milestone D1 — `bsp-pi4` scaffolding

A new BSP crate that compiles for `aarch64-unknown-none` and provides a minimal reset path. No HAL impls yet; just the shell.

### Sub-breakdown

1. **ADR-0032 — Pi 4 boot flow.** Load address under Pi firmware (`kernel_address` in `config.txt`); Pi firmware's initial CPU mode; what `config.txt` settings Umbrix expects.
2. **New crate** `bsp-pi4/` with its own `Cargo.toml`, `build.rs`, `linker.ld`, `boot.s`, `main.rs`, `console.rs` — mirroring `bsp-qemu-virt` structure.
3. **Pi firmware interaction** — `config.txt` documentation and the expected load / entry addresses.
4. **Placeholder main** that just spins in `wfe`; no console yet (D3 adds that).

### Acceptance criteria

- ADR-0032 Accepted.
- `cargo build --target aarch64-unknown-none -p umbrix-bsp-pi4` produces an ELF.
- `config.txt` example committed alongside.

---

## Milestone D2 — GIC-400 implementation

Pi 4 uses GIC-400 (GICv2-ish, compatible subset). The `IrqController` impl differs from `bsp-qemu-virt`'s GICv3.

### Sub-breakdown

1. **ADR-0033 — GIC-400 register layout.** Distributor / CPU-interface base addresses on BCM2711; register offsets used; which features are used vs. ignored.
2. **`IrqController` impl** in `bsp-pi4/src/irq.rs`.
3. **Tests** — host-side register layout; the real verification is D8 on hardware.

### Acceptance criteria

- `IrqController` trait is implemented for Pi 4.
- Implementation compiles and passes host-side layout tests.

---

## Milestone D3 — Pi 4 PL011 UART

Pi 4 has both a mini-UART and a PL011 (UART0). We use the PL011 for diagnostic output, with the board-specific baud-rate init that QEMU skipped.

### Sub-breakdown

1. **ADR-0034 — Pi 4 console choice.** PL011 vs. mini-UART; which pins; what baud rate; whether GPIO pin-muxing is part of the BSP or out of scope.
2. **PL011 init sequence** — baud-rate register programming (QEMU's PL011 is pre-initialized; Pi's is not).
3. **`Console` impl** in `bsp-pi4/src/console.rs` using the same trait as `bsp-qemu-virt` with the Pi-specific init.
4. **Tests** — host-side: none meaningful (hardware behaviour); D7 exercises it on real hardware.

### Acceptance criteria

- ADR-0034 Accepted.
- `Console` impl compiles; the first real-hardware smoke will validate it.

---

## Milestone D4 — ARM generic timer on Pi 4

The generic timer works like on QEMU; the difference is the frequency (from `CNTFRQ_EL0`) and any Pi-specific interrupt routing.

### Sub-breakdown

1. **`Timer` impl** in `bsp-pi4/src/timer.rs`, reading `CNTFRQ_EL0` for frequency.
2. **Interrupt-line number** for the timer IRQ on Pi 4 (PPI, line number per BCM2711).
3. **Tests** — parity with `bsp-qemu-virt` where possible.

### Acceptance criteria

- `Timer` impl compiles; frequency reporting is correct when tested on hardware (D7 / D8 validation).

---

## Milestone D5 — MMU on Pi 4

MMU activation on Pi 4. Memory layout is different (RAM at `0x0000_0000` on Pi vs. `0x4000_0000` on QEMU); peripherals at high addresses.

### Sub-breakdown

1. **ADR-0035 — Pi 4 memory layout.** Kernel load address; peripheral window (`0xFE00_0000` class on BCM2711); identity vs. high-half choices here.
2. **`Mmu` impl** — inherits VMSAv8 from QEMU's impl; differences in the linker script and the MMIO mapping tables.
3. **Cache maintenance** — Pi 4 specifics (cache lines, I/D separation, which invalidate sequences are necessary).
4. **Tests** — B2's test suite applied to Pi 4.

### Acceptance criteria

- ADR-0035 Accepted.
- Kernel runs with the MMU on on Pi 4.

---

## Milestone D6 — DTB parser (`umbrix-dt`)

A userspace-agnostic library crate that parses a flattened device tree into a typed structure. Used by the BSP at boot to read what the firmware told it about the machine.

### Sub-breakdown

1. **ADR-0036 — DTB parsing scope.** Full FDT spec support vs. a minimal read-only subset; zero-copy vs. owned parsing; allocation strategy (probably `no_std + alloc` with an arena).
2. **New crate** `umbrix-dt/` — separate from `umbrix-hal` so BSPs opt in.
3. **Parser API** — `DeviceTree::from_bytes(ptr) -> Result<DeviceTree, Error>`; iterators over nodes; property lookup.
4. **Pi 4 integration** — `kernel_entry` parses the DTB passed in `x0` and emits a `BootInfo` struct.
5. **Host tests** — parse known fixtures (QEMU-generated DTB, Pi 4 DTB samples).

### Acceptance criteria

- ADR-0036 Accepted.
- `umbrix-dt` parses a real DTB into typed records.
- `bsp-pi4` uses it at boot; the kernel's `BootInfo` contains at least memory-map and UART-address entries read from the DTB.

---

## Milestone D7 — SD-card boot

The kernel image, along with firmware, `config.txt`, and any boot files, is placed on an SD card; the Pi 4 boots from that card.

### Sub-breakdown

1. **Image packaging** — a script in `tools/` that produces an `sdcard/` directory (or a `.img` file) ready to be written with `dd`.
2. **First real-hardware boot** — runs the D3 console output; the maintainer sees the kernel greeting on a USB-UART cable.
3. **Guide** — `docs/guides/boot-pi4.md` walking through building, writing to SD, connecting UART, booting.

### Acceptance criteria

- Kernel prints its greeting on Pi 4 hardware via the PL011 UART.
- Guide is reproducible.

---

## Milestone D8 — QEMU parity on Pi 4

All Phase A / B / (C if done) features work on Pi 4 as they do on QEMU virt.

### Sub-breakdown

1. **Run the two-task IPC demo** (A6) on Pi 4.
2. **Run the first userspace "hello"** (B6) on Pi 4.
3. **If Phase C is done:** preemption and multi-core IPC on Pi 4.
4. **Business review.**

### Acceptance criteria

- A6 / B6 (and C5 if applicable) produce the expected traces on real Pi 4 hardware.
- Review records any hardware-specific learnings for future BSPs.

### Phase D closure

Business review; the phase is the most significant in terms of validating that "portable code" claim. Phase E (driver model) follows.

---

## ADR ledger for Phase D

| ADR | Purpose | Expected state |
|-----|---------|----------------|
| ADR-0032 | Pi 4 boot flow | D1 |
| ADR-0033 | GIC-400 register layout | D2 |
| ADR-0034 | Pi 4 console choice (PL011 vs. mini-UART) | D3 |
| ADR-0035 | Pi 4 memory layout | D5 |
| ADR-0036 | DTB parsing scope | D6 |

## Open questions carried into Phase D

- Whether we target Pi 4 rev 1.4 specifically or accept a range.
- USB-to-TTL cable model the guide assumes (community standards).

## Resolved

- **SD-image composition including the Pi's closed-source firmware blobs.** Resolved per [ADR-0004](../../decisions/0004-target-platforms.md): closed-source blobs that sit *below* the kernel (e.g., the VC4 stage-0 firmware on Pi 4) are out of our blob-policy scope. The SD image may therefore include them when that is the only way to boot the hardware.
