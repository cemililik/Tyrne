# Performance optimization reviews

Hypothesis-driven performance cycles: baseline → hotspot → proposal → measurement → regression check. Each cycle produces one artifact.

## When to conduct

- **Periodic.** When the project feels slower than expected, or when a new subsystem lands that makes previous measurements stale.
- **On concern.** A user-visible slowness report, a benchmark regression, or a design question whose answer depends on measured performance.
- **Before shipping a milestone that claims a performance property.** If a milestone's acceptance criteria mention performance, a review is required before it is marked Done.

## What this review produces

A dated file `YYYY-MM-DD-<context>.md` in this folder, following the shape in [`master-plan.md`](master-plan.md). Sections: baseline, hotspot, proposal, measurement, regression check, verdict.

## What this review is not

- It is not a **performance tuning log** — code changes live in their own commits and tasks.
- It is not a **benchmark infrastructure project** — building benchmarks is a task; running them is part of this review.
- It is not an **architectural redesign** — if a review concludes the design is fundamentally wrong for the workload, the outcome is an ADR, not a series of patches.

## Index

_No reviews yet._ Performance reviews begin when the project has enough running code to measure something meaningful — probably during or after Phase A6 (first end-to-end IPC demo).

| Date | Scope | File |
|------|-------|------|
| _pending_ | _first IPC benchmark_ | — |
