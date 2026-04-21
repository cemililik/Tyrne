# Current focus

A short pointer file updated as work progresses. For the full plan see [`phases/`](phases/); for the process see [ADR-0013](../decisions/0013-roadmap-and-planning.md).

---

- **Active phase:** A — Kernel core on QEMU `virt`.
- **Active milestone:** A5 — Cooperative scheduler and context switch.
- **Active task:** [T-004 — Cooperative scheduler](../analysis/tasks/phase-a/T-004-cooperative-scheduler.md) (status: **Ready**).
- **Working branch:** `development`.
- **Last completed milestone:** A4 — IPC primitives, on 2026-04-21 (PR merged to `main`).
- **Last completed task:** [T-003 — IPC primitives](../analysis/tasks/phase-a/T-003-ipc-primitives.md) — `Done` 2026-04-21.
- **Last review:** [A2 completion business review](../analysis/reviews/business-reviews/2026-04-21-A2-completion.md) — 2026-04-21.
- **Next review trigger:** code + security review of T-004 when it reaches `In Review`; A4/A5 business review waits for A6 per [phase-a.md closure](phases/phase-a.md).

## Notes

- The capability subsystem (T-001), kernel-object subsystem (T-002), and IPC-primitive subsystem (T-003) form the Phase A stack. All three shipped with zero `unsafe` and no heap. None is wired into `run` yet; that is Phase-A later-milestone work.
- [ADR-0014](../decisions/0014-capability-representation.md), [ADR-0016](../decisions/0016-kernel-object-storage.md), and [ADR-0017](../decisions/0017-ipc-primitive-set.md) all Accepted.
- T-003 (A4) delivered `ipc_send` / `ipc_recv` / `ipc_notify` with generation-tracked `IpcQueues`, atomic capability transfer, and TRANSFER-right enforcement. 64 host tests green; status → Done 2026-04-21.
- A5 (T-004) opens: cooperative scheduler with context switch. ADR-0019 (scheduler shape) and ADR-0020 (`Cpu` trait v2 / context-switch extension) both Accepted 2026-04-21. T-004 → Ready.
- The maintainer updates this file when the active task changes. AI agents update it when they move a task to `In Progress` or `Done` via the [`start-task`](../../.claude/skills/start-task/SKILL.md) and [`conduct-review`](../../.claude/skills/conduct-review/SKILL.md) skills.
