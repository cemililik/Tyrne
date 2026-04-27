---
name: write-adr
description: Propose and draft a new Architecture Decision Record (ADR) in MADR format for Tyrne.
when-to-use: When a non-trivial architectural, security, platform, or process decision is being made, or when a change depends on a decision that has not yet been recorded.
---

# Write ADR

## Inputs

Before starting, the agent must have:

- A **short kebab-case slug** for the decision (e.g., `kernel-allocator`, `log-wire-format`).
- A **context statement**: what situation the project is in, what question it is facing, what the stakes are.
- At least **two considered options**, with real pros and cons for each.
- A **decision outcome** — which option is being chosen and why.
- If unclear: ask the maintainer before proceeding.

## Procedure

1. **Determine the next ADR number.**
   - List files in `docs/decisions/` matching `NNNN-*.md`.
   - The next number is the highest existing number + 1, zero-padded to four digits.

2. **Create the file** at `docs/decisions/NNNN-<slug>.md`.

3. **Copy the shape** from [`docs/decisions/template.md`](../../../docs/decisions/template.md). Do not skip sections.

4. **Fill the header:**
   - `Status: Proposed` (or `Accepted` if the decision is being recorded after the fact, with agreement from the maintainer).
   - `Date:` today's date in ISO-8601 (`YYYY-MM-DD`).
   - `Deciders: @cemililik` (add others if applicable).

5. **Fill the body:**
   - **Context.** One to three paragraphs. State the problem, the constraints, and what will go wrong if the decision is left implicit.
   - **Decision drivers.** Bulleted list of the forces pushing toward or away from each option. These must be specific to this decision; do not reuse generic drivers.
   - **Considered options.** At least two; three is often better. Each option gets a one-sentence description here.
   - **Decision outcome.** State the chosen option and connect the decision drivers to the choice. One or two paragraphs.
   - **Decision outcome → Dependency chain.** A subsection listing every task / piece of infrastructure / prior decision that must already exist for the chosen option to be **fully** in effect. Each line either points at an existing T-NNN file or is opened as part of the same commit that lands the ADR (per [ADR-0025](../../../docs/decisions/0025-adr-governance-amendments.md) §Rule 1 — "future, not-yet-opened task" wording is forbidden). If a step has no T-NNN slot, **stop** and open the slot first; the ADR cannot Accept until the chain is grounded.
   - **Consequences.** Three subsections: *Positive*, *Negative*, *Neutral*. Negative consequences must include a mitigation or an explicit "we accept this cost because…".
   - **Pros and cons of the options.** For each option, a short list of pros and cons. The rejected options need real cons, not strawmen.
   - **References.** External links, prior art, papers, existing systems. At least one reference for anything non-obvious.

6. **Check for contradictions** with prior ADRs.
   - If the new ADR contradicts an existing one, stop — use the [supersede-adr](../supersede-adr/SKILL.md) skill instead. Do not silently override.

7. **Update the ADR index** at [`docs/decisions/README.md`](../../../docs/decisions/README.md):
   - Add a new row at the bottom of the index table.
   - Format: `| NNNN | [Title](NNNN-slug.md) | Proposed | YYYY-MM-DD |`.

8. **Cross-link** the new ADR from any standard or architecture document that motivated it.

9. **Commit** per [commit-style.md](../../../docs/standards/commit-style.md):
   - Message: `docs(adr): propose ADR-NNNN — <short title>`.
   - Body: one or two sentences explaining the decision.
   - Trailer: `Refs: ADR-NNNN`.

10. **Careful re-read before Accept.** The ADR lands at status `Proposed`. Before flipping to `Accepted`, re-read the ADR end-to-end — out loud or in a fresh editor pane — paying particular attention to: (a) every forward-reference points at a real T-NNN (per [ADR-0025](../../../docs/decisions/0025-adr-governance-amendments.md) §Rule 1); (b) the dependency chain is complete and each step's slot exists; (c) the *Negative consequences* are real costs the project is willing to pay, not hand-waved mitigations. An earlier draft of ADR-0025 imposed a hard 24-hour cool-down between `Proposed` and `Accepted` — that rule was withdrawn before Accept on maintainer judgement that the *substance* (deliberate re-reading) is achievable through this step plus independent agent reviews, without a calendar delay (see [ADR-0025 §Revision notes](../../../docs/decisions/0025-adr-governance-amendments.md)). Same-day Accept is therefore permitted, *provided* this re-read step actually happens; if the re-read surfaces a gap, fix it and re-read again before flipping the status. Accept is a separate commit from the initial Propose commit so that the careful-re-read pass shows up as its own diff.

## Acceptance criteria

- [ ] File exists at `docs/decisions/NNNN-<slug>.md`.
- [ ] All MADR sections filled with real content (no `<angle-bracket>` placeholders from the template).
- [ ] At least two considered options with real pros and cons.
- [ ] **Decision outcome includes a *Dependency chain* subsection** with every step grounded in either an existing T-NNN file or one opened as part of the same commit. No "future, not-yet-opened task" wording.
- [ ] Consequences include both positive and negative items.
- [ ] ADR index at [`docs/decisions/README.md`](../../../docs/decisions/README.md) has the new row.
- [ ] No contradiction with a prior Accepted ADR; if there was one, the [supersede-adr](../supersede-adr/SKILL.md) skill was used instead.
- [ ] **Initial commit lands the ADR at `Proposed`.** The Propose commit is separate from any subsequent Accept commit so the careful-re-read pass shows up as its own diff. Accept may follow same-day after the re-read of step 10 (no calendar gate per [ADR-0025 §Revision notes](../../../docs/decisions/0025-adr-governance-amendments.md)), but never in the same commit as the initial draft.

## Anti-patterns

- **Single-option ADR.** "We chose X. Here is why X is good." Without real alternatives considered, an ADR is marketing, not a record.
- **Strawman alternatives.** "Option B: use C. This is obviously bad." The rejected options must be credible.
- **Template placeholders in the committed file.** `<Option A>` in the pros-and-cons section is a failed review.
- **Skipping "Consequences — Negative".** Every non-trivial decision has costs. Pretending otherwise is unhelpful.
- **Writing the ADR after the code.** ADRs are proposed before work begins, accepted when the design is settled. Retroactive ADRs are acceptable only as a recovery move and should be marked explicitly.
- **Skipping the careful re-read before Accept.** Accept is permitted same-day, but only if step 10's deliberate re-read actually happens. Flipping `Proposed → Accepted` in the same diff as the initial draft (or without a real second pass) is the failure mode that ADR-0025's withdrawn cool-down rule was originally trying to prevent — the rule was withdrawn, the discipline was not.
- **Forward-reference handwaving.** "Future task X will do Y" — without X being a real T-NNN file — drifts into purgatory. Open the slot in the same commit, even as `Draft`. (Per [ADR-0025 §Rule 1](../../../docs/decisions/0025-adr-governance-amendments.md).)

## References

- [docs/decisions/README.md](../../../docs/decisions/README.md) — ADR process and index.
- [docs/decisions/template.md](../../../docs/decisions/template.md) — the template to copy from.
- [commit-style.md](../../../docs/standards/commit-style.md) — commit format.
- [architectural-principles.md](../../../docs/standards/architectural-principles.md) — principles that constrain every ADR.
