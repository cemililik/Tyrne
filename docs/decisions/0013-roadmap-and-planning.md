# 0013 — Roadmap and planning process

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

Tyrne is a multi-year solo project with AI assistance. The ADR process ([ADR-0001](0001-microkernel-architecture.md) onward) answers *why* decisions are made; the [standards](../standards/) answer *how* work is done; the [skills](../../.claude/skills/) encode procedures for recurring tasks. What is missing is the **sequencing and tracking layer**: which phase of the project we are in, which milestone is active, which task is being worked on, and what reviews happen when a milestone lands or when a change affects security / performance.

A project with no deadline still needs a roadmap — not to meet dates, but so that work proceeds in a considered order, so that the maintainer can pause for weeks and return without having to page the entire project back in, and so that contributors (human or AI) arriving later know the direction without guessing. Without a plan, time does not produce progress; it produces ad-hoc choices that accumulate.

This ADR establishes the roadmap and analysis system: where the plan lives, where tasks are tracked, how reviews are conducted across four distinct concerns (business / code / security / performance), and the conventions that make the whole thing navigable months later.

## Decision drivers

- **Sequencing over scheduling.** Tyrne is not estimable. What matters is *what comes before what*, not *how long each piece takes*.
- **Living document.** The plan will change. A plan that rots the moment it is written is worse than no plan; a plan we routinely update is valuable.
- **Repo-native.** External tools (Trello, Linear, GitHub Projects) are not part of the ground truth.
- **Per-phase task folders.** Each phase has its own directory under `docs/analysis/tasks/`, so a phase's tasks are read together and ID collisions between phases are obvious.
- **Typed reviews.** Reviews fall into four distinct concerns — business (milestone retrospectives), code (per-change quality), security (deep pass on sensitive changes), performance (optimization cycles). Each lives under its own directory with a **master plan** describing the procedure in enough detail that multiple agents can work on pieces in parallel.
- **Task-as-user-story.** Each concrete unit of work is a short Markdown document in a consistent format. The three-part narrative (role, capability, benefit) is flexible enough for kernel-internal work where the "user" is another subsystem.
- **Robust to long pauses.** A maintainer returning after months must be able to read `docs/roadmap/current.md` and immediately know what they were doing and why.
- **Orthogonal to ADRs.** The roadmap sequences work; architectural decisions *inside* tasks still go through ADRs.
- **Split roadmap vs. analysis.** `docs/roadmap/` holds the plan — what we *intend* to do, in what order. `docs/analysis/` holds the work — individual tasks as they move through the pipeline, and reviews as they are conducted. The two are cross-linked but separate so the plan does not drown in execution detail.

## Considered options

### Option A — GitHub Projects / issues as the source of truth
### Option B — a single `roadmap.md` file
### Option C — `docs/roadmap/` with tasks and reviews inside it
### Option D — external project-management tool (Linear, Jira, Notion)
### Option E — split `docs/roadmap/` (plan) from `docs/analysis/` (execution), per-phase task folders, per-type review folders with master plans (chosen)

## Decision outcome

**Chosen: Option E.**

### Folder layout

```text
docs/
├── roadmap/                   — the plan, in order of execution
│   ├── README.md              — purpose, conventions, quick-navigation
│   ├── phases/                — one file per phase (A–J)
│   │   ├── README.md          — index + rationale for the split
│   │   ├── phase-a.md         — detailed per-milestone breakdown
│   │   ├── phase-b.md         — detailed
│   │   ├── phase-c.md         — detailed
│   │   ├── phase-d.md         — detailed
│   │   ├── phase-e.md         — medium detail
│   │   ├── phase-f.md         — medium detail
│   │   ├── phase-g.md         — medium detail
│   │   ├── phase-h.md         — light detail
│   │   ├── phase-i.md         — light detail
│   │   └── phase-j.md         — sketch (opt-in AI-native userspace)
│   └── current.md             — "we are here" pointer
│
└── analysis/                  — the work, as it moves through the pipeline
    ├── README.md              — purpose + how tasks / reviews relate to the roadmap
    ├── tasks/                 — user-story task files, organized by phase
    │   ├── README.md          — task index, status vocabulary, ID schema
    │   ├── TEMPLATE.md        — user-story template to copy from
    │   ├── phase-a/           — Phase A tasks
    │   │   ├── README.md
    │   │   └── T-NNN-<slug>.md
    │   ├── phase-b/
    │   └── … (one folder per phase)
    └── reviews/               — reviews, by type
        ├── README.md          — review philosophy, the four types, when to conduct each
        ├── business-reviews/
        │   ├── README.md
        │   ├── master-plan.md
        │   └── YYYY-MM-DD-<scope>.md
        ├── code-reviews/
        │   ├── README.md
        │   ├── master-plan.md
        │   └── YYYY-MM-DD-<scope>.md
        ├── security-reviews/
        │   ├── README.md
        │   ├── master-plan.md
        │   └── YYYY-MM-DD-<scope>.md
        └── performance-optimization-reviews/
            ├── README.md
            ├── master-plan.md
            └── YYYY-MM-DD-<scope>.md
```

