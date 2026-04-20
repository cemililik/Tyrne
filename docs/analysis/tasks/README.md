# Tasks

Individual task user stories, organized per phase. Each task is a short markdown document following the shape in [`TEMPLATE.md`](TEMPLATE.md).

## Layout

```
tasks/
├── README.md        — this file
├── TEMPLATE.md      — user-story template
├── phase-a/         — Phase A tasks
│   ├── README.md    — phase-local task index
│   └── T-NNN-<slug>.md
├── phase-b/
├── phase-c/
└── … (one folder per phase)
```

Task IDs (`T-NNN`) are sequential **across the whole project**, not restarted per phase. The phase folder structure is for reading convenience, not for namespacing IDs.

## Status vocabulary

| Status | Meaning |
|--------|---------|
| `Draft` | Exists but criteria are not settled. |
| `Ready` | Criteria settled; dependencies resolved; ready to be picked up. |
| `In Progress` | Actively being worked on. Reflected in [`../../roadmap/current.md`](../../roadmap/current.md). |
| `In Review` | Code / docs landed; awaiting maintainer review. |
| `Done` | Merged and reviewed; acceptance criteria met. |
| `Blocked` | Cannot proceed; reason documented inline. |
| `Superseded` | Replaced; link forward to the replacement(s). |

Status transitions are author-driven. Only the maintainer authorizes `Done`.

## Lifecycle

1. **Create** the task with the [`start-task`](../../../.claude/skills/start-task/SKILL.md) skill. It copies [`TEMPLATE.md`](TEMPLATE.md) into the right phase folder and assigns the next T-NNN.
2. **Move to `In Progress`** when work begins. Update [`../../roadmap/current.md`](../../roadmap/current.md) to point at the task.
3. **Work the acceptance criteria.** Any ADRs listed as dependencies are written (via `write-adr`) before implementation code lands.
4. **Move to `In Review`** when all criteria appear satisfied. The maintainer's review confirms.
5. **Move to `Done`** after review. Update `current.md`; if it was the last task of the milestone, trigger a business review via [`conduct-review`](../../../.claude/skills/conduct-review/SKILL.md).

## Conventions

- Filename: `T-NNN-<kebab-slug>.md`, where `<slug>` is an imperative-verb phrase (`capability-table-foundation`, `el-drop-to-el1`).
- Exactly one task per file.
- Frontmatter fields are always present — no placeholder values left over from the template.
- Acceptance criteria are concrete and testable (3–8 items; at most 10, or split the task).
- If a task has open design decisions, they become ADR acceptance criteria on the task.

See [ADR-0013](../../decisions/0013-roadmap-and-planning.md) for the full planning-process rules.
