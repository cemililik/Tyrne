# Umbrix roadmap

The roadmap is **the plan** — what we intend to build and in what order. It answers two questions:

1. **Where are we going?** — [`phases/`](phases/) (Phase A through Phase J).
2. **Where are we now?** — [`current.md`](current.md).

The plan is separate from the work. Individual tasks and reviews live under [`../analysis/`](../analysis/) and are cross-linked from here. See [ADR-0013](../decisions/0013-roadmap-and-planning.md) for the rationale behind this split.

## What lives here

| Path | Purpose |
|------|---------|
| [`README.md`](README.md) | This file. |
| [`phases/README.md`](phases/README.md) | Index of phase files with a one-paragraph summary each. |
| [`phases/phase-<letter>.md`](phases/) | Detailed breakdown of one phase, with its milestones and acceptance criteria. |
| [`current.md`](current.md) | Short pointer to the active phase, milestone, and task. Updated often. |

## What does **not** live here

- **Individual task user stories** — they live under [`../analysis/tasks/phase-<letter>/`](../analysis/tasks/).
- **Reviews** — they live under [`../analysis/reviews/<type>/`](../analysis/reviews/).
- **Design rationale for architectural choices** — [`../decisions/`](../decisions/) (ADRs).
- **Procedural rules** — [`../standards/`](../standards/).
- **Repeatable procedures** — [`../../.claude/skills/`](../../.claude/skills/).

## How to read the roadmap

- **New to the project:** read [`phases/README.md`](phases/README.md), then skim the phase files top-to-bottom. Then look at [`current.md`](current.md) to see where work is.
- **Returning after a pause:** open [`current.md`](current.md) first. It points to the active phase / milestone / task and the date of the last review.
- **Planning the next work session:** [`current.md`](current.md) names the active task; the task file lists its acceptance criteria; the phase file shows what it leads to.

## Conventions

- **Phase IDs** are letters (A, B, C, …), in execution order, stable once published.
- **Milestone IDs** are `<phase><N>` (A1, A2, …), stable once the milestone becomes active.
- **Task IDs** (`T-NNN`) are stable across the whole project and live under [`../analysis/tasks/`](../analysis/tasks/).

See [ADR-0013](../decisions/0013-roadmap-and-planning.md) for the full identifier and change-process rules.

## Changing the roadmap

- **Tweaking a phase's milestones or sub-breakdowns** — edit the phase file.
- **Adding or dropping a phase, or reordering phases at the top level** — structural change; requires an ADR that supersedes the affected statements.
- **Opening a new task** — [`start-task`](../../.claude/skills/start-task/SKILL.md) skill.
- **Running a review** — [`conduct-review`](../../.claude/skills/conduct-review/SKILL.md) skill.