### Identifiers

- **Phases:** letters A–Z (A, B, C, …) in order of execution. Stable once published. Each phase has its own `phases/phase-<letter>.md` file and its own `analysis/tasks/phase-<letter>/` folder.
- **Milestones:** `<phase><N>` (A1, A2, …). Stable once the milestone becomes active.
- **Tasks:** `T-NNN` zero-padded across the whole project; stable forever.

### Status values for tasks

`Draft`, `Ready`, `In Progress`, `In Review`, `Done`, `Blocked`, `Superseded`. Status transitions are author-driven; only the maintainer authorizes `Done`.

### The four review types

Each review type has its own directory under `docs/analysis/reviews/` and its own `master-plan.md` that documents the procedure in enough detail that pieces can be parallelized across multiple AI agents.

| Type | Purpose | When |
|------|---------|------|
| **business-reviews** | Milestone retrospectives and strategic-direction checks. What landed, what changed in the plan, what was learned, what comes next. | At every milestone completion; maintainer-initiated at any point. |
| **code-reviews** | Per-change code quality: correctness, style, test coverage, doc updates. An artifact per non-trivial change so "has this been reviewed?" is answerable from the repo. | Per non-trivial PR / change, once PR flow begins. |
| **security-reviews** | Deep security pass on changes that touch capabilities, IPC, syscalls, memory, boot, crypto, `unsafe`, or security-sensitive dependencies. | Per sensitive change, per the trigger list in [security-review.md](../standards/security-review.md). |
| **performance-optimization-reviews** | Hypothesis-driven performance cycles: baseline, hotspot, proposal, measurement, regression check. | Periodic or on concern. |

### Master plan

Each review type's `master-plan.md` describes:

- The review's purpose and when it is triggered.
- **Agent roles** — the distinct concerns a multi-agent review splits into (e.g. code-review has "correctness agent", "style agent", "test-coverage agent", etc.). Each role owns a section of the resulting artifact.
- The **procedure** — how an agent (or the maintainer) performs its role, step by step.
- The **merge step** — how agent outputs combine into a single review artifact.
- The **acceptance criteria** that define "this review is complete."
- The **output format** — the shape of the dated review file.

Master plans are maintained; they are amended as the project learns how to review itself better. Structural changes to a master plan do not need an ADR unless they also change the categorization (new review type, merged types, etc.).

### Review cadence

Reviews are **event-triggered**, not calendar-triggered. See the individual review-type READMEs for each category's triggers.

### Changing the roadmap

- **Adding a task** — use the [`start-task`](../../.claude/skills/start-task/SKILL.md) skill; new tasks get the next T-NNN and land under the right phase folder.
- **Reordering tasks within a milestone** — edit `phases/phase-<letter>.md` and each affected task's frontmatter.
- **Moving tasks between milestones (same phase)** — edit the phase file and the task's frontmatter.
- **Moving tasks between phases** — move the file from `analysis/tasks/phase-<from>/` to `analysis/tasks/phase-<to>/`, update the task's frontmatter, note the move in the task's review-history section.
- **Adding a milestone** — edit the relevant `phases/phase-<letter>.md`.
- **Adding, dropping, or reordering phases at the top level** — structural change; requires an ADR that supersedes the affected `phases.md` statements.
- **Superseding a task** — set status to `Superseded`, link forward to the replacement(s).

### Skills

