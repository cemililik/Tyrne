# Performance optimization review master plan

A performance review is a hypothesis-driven cycle. It starts with a concern and ends with a measured change (or a measured non-change and a closed question). The shape is always: baseline, hotspot, proposal, measurement, regression check.

This plan is written to be parallelizable across multiple agents — six roles — but it is equally valid as a single-agent sequential walkthrough.

## Inputs

- A **concern** — a specific performance question ("how fast is IPC round-trip?") or complaint ("the scheduler tick feels slow under load").
- The **subsystem scope** — which code is in bounds for this review.
- Access to: the source, the benchmarks (existing or to be built), the hardware / emulator used for measurement, and any prior performance reviews of the same scope.

## Output

A file at `docs/analysis/reviews/performance-optimization-reviews/YYYY-MM-DD-<context>.md` following the shape below. Added to the index in this folder's [`README.md`](README.md). Any code that changes as a result is in a separate commit referenced by the artifact; the review itself records decisions and measurements.

## Pre-flight: hypothesis

State the concern concretely. Unit, expectation, allowed deviation. Example:

> "IPC send→receive round-trip should be under 5 µs on QEMU virt with the cooperative scheduler (A5). Current informal observation suggests it is higher. Hypothesis: the context switch's save/restore is the dominant cost."

A review without an explicit hypothesis is exploration; exploration is fine but is logged as a *baseline* artifact rather than a full review.

## Agent roles

### 1. Baseline

**Task:** establish the current state.

- Identify or build the benchmarks for the scope. "Identify or build" means: if the benchmarks exist, run them and record; if they do not, building them is part of the review's work.
- Capture results with the methodology used (how many iterations, what hardware / emulator, what build profile, how outliers handled).
- Record variance (stdev, min/max over N runs) — a single number is insufficient.

**Output:** `Baseline` section — numbers with methodology.

### 2. Hotspot

**Task:** find what is slow.

- Profile the scope. Tools: `cargo-flamegraph`, `perf`, manual instrumentation (`core::arch::x86::_rdtsc` / `CNTVCT_EL0` reads around code).
- Identify the top candidates by time, cache-miss rate, or branch-mispredict rate as applicable.
- Note: the hotspot is *where time is spent*, not *where optimization is easy*.

**Output:** `Hotspot` section — ranked list with evidence.

### 3. Proposal

**Task:** propose specific changes with expected impact.

- For each hotspot considered actionable, propose one or more changes.
- State the expected impact *before* measuring: "This should reduce X by ~20%." A proposal without a prior estimate is not a hypothesis.
- Note rejected proposals too, with reasoning (the review artifact is valuable for *not* doing things as well as for doing them).

**Output:** `Proposal` section — list of proposed changes, each with expected impact.

### 4. Measurement

**Task:** implement the proposals and measure.

- Land the code changes in a branch (not main yet).
- Re-run the baseline benchmarks with the changes applied.
- Record actual impact; compare against the pre-measurement estimate.
- If the actual impact disagrees with the estimate materially, explain why (this is the most valuable learning a performance review produces).

**Output:** `Measurement` section — per-proposal before/after numbers with variance.

### 5. Regression check

**Task:** confirm correctness has not regressed.

- `cargo host-test` passes.
- Relevant QEMU smoke tests pass.
- Security-sensitive code paths re-reviewed if the optimization touched them (cross-reference a [security review](../security-reviews/) if needed).
- `unsafe` count diff if applicable: an optimization that adds `unsafe` requires an audit entry.

**Output:** `Regression check` section — list of checks with status.

### 6. Reporter

**Task:** write up the verdict.

- Summarize the cycle in one paragraph.
- State the verdict: **Merge** (land the proposed changes), **Reject** (changes did not meet the hypothesis; proposal is closed), or **Iterate** (further proposals identified).
- If **Merge**: the code branch is merged in a separate commit referenced by the artifact.
- If **Reject** or **Iterate**: the review artifact is the primary output; the branch is either discarded or kept for follow-up.

**Output:** `Verdict` section.

## Merge step

Combine the six role outputs into a single artifact. The Measurement and Regression-check roles gate the verdict: a Merge verdict is not valid if the regression check failed.

## Acceptance criteria

- [ ] File at `docs/analysis/reviews/performance-optimization-reviews/YYYY-MM-DD-<context>.md`.
- [ ] Frontmatter (concern, scope, hypothesis, reviewer, date) filled.
- [ ] All six sections present.
- [ ] Baseline methodology reproducible (a future reviewer could rerun the benchmarks and check).
- [ ] Measurement records actual vs. estimated impact; the difference is explained if material.
- [ ] Regression check confirms correctness (or the review is Rejected / Escalated).
- [ ] Verdict stated.
- [ ] Index in [`README.md`](README.md) updated.

## Output template

```markdown
# Performance review YYYY-MM-DD — <concern>

- **Concern:** <one-sentence statement>
- **Scope:** <subsystem>
- **Hypothesis:** <concrete, measurable statement>
- **Reviewer:** @cemililik (+ any AI agent acting in the roles below)

## Baseline

<methodology + numbers with variance>

## Hotspot

<ranked findings with evidence>

## Proposal

<proposed changes, each with expected impact>

## Measurement

<per-proposal before/after numbers; note any estimate-vs-actual mismatches>

## Regression check

<test results, security cross-refs, unsafe diff>

## Verdict

**Merge | Reject | Iterate**

<If Merge: link the merge commit. If Reject: summary of why the hypothesis failed. If Iterate: list follow-up proposals.>
```

## Anti-patterns

- **No baseline.** Optimization without a baseline is guessing.
- **Single-number measurement.** Without variance, "5% faster" is noise.
- **No prior estimate.** Without a pre-measurement expectation, the review is not a hypothesis test; it is a narration.
- **Hiding the regression check.** If correctness slips, the verdict is Reject, regardless of performance gains.
- **Too-broad scope.** "Make the kernel faster" is not a review — it is a project. Narrow the concern.
- **Rewriting the design under the banner of performance.** If performance requires a design change, that belongs to an ADR, not a perf review.

## Amendments

- _2026-04-20_ — initial version; scaffolded by [ADR-0013](../../../decisions/0013-roadmap-and-planning.md).
