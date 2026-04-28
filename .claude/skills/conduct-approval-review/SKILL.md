---
name: conduct-approval-review
description: Run an independent verification pass over artefacts in `Proposed` / `In Review` waiting-for-promotion states. Distinct from code-review (style + correctness on a diff) and security-review (adversarial axis pass) — this skill verifies that the artefacts' claims about their own state match reality and produces a Done-promotion verdict.
when-to-use: When one or more tasks, ADRs, or audit-log entries reach a waiting-for-promotion status (`In Review`, `Proposed`) and the maintainer wants an independent pair of eyes before flipping the status. Validated 2026-04-27 across three runs (T-006/T-007/T-009 promotion, ADR-0024/0025 Accept, T-008/T-011/T-013 promotion); canonised as a skill at PR #9 closure per the [B0 closure retrospective](../../../docs/analysis/reviews/business-reviews/2026-04-27-B0-closure.md) Adjustments item.
---

# Conduct approval review

## Inputs

- A **list of artefacts under review** — task files (`docs/analysis/tasks/<phase>/T-NNN-*.md`), ADRs (`docs/decisions/NNNN-*.md`), audit-log entries (`docs/audits/unsafe-log.md` UNSAFE-2026-NNNN), or review documents (`docs/analysis/reviews/<type>/YYYY-MM-DD-*.md`) in `Proposed` / `In Review` / similar waiting-for-promotion states.
- The **commit range or PR** that produced the artefacts.
- Any **deliberate deferrals or settled-decision context** the maintainer has already established (e.g. settled items from prior review rounds; rejection rationales).

## What this skill is, and is not

This skill **is**:

- An *independent verification pass* over self-claimed deliveries. Each artefact under review claims, in its own body or review-history table, that certain things were done. The skill checks whether the claims hold against the source-of-truth (code, gates, audit-log entries, cross-references).
- A *promotion-gate* skill. The output is a per-artefact verdict — ✅ ready / 🟡 ready-with-follow-up / ❌ blocked — that the maintainer uses to flip status from `In Review` → `Done` (or `Proposed` → `Accepted`).
- A *fresh-eyes* pass. The reviewer must not have authored the work being reviewed. In a solo / agent-assisted project, "fresh eyes" means a separate agent context invoked via a structured prompt that names what is settled and what to scrutinise.

This skill **is not**:

- A *code-review* (style, correctness on a diff). That is [`perform-code-review`](../perform-code-review/SKILL.md).
- A *security-review* (adversarial axis pass over capabilities, IPC, syscalls, memory, etc.). That is [`perform-security-review`](../perform-security-review/SKILL.md).
- A *retrospective* (what landed, what we learned, adjustments). That is [`conduct-review`](../conduct-review/SKILL.md) with `business-review` as the type.
- A duplicate of any of the above. An approval review may *cite* their findings — for example, "the consolidated security review at `<path>` returned clean; this approval review accepts that as the security-axis verdict" — but it does not redo their work.

The independent-verification pass is necessary because authors are systematically blind to their own claims. T-009's review-fix arc surfaced this: the agent self-reported "all accepted findings fixed"; the maintainer's question ("Have all accepted findings been fixed?") caught a silently-skipped runtime EL check. That exact failure mode — "we say it's done; nobody else checked" — is what this skill closes.

## Procedure

1. **Read the artefact list and the settled-decisions context.** Approval reviews are most useful when the agent does not re-litigate decisions that have already been made. The prompt to the reviewing agent must name explicitly:
   - What artefacts are under review.
   - What decisions were made deliberately and should not be reversed (e.g. "free-function form of `current_el` settled by ADR-0024 §Open questions", "QEMU smoke deferred per settled item N").
   - The commit range that produced the artefacts.

2. **Build a verification list (V1..VN).** For each non-trivial claim an artefact makes about its own state, write one verification item. Each item names the file:line that should evidence the delivery. If the artefact is a task file, walk its Acceptance Criteria + Definition of Done line by line; each item becomes one verification entry.

3. **Run the gates yourself.** Do not trust the artefact's commit message about what gates passed. Reproduce locally:

   ```sh
   cargo fmt --all -- --check
   cargo host-clippy
   cargo kernel-clippy
   cargo host-test
   cargo kernel-build
   cargo +nightly miri test --workspace --exclude tyrne-bsp-qemu-virt
   cargo llvm-cov --workspace --exclude tyrne-bsp-qemu-virt --summary-only
   ```

   If any number drifts from what the artefacts claim, that is a High finding. (T-011's commit body's per-file pre-state test counts were off by 1–2 in two of three files; the headline was correct. This kind of small drift is normal but worth recording.)

4. **Spot-check the audit-log discipline.** For each `unsafe`-touching artefact, verify:
   - Every new `unsafe` block has a `// SAFETY:` comment with (a) why-unsafe-is-required, (b) invariants, (c) rejected alternatives.
   - Each cited audit-log entry is real, complete, and matches the source.
   - Any post-introduction body changes to an audit entry went via Amendment blocks per [`unsafe-policy.md`](../../../docs/standards/unsafe-policy.md) §3 — *not* in-place edits. The introducing-commit boundary, not the merge boundary, locks an entry's body. UNSAFE-2026-0017's "Discipline note for future readers" paragraph is the canonical reference.

