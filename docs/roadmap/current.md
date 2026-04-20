# Current focus

A short pointer file updated as work progresses. For the full plan see [`phases/`](phases/); for the process see [ADR-0013](../decisions/0013-roadmap-and-planning.md).

---

- **Active phase:** A — Kernel core on QEMU `virt`.
- **Active milestone:** A2 — Capability table foundation.
- **Active task:** [T-001 — Capability table foundation](../analysis/tasks/phase-a/T-001-capability-table-foundation.md) (status: **In Review** — awaiting PR review from `development` to `main`).
- **Working branch:** `development`.
- **Last completed milestone:** A1 — Bootable skeleton, on 2026-04-20 (commit `2944e7d`).
- **Last review:** none yet; the first business review will accompany A2 completion.
- **Next review trigger:** maintainer PR review → if merged, T-001 moves to `Done` and A2 milestone review follows.

## Notes

- T-001 implementation landed on `development`; see the task file for evidence and test coverage: [T-001](../analysis/tasks/phase-a/T-001-capability-table-foundation.md).
- [ADR-0014](../decisions/0014-capability-representation.md) Accepted; the kernel now exposes `cap::CapabilityTable`, `cap::CapHandle`, `cap::CapRights`, `cap::Capability`, and `cap::CapError`.
- The new module is not yet wired into `run`.
- The maintainer updates this file when the active task changes. AI agents update it when they move a task to `In Progress` or `Done` via the [`start-task`](../../.claude/skills/start-task/SKILL.md) and [`conduct-review`](../../.claude/skills/conduct-review/SKILL.md) skills.
