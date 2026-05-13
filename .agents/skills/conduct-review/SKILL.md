---
name: conduct-review
description: Produce a review artifact in `docs/analysis/reviews/<type>/`, following that type's master plan. Works for business / code / security / performance-optimization reviews.
when-to-use: When a trigger for any of the four review types fires — milestone completion (business), non-trivial change (code), security-sensitive change (security), or performance concern (performance-optimization). The review type is chosen by the trigger.
---

# Conduct review

## Inputs

- The **review type** — `business`, `code`, `security`, or `performance-optimization`.
- The **scope** — a milestone id (for business), a change identifier (for code / security), or a concern statement (for performance-optimization).
- The **trigger** — milestone completion, maintainer-initiated, sensitive-change detection, performance concern, etc.
- Access to the master plan for the selected type:
  - [`business-reviews/master-plan.md`](../../../docs/analysis/reviews/business-reviews/master-plan.md)
  - [`code-reviews/master-plan.md`](../../../docs/analysis/reviews/code-reviews/master-plan.md)
  - [`security-reviews/master-plan.md`](../../../docs/analysis/reviews/security-reviews/master-plan.md)
  - [`performance-optimization-reviews/master-plan.md`](../../../docs/analysis/reviews/performance-optimization-reviews/master-plan.md)

## Procedure

1. **Select the type** from the trigger.
   - Milestone completion or maintainer-initiated retrospective → `business`.
   - Non-trivial code change → `code`.
   - Change touching capabilities / IPC / syscalls / memory / scheduler / boot / crypto / `unsafe` / sensitive deps (per [`security-review.md`](../../../docs/standards/security-review.md)) → `security`.
   - Performance concern or hypothesis → `performance-optimization`.

2. **Open the type's master plan** and read it in full. Each master plan is self-contained and describes its agent roles, procedure, merge step, acceptance criteria, and output template.

3. **Read the previous review of the same type on the same scope, if any.** Reviews build on each other; orphan reviews drift.

4. **Execute the roles** described in the master plan. Multi-agent setups can run roles in parallel; single-agent sessions run them sequentially. Either way, each role produces one section of the final artifact.

5. **Merge** per the master plan's merge step.

6. **Write the artifact** at `docs/analysis/reviews/<type>/YYYY-MM-DD-<context>.md`.
   - `<context>` is a short kebab-case slug: milestone id for business, PR or commit id for code / security, concern slug for performance.
   - Use the master plan's output template. Do not reshape sections.

7. **Update the type's index** in `docs/analysis/reviews/<type>/README.md` with a new row for this review.

8. **Type-specific side-effects:**
   - **business** — update [`docs/roadmap/current.md`](../../../docs/roadmap/current.md) with the new active phase / milestone / task / next-review-trigger per the master plan's Pathfinder role.
   - **code** — the corresponding commit should gain a reference to this review (convention emerging; once stable, a `Code-Review:` trailer).
   - **security** — the corresponding commit gets a `Security-Review:` trailer per [`commit-style.md`](../../../docs/standards/commit-style.md); any `unsafe` changes in scope are cross-referenced with their audit-log entries.
   - **performance-optimization** — if the verdict is Merge, the merge commit references this artifact.

9. **Commit** per [`commit-style.md`](../../../docs/standards/commit-style.md):
   - Scope: `roadmap` for business reviews (they touch `current.md` and the roadmap's forward motion); `analysis` for code / security / performance-optimization reviews (their artifact lives in `docs/analysis/reviews/<type>/`).
   - Message: `docs(<scope>): <type>-review <slug>` — e.g. `docs(roadmap): business-review A2-completion`, `docs(analysis): code-review PR-42`, `docs(analysis): security-review cap-table-revoke`, `docs(analysis): performance-review ipc-hot-path`.
   - Body: one-paragraph summary of the verdict / adjustments.
   - Trailer: `Refs: ADR-0013` and any ADRs the review cites.

## Acceptance criteria

- [ ] Correct review type chosen for the trigger.
- [ ] Master plan for that type followed — every role section present in the output.
- [ ] Artifact at `docs/analysis/reviews/<type>/YYYY-MM-DD-<context>.md`.
- [ ] Verdict stated in the artifact.
- [ ] Type-specific side-effects applied (see step 8).
- [ ] Type's index README updated.

## Anti-patterns

- **Wrong type.** A security-sensitive change reviewed as `code` alone is a failure. When in doubt, run both.
- **Skipping the master-plan read.** The master plan is the truth of the procedure; reviewing from memory drifts.
- **Missing a role.** Each role's section is load-bearing. "I didn't have time for integration" is not a valid outcome; it is a blocker.
- **Forgetting the type-specific side-effect.** The commit trailer, the `current.md` update, the audit-log cross-reference — these are not optional.
- **Reviewing the same change twice in the same type.** If a previous review exists, amend it or note it explicitly; do not overwrite.

## References

- [ADR-0013: Roadmap and planning process](../../../docs/decisions/0013-roadmap-and-planning.md).
- [`docs/analysis/reviews/README.md`](../../../docs/analysis/reviews/README.md) — cross-cutting review philosophy.
- The four master plans:
  - [business](../../../docs/analysis/reviews/business-reviews/master-plan.md)
  - [code](../../../docs/analysis/reviews/code-reviews/master-plan.md)
  - [security](../../../docs/analysis/reviews/security-reviews/master-plan.md)
  - [performance-optimization](../../../docs/analysis/reviews/performance-optimization-reviews/master-plan.md)
- [`commit-style.md`](../../../docs/standards/commit-style.md) — commit format and the `Security-Review:` trailer.
- [`perform-code-review`](../perform-code-review/SKILL.md) and [`perform-security-review`](../perform-security-review/SKILL.md) — the skills that execute code / security reviews during development; they call back to this skill for artifact production.
