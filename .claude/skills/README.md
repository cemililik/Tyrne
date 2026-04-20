# Skills

Task-specific guides for AI agents working on Umbrix. Each skill describes **one recurring task** — how to perform it correctly against this project's standards and decision log. Skills are procedural, not expository: they tell an agent *what to do, in what order, and how to know they are done*.

Skills live under `.claude/skills/<name>/SKILL.md`. This layout follows the Anthropic skill convention so that Claude Code can auto-discover them; other AI agents are directed here by [AGENTS.md](../../AGENTS.md) and [CLAUDE.md](../../CLAUDE.md).

## Relationship to other agent files

- [CLAUDE.md](../../CLAUDE.md) and [AGENTS.md](../../AGENTS.md) — the **rules** that constrain every agent action. Read first.
- [docs/standards/](../../docs/standards/) — the **standards** that define how code, commits, reviews, logs, and releases should look.
- [docs/decisions/](../../docs/decisions/) — the **decisions** that give context for why the rules and standards exist.
- **This folder (`.claude/skills/`)** — the **procedures** an agent follows when the maintainer asks for a specific recurring task.

Rules and standards tell an agent what *must* be true; skills tell an agent what to *do* to make it true.

## When an agent uses a skill

An agent uses the skill at `.claude/skills/<slug>/SKILL.md` when:

- The maintainer references it by name ("follow the `write-adr` skill").
- The task described in the skill's `description` frontmatter matches what the maintainer is asking for.
- The agent independently recognizes that the task at hand is one of the recurring tasks covered by a skill.

Before executing a skill, the agent reads the skill file in full and applies every step, checking the acceptance criteria at the end. Skipping steps is not allowed: if a step is judged non-applicable, the agent writes a short note in chat saying so and why.

## Format

Every skill lives in its own directory at `.claude/skills/<slug>/` and contains at least a `SKILL.md` file with this shape:

```markdown
---
name: <slug>
description: <what the skill does — used by agents to decide when to use it>
when-to-use: <situations that should trigger this skill>
---

# <Title>

## Inputs
<What the agent needs before starting.>

## Procedure
<Numbered, specific steps.>

## Acceptance criteria
<How the agent knows the skill was executed correctly.>

## Anti-patterns
<Common shortcuts the agent must avoid.>

## References
<Standards and ADRs the skill relies on.>
```

A skill directory may contain additional files (templates, scripts, examples) that `SKILL.md` references. Skills that fit in a single file — which is the current case for every Umbrix skill — only need `SKILL.md`.

Skills are short. If a skill needs more than ~200 lines, either (a) the underlying task is actually two tasks and should be split, or (b) the task has non-skill-sized complexity and belongs in a guide under [docs/guides/](../../docs/guides/) instead.

## Index

| Skill | Purpose |
|-------|---------|
| [write-adr](write-adr/SKILL.md) | Propose and draft a new ADR using the MADR template. |
| [supersede-adr](supersede-adr/SKILL.md) | Override a prior ADR with a new one, updating both directions. |
| [propose-standard-change](propose-standard-change/SKILL.md) | Change a standard correctly (ADR first, then standard file). |
| [perform-code-review](perform-code-review/SKILL.md) | Run a structured code-review pass per the code-review standard. |
| [perform-security-review](perform-security-review/SKILL.md) | Run the dedicated security-review pass for security-sensitive changes. |
| [justify-unsafe](justify-unsafe/SKILL.md) | Introduce or audit an `unsafe` block with `SAFETY:` comment and audit-log entry. |
| [add-dependency](add-dependency/SKILL.md) | Add a new Rust crate following the dependency policy. |
| [write-architecture-doc](write-architecture-doc/SKILL.md) | Write or update an architecture document with Mermaid diagrams. |
| [write-guide](write-guide/SKILL.md) | Write a new task-oriented guide under `docs/guides/`. |
| [update-glossary](update-glossary/SKILL.md) | Add or update an entry in the glossary. |
| [sync-adr-index](sync-adr-index/SKILL.md) | Rebuild the ADR index table from the files on disk. |

## Conventions for adding a new skill

1. Identify a task the project performs **at least three times** that has a non-trivial correct procedure. One-off tasks do not need a skill.
2. Create a directory `.claude/skills/<slug>/` where `<slug>` is a kebab-case imperative-verb name (`add-driver`, not `driver-addition`).
3. Create `SKILL.md` inside it with the frontmatter above and the section structure.
4. Fill the frontmatter with an accurate `description` and `when-to-use`.
5. Keep the procedure to numbered steps that a cold-start agent could execute.
6. Reference the standards and ADRs — do not duplicate their content in the skill.
7. Add the new skill to the index above.
8. Commit with `docs(skills): add <slug>`.