5. **Walk the review dimensions across the diff.** For each modified file:
   - **Correctness** — does the code do what the artefact says? Are edge cases handled?
   - **Security** — does any path bypass a capability check or pre-flight?
   - **Performance / optimization** — any path that introduces unjustified overhead?
   - **Potential bugs** — race conditions, TOCTOU, off-by-one, integer overflow.
   - **Refactor suggestions** — places where the code could be simpler without changing behaviour.
   - **Documentation quality** — broken cross-references, stale prose, Mermaid syntax.

   Each finding is an entry, not a free-form note.

6. **Per-artefact AC audit.** For each task file under review, produce a table mapping every AC item and DoD item to ✅ / 🟡 / ❌ with a file:line citation. The maintainer uses this table to flip checkboxes in the task file.

7. **Verdict.** One paragraph per artefact:
   - ✅ ready — promote to Done / Accepted.
   - 🟡 ready with follow-up — promote, but the cited follow-up should land in the next commit / PR.
   - ❌ blocked — do not promote until the cited finding is fixed.

8. **Open questions section** — for decisions you would like the maintainer to confirm but do *not* recommend reversing on your own authority. Especially useful when the work makes a non-obvious choice and the reviewer wants to flag it without claiming it's wrong.

9. **Process notes** — observations about *how* the work was developed (commit granularity, audit-log discipline, review-prompt quality) that the next task should adopt or avoid. This is the slow-feedback channel that makes the project's discipline compound across sessions.

## What to NOT recommend

The settled-decisions context (input #1) names things that should not be re-litigated. Within the review itself, also avoid:

- **Re-litigating decisions documented in ADRs / audit entries.** If the artefact's design call is grounded in a §Decision outcome or a §Rejected alternatives field, recommending its reversal is a different kind of finding (an ADR-level concern, not a promotion-gate concern). Surface as an **Open question** or escalate separately.
- **Style or formatting issues `rustfmt` / `clippy` would catch.** CI is necessary, not always sufficient — but if the gates ran clean, do not re-flag the style.
- **Out-of-scope refactor requests.** "While you're here" comments are a code-review anti-pattern and an approval-review anti-pattern.

## Output format

Structure the review as:

```markdown
## Verdict (one paragraph)
Overall + per-artefact promotion verdicts.

## Verification audit (V1 .. VN)
Table: each item ✅ delivered / 🟡 partial / ❌ missing, with file:line cite.

## New findings
### High (blocking)
### Medium (should fix this round)
### Low (informational)
### Open questions

## Per-artefact AC audit
Per task file under review, AC + DoD as a checklist with file:line cites.

## Gates and counts
Reproduced locally; note any drift from the commit body's claims.

## Process notes
Anything about HOW the work was developed worth carrying forward.
```

## Acceptance criteria

- [ ] Artefact list and settled-decisions context read in full before review starts.
- [ ] Verification list V1..VN written before walking the diff (forces the reviewer to know what they are checking).
- [ ] All gates reproduced locally; any drift from claimed numbers flagged.
- [ ] Audit-log discipline spot-checked for every `unsafe`-touching artefact.
- [ ] Per-artefact AC audit table produced.
- [ ] Per-artefact verdict (✅ / 🟡 / ❌) explicit.
- [ ] Settled-decisions context respected; recommended-reversal items, if any, surfaced as Open questions rather than findings.

## Anti-patterns

- **LGTM without a verification list.** Approval without a written V1..VN is just deference.
- **Re-running tests via the commit message.** "Commit body says 143 host tests" is not "I ran `cargo host-test` and saw 143 host tests." Reproduce locally.
- **Re-litigating settled decisions.** If a free-function-form choice is settled by an ADR, the reviewer who suggests "switch to a trait method" is not adding signal; they are noise. Convert to Open question or escalate separately.
- **Vague verdicts.** "Mostly ready" is not a verdict. ✅ / 🟡 / ❌ with a per-artefact citation is a verdict.
- **Skipping the audit-log spot-check.** Every `unsafe`-touching PR exits with at least one new audit entry; an approval review that does not check the entries against the source is missing the most consequential discipline gate the project has.
- **Treating the AC checklist as decoration.** A task with `Status: In Review` whose AC items are all `[ ]` is not in review — it is in a state-machine corruption that the approval review must surface and (via the maintainer) close.

## References

- [`docs/standards/code-review.md`](../../../docs/standards/code-review.md) — the structured-review checklist this skill leans on for review-dimension shape.
- [`docs/standards/security-review.md`](../../../docs/standards/security-review.md) — the parallel-pass discipline; approval review may cite a separate security-review verdict but does not redo it.
- [`docs/standards/unsafe-policy.md`](../../../docs/standards/unsafe-policy.md) §3 — append-only audit-log discipline; the introducing-commit boundary rule.
- [`perform-code-review`](../perform-code-review/SKILL.md) — what an approval review is *not*.
- [`perform-security-review`](../perform-security-review/SKILL.md) — what an approval review is *not*.
- [`conduct-review`](../conduct-review/SKILL.md) — for retrospectives; what an approval review is *not*.
- [B0 closure retrospective (2026-04-27)](../../../docs/analysis/reviews/business-reviews/2026-04-27-B0-closure.md) §What we learned → "Reviews-as-pattern: now reproducibility-shaped" — the validation that motivated canonisation.
- Validation runs (sample-size 3 at canonisation):
  - 2026-04-27 — T-006/T-007/T-009 promotion (PR #9 first review-fix round).
  - 2026-04-27 — ADR-0024 / ADR-0025 Accept verification.
  - 2026-04-27 — T-008/T-011/T-013 promotion (PR #9 second review-fix round).
