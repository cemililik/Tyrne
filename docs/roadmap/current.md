# Current focus

A short pointer file updated as work progresses. For the full plan see [`phases/`](phases/); for the process see [ADR-0013](../decisions/0013-roadmap-and-planning.md).

---

- **Active phase:** B — opened 2026-04-21. First milestone B0 in progress.
- **Active milestone:** B0 — Phase A exit hygiene (T-006, T-007, T-009 **Done** 2026-04-27 per independent-agent approval review; T-008, T-011 **In Review** 2026-04-27). B1 — Drop to EL1 + exception infrastructure (ADR-0024 **Accepted** 2026-04-27; T-013 **In Review** 2026-04-27 — boot.s EL drop + current_el helper; T-012 **Draft**).
- **Active task:** None — T-008, T-011, T-013 all in review awaiting promotion. Next: review pass to promote to Done; then T-012 (exception infra) is the remaining open thread.
- **In review:**
  - [T-011](../analysis/tasks/phase-b/T-011-missing-tests-bundle.md) — Missing tests bundle (B0); 130 → 143 host tests; sched coverage 83.93 → 93.97 % regions; coverage rerun report at [docs/analysis/reports/2026-04-27-coverage-rerun.md](../analysis/reports/2026-04-27-coverage-rerun.md).
  - [T-008](../analysis/tasks/phase-b/T-008-architecture-docs.md) — Architecture docs (B0); new `scheduler.md` + `ipc.md` + Timer subsection update in `hal.md` + Phase A status banner in `overview.md`.
  - [T-013](../analysis/tasks/phase-b/T-013-el-drop-to-el1.md) — EL drop to EL1 (B1); `boot.s` reset-vector extended with K3-12 + EL2→EL1 transition; new `tyrne_hal::cpu::current_el()` helper; UNSAFE-2026-0017 + 0018 audited; `boot.md` and `bsp-boot-checklist.md` updated. QEMU smoke pending — must run from a host harness with `qemu-system-aarch64`.
- **Working branch:** `development`.
- **Last completed milestone:** A6 — Two-task IPC demo, 2026-04-21. **Phase A exit bar met.** B0 closure pending T-008 and T-011.
- **Last completed tasks:** T-006 / T-007 / T-009 — all `Done` 2026-04-27. Approval review at [docs/analysis/reviews/business-reviews/2026-04-27-T-009-mini-retro.md](../analysis/reviews/business-reviews/2026-04-27-T-009-mini-retro.md) and the close-out approval-review pass commit.
- **Last reviews:**
  - [T-009 mini-retro (2026-04-27)](../analysis/reviews/business-reviews/2026-04-27-T-009-mini-retro.md)
  - [T-006 mini-retro (2026-04-22)](../analysis/reviews/business-reviews/2026-04-22-T-006-mini-retro.md)
  - [A6 completion / Phase A retrospective (2026-04-21)](../analysis/reviews/business-reviews/2026-04-21-A6-completion.md)
  - [Code review — Tyrne → Phase A exit (2026-04-21)](../analysis/reviews/code-reviews/2026-04-21-tyrne-to-phase-a.md)
  - [Security review — Tyrne → Phase A exit (2026-04-21)](../analysis/reviews/security-reviews/2026-04-21-tyrne-to-phase-a.md)
  - [A6 baseline performance review (2026-04-21)](../analysis/reviews/performance-optimization-reviews/2026-04-21-A6-baseline.md)
- **Active decisions:**
  - [ADR-0010 — Timer HAL trait](../decisions/0010-timer-trait.md) — `Accepted` (2026-04-20). BSP side implemented by T-009 (2026-04-23); IRQ-delivery half deferred to T-012.
  - [ADR-0021 — Raw-pointer scheduler IPC-bridge API](../decisions/0021-raw-pointer-scheduler-ipc-bridge.md) — `Accepted` (2026-04-22). Implemented by T-006.
  - [ADR-0022 — Idle task and typed scheduler deadlock error](../decisions/0022-idle-task-and-typed-scheduler-deadlock.md) — `Accepted` (2026-04-22). Implemented by T-007. First rider's WFI activation gated on T-012; T-009 closes only the time-source half.
  - [ADR-0024 — EL drop to EL1 policy](../decisions/0024-el-drop-policy.md) — `Accepted` (2026-04-27). Implemented by T-013 (B1, Draft, now unblocked). First ADR to use ADR-0025's *Dependency chain* section in production; same-day Accept after careful re-read per [ADR-0025 §Revision notes](../decisions/0025-adr-governance-amendments.md) (cool-down rule withdrawn pre-Accept).
  - [ADR-0025 — ADR governance amendments](../decisions/0025-adr-governance-amendments.md) — `Accepted` (2026-04-27). Two normative rules for ADR drafting: (§Rule 1) every forward-reference points at a real T-NNN, (§Rule 2) riders are not failures — their *frequency* is the signal. Cool-down rule withdrawn pre-Accept on maintainer feedback; substance preserved in the write-adr skill's careful-re-read step.
- **Next task to open:** none — every B0 task now has a file (T-008 opened 2026-04-27; T-011 already Draft). Next status flip: T-011 → `In Progress`, T-008 → `In Progress`, or T-013 → `In Progress` (any of the three is unblocked).
- **Next review trigger:** B0 closure — a full business review once T-006..T-011 are all Done. (T-006 mini-retro filed 2026-04-22.)

## Notes

- **T-006 resolved UNSAFE-2026-0012.** The scheduler's IPC bridge is now a set of `unsafe fn` free functions over `*mut Scheduler<C>`; the BSP's `task_a` / `task_b` never materialise a `&mut` to shared kernel state across a cooperative context switch. Two new audit entries (UNSAFE-2026-0013 / 0014) cover the two new helper patterns — the BSP's `StaticCell::as_mut_ptr` and the scheduler's momentary-borrow discipline. See [`docs/audits/unsafe-log.md`](../audits/unsafe-log.md).
- **Phase A stack is closed cleanly.** T-001 (caps), T-002 (kernel objects), T-003 (IPC), T-004 (scheduler + context switch), T-005 (IPC demo) all Done. 109 host tests green; QEMU smoke verified; QEMU smoke still matches the A6 trace after T-006.
- **Phase-A review follow-ups are all mapped into `phases/phase-b.md`.** T-006 (UNSAFE-2026-0012), T-007 (idle task + deadlock), T-008 (architecture docs), T-009 (timer init), T-011 (missing tests). Every Kova-3 tracking item is placed in its natural milestone with a 🚩 flag where a decision is still open.
- **`unsafe` audit status (2026-04-22):** UNSAFE-2026-0001..0011 Active (sound under v1 invariants). UNSAFE-2026-0012 `Removed` (commit `f9b72f8`). UNSAFE-2026-0013 / 0014 Active (T-006 helpers).
- The maintainer updates this file when the active task changes. AI agents update it when they move a task to `In Progress` or `Done` via the [`start-task`](../../.claude/skills/start-task/SKILL.md) and [`conduct-review`](../../.claude/skills/conduct-review/SKILL.md) skills.
