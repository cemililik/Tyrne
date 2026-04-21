# Current focus

A short pointer file updated as work progresses. For the full plan see [`phases/`](phases/); for the process see [ADR-0013](../decisions/0013-roadmap-and-planning.md).

---

- **Active phase:** A — Kernel core on QEMU `virt`.
- **Active milestone:** A6 — Two-task IPC demo.
- **Active task:** [T-005 — Two-task IPC demo](../analysis/tasks/phase-a/T-005-two-task-ipc-demo.md) — `In Progress` since 2026-04-21.
- **Working branch:** `development`.
- **Last completed milestone:** A5 — Cooperative scheduler and context switch, on 2026-04-21.
- **Last completed task:** [T-004 — Cooperative scheduler](../analysis/tasks/phase-a/T-004-cooperative-scheduler.md) — `Done` 2026-04-21.
- **Last review:** [A2 completion business review](../analysis/reviews/business-reviews/2026-04-21-A2-completion.md) — 2026-04-21.
- **Next review trigger:** code + security review before A6 work lands; A4/A5/A6 business review at Phase A closure.

## Notes

- The capability subsystem (T-001), kernel-object subsystem (T-002), and IPC-primitive subsystem (T-003) form the Phase A stack. All three shipped with zero `unsafe` and no heap.
- [ADR-0014](../decisions/0014-capability-representation.md), [ADR-0016](../decisions/0016-kernel-object-storage.md), and [ADR-0017](../decisions/0017-ipc-primitive-set.md) all Accepted.
- T-003 (A4) delivered `ipc_send` / `ipc_recv` / `ipc_notify` with generation-tracked `IpcQueues`, atomic capability transfer, and TRANSFER-right enforcement. 64 host tests green; status → Done 2026-04-21.
- T-004 (A5) delivered cooperative context switch (`#[unsafe(naked)]` aarch64 asm), `ContextSwitch` HAL trait, `Scheduler<C>` with `yield_now`, and a QEMU smoke test showing two tasks alternating output across 3 iterations. 75 host tests green; QEMU smoke confirmed 2026-04-21. Status → Done 2026-04-21.
- Three implementation bugs required fixing in A5: IrqGuard fat-pointer vtable corruption → generic `IrqGuard<C: Cpu>`; CPACR_EL1.FPEN not initialised → NEON trap at EL1; context_switch_asm compiler prologue corrupted saved sp → `#[unsafe(naked)]`.
- The maintainer updates this file when the active task changes. AI agents update it when they move a task to `In Progress` or `Done` via the [`start-task`](../../.claude/skills/start-task/SKILL.md) and [`conduct-review`](../../.claude/skills/conduct-review/SKILL.md) skills.
