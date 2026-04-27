# Business reviews

Milestone retrospectives and strategic-direction checks. A "business review" in a solo OS-project context is the kind of honest, what-just-happened conversation a team would have at the end of a sprint — compressed into a written artifact because there is nobody else in the room.

## When to conduct

- **End of every milestone.** When all tasks in a milestone reach `Done`, a business review is produced before the next milestone is declared active.
- **Maintainer-initiated.** Anytime the project feels drifted, the maintainer can call a review. Typical trigger: returning to the project after a pause.
- **Phase closure.** The last milestone of a phase triggers both its normal milestone review and, implicitly, a phase-level summary at the top of that review.

## What this review produces

A dated file `YYYY-MM-DD-<scope>.md` in this folder, following the shape in [`master-plan.md`](master-plan.md). Five sections: what landed, what changed in the plan, what we learned, adjustments, next.

## What this review is not

- It is not a **code review**. Code-level concerns go to [`../code-reviews/`](../code-reviews/).
- It is not a **security review**. Security-sensitive changes go to [`../security-reviews/`](../security-reviews/).
- It is not a **performance review**. Perf cycles go to [`../performance-optimization-reviews/`](../performance-optimization-reviews/).

A business review may point at outcomes from those other reviews as part of "what landed" — it does not duplicate their content.

## Index

| Date | Scope | File |
|------|-------|------|
| 2026-04-21 | Milestone A2 — Capability table foundation | [2026-04-21-A2-completion.md](2026-04-21-A2-completion.md) |
| 2026-04-21 | A6 completion / Phase A retrospective (A3–A6) | [2026-04-21-A6-completion.md](2026-04-21-A6-completion.md) |
| 2026-04-22 | T-006 mini-retro — raw-pointer scheduler API refactor | [2026-04-22-T-006-mini-retro.md](2026-04-22-T-006-mini-retro.md) |
| 2026-04-27 | T-009 mini-retro — Timer impl + lessons-into-rules in ADR-0013 | [2026-04-27-T-009-mini-retro.md](2026-04-27-T-009-mini-retro.md) |
