# AI agents guide — Umbrix

All AI agents working on this repository should read this file first.

The canonical agent guide is [CLAUDE.md](CLAUDE.md). It is written with Claude-based tooling in mind, but its rules apply to every AI agent — regardless of model or runner:

1. Security-first mindset.
2. Memory safety through Rust; disciplined, justified, audited `unsafe`.
3. English inside the repository (chat with the maintainer may be Turkish; commits must not be).
4. Mermaid for diagrams.
5. Non-trivial decisions are recorded as ADRs in [docs/decisions/](docs/decisions/).
6. Phased, methodical pace — propose, execute one phase, pause for review.
7. No proprietary binary blobs.

Before acting:

1. Read [CLAUDE.md](CLAUDE.md) in full.
2. Check [docs/roadmap/current.md](docs/roadmap/current.md) — this is where the project says what is active right now.
3. Read the ADRs in [docs/decisions/](docs/decisions/) in numerical order.
4. Read the standards in [docs/standards/](docs/standards/).
5. Check [.claude/skills/](.claude/skills/) — each recurring task has a `SKILL.md` file under `<slug>/`. If the task matches one, follow that skill's procedure.

If the task is non-trivial, propose a plan before editing. If a requested change would violate any of the seven rules above, stop and ask the maintainer.
