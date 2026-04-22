# Code reviews

Per-change code quality review: correctness, style, test coverage, documentation. One artifact per non-trivial change, so "has this been reviewed?" is answerable from the repo.

## When to conduct

- **Every PR / non-trivial change** once the project has moved past the bootstrap phase and PRs become routine.
- **Not for trivial changes.** A typo fix, a single-line comment update, or a formatter-only diff does not require a code-review artifact. The line is judgement — if you would pause at the PR to think about it, produce a review.
- **During the solo phase,** the maintainer performs code-review passes on their own work. The artifact exists so that future-maintainer or a contributor arriving later can see the review trail.

## What this review produces

A dated file `YYYY-MM-DD-<context>.md` in this folder, following the shape in [`master-plan.md`](master-plan.md). Sections: correctness, style, test coverage, documentation, verdict.

## Relationship to the `perform-code-review` skill

[`perform-code-review`](../../../../.claude/skills/perform-code-review/SKILL.md) describes how to **conduct** the review during development. This folder holds the resulting artifact.

For security-sensitive changes, a code review is **not** sufficient — a security review (in [`../security-reviews/`](../security-reviews/)) is additionally required. The code-review artifact notes which security reviews are expected or already produced.

## Index

| Date | Scope | File |
|------|-------|------|
| 2026-04-21 | Tyrne project → Phase A exit (Phase 1–4c bootstrap + A1–A6 kernel core) | [2026-04-21-tyrne-to-phase-a.md](2026-04-21-tyrne-to-phase-a.md) |
