---
name: update-glossary
description: Add or update an entry in `docs/glossary.md`, preserving format and alphabetical order.
when-to-use: Whenever a new project-specific term is introduced or an existing term's meaning is refined. Used in tandem with ADRs, architecture docs, and standards that introduce vocabulary.
---

# Update glossary

## Inputs

- The **term** being added or updated (in the form it will appear: capitalization, attribution if it is a borrowed term).
- A **one-to-three-paragraph definition** of the term.
- The **context**: what docs motivated the addition; what existing docs should cross-link to this entry.

## Procedure

1. **Read the current glossary.** Open [`docs/glossary.md`](../../../docs/glossary.md) and check whether the term is already present.
   - If present: this is an update. Read the existing entry carefully; preserve its structure; change only what needs changing.
   - If absent: this is an addition.

2. **Determine alphabetical position.** The glossary is sorted alphabetically by headword. Ignore leading articles ("the", "a"). Locate the correct insertion point.

3. **Write or update the entry.** Format:

   ```markdown
   **Term.** One-sentence definition that could stand alone. Follow-up sentences or a short paragraph explaining nuance, origin, or relationship to other terms. Link to the relevant ADR or architecture doc on the first substantive cross-reference.
   ```

   - The term appears in **bold**, followed by a period and a space. No headings per term — the format is a flowing list.
   - The first sentence is self-contained; it is sufficient as a quick-reference answer.
   - Subsequent sentences add nuance, attribution, or relationship to other glossary terms.
   - If the term is borrowed from another system (seL4, Hubris, POSIX), note the attribution.

4. **Insert alphabetically.** Place the entry between the entries that bracket it alphabetically. The glossary uses `---` separators only between groups of entries (e.g., at the top); do not insert or remove `---` while adding an entry.

5. **Cross-link outward.** If the new term appears in one or more existing docs, add links from those docs to the glossary entry on first use: `[term](../glossary.md)` or equivalent relative path.

6. **Cross-link inward.** If the new entry references other glossary terms ("see also capability"), link to those entries within the glossary itself.

7. **Commit** per [commit-style.md](../../../docs/standards/commit-style.md):
   - Message: `docs(glossary): add <term>` or `docs(glossary): refine <term>`.
   - Body: optional; the diff is usually self-explanatory for single-term changes.

## Acceptance criteria

- [ ] Term in **bold** format.
- [ ] First sentence self-contained.
- [ ] Alphabetical order preserved.
- [ ] Related terms cross-linked.
- [ ] Existing docs that use the term link to the glossary on first use.
- [ ] Attribution given if the term is borrowed from another system.

## Anti-patterns

- **Entries that are longer than the concept warrants.** If an entry grows beyond ~150 words, it probably wants its own architecture document or ADR, linked from the glossary.
- **Circular definitions.** "A capability is a capability token." Define in terms of simpler concepts.
- **Missing attribution for borrowed terms.** "Endpoint" in a capability context has history (seL4); say so.
- **Headings per entry.** The glossary is intentionally flat; do not introduce `##` sub-sections.
- **Out-of-order insertion.** The alphabetical invariant is load-bearing for grep-ability.
- **Turkish or other non-English entries.** The glossary follows [documentation-style.md](../../../docs/standards/documentation-style.md) — English only.

## References

- [docs/glossary.md](../../../docs/glossary.md) — the glossary itself.
- [documentation-style.md](../../../docs/standards/documentation-style.md) — docs style rules.
- [commit-style.md](../../../docs/standards/commit-style.md) — commit format.