- [`start-task`](../../.claude/skills/start-task/SKILL.md) — create a new task file in the correct phase folder, assign next T-NNN, update `current.md` on status transition.
- [`conduct-review`](../../.claude/skills/conduct-review/SKILL.md) — produce a review artifact, taking the review type as input, following that type's master plan.
- [`perform-code-review`](../../.claude/skills/perform-code-review/SKILL.md) and [`perform-security-review`](../../.claude/skills/perform-security-review/SKILL.md) — pre-existing skills for executing a review during development; they now also produce an artifact in the corresponding `analysis/reviews/<type>/` directory.

### Integration with ADRs

The roadmap does not replace ADRs. It sequences them. A task may require an ADR as an acceptance criterion; an ADR may result in new tasks being added.

## Consequences

### Positive

- **No time pressure.** Estimates are not part of the artifact.
- **Resume-friendly.** `current.md` orients a returning maintainer in under a minute.
- **Phase-scoped browsing.** Opening a phase folder shows its tasks; opening a phase file shows the plan for the phase; the two mirror each other by number.
- **Typed review history.** "What security reviews have we done this quarter?" is a directory listing.
- **Multi-agent-friendly.** Master plans are written to support distinct agents taking distinct sections of a review and merging.
- **Diff-able.** Plan changes are visible in `git log`.

### Negative

- **More files.** The structure has ~30 directory-level artifacts before the first piece of work lands in them. Mitigation: the empty-folder `README.md`s are short and stop being empty as work happens.
- **Discipline required.** Task files, status fields, and `current.md` have to stay in sync. Mitigation: skills automate the boilerplate; reviews check the state.

### Neutral

- **IDs grow forever.** T-NNN is sequential across the project's lifetime.
- **Solo-phase simplifications.** Fields like assignee / priority / size are omitted; they can be added if and when a second maintainer arrives.

## Pros and cons of the options

### Option A — GitHub issues / Projects

- Pro: visible from the GitHub repo page; integrates with PRs.
- Con: source of truth is the cloud service, not the repo clone.
- Con: self-hosting later becomes a migration problem.
- Con: review retrospectives have no natural home in issues.

### Option B — single `roadmap.md`

- Pro: minimal.
- Con: does not scale past ~20 tasks; merging concurrent updates is painful.

### Option C — `docs/roadmap/` with tasks and reviews nested

- Pro: single tree to remember.
- Con: conflates plan (what we intend) with execution (what happened); these have different update cadences and different readers.
- Con: per-type review directories bloat the roadmap directory.

### Option D — external tool

- Pro: rich UI, notifications, team features.
- Con: solo project gains little; external tool goes stale.

### Option E — roadmap + analysis split, per-phase tasks, per-type reviews with master plans (chosen)

- Pro: plan and execution are cleanly separated; each has its own navigation.
- Pro: per-phase task folders scale to many tasks without a flat sea of files.
- Pro: per-type review folders + master plans make "how we review" explicit and parallelizable across agents.
- Pro: the structure tells the story of the project to a cold reader.
- Con: more structure to learn; more `README.md` files at the leaves. Accepted as the cost of scale.

## Open questions

- **GitHub-side mirror.** Surface milestones as GitHub milestones / projects for external visibility? The repo remains authoritative; the GitHub view is a generated reflection if we add it.
- **Priority and size fields on tasks.** Add if pain shows.
- **Review archive policy.** As `analysis/reviews/<type>/` grows, an index by milestone or by calendar period may be needed.
- **Master-plan evolution history.** A master plan is a living document; should we track its amendments in a `CHANGELOG.md` within each review type, or inline in the file? Defer until the first amendment.

## Revision notes

- **2026-04-28 — pointer to ADR-0025.** [ADR-0025 — ADR governance amendments](0025-adr-governance-amendments.md) extends this ADR with three rules surfaced by the Phase A → B0 implementation arc: ADR cool-down (no same-day Accept), Forward-reference contract (every "future task X will do Y" grounded at a real T-NNN), and Riders-are-not-failures (frequency is the signal). ADR-0025 stands on its own; this ADR's body is intact. An earlier draft (commit `56fd9eb`, since reverted) attempted to rewrite this ADR's body in place — that was an append-only-policy violation surfaced by the second-read review and corrected by extracting the rules into ADR-0025.

## References

- Existing [ADRs](.), [standards](../standards/), [skills](../../.claude/skills/).
- User story format — agile community prior art, adapted here.
- Amazon six-pager / working-backwards — partial inspiration for the "what, why, definition of done" structure.
- Hubris roadmap practices — public prior art.
- The maintainer's repeatedly stated motto (see memory) — measured pace, systematic over hurried.
