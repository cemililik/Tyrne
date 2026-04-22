# 0004 — Target hardware platforms and support tiers

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

Tyrne needs a concrete first hardware target to make implementation choices — architecture-specific code, HAL surfaces, toolchain pinning — possible. The long-term vision spans constrained smart-home devices, single-board computers, and eventually mobile-class aarch64 SoCs. The short-term question is: *which platform do we bring up first, and what is the roadmap for the rest?*

## Decision drivers

- **Alignment with long-term vision.** The first target should teach us skills we will reuse, not ones we will throw away. Starting on x86_64 would be the easiest path for emulation, but the long-term portfolio is entirely aarch64 and RISC-V. x86_64 experience does not transfer well to Cortex-A or embedded ARM peripherals.
- **Toolchain and emulator maturity.** The first target must be trivially emulatable. We need fast, reproducible `qemu` runs for every build.
- **Documentation availability.** The first real-hardware port should be a platform with thorough documentation (datasheets, device trees, community drivers).
- **Rust ecosystem support.** Crates for the target CPU (e.g. `aarch64-cpu`, `cortex-a`) should be mature.
- **No proprietary blobs in the kernel.** Per project policy, the kernel must not require proprietary binary drivers.
- **Cost and availability of real hardware.** The first real-hardware target must be cheap and available.

## Considered options

1. **Start with QEMU `virt` aarch64.** Emulated aarch64 platform, GIC, PL011 UART, virtio devices, PSCI. Then move to real aarch64 hardware.
2. **Start with QEMU `x86_64`.** Easiest emulation, most tutorial material, but no forward path to our real targets.
3. **Start with real Raspberry Pi 4 from day one.** Skip emulation.
4. **Start with RISC-V QEMU (`virt`).** Matches some of our long-term embedded targets.

## Decision outcome

**Chosen: start on QEMU `virt` aarch64, then port to Raspberry Pi 4 as the first real hardware. Adopt a tiered support model (below) for all future targets.**

aarch64 is where Tyrne lives long-term — all current targets (Raspberry Pi, Jetson, eventual mobile) are aarch64. Starting there avoids paying a learning tax on x86_64 that would have to be re-paid when porting. QEMU's `virt` machine is a fully documented, standard, deterministic aarch64 environment (GICv3, PL011 UART, virtio transports, PSCI for CPU start), ideal for CI-driven kernel development.

The Raspberry Pi 4 (BCM2711, Cortex-A72, 4 cores, 1 – 8 GB RAM) is chosen as the first real-hardware port because it is cheap, available everywhere, extensively documented, aarch64-native, and has a large community of open-source kernel work to cross-reference. Pi 5 (BCM2712, Cortex-A76) follows.

RISC-V is not rejected — it is on the Tier 3 roadmap — but RISC-V as the *first* platform would reduce the amount of shared Rust-ecosystem knowledge available during the critical bring-up phase.

### Support tiers

| Tier | Meaning | Requirements |
|------|---------|--------------|
| **1 — Primary** | Always builds and boots; all CI gates run here. | QEMU emulation of this platform is a required CI job. |
| **2 — Supported** | Expected to build and boot; CI checks build; boot tested manually on release. | Manual bring-up evidence per release. |
| **3 — Best-effort** | Maintained opportunistically; may lag. | No CI gate, but should not knowingly be broken. |
| **4 — Aspirational** | On the roadmap, not being actively developed. | None yet. |

### Tier assignments at ADR time

| Tier | Target | Notes |
|------|--------|-------|
| 1 | QEMU `virt` aarch64 | First bring-up; CI authority; standard PSCI + GIC + PL011 + virtio. |
| 2 | Raspberry Pi 4 (BCM2711, Cortex-A72) | First real-hardware port. |
| 2 | Raspberry Pi 5 (BCM2712, Cortex-A76) | Follows Pi 4 bring-up. |
| 3 | NVIDIA Jetson Nano (Tegra X1, Cortex-A57) | aarch64 CPU port only — see Jetson caveat. |
| 3 | NVIDIA Jetson Orin Nano / Orin NX / AGX Orin (Cortex-A78AE) | aarch64 CPU port only — see Jetson caveat. |
| 3 | RISC-V embedded (SiFive HiFive, ESP32-C3/C6) | Smart-home class; secondary priority behind ARM. |
| 4 | Mobile-class aarch64 SoCs (e.g. Snapdragon, MediaTek) | Long-horizon; requires significant additional platform work. |

