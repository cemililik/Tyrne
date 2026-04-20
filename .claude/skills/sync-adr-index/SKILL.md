---
name: sync-adr-index
description: Rebuild the ADR index table in `docs/decisions/README.md` from the actual ADR files on disk.
when-to-use: After adding, superseding, or deprecating ADRs in a way that the index may have fallen out of sync. Also as a periodic maintenance task.
---

# Sync ADR index

## Inputs

None. This skill scans the filesystem and reconciles.

## Procedure

1. **Enumerate ADR files.**
   - List every file in `docs/decisions/` matching the pattern `NNNN-*.md`, where `NNNN` is a zero-padded four-digit number.
   - Exclude `README.md` and `template.md`.
   - Sort numerically by `NNNN`.

2. **Read metadata from each ADR.**
   - Open the file.
   - Extract:
     - **Number** — from the filename.
     - **Title** — from the first `#` heading, with the "NNNN — " prefix stripped.
     - **Status** — from the `- **Status:** ...` line in the header block. Valid values: `Proposed`, `Accepted`, `Deprecated`, `Superseded by NNNN`.
     - **Date** — from the `- **Date:** ...` line.
   - If any field is missing or malformed, record the anomaly and continue. Anomalies are reported at the end, not silently ignored.

3. **Cross-check supersession.**
   - For every ADR whose status is `Superseded by MMMM`, confirm that ADR `MMMM` exists and is `Accepted`.
   - For every ADR whose body (or `References`) points forward to a superseding ADR, confirm the older ADR's status actually says `Superseded by`.
   - Record inconsistencies as anomalies.

4. **Rebuild the index table.** In [`docs/decisions/README.md`](../../../docs/decisions/README.md), replace the contents of the `## Index` section's table with freshly generated rows:

   ```markdown
   | # | Title | Status | Date |
   |---|-------|--------|------|
   | 0001 | [<Title>](0001-slug.md) | Accepted | 2026-04-20 |
   | 0002 | [<Title>](0002-slug.md) | Accepted | 2026-04-20 |
   ...
   ```

   - Rows are in numeric order.
   - Each title is a relative link to its ADR file.
   - Status column reflects exactly what the ADR header says.
   - Date column is the ADR's authoritative `Date:` field.

5. **Do not touch the rest of `README.md`.** The surrounding sections (Why ADRs, Format, Creating a new ADR) are not this skill's responsibility.

6. **Report anomalies** to the maintainer, if any:
   - ADRs missing a Status or Date.
   - ADRs with status `Superseded by MMMM` where MMMM does not exist or is not Accepted.
   - ADRs whose title does not match their filename slug.
   - Gaps in numbering (unless justified — e.g. ADR-0003 was proposed and rejected before being saved; record the gap in the anomalies report).

7. **Commit** per [commit-style.md](../../../docs/standards/commit-style.md):
   - Message: `docs(adr): sync index`.
   - Body: optional; if the sync resolves any anomalies, note them.

## Acceptance criteria

- [ ] Index table rows match the set of `NNNN-*.md` files on disk.
- [ ] Each row's title, status, and date match the corresponding ADR's header.
- [ ] Supersession back-pointers consistent in both directions.
- [ ] Anomalies (if any) reported explicitly, not silently patched.

## Anti-patterns

- **Editing ADR bodies during a sync.** This skill only touches `docs/decisions/README.md`.
- **Silently fixing anomalies.** If an ADR lacks a Date, the sync reports the missing field; it does not guess.
- **Reformatting the surrounding `README.md` prose.** The table is all this skill rebuilds.
- **Renumbering ADRs to close gaps.** ADR numbers are stable history; gaps are acceptable, renumbering breaks citations.

## References

- [docs/decisions/README.md](../../../docs/decisions/README.md) — the index this skill maintains.
- [write-adr](../write-adr/SKILL.md) — used when adding an ADR.
- [supersede-adr](../supersede-adr/SKILL.md) — used when overriding an ADR.
- [commit-style.md](../../../docs/standards/commit-style.md) — commit format.
