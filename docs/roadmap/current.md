# Current focus

A short pointer file updated as work progresses. For the full plan see [`phases/`](phases/); for the process see [ADR-0013](../decisions/0013-roadmap-and-planning.md).

---

- **Active phase:** B — opened 2026-04-21 after Phase-A reviews landed; first milestone is B0 (Phase A exit hygiene).
- **Active milestone:** B0 — Phase A exit hygiene. Contains the three Phase-B blockers surfaced by the 2026-04-21 security review (raw-pointer scheduler API, idle task + typed deadlock, cross-table revocation policy) plus the architecture-doc follow-ups, timer initialisation, and the scheduler/IPC hardening bundle.
- **Active task:** none in progress. Next task to open is **T-006 — Raw-pointer scheduler API refactor** (ADR-0021 writing is the first step inside that task).
- **Working branch:** `development`.
- **Last completed milestone:** A6 — Two-task IPC demo, 2026-04-21. **Phase A exit bar met.**
- **Last completed task:** [T-005 — Two-task IPC demo](../analysis/tasks/phase-a/T-005-two-task-ipc-demo.md) — `Done` 2026-04-21.
- **Last reviews (2026-04-21):**
  - [A6 completion / Phase A retrospective](../analysis/reviews/business-reviews/2026-04-21-A6-completion.md)
  - [Code review — Umbrix → Phase A exit](../analysis/reviews/code-reviews/2026-04-21-umbrix-to-phase-a.md) — Approve (4 non-blocking follow-ups absorbed into B0)
  - [Security review — Umbrix → Phase A exit](../analysis/reviews/security-reviews/2026-04-21-umbrix-to-phase-a.md) — Changes requested (3 Phase-B blockers absorbed into B0)
  - [A6 baseline performance review](../analysis/reviews/performance-optimization-reviews/2026-04-21-A6-baseline.md) — baseline-only; full hypothesis-driven cycle blocked on B0 timer initialisation.
- **Next review trigger:** B0 closure — a business review once ADR-0021..0023 are Accepted and the B0 tasks (T-006..T-011) are all Done.

## Notes

- **Phase A stack closed cleanly.** T-001 (caps), T-002 (kernel objects), T-003 (IPC), T-004 (scheduler + context switch), T-005 (IPC demo) all Done. 109 host tests green; QEMU smoke verified. 20 ADRs Accepted (0018 deferred by design). 12 audited `unsafe` entries.
- **Phase-A reviews produced 3 Phase-B blockers and 13 non-blocking tracking items.** All of them are now mapped to a specific Phase B milestone in [`phases/phase-b.md`](phases/phase-b.md) — none are "open follow-ups sitting in a ledger". Items that need a concrete decision during Phase B are marked with 🚩 in the phase plan and collected under its *Open questions* section.
- **ADR numbering shifted.** The pre-review phase-b.md reserved ADR-0021..0026 for EL-drop / MMU / userspace. Those are now ADR-0024..0029; the fresh ADR-0021..0023 claim the review blockers. Numbers remain tentative per ADR-0013 (final number assigned at write-time).
- **Phase-A-era `unsafe` audit status:** UNSAFE-2026-0001..0011 Active (sound under v1 invariants). UNSAFE-2026-0012 Active → to be Removed during T-006 (B0). See [`docs/audits/unsafe-log.md`](../audits/unsafe-log.md).
- The maintainer updates this file when the active task changes. AI agents update it when they move a task to `In Progress` or `Done` via the [`start-task`](../../.claude/skills/start-task/SKILL.md) and [`conduct-review`](../../.claude/skills/conduct-review/SKILL.md) skills.
