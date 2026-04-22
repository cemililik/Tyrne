# Architectural principles

The non-negotiable design invariants that every Tyrne change must uphold. These are distilled from the Architecture Decision Records and represent the project's long-term commitments. If a proposed change would violate a principle here, the remedy is to write a new ADR overriding the relevant prior decision — not to make an exception in code.

This document is prescriptive. The ADRs it cites are the *reasoning*; this document is the *rule*. When in conflict, the ADR wins (with the caveat that an intended contradiction in this file is a bug to be fixed).

## Scope

Every kernel, HAL, driver, and service change is subject to these principles. Documentation and standards contribute to or document the principles but are not themselves principles.

## The principles

### P1 — No ambient authority

**Statement.** There is no "who" that automatically has power in Tyrne. Every privileged operation requires a capability held by the caller.

**Why.** Ambient authority is the root cause of confused deputy problems, privilege escalation via forgotten code paths, and "root can do anything" failures. Capability systems make authority explicit and auditable. See [ADR-0001](../decisions/0001-microkernel-architecture.md).

**Applies to.** Every syscall. Every IPC operation. Every HAL primitive. Every userspace service.

### P2 — Smallest defensible trusted computing base

**Statement.** Code that runs in privileged mode (the kernel) is kept as small as possible. When in doubt, push functionality into userspace.

**Why.** Every line of kernel code is a line of privileged surface. A smaller TCB is easier to review, test, and eventually verify. See [ADR-0001](../decisions/0001-microkernel-architecture.md).

**Applies to.** Design decisions for new subsystems. If a subsystem can be implemented in userspace without an architectural penalty, it must be.

### P3 — Drivers, filesystems, and network stacks live in userspace

**Statement.** The kernel provides scheduling, memory management, IPC, and capability primitives. Everything else — driver frameworks, filesystems, network stacks, service managers — runs as userspace tasks.

**Why.** Driver bugs must not be kernel bugs. Driver compromises must not be kernel compromises. This is the primary architectural tool for fault containment in a microkernel. See [ADR-0001](../decisions/0001-microkernel-architecture.md).

**Applies to.** Placement of any new code that touches hardware or state beyond core kernel primitives. MMIO access goes through a userspace driver that holds the relevant `MemoryCap`.

### P4 — Capability checks at every trust boundary

**Statement.** Wherever trust changes — task-to-kernel, kernel-to-userspace, userspace service-to-service — the entry point performs a capability check. There is no "implicit trust because the caller is in the same address space."

**Why.** Trust boundaries are where invariants meet adversarial input. Uniform capability checking means security reasoning is uniform rather than ad-hoc.

**Applies to.** Every syscall entry, every IPC receive, every cross-task function call.

### P5 — Rust-only for kernel code

**Statement.** All kernel, HAL, and service code is Rust. Assembly is permitted only where Rust cannot express the required semantics (e.g., context switch primitives, vector tables). Every assembly stub has a safe Rust wrapper.

**Why.** Memory-safe by construction where the compiler permits; surgical `unsafe` where it cannot. See [ADR-0002](../decisions/0002-implementation-language-rust.md) and [unsafe-policy.md](unsafe-policy.md).

**Applies to.** All new kernel and HAL code. Contributions in other languages are rejected.

### P6 — HAL separation

**Statement.** Kernel code does not directly reference any specific CPU, board, or peripheral. Hardware details live behind HAL traits and types. Board Support Packages (BSPs) implement those traits concretely.

**Why.** Portability across aarch64 QEMU, Raspberry Pi, Jetson (CPU), and eventual RISC-V targets requires hard isolation of hardware-specific code. See [ADR-0004](../decisions/0004-target-platforms.md).

**Applies to.** Any code in a kernel crate that references a register, an instruction, or a board-specific detail. That code belongs in a HAL trait implementation (i.e., a BSP), not in the kernel.

### P7 — No proprietary binary blobs in the kernel

**Statement.** Tyrne does not link, embed, or depend on proprietary binary firmware or drivers in the kernel. Open-source firmware that the board requires (e.g. Raspberry Pi boot firmware) may be documented and used *outside* the kernel, but no closed-source code is part of the Tyrne build.

