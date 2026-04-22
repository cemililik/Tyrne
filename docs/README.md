# Tyrne documentation

This is the canonical documentation tree for Tyrne. It is organized by the *kind of question* each section answers, not by the component of the system it describes. This way, when a reader has a question, they know which folder to open before they know which subsystem touches the answer.

## Layout

| Folder | Answers the question |
|--------|----------------------|
| [architecture/](architecture/) | **How is Tyrne built?** High-level design, subsystem decompositions, component interactions, data and control flow, the security model. |
| [decisions/](decisions/) | **Why is Tyrne built this way?** Architecture Decision Records (ADRs) in MADR format. One ADR per non-trivial choice. |
| [guides/](guides/) | **How do I do X?** Task-oriented walkthroughs: setting up the toolchain, running the kernel under QEMU, porting to a new board, writing a new userspace driver. |
| [standards/](standards/) | **How should things be written?** Documentation style, code style, commit message style, review checklists, security-review checklist. |
| [glossary.md](glossary.md) | Project-specific terminology. |

## Suggested reading order for newcomers

1. [glossary.md](glossary.md) — terms used throughout the project.
2. [decisions/](decisions/) — the numbered ADRs, in order. These capture the reasoning behind the design and are the fastest way to get oriented.
3. [architecture/](architecture/) — start with the overview (Phase 2), then dive into whichever subsystem interests you.
4. [standards/documentation-style.md](standards/documentation-style.md) — before you send a documentation PR.

## Conventions in this tree

- **Language:** English only. See [ADR-0005](decisions/0005-documentation-language-english.md).
- **Diagrams:** Mermaid only, embedded as inline fenced code blocks. No binary diagram formats. See [standards/documentation-style.md](standards/documentation-style.md).
- **Links:** relative within this tree (e.g. `../decisions/0001-microkernel-architecture.md`), absolute for external resources.
- **File names:** `kebab-case.md`. ADRs are `NNNN-short-slug.md`.
- **Tone:** explain the reasoning, not just the outcome. Prefer depth over brevity when a topic is subtle; give the *why* and the trade-offs, not just the *what*.
