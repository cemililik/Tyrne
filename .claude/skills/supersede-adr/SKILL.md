---
name: supersede-adr
description: Override a prior Accepted ADR with a new one, updating both files so the forward and backward pointers are consistent.
when-to-use: When a decision recorded in a prior ADR is being reversed or significantly changed. Not for minor clarifications (edit the original with a Proposed-amendment note instead).
---

# Supersede ADR

## Inputs

Before starting, the agent must have:

- The **number of the old ADR** being superseded (e.g., `0003`).
- A **justification** — what has changed, what new information or constraint is driving the supersession. This must be different from, or a strengthening of, the reasoning in the original ADR.
- A **new decision outcome** — the new option being chosen.
- If unclear: confirm with the maintainer that supersession (not amendment) is the right move.

## Procedure

1. **Read the old ADR in full.** Understand what it claimed, what alternatives it considered, and what consequences it accepted. Do not supersede what you have not read.

2. **Decide: supersession or amendment?**
   - **Amendment** (not this skill): small clarifications, typo fixes, reference updates. Edit the original ADR in place.
   - **Supersession** (this skill): the decision outcome changes, the considered options change meaningfully, or the consequences shift enough that a reader should understand a new decision has been made.

3. **Write the new ADR** using the [write-adr](../write-adr/SKILL.md) skill.
   - In the **Context** section of the new ADR, explicitly say:
     > This ADR supersedes [ADR-NNNN: Title](NNNN-slug.md). The earlier decision…
     Then explain what was decided originally and what has changed to warrant the new decision.
   - In the **References** section, link the superseded ADR.

4. **Update the old ADR's header.**
   - Change `Status:` from `Accepted` to `Superseded by MMMM` (where MMMM is the new ADR's number).
   - Add a `> **Superseded.**` callout at the very top of the body, before the Context section:
     > **Superseded by [ADR-MMMM: New Title](MMMM-new-slug.md) (YYYY-MM-DD).** The original decision below is preserved for the historical record.
   - **Do not edit the original body.** The original reasoning is the historical record and must stay intact.

5. **Update the ADR index** at [`docs/decisions/README.md`](../../../docs/decisions/README.md):
   - Add a row for the new ADR (Accepted, today's date).
   - Change the old ADR's status column to `Superseded by MMMM`.

6. **Update downstream references.**
   - Any standard, architecture document, or skill that cited the old ADR is updated to cite the new one. Search the repo for the old ADR number.
   - If a downstream document legitimately still refers to the old (historical) reasoning, leave that citation and note the supersession inline.

7. **Commit as a sequence** (ideally two commits for clarity):
   - Commit A: `docs(adr): propose ADR-MMMM — <short title>`, body explains the supersession, trailer `Refs: ADR-MMMM, ADR-NNNN`.
   - Commit B (after ADR-MMMM is Accepted): `docs(adr): supersede ADR-NNNN with ADR-MMMM`, body notes the status flip, trailer `Refs: ADR-NNNN, ADR-MMMM`.

   In solo phase, one combined commit is acceptable if the decision is already settled.

## Acceptance criteria

- [ ] New ADR exists, uses MADR template, justifies supersession in Context.
- [ ] New ADR references the old one in References.
- [ ] Old ADR's Status is `Superseded by MMMM`.
- [ ] Old ADR has a callout at the top linking to the new ADR.
- [ ] Old ADR's body is **unmodified** apart from the Status and the callout.
- [ ] ADR index reflects both changes.
- [ ] Downstream references updated.

## Anti-patterns

- **Editing the old ADR's body** to match the new decision. The historical record must stay.
- **Superseding without new information.** If nothing has changed, the old decision is still right and supersession is not needed.
- **Silent overrides.** Making a contradicting decision in a new ADR without flipping the old one's status leaves the repo with two contradictory "Accepted" ADRs.
- **Forgetting the back-pointer.** The old ADR must link forward to the new one; otherwise a reader following references misses the supersession.
- **Using supersession for a minor fix.** That is an amendment, not a supersession. Edit in place.

## References

- [write-adr](../write-adr/SKILL.md) — used to draft the new ADR.
- [docs/decisions/README.md](../../../docs/decisions/README.md) — ADR process and the supersession convention.
- [commit-style.md](../../../docs/standards/commit-style.md) — commit format.
