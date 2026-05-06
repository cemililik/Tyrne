# Phase B — tasks

Tasks belonging to [Phase B — Real userspace](../../../roadmap/phases/phase-b.md).

## Index

| ID | Title | Milestone | Status |
|----|-------|-----------|--------|
| [T-006](T-006-raw-pointer-scheduler-api.md) | Raw-pointer scheduler API refactor | B0 | Done (2026-04-27) |
| [T-007](T-007-idle-task-typed-deadlock.md) | Idle task + typed `SchedError::Deadlock` + resume-path hardening | B0 | Done (2026-04-27) |
| [T-008](T-008-architecture-docs.md) | Architecture docs (scheduler.md + ipc.md + hal.md/overview.md updates) | B0 | Done (2026-04-27) |
| [T-009](T-009-timer-init-cntvct.md) | Timer init + `CNTVCT_EL0` measurement | B0 | Done (2026-04-27) |
| [T-011](T-011-missing-tests-bundle.md) | Missing tests bundle (ReceiverTableFull + slot-reuse + ipc_send_and_yield + start_prelude + cap-table sweep) | B0 | Done (2026-04-27) |
| [T-012](T-012-exception-and-irq-infrastructure.md) | Exception infrastructure and interrupt delivery (GIC + IVT + timer-IRQ + idle-WFI) | B1 | Done (2026-04-28) |
| [T-013](T-013-el-drop-to-el1.md) | EL drop to EL1 in boot (boot.s asm extension + `current_el` HAL helper) | B1 | Done (2026-04-27) |
| [T-014](T-014-idle-dispatch-fallback.md) | Idle dispatch via separate fallback slot (closes B1 smoke regression; implements ADR-0026) | B1 | In Review (2026-05-06) |

Tasks are added here as they become active. See [`../../../roadmap/phases/phase-b.md`](../../../roadmap/phases/phase-b.md) for the full phase plan.
