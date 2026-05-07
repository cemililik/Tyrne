# Business review master plan

A business review is the milestone-level retrospective. Its goal is to capture in writing what the project just learned so that returning to the project after weeks does not require page-in from scratch.

This plan is written to be parallelizable across multiple agents, but is equally valid as a single-agent sequential walkthrough. When run solo, execute the roles in order; the merge step is trivial.

## Inputs

- The **scope** being reviewed — a milestone id (e.g., `A2`) or `adhoc-<label>`.
- The **trigger** — milestone completion, maintainer-initiated, or phase closure.
- The **date range** — the last business review (if any) through today.
- Access to: commit history (`git log`), task files under [`../../tasks/`](../../tasks/), ADRs added in the period, and the previous business review.

## Output

A file at `docs/analysis/reviews/business-reviews/YYYY-MM-DD-<scope>.md` following the five-section shape below. The file is added to the index in this folder's [`README.md`](README.md).

## Agent roles

A single reviewer can cover all roles sequentially; the benefit of naming them is that each role is a narrow, focused pass.

### 1. Chronicler

**Task:** enumerate what landed.

- Walk the git history for the period.
- For each commit, note: SHA (short), date, subject line, which task (if any) it advanced.
- For each ADR that landed, note its number, title, and status.
- For each task that reached `Done`, note its id and one-sentence summary.

**Output (into "What landed" section):** three bulleted lists — commits, ADRs, tasks closed.

### 2. Plan-diff

**Task:** enumerate how the plan itself changed.

- Diff [`../../roadmap/phases/`](../../roadmap/phases/) between the previous review and today.
- Identify: added / removed / reordered milestones, added / deprecated tasks at the phase level, wording changes to acceptance criteria, any structural changes.
- Cross-check against the decision log: ADRs that superseded earlier ones or amended the roadmap belong here.

**Output (into "What changed in the plan" section):** bulleted list with a diff summary per change.

### 3. Learning

**Task:** extract genuine new understanding from the period.

- Read the period's ADRs, especially their "Consequences — Negative" and "Open questions" sections. Learnings often hide there.
- Read any security review and performance review from the period. Their "what we learned" sections feed into the business review.
- Walk the task review-history tables for notes that might have escaped other agents.
- Reject filler. "Things went well" is not a learning. "Option B for the capability table required more `unsafe` than expected; indexed approach is confirmed right" is.
- If nothing was learned, say so in one sentence and move on. A review with no learnings is fine if the work was purely execution.

**Output (into "What we learned" section):** prose paragraphs, honest and specific.

### 4. Adjuster

**Task:** translate the learnings into concrete adjustments.

- For each learning that implies action, propose: a new task (to be opened via [`start-task`](../../../../.claude/skills/start-task/SKILL.md)), an ADR to write (via [`write-adr`](../../../../.claude/skills/write-adr/SKILL.md)), or a standard/guide update.
- For each adjustment, name the trigger (the next thing to do to act on it).
- Do not execute the adjustments here — just record them. Execution happens after the review is committed.

**Output (into "Adjustments" section):** checklist-style bullet list.

### 5. Pathfinder

**Task:** state where the project goes next.

- Active phase after this review.
- Active milestone after this review.
- The first task that should become `In Progress` next — typically already chosen in the Adjuster's output.
- The next review trigger — usually "milestone X completion".

**Output (into "Next" section):** a four-line block of these four values.

## Merge step

When roles are run in parallel, combine outputs by pasting each role's section into a single file in the order above. The Adjuster's output is checked against the Pathfinder's: if the adjustments list implies a different "next" than the Pathfinder proposed, reconcile before committing.

## Acceptance criteria

- [ ] File at `docs/analysis/reviews/business-reviews/YYYY-MM-DD-<scope>.md`.
- [ ] Frontmatter (trigger, scope, period, participants) filled.
- [ ] All five body sections present. A section may be intentionally brief if nothing substantive happened, but it is not skipped.
- [ ] Each role's output has been produced by the maintainer or an agent; notes are traceable.
- [ ] The index in [`README.md`](README.md) has a new row for this review.
- [ ] [`../../../roadmap/current.md`](../../../roadmap/current.md) is updated to match the Pathfinder's output.
- [ ] **For *milestone-completion* and *phase-closure* triggers only:** the *What landed* section includes a verbatim copy of the most recent QEMU smoke trace (full serial output through the milestone's success marker, plus the `boot-to-end elapsed = ... ns` line if the BSP emits it) and an `-d int,unimp,guest_errors` event count. A milestone cannot promote past `In Review` to `Done` without this evidence; *narrative* claims of smoke-pass are insufficient. Codified after the [2026-05-06 B1 smoke-regression mini-retro](2026-05-06-B1-smoke-regression.md) and the [2026-05-07 B1 closure retrospective](2026-05-07-B1-closure.md) §"What we learned" — host tests + miri + paper-review can pass a kernel that hangs at runtime; the smoke is the project's only end-to-end liveness oracle. Maintainer-initiated mini-retros and adhoc reviews are exempt (no closure semantics).

## Output template

```markdown
# Business review YYYY-MM-DD — <scope>

- **Trigger:** <milestone-completion | maintainer-initiated | phase-closure>
- **Scope:** <milestone id or "adhoc">
- **Period:** <previous review date> → <today>
- **Participants:** @cemililik (+ any AI agent acting as scribe)

## What landed

<Chronicler output>

## What changed in the plan

<Plan-diff output>

## What we learned

<Learning output>

## Adjustments

<Adjuster output>

## Next

- Active phase: <letter>
- Active milestone: <id>
- Active task: <T-NNN>
- Next review trigger: <condition>
```

## Anti-patterns

- **Activity log instead of learning.** "I wrote the capability table. I ran the tests." The commit history already says this.
- **Vague learnings.** "Things went well" — unusable.
- **Reviewing without reading the previous review.** Reviews build on each other.
- **Skipping sections.** Five sections, every time. Sparse is better than missing.
- **Hiding problems.** Hiding is more expensive than recording. Record and move on.
- **Too-frequent reviews.** A review per task is noise. Milestone-level or trigger-based only.

## Amendments

This master plan is a living document. When the process evolves, amend the relevant sections and note the change at the bottom here.

- _2026-04-20_ — initial version; scaffolded by [ADR-0013](../../../decisions/0013-roadmap-and-planning.md).