### Jetson caveat

NVIDIA Jetson devices are aarch64 and their CPU cores boot using the normal ARM boot protocol (CBoot → U-Boot → kernel is typical on Jetson L4T; Tyrne will likely replace the Linux kernel in that chain). Running a custom kernel on the Jetson CPU is feasible and in scope for Tier 3.

However, the value of a Jetson — GPU compute via CUDA and Tensor core acceleration — depends on **proprietary NVIDIA userspace libraries and kernel modules**. There is no open-source driver for modern NVIDIA mobile GPUs, and Tyrne's policy (derived from the security-first posture of [ADR-0001](0001-microkernel-architecture.md)) rejects proprietary binary blobs in the kernel.

Concretely:

- Tyrne on Jetson will provide CPU-level functionality: scheduling, IPC, memory management, and whatever userspace drivers can be written for non-GPU peripherals.
- Tyrne on Jetson will **not** provide CUDA, cuDNN, TensorRT, Isaac, or any other NVIDIA-proprietary stack.
- If a project needs on-device NPU acceleration under Tyrne, suitable open alternatives include Rockchip NPUs (documented), Hailo accelerators (documented), and Google Coral Edge TPU. A future ADR will track the first open NPU target.

## Consequences

### Positive

- Every hour spent on bring-up pays forward to the long-term targets (Pi, Jetson, mobile are all aarch64).
- QEMU `virt` is a strict, well-documented environment — easy to write specifications against.
- The tier model gives contributors, users, and AI agents a clear signal about which targets are first-class vs. experimental.
- The Jetson caveat is documented now, while the decision is fresh, rather than surfacing as a question later.

### Negative

- **Slightly steeper initial slope than starting on x86_64.** aarch64 requires us to learn PSCI, GIC, ARM boot, MMU translation table layout (TTBR0/1, granules). We accept this tax to avoid rework.
- **Two real-hardware targets early (Pi 4 then Pi 5).** Supporting two Pi generations before any other board may divert effort; mitigation is to treat Pi 5 as purely additive — the HAL surface defined for Pi 4 must be sufficient, and Pi 5 should only add what is genuinely new.
- **Jetson support will disappoint users who expect AI acceleration.** This is a documentation and expectation-setting problem, not a kernel problem; README and ADR-0004 explain it clearly.

### Neutral

- Adding a new tier-3 target does not compromise tier 1. The tier system isolates support promises.

## Pros and cons of the options

### QEMU `virt` aarch64 first, then Pi 4

- Pro: matches long-term direction; reusable knowledge; mature emulation.
- Pro: excellent community and Rust-crate support (`aarch64-cpu`, `aarch64-paging`, `virtio-drivers`).
- Con: aarch64 bring-up is a touch harder than x86 for first-timers.

### QEMU `x86_64` first

- Pro: most tutorials and legacy OSDev material target x86.
- Pro: simplest initial boot (BIOS / UEFI / multiboot).
- Con: knowledge does not transfer to any of our real-hardware targets.
- Con: pushes real-hardware-relevant questions out to "the next port" where they are harder to answer.

### Raspberry Pi 4 from day one (no emulator)

- Pro: real hardware validates the architecture choices sooner.
- Con: every debug cycle requires an SD card swap or netboot; slow inner loop.
- Con: no CI without physical devices in the loop.

### RISC-V QEMU first

- Pro: matches part of our long-term embedded vision.
- Con: smaller crate and documentation ecosystem than aarch64; bring-up cost higher.
- Con: main long-term targets (Pi, Jetson, mobile) are ARM, not RISC-V.

## References

- Arm Architecture Reference Manual (ARMv8-A).
- QEMU aarch64 `virt` machine documentation: https://qemu.readthedocs.io/en/latest/system/arm/virt.html
- Raspberry Pi hardware documentation: https://www.raspberrypi.com/documentation/computers/raspberry-pi.html
- ARM PSCI specification.
- GICv3/v4 architecture specification.
- NVIDIA Jetson Linux (L4T) documentation (for context on why full Jetson support would require proprietary blobs): https://developer.nvidia.com/embedded/jetson-linux
