# Architecture Decision Records

Every non-trivial architectural, security, or process decision in Umbrix is recorded here as an **ADR** (Architecture Decision Record) using a lightweight **MADR** (Markdown Architectural Decision Records) style.

## Why ADRs

- They preserve the **why**, not just the **what**. This is the information that decays fastest and is most expensive to re-derive years later.
- They make the evolution of the project readable: you can follow the numbered history and see how the design language developed, including the options that were rejected.
- They give future contributors (human or AI) a way to disagree with a decision by writing a new ADR that supersedes an old one, rather than silently changing code.

## Format

All ADRs live in this folder, named `NNNN-short-kebab-slug.md`, where `NNNN` is a zero-padded four-digit sequence number. Use [template.md](template.md) as the starting point for a new ADR.

Each ADR contains:

- **Title** (matches the filename, minus the number prefix).
- **Status** — `Proposed`, `Accepted`, `Deprecated`, or `Superseded by NNNN`.
- **Date** — ISO-8601.
- **Deciders** — who signed off.
- **Context** — the question the project was facing and the constraints that applied.
- **Decision drivers** — the forces that influenced the choice.
- **Considered options** — the alternatives examined.
- **Decision outcome** — the option chosen, with a short justification.
- **Consequences** — positive, negative, and neutral effects, with mitigations where relevant.
- **References** — prior art, literature, upstream discussions.

## Index

| # | Title | Status | Date |
|---|-------|--------|------|
| 0001 | [Capability-based microkernel architecture](0001-microkernel-architecture.md) | Accepted | 2026-04-20 |
| 0002 | [Rust as the implementation language](0002-implementation-language-rust.md) | Accepted | 2026-04-20 |
| 0003 | [Apache-2.0 license](0003-license-apache-2.md) | Accepted | 2026-04-20 |
| 0004 | [Target hardware platforms and tiers](0004-target-platforms.md) | Accepted | 2026-04-20 |
| 0005 | [English as the documentation and code language](0005-documentation-language-english.md) | Accepted | 2026-04-20 |
| 0006 | [Workspace layout and initial crate boundaries](0006-workspace-layout.md) | Accepted | 2026-04-20 |
| 0007 | [Console HAL trait signature](0007-console-trait.md) | Accepted | 2026-04-20 |
| 0008 | [Cpu HAL trait signature (v1, single-core scope)](0008-cpu-trait.md) | Accepted | 2026-04-20 |
| 0009 | [Mmu HAL trait signature (v1)](0009-mmu-trait.md) | Accepted | 2026-04-20 |
| 0010 | [Timer HAL trait signature (v1)](0010-timer-trait.md) | Accepted | 2026-04-20 |
| 0011 | [IrqController HAL trait signature (v1)](0011-irq-controller-trait.md) | Accepted | 2026-04-20 |
| 0012 | [Boot flow and memory layout for bsp-qemu-virt](0012-boot-flow-qemu-virt.md) | Accepted | 2026-04-20 |
| 0013 | [Roadmap and planning process](0013-roadmap-and-planning.md) | Accepted | 2026-04-20 |
| 0014 | [Capability representation](0014-capability-representation.md) | Accepted | 2026-04-20 |
| 0015 | [AI integration stance: userspace-only, kernel-neutral](0015-ai-integration-stance.md) | Accepted | 2026-04-20 |
| 0016 | [Kernel object storage](0016-kernel-object-storage.md) | Accepted | 2026-04-21 |
| 0017 | [IPC primitive set](0017-ipc-primitive-set.md) | Accepted | 2026-04-21 |
| 0018 | [Badge scheme and `reply_recv` fastpath: formal deferral](0018-badge-scheme-and-reply-recv-deferral.md) | Accepted | 2026-04-21 |
| 0019 | [Scheduler shape](0019-scheduler-shape.md) | Accepted | 2026-04-21 |
| 0020 | [`ContextSwitch` trait and `Cpu` v2](0020-cpu-trait-v2-context-switch.md) | Accepted | 2026-04-21 |

## Creating a new ADR

1. Copy [template.md](template.md) to the next available number: `NNNN-your-slug.md`.
2. Fill it in. Start with status `Proposed`.
3. Open a PR (once the PR process is established) or, in the solo phase, commit directly with a descriptive commit message referencing the ADR number.
4. When the decision is settled, change the status to `Accepted`.
5. If a later ADR overrides this one, mark the old one `Superseded by NNNN` and link forward to the new record. Do **not** delete or rewrite the old ADR — the historical reasoning is the point.
