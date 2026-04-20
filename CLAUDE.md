# Claude agent guide — Umbrix

This file is the entry point for Claude-based AI agents (Claude Code, Claude API agents, subagents) working in this repository. Read it fully before taking any action in the repo. Other AI agents should read [AGENTS.md](AGENTS.md), which points back here.

## What this project is

Umbrix is a **capability-based microkernel** written in Rust, in the lineage of seL4 and Hubris. The project is **pre-alpha** — most code is not yet written, and the current phase is architecture design captured in Architecture Decision Records (ADRs). Primary development target is QEMU `virt` on aarch64; first real hardware target is the Raspberry Pi 4.

See [README.md](README.md) for the public overview.

## Non-negotiable rules for AI agents

These rules apply to every AI agent acting inside this repository, regardless of model, runner, or tool.

1. **Security-first mindset.** Umbrix is built to be a high-assurance OS. When in doubt, choose the more conservative option. Never weaken a capability check, never introduce ambient authority, never suppress a failing security test.
2. **Memory safety through Rust.** All kernel and userspace code is Rust. Every `unsafe` block must have a comment explaining (a) why it is needed, (b) what invariants it upholds, (c) why safer alternatives were rejected. Audit tracking for `unsafe` is defined in [docs/standards/](docs/standards/).
3. **English in the repository.** Source code, comments, doc-comments, documentation, commit messages, PR descriptions, issue text, and this file are English. Conversation with the maintainer in chat may be Turkish, but nothing committed to the repo should be.
4. **Mermaid for diagrams.** All architectural diagrams are inline Mermaid code fences. Do not add PNG, SVG, ASCII-art, or other binary diagram formats.
5. **Record decisions as ADRs.** Any non-trivial architectural, security, or process decision is recorded as an ADR in [docs/decisions/](docs/decisions/) using the MADR template. ADRs are append-only; to override an old decision, write a new ADR that supersedes it.
6. **Respect the pace.** The maintainer explicitly wants methodical, phased progress. For non-trivial work, propose a phase plan first, execute one phase, and pause for review before continuing. Do not dump entire subsystems in a single pass.
7. **No proprietary blobs.** Umbrix does not incorporate proprietary binary firmware or drivers into the kernel. This is a design constraint and affects platform decisions (see ADR-0004).

## Where to find things

| Need | Path |
|------|------|
| High-level architecture | [docs/architecture/](docs/architecture/) |
| Why we chose X | [docs/decisions/](docs/decisions/) |
| How to write docs, code, commits | [docs/standards/](docs/standards/) |
| **Step-by-step procedures for recurring tasks** | [.claude/skills/](.claude/skills/) |
| **What work is active, and what's next** | [docs/roadmap/current.md](docs/roadmap/current.md) |
| **Full phase plan (one file per phase)** | [docs/roadmap/phases/](docs/roadmap/phases/) |
| **Individual task user stories (per-phase)** | [docs/analysis/tasks/](docs/analysis/tasks/) |
| **Reviews (business / code / security / perf)** | [docs/analysis/reviews/](docs/analysis/reviews/) |
| How-to guides | [docs/guides/](docs/guides/) |
| Project-specific terms | [docs/glossary.md](docs/glossary.md) |
| Security policy | [SECURITY.md](SECURITY.md) |
| Contributor expectations | [CONTRIBUTING.md](CONTRIBUTING.md) |
| License | [LICENSE](LICENSE) |

If a path referenced above does not yet exist, it is on the near-term roadmap — check [docs/decisions/README.md](docs/decisions/README.md) and the ADR index for the current state of the project.

## Skills

When the maintainer asks for a recurring task — write an ADR, perform a code review, introduce an `unsafe` block, add a dependency — there is usually a **skill** at `.claude/skills/<slug>/SKILL.md` that describes the correct procedure step by step. Read the skill in full before executing; check the acceptance criteria at the end. Skills are not optional shortcuts; they are how the project keeps recurring work consistent.

Skills follow the Anthropic skill convention so Claude Code can auto-discover them. Non-Claude agents should treat the same files as their canonical procedure library.

See [.claude/skills/README.md](.claude/skills/README.md) for the full index.

## Before starting work

1. Read the ADRs in numerical order — they establish the design language of the project.
2. Read [docs/standards/documentation-style.md](docs/standards/documentation-style.md) before writing or editing documentation.
3. If a task spans more than two or three files, propose a plan before editing.
4. If a task touches security-relevant code (capabilities, IPC, memory, crypto), flag the change for explicit review.

## Escalation

If a requested change would violate any of the seven non-negotiable rules above, stop and ask the maintainer before proceeding. It is better to pause than to silently weaken a guarantee.
