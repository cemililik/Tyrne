# 0006 — Workspace layout and initial crate boundaries

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

Phase 4 of the project begins implementation. The architecture documents ([overview.md](../architecture/overview.md), [hal.md](../architecture/hal.md), [security-model.md](../architecture/security-model.md)) have settled that Tyrne is a narrow kernel core, a trait-based HAL, and per-board BSPs that implement the HAL. The Cargo workspace must reflect that decomposition concretely: the set of crates, their boundaries, and their roles.

The workspace choice is consequential and slow to change after the fact:

- Crate boundaries become `pub` API surfaces; moving code across crates is non-trivial once consumers depend on the old paths.
- The kernel's dependency graph determines what code is in the TCB. A kernel crate that depends (transitively) on board specifics violates [architectural principle P6](../standards/architectural-principles.md#p6--hal-separation).
- Host-side testability depends on whether the kernel's dependencies compile on the host target. A kernel crate that depends on a BSP cannot be tested on a laptop.
- Adding a future BSP (Pi 4, Pi 5, Jetson, RISC-V, mobile) must not require changes to any crate upstream of it in the dependency graph.

This ADR records the initial layout before the code lands, so subsequent commits can cite it rather than justify the structure in every PR.

## Decision drivers

- **P6: HAL separation.** The kernel crate must not transitively depend on a BSP. The compiler, not convention, enforces this.
- **P3: drivers in userspace.** No privileged crate contains driver code; the workspace has no "drivers" crate at the kernel level.
- **P2: small TCB.** Every crate in the privileged build graph is reviewed as kernel code. Adding a crate has a non-zero review cost; the initial set is minimal.
- **Host-side testability.** The kernel core must be testable on a developer's laptop, without QEMU, through a fake HAL. That requires a crate whose `std`-allowed dependencies mock the HAL traits.
- **Idiomatic Rust.** Crate names, `Cargo.toml` conventions, and feature flags follow Rust community norms — no invented conventions that future contributors have to learn.
- **Minimal now, extensible later.** Deeper subdivisions (separate crates for IPC, memory, scheduler) can be carved out when their surface grows. They are not carved out now.

## Considered options

1. **Single-crate kernel.** All kernel, HAL, and BSP code in one crate.
2. **Four-crate split.** `tyrne-kernel` (portable core) + `tyrne-hal` (traits) + `tyrne-bsp-<board>` (per-board) + `tyrne-test-hal` (host-only fakes).
3. **Deep subdivision.** Separate crates per subsystem inside the kernel: `tyrne-kernel-core`, `tyrne-kernel-ipc`, `tyrne-kernel-mem`, `tyrne-kernel-sched`, plus the HAL, BSP, and test-hal.

## Decision outcome

**Chosen: Option 2 — a four-crate split.**

The split matches the three-layer architecture: one crate per layer, plus `tyrne-test-hal` to enable host testing. Single-crate would violate P6 at the compiler level (a BSP change would rebuild and potentially affect kernel semantics). Deep subdivision is premature — we do not yet have the subsystem boundaries pinned down precisely enough to bake them into crate boundaries, and the cost of re-carving later is higher than the cost of starting broader. When a subsystem's surface grows big enough that rebuilds or review friction justify a separate crate, an ADR carves it out.

### Initial crate set

| Crate | Kind | Target | `#![no_std]` | Role |
|-------|------|--------|--------------|------|
| `tyrne-kernel` | library | `aarch64-unknown-none` (and host for tests) | yes | Portable kernel core: capability table, scheduler, IPC, memory, interrupt dispatch. Depends only on `tyrne-hal` trait definitions. |
| `tyrne-hal` | library | any | yes | HAL trait definitions (`Cpu`, `Mmu`, `IrqController`, `Timer`, `Console`, `Iommu`). No implementations. No kernel logic. |
| `tyrne-bsp-qemu-virt` | binary | `aarch64-unknown-none` | yes, `no_main` | QEMU `virt` aarch64 BSP: reset vector, early-init, `Cpu`/`Mmu`/`IrqController`/`Timer`/`Console`/`Iommu` impls for GICv3 + PL011 + SMMUv3, and the `main` entry that links `tyrne-kernel` into a bootable image. |
| `tyrne-test-hal` | library | host | no (std allowed) | Deterministic fake implementations of HAL traits for unit tests. Used by `tyrne-kernel`'s `#[cfg(test)]` tests. |

### Directory layout

