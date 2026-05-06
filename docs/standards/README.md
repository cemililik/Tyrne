# Standards

This folder holds the prescriptive rules that contributors — human and AI — are expected to follow when writing documentation, source code, commits, reviews, security analysis, logs, and release artifacts for Tyrne.

Standards are narrow and enforceable. They are not where we discuss philosophy (that belongs in ADRs) or how to do things step-by-step (that belongs in `../guides/`). Think of them as the short list a reviewer checks a change against.

## Index

| Document | Scope | Status |
|----------|-------|--------|
| [architectural-principles.md](architectural-principles.md) | The non-negotiable design invariants distilled from the ADRs. Every change must respect these. | Accepted |
| [code-style.md](code-style.md) | Rust style: formatter, linter, naming, module organization, doc comments, capability conventions, `no_std` discipline. | Accepted |
| [unsafe-policy.md](unsafe-policy.md) | How `unsafe` is justified in `SAFETY:` comments, audited, tracked in an audit log, and reviewed. | Accepted |
| [error-handling.md](error-handling.md) | `Result` discipline, panic policy, per-module error types, kernel fault containment, ISR rules. | Accepted |
| [testing.md](testing.md) | Four test layers (unit, integration, QEMU smoke, hardware smoke), coverage expectations, test-naming. | Accepted |
| [code-review.md](code-review.md) | Author prep, reviewer checklist, PR size, approval semantics, solo-phase self-review. | Accepted |
| [security-review.md](security-review.md) | Dedicated review pass for capability / IPC / memory / boot / crypto / `unsafe` changes. | Accepted |
| [commit-style.md](commit-style.md) | Conventional Commits format, scopes, trailers (`Refs: ADR-NNNN`, `Audit:`, `Security-Review:`), granularity. | Accepted |
| [logging-and-observability.md](logging-and-observability.md) | Structured records, five levels, secret redaction, ISR-safe ring path, spans, metrics. | Accepted |
| [infrastructure.md](infrastructure.md) | Toolchain pinning, dependency policy, CI gates, supply-chain (`cargo-vet`, `cargo-audit`, SBOM), reproducibility, branch protection, secrets. | Accepted |
| [bsp-boot-checklist.md](bsp-boot-checklist.md) | The reset-vector and early-boot discipline every BSP must observe (DAIF mask, EL drop to EL1, SP / BSS, FP/SIMD enable, vector-table install, GIC init, IRQ unmask order). | Accepted |
| [release.md](release.md) | Semver convention, changelog, release gates (process / content / security), signing, rollback, security releases. | Accepted |
| [localization.md](localization.md) | UTF-8 internal, English kernel output, no locale in the kernel, localization is a userspace concern. | Accepted |
| [documentation-style.md](documentation-style.md) | English-only docs, Mermaid-only diagrams, file structure, linking, file naming, change policy. | Accepted |

## How standards relate to other things in the repo

- **To ADRs.** An ADR says *"here is why we chose this way over alternatives"*. A standard says *"do it this way"*. If a standard does something non-obvious, it cites the ADR that justifies it. Changing a standard means updating (or writing) the underlying ADR first.
- **To guides.** A guide is task-oriented: *"how do I do X?"*. It assumes the standards and walks through a procedure. Guides live in [`../guides/`](../guides/).
- **To principles.** [`architectural-principles.md`](architectural-principles.md) is the meta-standard: the invariants every other standard and every code change must preserve.
- **To CLAUDE.md / AGENTS.md.** The top-level agent guides enumerate the non-negotiable rules at a glance; the standards here are the operational detail.

## Proposing a change to a standard

1. Identify the ADR (or ADRs) that motivated the current form of the standard.
2. Write an ADR that proposes the new position and explicitly references and, if necessary, supersedes the old one.
3. Update the standard file once the ADR is Accepted.
4. Link between them in both directions.

Standards are not edited casually. If the ADR says one thing and the standard says another, the ADR wins and the standard is fixed.

## Reading order for newcomers to the standards

1. [architectural-principles.md](architectural-principles.md) — understand the invariants first.
2. [documentation-style.md](documentation-style.md) — how to read and contribute to docs.
3. [code-style.md](code-style.md) — how Rust is written here.
4. [unsafe-policy.md](unsafe-policy.md) and [error-handling.md](error-handling.md) — the two most distinctive standards for kernel work.
5. Everything else as the situation demands.
