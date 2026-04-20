# T-NNN — <Title>

- **Phase:** <A / B / C / …>
- **Milestone:** <A1 / A2 / …>
- **Status:** Draft
- **Created:** YYYY-MM-DD
- **Author:** @cemililik (or the AI agent that opened it)
- **Dependencies:** <comma-separated T-NNN list, or "none">
- **Informs:** <what downstream tasks become reachable once this is done>
- **ADRs required:** <comma-separated ADR-NNNN list, or "none">

---

## User story

As <role>, I want <capability>, so that <benefit>.

For kernel-internal tasks where the "role" is another subsystem, say so directly (e.g., "As the scheduler, …" or "As the kernel's ISR entry path, …"). The three-part shape is deliberately flexible; what matters is that the task opens with a one-sentence answer to *what* is being built and *why*.

## Context

<One or two paragraphs. Describe the situation that motivates this task. Explain the consequences if it is skipped. Summarize prior work — linked ADRs, earlier tasks, architecture documents — that the reader needs to interpret this task.>

## Acceptance criteria

A checklist of things that must be true for the task to move from `In Review` to `Done`. Concrete, testable, specific. Aim for 3–8 items; if you hit 10, consider splitting the task.

- [ ] <Criterion 1>
- [ ] <Criterion 2>
- [ ] Tests: <what must be exercised>
- [ ] Documentation: <what changes where>
- [ ] ADR: <if required, must be Accepted before code lands>

## Out of scope

What this task does **not** do. Useful for deflecting scope creep during review.

- <Item 1>
- <Item 2>

## Approach

<Brief technical direction. Not a full design — that lives in an ADR or architecture doc. Roughly a paragraph of "here's the plan at a sketch level." If several approaches are under consideration, list them here and either pick one with brief justification or defer to an ADR as an acceptance-criterion dependency.>

## Definition of done

Beyond the acceptance criteria:

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo host-clippy` clean with `-D warnings`.
- [ ] `cargo kernel-clippy` clean (if the task touches anything reachable from the kernel build).
- [ ] `cargo host-test` passes with the new tests added.
- [ ] Any new `unsafe` has an audit entry in [`../../audits/unsafe-log.md`](../../audits/unsafe-log.md) per [`unsafe-policy.md`](../../standards/unsafe-policy.md) / [`justify-unsafe`](../../../.claude/skills/justify-unsafe/SKILL.md).
- [ ] Commit message follows [`commit-style.md`](../../standards/commit-style.md) with `Refs: <ADR-NNNN>` and, if applicable, `Audit: <UNSAFE-YYYY-NNNN>` / `Security-Review: <reviewer>` trailers.
- [ ] Task status updated to `In Review`; [`../../roadmap/current.md`](../../roadmap/current.md) updated.

## Design notes

<Free-form space for sketches, open questions the task raised, references, measurements, anything that does not fit elsewhere but the implementer wants future-them to know.>

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| YYYY-MM-DD | author | opened; status Draft |
