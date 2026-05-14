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
| 2026-04-28 | B1 closure baseline — post-T-013 + T-012 footprint (kernel image, .bss, instruction counts; new Metric 6 — IRQ delivery cost) | [2026-04-28-B1-closure.md](2026-04-28-B1-closure.md) |
| 2026-05-07 | B1 closure post-T-014 re-baseline — net footprint-neutral (`.text` −116 / `.rodata` +144 / `.bss` +8 bytes) after the T-014 idle-dispatch refactor and the comprehensive-review follow-up sweeps; smoke ~5.8 ms boot-to-end | [2026-05-07-B1-closure.md](2026-05-07-B1-closure.md) |
| 2026-05-09 | B2 closure baseline — post-T-016 footprint (`.text +364` / `.rodata +16` / `.bss +17,952` — dominated by `.boot_pt` 16 KiB reservation); first release-build harness band p10/p50/p90 = 4.262 / 4.642 / 6.456 ms; `-d guest_errors` 379 events (all pre-existing PL011 noise) | [2026-05-09-B2-closure.md](2026-05-09-B2-closure.md) |
| 2026-05-14 | B3 closure baseline — post-T-017 + T-018 footprint (`.text +1,624` / `.rodata +592` / `.bss +1,872`); release-build harness band p10/p50/p90 = 10.311 / 11.884 / 13.823 ms (+6 to +7 ms vs B2 — pure QEMU TCG translation overhead from new code paths; real-hardware projection sub-5 ms); `-d guest_errors` 526 events (all pre-existing PL011 noise; zero non-PL011) | [2026-05-14-B3-closure.md](2026-05-14-B3-closure.md) |

> First full hypothesis-driven cycle is now infrastructure-unblocked — T-009 + T-012 lit up `now_ns()` at EL1 and provide the measurement primitive IPC round-trip latency needs. The B1 closure baseline above records the static-only metrics; future hypothesis-driven cycles will add IPC round-trip wall-clock measurement, stack high-water-mark probes, and `TrapFrame` slimming for ack-and-ignore IRQ handlers.
