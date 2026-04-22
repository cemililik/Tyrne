---
name: propose-standard-change
description: Change an existing Tyrne standard correctly — write or update the motivating ADR first, then update the standard file.
when-to-use: Whenever a rule in `docs/standards/` needs to change. Covers additions, removals, and modifications. Does not cover typo fixes, which are ordinary edits.
---

# Propose standard change

## Inputs

- The **standard file** being changed (e.g. `docs/standards/code-style.md`).
- The **nature of the change**: what rule is being added, removed, or modified, and why.
- **Evidence that the change is warranted**: a recurring problem the current rule causes, a new ADR that demands the change, a principle update.

## Procedure

1. **Classify the change.**
   - **Typo / clarity / link fix** — this is not a "standard change"; edit the file directly with a normal docs PR.
   - **Substantive change** — a rule changes, is added, or is removed. Continue this skill.

2. **Find the motivating ADR.** Standards implement decisions; decisions live in ADRs.
   - If the standard's rule was put there by a prior ADR, find that ADR.
   - If no ADR explicitly motivated the rule, the current standard is effectively an implicit decision that never had its ADR. The first step is to write the ADR that captures it — then supersede it — rather than silently changing the standard.

3. **Decide ADR path.**
   - **Amendment of existing ADR.** The old ADR is still largely right; it just needs to be updated or clarified. Use the *amendment* pattern — edit the ADR in place with a Proposed-amendment note, get it accepted, then update the standard.
   - **Supersession.** The old decision is being reversed or significantly changed. Use [supersede-adr](../supersede-adr/SKILL.md): write a new ADR that supersedes the old one.
   - **New ADR, no supersession.** The change is additive — a new principle, a new constraint — that the old ADRs did not address. Use [write-adr](../write-adr/SKILL.md) to record the new decision.

4. **Author the ADR first.** Standards follow decisions. Do not change the standard file until the ADR is `Accepted`. Drafting both in parallel is fine; committing the standard change before the ADR is not.

5. **Update the standard file.** Once the ADR is Accepted:
   - Edit `docs/standards/<file>.md`.
   - Make the change — minimally, without rewriting adjacent rules that are not part of this change.
   - Add or update the **reference** at the bottom of the standard to point to the motivating ADR.
   - If a rule has changed semantically, update any examples that illustrated the old rule.

6. **Update the [architectural-principles.md](../../../docs/standards/architectural-principles.md)** document **if** the change affects one of the P1–P12 principles. A principle change is a major event and usually warrants an explicit ADR of its own.

7. **Search for downstream effects.**
   - Other standards that cite the changed rule or principle.
   - ADRs whose outcome depended on the old rule.
   - Skills (`.claude/skills/`) that invoke the changed rule as a step.
   - Architecture documents that rested on the rule.
   - Update each one or open an issue to track the update if it is non-trivial.

8. **Commit as a sequence** per [commit-style.md](../../../docs/standards/commit-style.md):
   - Commit A: the ADR. Message: `docs(adr): propose ADR-NNNN — <short>`.
   - Commit B: the standard change. Message: `docs(standards): <what-changed>` — e.g. `docs(standards): allow workspace-root clippy allow-list`.
   - Trailers in both: `Refs: ADR-NNNN`.
   - For supersessions, also include `Refs: ADR-MMMM` (the superseded ADR).

## Acceptance criteria

- [ ] Motivating ADR written (or identified if pre-existing).
- [ ] ADR is `Accepted` before the standard file is modified.
- [ ] Standard file change minimal; unchanged adjacent rules untouched.
- [ ] References at the bottom of the standard point to the new ADR.
- [ ] Downstream effects searched; affected docs updated or tracked.
- [ ] Commit sequence is ADR → standard, not the reverse.

## Anti-patterns

- **Changing the standard without an ADR.** Silent standard drift is exactly what the ADR process exists to prevent.
- **Reverse commit order.** Committing the standard change first leaves the repo in a state where the standard contradicts the still-accepted prior ADR.
- **Rewriting adjacent rules.** A PR to change one rule should not become a rewrite of the whole standard. Split.
- **Skipping the downstream search.** A changed standard with stale references elsewhere fragments the decision record.
- **Treating a typo fix as a standard change.** Ordinary docs PRs for typos and clarifications are fine and should not go through this skill.

## References

- [write-adr](../write-adr/SKILL.md) — when a new ADR is needed.
- [supersede-adr](../supersede-adr/SKILL.md) — when the change reverses a prior decision.
- [docs/standards/README.md](../../../docs/standards/README.md) — index of current standards.
- [architectural-principles.md](../../../docs/standards/architectural-principles.md) — the twelve principles.
- [commit-style.md](../../../docs/standards/commit-style.md) — commit format.
