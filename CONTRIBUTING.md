# Contributing to Tyrne

Thank you for your interest. Tyrne is currently in the **architecture phase** — the foundational design documents are being written and the codebase is not yet open for code contributions.

## What is useful right now

- Reading the [architecture documents](docs/architecture/) and [ADRs](docs/decisions/) and opening issues if something seems unclear, inconsistent with prior art, or contradicted by experience from related systems.
- Suggesting references — academic papers, existing systems (seL4, Hubris, Tock, Redox, Fuchsia/Zircon, Theseus, Drawbridge), talks, books — that would strengthen or refute specific design decisions.
- Reviewing the [standards](docs/standards/) in this repository and proposing improvements via issues.
- Catching typos, broken links, or unclear passages in the documentation; small PRs for those are welcome.

## What is not useful yet

- Pull requests against source code. There is no source code to extend or refactor meaningfully yet. Adding code before the architecture settles would force premature rewrites.
- Feature requests for subsystems that have not yet been designed. File those as discussion issues if you want to influence the design, not as feature requests.

## When the project enters the implementation phase

This document will be expanded with:

- Branching strategy and release process.
- PR template and review expectations.
- CI gates (tests, clippy, rustfmt, capability auditing, `unsafe` auditing).
- Coding conventions (see [docs/standards/](docs/standards/)).

## Communication

- Architectural questions or ambiguities → issues in this repository.
- Security-relevant observations → see [SECURITY.md](SECURITY.md).

## Licensing of contributions

All contributions to Tyrne are licensed under the [Apache License, Version 2.0](LICENSE). By submitting a contribution, you agree that it may be redistributed under those terms, and you confirm that you have the right to submit it.
