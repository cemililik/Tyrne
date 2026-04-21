# Current focus

A short pointer file updated as work progresses. For the full plan see [`phases/`](phases/); for the process see [ADR-0013](../decisions/0013-roadmap-and-planning.md).

---

- **Active phase:** A — Kernel core on QEMU `virt`.
- **Active milestone:** A3 — Kernel objects.
- **Active task:** [T-002 — Kernel object storage foundation](../analysis/tasks/phase-a/T-002-kernel-object-storage.md) (status: **In Review** — implementation landed on `development`, awaiting maintainer PR review).
- **Working branch:** `development`.
- **Last completed milestone:** A2 — Capability table foundation, on 2026-04-21 (PR #1 merged to `main`).
- **Last completed task:** [T-001 — Capability table foundation](../analysis/tasks/phase-a/T-001-capability-table-foundation.md) — `Done` 2026-04-21.
- **Last review:** [A2 completion business review](../analysis/reviews/business-reviews/2026-04-21-A2-completion.md) — 2026-04-21.
- **Next review trigger:** code + security review of T-002's implementation when it reaches `In Review`; business review waits for A6 per [phase-a.md closure](phases/phase-a.md).

## Notes

- The capability subsystem (T-001) and kernel-object subsystem (T-002) both shipped with zero `unsafe` and no heap. Neither is wired into `run` yet; that is Phase-A later-milestone work.
- [ADR-0014](../decisions/0014-capability-representation.md) and [ADR-0016](../decisions/0016-kernel-object-storage.md) both Accepted.
- T-002 introduced `obj::{Arena, Task, Endpoint, Notification}` with typed handles and rewired `CapObject` to a typed enum paralleling `CapKind`. `Capability::new` lost its redundant `kind` parameter (kind is now derived from the object's variant). All 77 host tests green.
- The maintainer updates this file when the active task changes. AI agents update it when they move a task to `In Progress` or `Done` via the [`start-task`](../../.claude/skills/start-task/SKILL.md) and [`conduct-review`](../../.claude/skills/conduct-review/SKILL.md) skills.
