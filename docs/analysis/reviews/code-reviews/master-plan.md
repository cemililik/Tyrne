# Code review master plan

A code review exists to answer, for a specific change: *does this belong in `main`?* It does so by running five concurrent passes — one per concern — and merging their verdicts into a single artifact.

This plan is written to be parallelizable across multiple agents. In a single-agent session, execute roles sequentially; the merge step is trivial.

## Inputs

- The **change under review** — a PR URL, a commit range, or a branch name.
- The PR / change **description**.
- Access to: the source code, the test results from CI, the affected tasks (if any) and the ADRs they cite.

## Output

A file at `docs/analysis/reviews/code-reviews/YYYY-MM-DD-<context>.md` following the shape below. Added to the index in this folder's [`README.md`](README.md). The commit that merges the change carries a trailer referencing this artifact (`Code-Review: docs/analysis/reviews/code-reviews/YYYY-MM-DD-<context>.md`) when the trailer becomes convention.

## Pre-flight: risk class

Before starting, the reviewer determines the change's risk class:

- **Security-sensitive** (touches capabilities, IPC, syscalls, memory, scheduler, boot, crypto, `unsafe`, security-sensitive deps) — this code review is *not sufficient alone*; a [security review](../security-reviews/) is also required. The code review records the security-review artifact as a cross-reference in its verdict.
- **Ordinary** — this code review alone is enough.

## Agent roles

### 1. Correctness

**Task:** does the code do what the description says?

- Read the PR description first. Understand stated intent.
- Read the tests second. They encode the author's understanding of the behaviour.
- Read the diff in topological order: new types → their impls → callers → tests.
- For each modified function: does the implementation match the signature and the doc?
- For each new public API: is the contract clear? Is there a hidden assumption?
- Off-by-ones, missing error handling, confused-deputy patterns, forgotten `unsafe` invariants.

**Output:** `Correctness` section — bulleted findings, each with file/line if applicable.

### 2. Style

**Task:** does the code match the project's style standards?

- Spot-check things CI should have caught (rustfmt, clippy) — if CI is red, don't bother; send back first.
- For things CI does not catch: naming conventions per [`code-style.md`](../../../standards/code-style.md), module organization, comment density, `unsafe` annotation rules from [`unsafe-policy.md`](../../../standards/unsafe-policy.md).
- Avoid bikeshedding: if the tool does not catch it and the code is not actively misleading, leave it.

**Output:** `Style` section — typically a short list; empty is fine.

### 3. Test coverage

**Task:** are the tests adequate?

- Is the public API of the change exercised?
- For a fix, confirm that a regression test fails before the patch and passes after.
- When a new `Error` variant is added, ensure a test provokes it.
- Behavioural changes need a QEMU smoke that demonstrates the behaviour end-to-end.
- What is missing? Be specific about what a good test would look like.

**Output:** `Test coverage` section — list of tests present and list of tests missing.

### 4. Documentation

**Task:** did the docs change along with the code?

- Ensure a rustdoc comment exists for every new public item.
- Add a `# Safety` section to each `unsafe fn`.
- Document new `Error` variants in an `# Errors` section where applicable.
- Update [`docs/architecture/`](../../../architecture/) when the change affects architecture.
- Create or queue a guide in [`docs/guides/`](../../../guides/) for user-facing workflow changes.
- Reference the relevant ADR in the commit trailer when the change implements or affects one.

**Output:** `Documentation` section — gaps and their severity.

### 5. Integration

**Task:** does the change play well with everything around it?

- Are there new dependencies? If so, was [`add-dependency`](../../../../.claude/skills/add-dependency/SKILL.md) followed?
- Does the change break downstream callers (symbol renames, API changes, behaviour changes)?
- Does the CI pipeline cover the new code paths?
- Any regressions in existing tests?

**Output:** `Integration` section — any cross-cutting concerns.

## Merge step

Combine the five role outputs into a single artifact. The verdict is computed from them:

- **Approve** — all five passes returned either "clean" or "minor, non-blocking" findings.
- **Request changes** — at least one pass returned a blocking finding. The list of blockers is the verdict's attachment.
- **Comment** — the review is partial or the reviewer wants discussion before committing to approve/reject.

## Acceptance criteria

- [ ] File at `docs/analysis/reviews/code-reviews/YYYY-MM-DD-<context>.md`.
- [ ] Frontmatter (change identifier, reviewer, date) filled.
- [ ] All five role sections present; blockers explicit.
- [ ] Verdict stated clearly.
- [ ] Cross-reference to any [security-review artifact](../security-reviews/) if the change is security-sensitive.
- [ ] Index in [`README.md`](README.md) updated.

## Output template

```markdown
# Code review YYYY-MM-DD — <change identifier>

- **Change:** <PR URL / branch / commit range>
- **Reviewer:** @cemililik (+ any AI agent acting in the roles below)
- **Risk class:** Ordinary | Security-sensitive
- **Security-review cross-reference:** <path or "n/a">

## Correctness

<Correctness role output>

## Style

<Style role output>

## Test coverage

<Test-coverage role output>

## Documentation

<Documentation role output>

## Integration

<Integration role output>

## Verdict

**Approve | Request changes | Comment**

<If Request changes: list blockers. If Comment: state what is outstanding.>
```

## Anti-patterns

- **LGTM without reading.** A code review that did not happen is worse than no review — it creates false confidence.
- **Re-litigating style the tools caught.** Waste.
- **Scope creep.** "While you're here, also …" — open a new task.
- **Approving a security-sensitive change without the paired security review.** Block the code review until the security review exists.
- **Hiding blockers in prose.** A blocker that is not on the blocker list will be missed.

## Amendments

- _2026-04-20_ — initial version; scaffolded by [ADR-0013](../../../decisions/0013-roadmap-and-planning.md).