**Why.** Security-first posture means we can audit what we ship. Proprietary blobs are unauditable and have historically been the vector for significant security and supply-chain failures. See [ADR-0004](../decisions/0004-target-platforms.md) for Jetson-specific consequences.

**Applies to.** Build system, dependency graph, installed artifacts. If a target requires proprietary blobs to function, it is not a supported target for the feature that needs them.

### P8 — Decisions are recorded as ADRs

**Statement.** Every non-trivial architectural, security, process, or platform decision is recorded as an ADR in [`docs/decisions/`](../decisions/) using the MADR template.

**Why.** The *why* decays faster than the *what*. ADRs are how the project remembers why it made a choice, and how future contributors (human or AI) can challenge that choice coherently.

**Applies to.** Any decision that would be expensive to reverse, any decision that affects multiple subsystems, any decision that changes a prior commitment.

### P9 — English in the repository, Turkish in chat

**Statement.** Every committed artifact — source, documentation, commits, PRs, agent configuration — is in English. Conversation with the maintainer in chat is Turkish.

**Why.** International accessibility of the artifact plus design-throughput of the maintainer. See [ADR-0005](../decisions/0005-documentation-language-english.md).

**Applies to.** Everything under version control.

### P10 — Mermaid for diagrams

**Statement.** All architectural diagrams are inline Mermaid code blocks in markdown. No PNG, SVG, ASCII-art, or other binary / external formats.

**Why.** Diffable, version-controlled, accessible, rendered natively on GitHub. See [documentation-style.md](documentation-style.md).

**Applies to.** Every diagram in the repository.

### P11 — Reproducibility from the toolchain up

**Statement.** A given commit must build to the same output given the same toolchain. The toolchain is pinned in `rust-toolchain.toml`. The dependency graph is pinned in `Cargo.lock` and audited with `cargo-vet`. Build artifacts do not bake in timestamps or host paths.

**Why.** Security review, incident response, and trust-transfer all depend on being able to verify that the binary we shipped matches the source we inspected.

**Applies to.** Every build configuration. Every toolchain choice. Every dependency addition (see [infrastructure.md](infrastructure.md)).

### P12 — No half-finished subsystems on main

**Statement.** Code merged into `main` is complete to the subsystem's current declared scope. Stubs, `todo!()` macros, and placeholder implementations do not merge unless they are explicitly marked as incremental and are part of an approved multi-commit sequence.

**Why.** A half-finished subsystem is worse than no subsystem — it gives the illusion of coverage without the behavior. It invites callers that then depend on stubs.

**Applies to.** PR acceptance. A PR may introduce a new module or crate with only skeleton code only if the scope of that skeleton is documented and the plan to complete it is stated in the PR description.

## Relationship to other standards

- **Code style** ([code-style.md](code-style.md)) tells you *how* to write Rust. Principles tell you *what* the Rust should be doing.
- **Unsafe policy** ([unsafe-policy.md](unsafe-policy.md)) operationalizes P5 (Rust-only) at the `unsafe` boundary.
- **Error handling** ([error-handling.md](error-handling.md)) operationalizes P3 (driver faults stay out of the kernel).
- **Security review** ([security-review.md](security-review.md)) operationalizes P1, P2, P3, P4.
- **Infrastructure** ([infrastructure.md](infrastructure.md)) operationalizes P11 (reproducibility).

## Proposing an exception

There is no "exception" mechanism for these principles. The only way to deviate is to:

1. Write a new ADR that explicitly supersedes the affected prior ADR.
2. Get it accepted.
3. Update this document to reflect the new principle (or to remove / replace the old one).

Expedient exceptions are the crack through which the principles leak out of the system. If a principle is genuinely wrong for a case, the correct response is to change the principle, in public, with reasoning.

## References

- All ADRs in [`../decisions/`](../decisions/).
- [CLAUDE.md](../../CLAUDE.md) — the principles also appear in the AI-agent guide, because agents are a primary audience for this document.
