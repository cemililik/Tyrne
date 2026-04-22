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

## Acceptance criteria

- [ ] File exists at `docs/decisions/NNNN-<slug>.md`.
- [ ] All MADR sections filled with real content (no `<angle-bracket>` placeholders from the template).
- [ ] At least two considered options with real pros and cons.
- [ ] Consequences include both positive and negative items.
- [ ] ADR index at [`docs/decisions/README.md`](../../../docs/decisions/README.md) has the new row.
- [ ] No contradiction with a prior Accepted ADR; if there was one, the [supersede-adr](../supersede-adr/SKILL.md) skill was used instead.

## Anti-patterns

- **Single-option ADR.** "We chose X. Here is why X is good." Without real alternatives considered, an ADR is marketing, not a record.
- **Strawman alternatives.** "Option B: use C. This is obviously bad." The rejected options must be credible.
- **Template placeholders in the committed file.** `<Option A>` in the pros-and-cons section is a failed review.
- **Skipping "Consequences — Negative".** Every non-trivial decision has costs. Pretending otherwise is unhelpful.
- **Writing the ADR after the code.** ADRs are proposed before work begins, accepted when the design is settled. Retroactive ADRs are acceptable only as a recovery move and should be marked explicitly.

## References

- [docs/decisions/README.md](../../../docs/decisions/README.md) — ADR process and index.
- [docs/decisions/template.md](../../../docs/decisions/template.md) — the template to copy from.
- [commit-style.md](../../../docs/standards/commit-style.md) — commit format.
- [architectural-principles.md](../../../docs/standards/architectural-principles.md) — principles that constrain every ADR.
