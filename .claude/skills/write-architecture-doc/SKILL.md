---
name: write-architecture-doc
description: Write or update a document under `docs/architecture/` describing a subsystem, component, or cross-cutting flow.
when-to-use: When a new subsystem is being designed, an existing subsystem's design changes materially, or the maintainer requests documentation for a specific part of the system.
---

# Write architecture doc

## Inputs

- The **subsystem or topic** the document covers (e.g. `kernel-core`, `ipc`, `security-model`, `hal`).
- The **slug** for the filename (kebab-case, matches the topic).
- A **pointer to the relevant ADR(s)** whose reasoning this document reflects. Architecture docs are the "how"; ADRs are the "why".

## Procedure

1. **Check the index** at [`docs/architecture/README.md`](../../../docs/architecture/README.md).
   - If the document is listed as planned, pick up its slug and status.
   - If it is not listed, add a row to the index first (or update this skill's output to include that change).

2. **Create the file** at `docs/architecture/<slug>.md`.

3. **Structure the document:**

   ```markdown
   # <Title>

   <One-paragraph summary. A reader who reads only this paragraph must be able to say what this document covers and who it is for.>

   ## Context

   <What this subsystem is responsible for, and which ADRs drove its design. One or two paragraphs with ADR links.>

   ## Design

   <The actual how. Subdivide with `###` headings as the topic requires: components, data structures, flows, boundaries.>

   ### <Component or flow 1>

   <Prose description. Include a Mermaid diagram if it clarifies boundaries, flow, or state transitions.>

   ### <Component or flow 2>

   …

   ## Invariants

   <Bulleted list of the invariants this subsystem maintains. These are the claims a reader can rely on and a tester can exercise.>

   ## Trade-offs

   <What this design gives up, and why. This is a design document, not marketing — the downsides must be present.>

   ## Open questions

   <Questions still to be answered. Linked to issues or future ADRs where applicable.>

   ## References

   - ADRs and external literature.
   ```

4. **Write the summary paragraph carefully.** This paragraph is the document's thesis. A reader who skims only the summary should come away with the subsystem's purpose and its place in the system.

5. **Use Mermaid for every diagram.** Per [documentation-style.md](../../../docs/standards/documentation-style.md):
   - Inline fenced code blocks with the `mermaid` language tag.
   - Preceded by a prose description (accessibility — screen readers depend on this).
   - Earn their place: boundary, flow, or state machine, not decoration.
   - Keep small (< 15 nodes). Split if larger.

   Example:

   ````markdown
   The IPC send flow crosses three boundaries: syscall entry, endpoint rendezvous, and receiver delivery.

   ```mermaid
   sequenceDiagram
       participant S as Sender task
       participant K as Kernel
       participant R as Receiver task

       S->>K: syscall: send(endpoint, msg)
       K->>K: check send capability
       K->>R: deliver if receiver waiting, else block
       R->>K: return after receive
       K->>S: return Ok
   ```
   ````

6. **Cross-reference ADRs** wherever the document makes a claim that rests on a decision. Format: `see [ADR-NNNN: Title](../decisions/NNNN-slug.md)`.

7. **Link the glossary** on first use of any project-specific term (`see [capability](../glossary.md)`).

8. **Update the index.**
   - Edit [`docs/architecture/README.md`](../../../docs/architecture/README.md) — change the document's status column from `Planned` to `Accepted` (or `Draft` if it is a substantial but incomplete pass).
   - Update the index description if the document's scope differs from what was planned.

9. **Commit** per [commit-style.md](../../../docs/standards/commit-style.md):
   - Message: `docs(arch): <subsystem>` — e.g. `docs(arch): ipc design`.
   - Body: a sentence or two on what the document covers.
   - Trailer: `Refs: ADR-NNNN` for each ADR the document reflects.

## Acceptance criteria

- [ ] File created at `docs/architecture/<slug>.md`.
- [ ] Summary paragraph present and substantive.
- [ ] Context section cites the ADR(s) the document reflects.
- [ ] Mermaid diagrams used where they earn their place, with prose preceding each.
- [ ] Invariants section enumerates concrete, testable claims.
- [ ] Trade-offs section is honest — the downsides are named.
- [ ] Cross-links to ADRs and glossary wherever applicable.
- [ ] Architecture index updated.
- [ ] `Refs: ADR-NNNN` trailer in the commit.

## Anti-patterns

- **No summary paragraph.** A reader should not have to get ten paragraphs in to discover what the document is about.
- **Decorative diagrams.** A diagram that does not clarify anything is noise.
- **No Trade-offs section.** Every design has costs. Hiding them is a review failure.
- **ADR claims unmoored.** "Umbrix does X" without a link to the ADR that decided X.
- **Mismatched scope.** A document titled "IPC" that secretly also covers scheduling — split it.
- **Untestable invariants.** "The system is secure" is not an invariant. "The kernel checks `SendCap` before transferring a message" is.
- **Inline PNG / SVG.** Only Mermaid.

## References

- [documentation-style.md](../../../docs/standards/documentation-style.md) — Mermaid rules, structure, linking.
- [docs/architecture/README.md](../../../docs/architecture/README.md) — the index and the list of planned documents.
- [docs/decisions/](../../../docs/decisions/) — the ADRs this document reflects.
- [commit-style.md](../../../docs/standards/commit-style.md) — commit format.
