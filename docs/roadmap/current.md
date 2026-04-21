# Current focus

A short pointer file updated as work progresses. For the full plan see [`phases/`](phases/); for the process see [ADR-0013](../decisions/0013-roadmap-and-planning.md).

---

- **Active phase:** B — opened 2026-04-21. First milestone B0 in progress.
- **Active milestone:** B0 — Phase A exit hygiene. T-006 is in review; T-007..T-011 remain to open.
- **Active task:** [T-006 — Raw-pointer scheduler API refactor](../analysis/tasks/phase-b/T-006-raw-pointer-scheduler-api.md) — `In Review` since 2026-04-22. All seven acceptance criteria met; awaiting maintainer sign-off to promote to `Done`.
- **Working branch:** `development`.
- **Last completed milestone:** A6 — Two-task IPC demo, 2026-04-21. **Phase A exit bar met.**
- **Last completed task:** [T-005 — Two-task IPC demo](../analysis/tasks/phase-a/T-005-two-task-ipc-demo.md) — `Done` 2026-04-21.
- **Last reviews (2026-04-21):**
  - [A6 completion / Phase A retrospective](../analysis/reviews/business-reviews/2026-04-21-A6-completion.md)
  - [Code review — Umbrix → Phase A exit](../analysis/reviews/code-reviews/2026-04-21-umbrix-to-phase-a.md)
  - [Security review — Umbrix → Phase A exit](../analysis/reviews/security-reviews/2026-04-21-umbrix-to-phase-a.md)
  - [A6 baseline performance review](../analysis/reviews/performance-optimization-reviews/2026-04-21-A6-baseline.md)
- **Active decisions (2026-04-22):**
  - [ADR-0021 — Raw-pointer scheduler IPC-bridge API](../decisions/0021-raw-pointer-scheduler-ipc-bridge.md) — `Accepted`. Implemented by T-006.
- **Next task to open:** T-007 — Idle task + typed `SchedError::Deadlock` + scheduler/IPC hardening bundle. ADR-0022 is the first step inside T-007.
- **Next review trigger:** B0 closure — a business review once T-006..T-011 are all Done (or a mini-retro if T-006 promotes to `Done` first and the rest are scheduled).

## Notes

- **T-006 resolved UNSAFE-2026-0012.** The scheduler's IPC bridge is now a set of `unsafe fn` free functions over `*mut Scheduler<C>`; the BSP's `task_a` / `task_b` never materialise a `&mut` to shared kernel state across a cooperative context switch. Two new audit entries (UNSAFE-2026-0013 / 0014) cover the two new helper patterns — the BSP's `StaticCell::as_mut_ptr` and the scheduler's momentary-borrow discipline. See [`docs/audits/unsafe-log.md`](../audits/unsafe-log.md).
- **Phase A stack is closed cleanly.** T-001 (caps), T-002 (kernel objects), T-003 (IPC), T-004 (scheduler + context switch), T-005 (IPC demo) all Done. 109 host tests green; QEMU smoke verified; QEMU smoke still matches the A6 trace after T-006.
- **Phase-A review follow-ups are all mapped into `phases/phase-b.md`.** T-006 (UNSAFE-2026-0012), T-007 (idle task + deadlock), T-008 (architecture docs), T-009 (timer init), T-011 (missing tests). Every Kova-3 tracking item is placed in its natural milestone with a 🚩 flag where a decision is still open.
- **`unsafe` audit status (2026-04-22):** UNSAFE-2026-0001..0011 Active (sound under v1 invariants). UNSAFE-2026-0012 `Removed` (commit `f9b72f8`). UNSAFE-2026-0013 / 0014 Active (T-006 helpers).
- The maintainer updates this file when the active task changes. AI agents update it when they move a task to `In Progress` or `Done` via the [`start-task`](../../.claude/skills/start-task/SKILL.md) and [`conduct-review`](../../.claude/skills/conduct-review/SKILL.md) skills.
