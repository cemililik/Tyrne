---
name: start-task
description: Open a new roadmap task — create the user-story file from the template under the right phase folder, set its status, update current.md if it goes active.
when-to-use: When the next task in the active milestone becomes ready to pick up, or when the maintainer asks to create a task for a new piece of work that fits the roadmap. Not for changes to the roadmap's structure (adding phases / milestones) — those need an ADR per ADR-0013.
---

# Start task

## Inputs

- The **phase and milestone** the task belongs to (e.g. `A`, `A2`).
- A **short imperative title** (e.g. "Capability table foundation").
- Enough context to write a useful user story: who wants this, what they want, why.
- The **dependencies** — prior tasks (T-NNN) that must be `Done` before this one starts, if any.
- Whether an **ADR is required** before the code lands.

## Procedure

1. **Read the current roadmap state.**
   - Check [`docs/roadmap/current.md`](../../../docs/roadmap/current.md).
   - Confirm the phase and milestone exist in [`docs/roadmap/phases/phase-<letter>.md`](../../../docs/roadmap/phases/).
   - If the milestone is not on the roadmap, stop and ask the maintainer — a missing milestone is a structural change and needs an ADR per [ADR-0013](../../../docs/decisions/0013-roadmap-and-planning.md).

2. **Assign the next T-NNN.**
   - List files across all phase folders: `docs/analysis/tasks/phase-*/T-*.md`.
   - The next number is the highest existing T-NNN + 1, zero-padded to three digits. Start at `T-001` if none exist.
   - IDs are sequential across the whole project, not restarted per phase.
   - **Single-writer assumption.** Two concurrent task-creation flows can both observe the same "highest existing T-NNN" and allocate the same ID. v1 relies on a *single active writer* per branch: run this skill on a branch rebased onto `development`, and if at merge time another `T-NNN-*.md` with your chosen id has landed, re-run the numbering step (list again, pick a fresh highest+1, `git mv` the file, update every internal reference). A ledger-based reservation mechanism may arrive later; for now, the single-writer convention is the contract.

3. **Create the task file** at `docs/analysis/tasks/phase-<letter>/T-NNN-<kebab-slug>.md`.
   - Copy from [`docs/analysis/tasks/TEMPLATE.md`](../../../docs/analysis/tasks/TEMPLATE.md).
   - Fill the frontmatter fields: `Phase`, `Milestone`, `Status`, `Created`, `Author`, `Dependencies`, `Informs`, `ADRs required`. No placeholder values left from the template.

4. **Fill the body sections:** user story, context, acceptance criteria (concrete and testable), out of scope, approach, definition of done, design notes, references, review history.

5. **Status choice.**
   - If dependencies are not yet `Done`, start at `Draft` (criteria not settled) or `Ready` (criteria settled, just waiting).
   - If the task is being picked up for immediate work, move to `In Progress`.
   - Do not jump straight to `In Review` or `Done`.

6. **Update the phase's task index** at `docs/analysis/tasks/phase-<letter>/README.md`. Add the new task to its table.

7. **Update the phase plan** at `docs/roadmap/phases/phase-<letter>.md` if the task is mentioned by id in the milestone's `Tasks under <Mx>` list.

8. **Update [`docs/roadmap/current.md`](../../../docs/roadmap/current.md)** if the status is `In Progress`.
   - The `Active task` line points at the new file.
   - If another task was `In Progress`, complete it first or move it explicitly to `Blocked` / `Ready` with a note.

9. **Cross-references.**
   - If the task requires an ADR, ensure the ADR number is reserved (inspect the highest existing ADR in `docs/decisions/`) and noted in `ADRs required`.
   - If prior tasks inform this one, open those tasks and add this T-NNN in their `Informs` field (bidirectional linking).

10. **Commit.**
    - Scope: `docs`.
    - Message: `docs(roadmap): open T-NNN — <short title>`.
    - Body: one-sentence user story plus the acceptance-criteria count and the phase/milestone.
    - Trailers: `Refs: <ADR-NNNN>` for any referenced ADR.

## Acceptance criteria

- [ ] New file at `docs/analysis/tasks/phase-<letter>/T-NNN-<slug>.md`.
- [ ] Frontmatter fields are filled — no placeholders from the template.
- [ ] User story section answers *who / what / why* in one sentence.
- [ ] Acceptance criteria are concrete and testable (3–8 items; at most 10).
- [ ] Out-of-scope section is explicit.
- [ ] If `In Progress`, [`docs/roadmap/current.md`](../../../docs/roadmap/current.md) points at it.
- [ ] The phase's task index has a new row.
- [ ] The phase plan mentions the task under the right milestone.
- [ ] Bidirectional dependency links are in place.

## Anti-patterns

- **Creating a task with vague criteria.** "Do the IPC work" is not acceptance. "Send and receive work on an endpoint with one sender and one receiver; a host test demonstrates the round trip" is.
- **Opening a task whose milestone does not exist on the roadmap.** Structural change; write an ADR per [ADR-0013](../../../docs/decisions/0013-roadmap-and-planning.md) first.
- **Re-numbering an existing task** on a split. Preserve the original ID; split produces new T-NNNs.
- **Skipping `current.md` update** when moving to `In Progress`. The pointer file is the first thing a returning maintainer reads.
- **Duplicating acceptance criteria between tasks.** Each criterion belongs to exactly one task.
- **Placing the task in the wrong phase folder.** The folder must match the task's `Phase` frontmatter.

## References

- [ADR-0013: Roadmap and planning process](../../../docs/decisions/0013-roadmap-and-planning.md).
- [`docs/analysis/README.md`](../../../docs/analysis/README.md) — analysis layout.
- [`docs/analysis/tasks/README.md`](../../../docs/analysis/tasks/README.md) — task conventions.
- [`docs/analysis/tasks/TEMPLATE.md`](../../../docs/analysis/tasks/TEMPLATE.md) — the template this skill copies.
- [`docs/roadmap/phases/`](../../../docs/roadmap/phases/) — phase files.
- [`commit-style.md`](../../../docs/standards/commit-style.md) — commit format.
- [`write-adr`](../write-adr/SKILL.md) — used when the task requires a fresh ADR.