```
tyrne/
├── Cargo.toml               (workspace)
├── rust-toolchain.toml      (pinned nightly)
├── rustfmt.toml
├── clippy.toml
├── .cargo/
│   └── config.toml          (target triples, rustflags)
├── kernel/
│   ├── Cargo.toml           (package name: tyrne-kernel)
│   └── src/
├── hal/
│   ├── Cargo.toml           (package name: tyrne-hal)
│   └── src/
├── bsp-qemu-virt/
│   ├── Cargo.toml           (package name: tyrne-bsp-qemu-virt)
│   ├── linker.ld
│   └── src/
└── test-hal/
    ├── Cargo.toml           (package name: tyrne-test-hal)
    └── src/
```

Directory names are short (`kernel/`, not `tyrne-kernel/`) for ergonomics; crate names carry the `tyrne-` prefix to avoid collisions if any crate is ever published.

### Dependency graph

```
                        tyrne-bsp-qemu-virt  ──►  tyrne-kernel  ──►  tyrne-hal
                                                         │                 ▲
                                                         └── tests: ───►  tyrne-test-hal
```

- `tyrne-hal` is a leaf: it defines traits and nothing else.
- `tyrne-kernel` depends on `tyrne-hal` (the traits) and, under `#[cfg(test)]`, on `tyrne-test-hal`.
- `tyrne-bsp-qemu-virt` depends on `tyrne-hal` (to implement the traits) and on `tyrne-kernel` (to run it).
- Future BSPs add themselves at the `bsp-*` level; they do not touch the kernel or the HAL crate.

No other cross-edges exist in the initial layout.

## Consequences

### Positive

- **Compiler-enforced P6.** The kernel crate's dependency graph contains no board-specific code; a violation would be a build failure.
- **Host-side unit testing is a first-class mode.** `cargo test -p tyrne-kernel` runs on the developer's laptop, wiring in `tyrne-test-hal` fakes.
- **Adding a new BSP is an additive change.** `bsp-pi4`, `bsp-pi5`, future Jetson / RISC-V BSPs plug in alongside `bsp-qemu-virt` without changes to the kernel or HAL crates.
- **Review boundaries match architectural boundaries.** A change to the HAL trait set shows up as a diff in `hal/`, which is the signal for a security review.
- **Cargo workspace lints can enforce style uniformly.** `[workspace.lints]` applies across all crates; per-crate overrides are explicit.

### Negative

- **More setup.** A single crate would boot faster; four crates require more `Cargo.toml` plumbing and slightly more initial ceremony.
- **Cross-crate refactoring cost.** Moving a type from `tyrne-hal` to `tyrne-kernel` (or vice-versa) is more work than moving between modules in one crate. Mitigation: we defer decisions that would force such moves until their shape is clearer, per the "minimal now" driver.
- **More crate-local documentation to maintain.** Each crate needs its own module-level documentation. Cost: one page of rustdoc per crate, amortized.

### Neutral

- The initial naming convention (`tyrne-` prefix) is a choice made now that future crates will follow. It does not constrain behaviour; it standardizes identifiers.

## Pros and cons of the options

### Single-crate kernel

- Pro: simplest start; one `Cargo.toml`, one lib.
- Con: compiler cannot distinguish kernel from board code; P6 is convention rather than enforced.
- Con: host-side testing harder — the crate contains board-specific types (MMIO register layouts) that do not build on host.
- Con: adding a second BSP requires feature-gating or substantial refactor.

### Four-crate split (chosen)

- Pro: compiler-enforced separation; host-side testability; additive BSP growth; review boundaries match architecture.
- Con: more setup; cross-crate refactors cost more than cross-module.

### Deep subdivision

- Pro: even stronger review isolation; each kernel subsystem becomes independently buildable and testable.
- Pro: eventual target if the kernel grows large.
- Con: premature — subsystem interfaces inside the kernel are not yet stable; forcing them into crate boundaries now would require re-carving as design settles.
- Con: multiplies `Cargo.toml` work without a current payoff.

## References

- [ADR-0001: Capability-based microkernel architecture](0001-microkernel-architecture.md).
- [ADR-0002: Rust as the implementation language](0002-implementation-language-rust.md).
- [ADR-0004: Target hardware platforms and support tiers](0004-target-platforms.md).
- [docs/architecture/overview.md](../architecture/overview.md) — the three-layer structure.
- [docs/architecture/hal.md](../architecture/hal.md) — HAL trait surface and BSP structure.
- [architectural-principles.md](../standards/architectural-principles.md) — P2, P3, P6, P8.
- Rust Cargo workspaces documentation: https://doc.rust-lang.org/cargo/reference/workspaces.html
- Rust Cargo lints (workspace-level): https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section
- Hubris workspace structure (prior art): https://github.com/oxidecomputer/hubris
