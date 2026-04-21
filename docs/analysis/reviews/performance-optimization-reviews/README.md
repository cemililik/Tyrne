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

| Date | Scope | File |
|------|-------|------|
| 2026-04-21 | A6 baseline — v0.0.1 kernel footprint after Phase A exit (no hypothesis; baseline exploration per master-plan §Pre-flight) | [2026-04-21-A6-baseline.md](2026-04-21-A6-baseline.md) |

> First full hypothesis-driven cycle is blocked on Phase B's timer initialisation — IPC round-trip latency cannot be measured without a free-running counter. The baseline above records size / section / instruction-count metrics that can be measured statically.
