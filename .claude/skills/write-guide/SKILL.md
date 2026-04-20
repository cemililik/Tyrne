---
name: write-guide
description: Write a new task-oriented guide under `docs/guides/` that walks a contributor or user through a specific task.
when-to-use: When a repeatable task would benefit from a step-by-step walkthrough — toolchain setup, running the kernel under QEMU, porting to a new board, writing a driver.
---

# Write guide

## Inputs

- The **task** the guide covers (one task, one guide).
- The **audience** — first-time contributor, experienced kernel developer, porter, operator.
- A **kebab-case slug** for the filename.

## Procedure

1. **Confirm the guide does not already exist.** Check [`docs/guides/README.md`](../../../docs/guides/README.md) and the files on disk. If a similar guide exists, update it instead of creating a new one (this skill covers both).

2. **Create the file** at `docs/guides/<slug>.md`.

3. **Structure the guide** with these sections, in this order:

   ```markdown
   # <Task title in imperative>

   <One-paragraph summary: what this guide accomplishes and who it is for.>

   ## Goal

   <Bulleted or prose statement of the outcome. "After this guide, you will have X running / Y configured / Z ported.">

   ## Prerequisites

   <Bulleted list. Software installed, repos cloned, accounts created, hardware available. Link out to prerequisite guides where applicable.>

   ## Steps

   1. **<Short imperative step>.** <Explanation. Fenced code block(s) with command lines.>
   2. **<Next step>.** …

   <Use `###` sub-sections if the number of steps exceeds ~8 and they group naturally.>

   ## Verifying it worked

   <How the reader knows they succeeded. Specific output to look for; command to run; file to inspect.>

   ## Troubleshooting

   <Common failure modes and fixes. Optional but recommended.>

   ## References

   <ADRs, architecture docs, standards, external links that are relevant.>
   ```

4. **Write for a reader cold.** Assume the reader has general kernel-development literacy but has never done this specific task. Do not assume they have read the rest of the repo. Link out when background is needed.

5. **Commands in code blocks.** Every command goes in a fenced code block with the `sh` language tag:

   ````markdown
   ```sh
   rustup target add aarch64-unknown-none
   ```
   ````

   Do not prefix commands with `$`. Do not mix command lines with output inside the same block unless the output is minimal and obviously distinct.

6. **Outputs in a separate block** when showing expected output:

   ````markdown
   ```text
   info: installing component 'rust-src'
   ```
   ````

7. **File paths and placeholders.**
   - Absolute paths verbatim.
   - Placeholders in `<angle-brackets>`: `<board-name>`, `<version>`, `<your-local-path>`.

8. **Follow [documentation-style.md](../../../docs/standards/documentation-style.md)** — English, Mermaid for any diagrams, relative links within the repo.

9. **Update the index** at [`docs/guides/README.md`](../../../docs/guides/README.md) — add a row for the new guide with audience and status.

10. **Commit** per [commit-style.md](../../../docs/standards/commit-style.md):
    - Message: `docs(guides): <slug>` — e.g. `docs(guides): toolchain-setup`.
    - Body: what the guide walks through.

## Acceptance criteria

- [ ] File at `docs/guides/<slug>.md`.
- [ ] Goal and Prerequisites sections present.
- [ ] Steps section with numbered, imperative steps.
- [ ] Commands in `sh` code blocks.
- [ ] Verifying-it-worked section present.
- [ ] Troubleshooting section present or explicitly noted as "none at time of writing."
- [ ] Guides index updated.
- [ ] Documentation style followed.

## Anti-patterns

- **A "guide" that is actually architecture documentation.** If the content is descriptive, not procedural, it belongs in `docs/architecture/`.
- **Undocumented prerequisites.** A reader who discovers on step 4 that they needed to install X on step 0 will blame the guide, correctly.
- **No verification step.** A guide that cannot be verified to have worked is a guide that cannot be debugged.
- **Untagged code fences.** ` ``` ` without `sh` / `rust` / `text` is a review rejection.
- **Writing for yourself.** If only the author can follow the guide, the guide has failed.

## References

- [documentation-style.md](../../../docs/standards/documentation-style.md) — applies to guides as to every doc.
- [docs/guides/README.md](../../../docs/guides/README.md) — the index.
- [commit-style.md](../../../docs/standards/commit-style.md) — commit format.
