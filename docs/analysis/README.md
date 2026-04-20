# Analysis

This folder is **the work**, tracked as it moves through the pipeline. Where [`../roadmap/`](../roadmap/) holds the plan, `analysis/` holds the execution:

- [`tasks/`](tasks/) — individual task user stories, organized per phase.
- [`reviews/`](reviews/) — reviews, organized per type (business, code, security, performance-optimization).

The separation is deliberate. The roadmap describes what we *intend*; analysis records what *happens*. The two are cross-linked in both directions but have different readers, different update cadences, and different lifetimes.

See [ADR-0013](../decisions/0013-roadmap-and-planning.md) for the rationale.

## Tasks

Tasks are the unit of work. Each one is a short markdown file following the user-story shape in [`tasks/TEMPLATE.md`](tasks/TEMPLATE.md).

- Tasks live under `tasks/phase-<letter>/` — the phase folder tells you which milestone the task is part of.
- Task IDs (`T-NNN`) are sequential across the whole project; they do not restart per phase.
- See [`tasks/README.md`](tasks/README.md) for the index, ID schema, status vocabulary, and lifecycle.

Tasks are created with the [`start-task`](../../.claude/skills/start-task/SKILL.md) skill.

## Reviews

Reviews come in four types. Each type has its own folder, its own master plan, and its own trigger criteria.

| Type | Folder | Trigger | Produces |
|------|--------|---------|----------|
| **Business** | [`reviews/business-reviews/`](reviews/business-reviews/) | Milestone completion; maintainer-initiated. | Retrospective covering what landed / what changed / what was learned / what's next. |
| **Code** | [`reviews/code-reviews/`](reviews/code-reviews/) | Per non-trivial change, once PR flow begins. | Quality review (correctness / style / tests / docs) — one file per change. |
| **Security** | [`reviews/security-reviews/`](reviews/security-reviews/) | Per change touching security-sensitive subsystems. | Deep security pass across capability / memory / kernel-discipline / crypto / secrets / dependencies axes. |
| **Performance optimization** | [`reviews/performance-optimization-reviews/`](reviews/performance-optimization-reviews/) | Periodic or on concern. | Hypothesis-driven perf cycle: baseline, hotspot, proposal, measurement, regression check. |

Each folder contains a `README.md` describing when that type applies, a `master-plan.md` with a detailed multi-agent procedure, and one dated file per review conducted.

Reviews are produced with the [`conduct-review`](../../.claude/skills/conduct-review/SKILL.md) skill, which takes the review type as input and applies that type's master plan.

See [`reviews/README.md`](reviews/README.md) for the cross-cutting review philosophy.

## Cross-links with the roadmap

- A task's frontmatter names the **phase** and **milestone** it belongs to; those are defined in [`../roadmap/phases/`](../roadmap/phases/).
- [`../roadmap/current.md`](../roadmap/current.md) always names the currently active task.
- A business review following a milestone closure references the tasks that made up the milestone.
- A code / security / performance review references the specific commit or PR it is assessing.

## Cross-links with decisions

- A task may require an ADR as an acceptance criterion. The task file names the expected ADR number.
- A review may produce new tasks or recommend ADRs; those adjustments are recorded in the review's "Adjustments" section and then executed via the `start-task` / `write-adr` skills.
