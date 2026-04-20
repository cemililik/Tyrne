# Reviews

Umbrix runs four kinds of reviews, each addressing a distinct concern. Each review type has its own folder with a master plan describing the multi-agent procedure and a log of dated review artifacts.

Reviews are **event-triggered**, not calendar-triggered. See each type's README for its specific triggers.

## The four types

| Type | Folder | Concern | Trigger |
|------|--------|---------|---------|
| **Business** | [`business-reviews/`](business-reviews/) | Milestone retrospectives and roadmap-level direction. What landed, what changed in the plan, what was learned. | Milestone completion; maintainer-initiated. |
| **Code** | [`code-reviews/`](code-reviews/) | Per-change code quality: correctness, style, test coverage, documentation. | Per non-trivial PR / change (once PR flow begins). |
| **Security** | [`security-reviews/`](security-reviews/) | Deep security pass on changes that touch capabilities, IPC, syscalls, memory, boot, crypto, `unsafe`, or security-sensitive dependencies. | Per change that hits the triggers in [`../../standards/security-review.md`](../../standards/security-review.md). |
| **Performance optimization** | [`performance-optimization-reviews/`](performance-optimization-reviews/) | Hypothesis-driven performance cycles: baseline → hotspot → proposal → measurement → regression check. | Periodic or on concern (user-reported slowness, scaling limits). |

## Per-type structure

Every review-type folder has the same three things:

1. **`README.md`** — what this review type is, when to conduct one, how the output differs from a task artifact.
2. **`master-plan.md`** — the procedure, written to be **parallelizable across multiple AI agents**. Enumerates agent roles, each role's checklist, and the merge step that combines their outputs.
3. **`YYYY-MM-DD-<context>.md`** — one file per review conducted.

The master plan is a living document. It is amended as the project learns how to review itself better; the amendments are recorded inline.

## Multi-agent review

Each master plan describes **agent roles**. For a single-maintainer solo phase, these roles can all be played by the same person (or the same AI) in sequence. The value of naming them separately is twofold:

1. Each role is a focused checklist that is easier to execute without losing state than a monolithic "do a review" instruction.
2. When AI tooling supports parallel agents, the roles compose directly: agent X does correctness, agent Y does style, agent Z does security-implications, and a merge step combines their sections.

The procedure does not *require* multiple agents; the single-agent sequential path is always valid. The multi-agent option exists for when it is efficient.

## Producing a review

Use the [`conduct-review`](../../../.claude/skills/conduct-review/SKILL.md) skill with the review type as an argument. The skill loads the type's master plan and walks through its steps.

For code and security reviews specifically, the execution skills [`perform-code-review`](../../../.claude/skills/perform-code-review/SKILL.md) and [`perform-security-review`](../../../.claude/skills/perform-security-review/SKILL.md) describe how to *do* the review during development; they produce the artifact in the corresponding folder as their final step.

## See also

- [ADR-0013 — Roadmap and planning process](../../decisions/0013-roadmap-and-planning.md).
- [`../tasks/`](../tasks/) — task files reviews refer to.
- [`../../roadmap/current.md`](../../roadmap/current.md) — points at the active work a review may be assessing.
