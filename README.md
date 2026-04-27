# Tyrne

> A capability-based microkernel for high-assurance devices, written in Rust.

**Status:** Pre-alpha. Phase A complete (2026-04-21) — the kernel boots on QEMU `virt` aarch64 and runs a two-task IPC demo end-to-end with capability-gated message transfer. Phase B underway: scheduler hardening, idle task, monotonic time source via the ARM Generic Timer, and the path toward EL drop / MMU / userspace. See [`docs/roadmap/current.md`](docs/roadmap/current.md) for the active task.

---

## What is Tyrne

Tyrne is an operating system microkernel being built from scratch around two hard design commitments:

1. **Security-first.** A capability-based design in the spirit of seL4, Hubris, and Fuchsia/Zircon. There is no ambient authority: every action a component can take is tied to a capability it was explicitly granted. Drivers, filesystems, and network stacks live in userspace compartments, not in the kernel. Memory safety is enforced by Rust; every `unsafe` block is justified, bounded, and audited.
2. **Heterogeneous hardware.** The same kernel is intended to scale from constrained smart-home devices to single-board computers, and eventually to mobile-class hardware, through a cleanly separated Hardware Abstraction Layer (HAL). The first real hardware target is the Raspberry Pi 4 (aarch64, Cortex-A72).

The name *Tyrne* is a clean-slate invented identifier — no etymology, no shadow / guardian motif, no shared trademark surface.

## Hardware tiers

Tiers describe the level of support committed to a target, not the quality of that target.

| Tier | Target | Role |
|------|--------|------|
| 1 — primary dev | QEMU `virt` machine, `aarch64` | First bring-up target; CI runs here |
| 2 — first real hardware | Raspberry Pi 4 (BCM2711, Cortex-A72) | First port off emulation |
| 2 | Raspberry Pi 5 (BCM2712, Cortex-A76) | Follow-on after Pi 4 |
| 3 | NVIDIA Jetson Nano / Orin (aarch64 CPU only) | Exploratory — see caveat below |
| 3 | RISC-V embedded SoCs (e.g. ESP32-C3/C6, SiFive) | Roadmap — smart-home class |
| 4 | Mobile-class aarch64 SoCs | Long-term vision |

**Jetson caveat.** Jetson devices are aarch64, so their CPU side is portable. Their GPU and Tensor cores, however, require proprietary NVIDIA userspace blobs with no open-source driver. Tyrne rejects proprietary kernel-adjacent blobs on principle, so Jetson will be supported only as a plain aarch64 board; on-device AI acceleration on Jetson is explicitly out of scope. Projects that need open NPU acceleration should target Rockchip NPUs, Hailo, or Google Coral instead. See [ADR-0004](docs/decisions/0004-target-platforms.md).

## Repository layout

```
.
├── docs/             # All project documentation
│   ├── architecture/ # System design, components, data flow
│   ├── decisions/    # Architecture Decision Records (ADRs)
│   ├── guides/       # How-to guides for contributors and porters
│   ├── standards/    # Coding, documentation, review standards
│   └── glossary.md
├── CLAUDE.md         # Entry point for Claude-based AI agents
├── AGENTS.md         # Entry point for all AI agents
├── CONTRIBUTING.md
├── SECURITY.md
├── LICENSE           # Apache-2.0
└── NOTICE
```

Source code layout — the Rust workspace, HAL crates, userspace services — will be added after the architecture phase.

## Where to start reading

1. [Glossary](docs/glossary.md) — terminology used throughout the project.
2. [Architecture documentation](docs/architecture/) — the high-level design (Phase 2).
3. [Architecture Decision Records](docs/decisions/) — why the project is built the way it is.
4. [Standards](docs/standards/) — how to contribute documentation, and eventually code.

## Contributing

Tyrne is in the architecture phase. External code contributions are not yet being accepted, because the foundational documents are still being written and accepting PRs too early would fragment the design. Issues, references, and prior-art suggestions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

If you have a security-relevant observation, see [SECURITY.md](SECURITY.md).

## License

Licensed under the [Apache License, Version 2.0](LICENSE). See [NOTICE](NOTICE) for attribution requirements.
