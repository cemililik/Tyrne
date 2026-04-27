# T-006 — Raw-pointer scheduler API refactor

- **Phase:** B
- **Milestone:** B0 — Phase A exit hygiene
- **Status:** Done
- **Created:** 2026-04-22
- **Author:** @cemililik (+ Claude Opus 4.7 agent)
- **Dependencies:** Phase A complete (T-001..T-005 all Done). No sibling task dependencies within B0 — T-006 may run in parallel with T-008 (architecture docs) and T-009 (timer init).
- **Informs:** [T-007 — Idle task + typed scheduler deadlock](T-007-idle-task-typed-deadlock.md) *(not yet opened)*; every subsequent B1..B6 milestone depends on the aliasing story this task settles.
- **ADRs required:** [ADR-0021 — Raw-pointer scheduler IPC-bridge API](../../../decisions/0021-raw-pointer-scheduler-ipc-bridge.md) *(Accepted 2026-04-22)*.

---

## User story

As the Tyrne kernel's cooperative scheduler, I want an IPC-bridge API whose calling convention never requires the caller to hold a live `&mut` reference to any shared kernel state across a `cpu.context_switch` — so that the Rust aliasing model is not violated when a suspended task resumes and another task accesses the same statics, closing **UNSAFE-2026-0012** before Phase B adds preemption, SMP, or any additional shared-state path where the cooperative cover no longer masks the hazard.

## Context

[UNSAFE-2026-0012](../../../audits/unsafe-log.md) records that the BSP's `task_a` / `task_b` functions hold `&mut` references to `SCHED`, `EP_ARENA`, `IPC_QUEUES`, and `TABLE_{A,B}` live across `cpu.context_switch`. When the other task resumes, it derives its own `&mut` references to the same `UnsafeCell` interiors. Under Rust's strict aliasing model two live `&mut` references to the same storage are immediately UB — the compiler is entitled to optimise as if each of them were uniquely aliased, regardless of whether the accesses actually occur simultaneously. Tyrne v1's single-core cooperative invariant (no two tasks execute at once) covers the operational case, but not the language-level contract.

The 2026-04-21 [security review of Phase A exit](../../reviews/security-reviews/2026-04-21-tyrne-to-phase-a.md) made this the **#1 Phase-B blocker**. Every milestone after B0 (EL drop in B1, MMU-on in B2, address spaces in B3, syscall boundary in B5, userspace in B6) introduces more shared state and more paths where the cooperative cover no longer holds. Preemption and SMP — even if they remain out of scope through Phase B — will eventually expose the same aliasing hazard in a form where the compiler's assumption of no-aliasing becomes directly observable. Fixing the API now costs one B0 task; deferring costs the same task plus a cascading set of band-aids across subsequent milestones.

T-006 is therefore the first Phase B task on the critical path. T-007 (idle task + typed deadlock) and the rest of B0 can land behind it but the API shape decided here determines what the scheduler calling convention looks like for every subsequent IPC caller.

## Acceptance criteria

- [x] **ADR-0021 Accepted** (2026-04-22) — settles the raw-pointer calling convention: bridge entry points are `unsafe fn` free functions over `*mut Scheduler<C>` with momentary `&mut` materialisation strictly outside the `cpu.context_switch` window. UNSAFE-2026-0012 targeted for full retirement (no residual aliasing window).
- [x] **Scheduler IPC-bridge API** — `yield_now`, `ipc_send_and_yield`, and `ipc_recv_and_yield` are now `unsafe fn` free functions in `kernel::sched`, each taking `*mut Scheduler<C>` + `*mut EndpointArena` / `*mut IpcQueues` / `*mut CapabilityTable`. Internal `&mut` references live only inside narrow inner blocks that end before `cpu.context_switch` and are reacquired after. Commit `f9b72f8`.
- [x] **BSP adoption** — `task_a` / `task_b` call the free-function bridge with `*mut` pointers produced by `StaticCell::as_mut_ptr()`. No `assume_init_mut()` on `SCHED`, `EP_ARENA`, `IPC_QUEUES`, `TABLE_*` at any call site. Commit `f9b72f8`.
- [x] **`TaskArena` migration** — `TaskArena` moved to `static TASK_ARENA: StaticCell<TaskArena>`, matching the pattern of the other kernel-object arenas (K3-11). Commit `1746bc8`.
- [x] **Audit retirement** — UNSAFE-2026-0012 status → `Removed — 2026-04-22, commit f9b72f8`. New entries UNSAFE-2026-0013 (`StaticCell::as_mut_ptr` helper) and UNSAFE-2026-0014 (scheduler free-function momentary `&mut` pattern) recorded. Commit `a1310ae`.
- [x] **Tests** — `cargo host-test` reports 109 green (75 kernel + 34 test-hal); `cargo kernel-build` clean; QEMU smoke produces the A6 trace unchanged.
- [x] **No new `unsafe` without audit** — UNSAFE-2026-0013 and UNSAFE-2026-0014 cover the two new patterns introduced by the refactor. Each call site carries a `// SAFETY:` comment per [`unsafe-policy.md`](../../../standards/unsafe-policy.md) §1 and references its audit tag.

## Out of scope

- Idle task and typed `SchedError::Deadlock` — covered by T-007 / ADR-0022.
- Architecture docs (`kernel-objects.md`, `ipc.md`, `scheduler.md`) — covered by T-008.
- Timer initialisation and `CNTPCT_EL0` wiring — covered by T-009.
- Scheduler / IPC hardening bundle (const-assert on `SchedQueue::new`, `debug_assert!` → typed error, etc.) — covered by T-010.
- Missing-tests bundle (ReceiverTableFull, slot-reuse with pending cap) — covered by T-011.
- Multi-waiter wake-up on an endpoint — an ADR-0019 open question, not addressed here.
- Any code outside `kernel/src/sched/mod.rs` and `bsp-qemu-virt/src/main.rs`, apart from the audit log and a possible small `hal` crate doc adjustment if the scheduler's HAL contract needs to state the raw-pointer invariants.
- MMU, EL drop, syscalls, userspace — Phase B's later milestones.

## Approach

Delegated to **ADR-0021** for the final shape. At a sketch level:

1. **ADR-0021 drafting.** Write the ADR first. Considered options should include at minimum:
   - *Option A:* Raw-pointer parameters (`*mut EndpointArena`, `*mut IpcQueues`, `*mut CapabilityTable`) on the IPC bridge, with a narrow `&mut` materialised inside the scheduler only outside the context-switch window.
   - *Option B:* Move the shared state into the `Scheduler` struct itself so the bridge takes no external `&mut`. Requires the BSP to hand ownership of all three arenas to the scheduler at bootstrap.
   - *Option C:* Continuation-passing style — callers hand the scheduler a closure describing the resume path, and the scheduler calls back after the switch. Most invasive.
   - The ADR also needs to state *exactly which aliasing window (if any) survives*, and why it is sound.
2. **Implementation.** Adopt the chosen option in `kernel/src/sched/mod.rs`. Internal scheduler code keeps its current shape but never lets a `&mut` to external state live across `cpu.context_switch`.
3. **BSP migration.** Rewrite the 4 `assume_init_mut` call-sites in `task_a` / `task_b` to construct raw pointers via `(*CELL.0.get()).as_mut_ptr()` (or equivalent) and pass them into the bridge. The only `&mut` that materialises is inside the scheduler, momentarily, strictly before or strictly after the switch.
4. **`TaskArena` global.** Add `static TASK_ARENA: StaticCell<TaskArena> = StaticCell::new();`, move the `TaskArena::default()` + `create_task(...)` calls to write into it via `(*TASK_ARENA.0.get()).write(...)`. No behavioural change; uniformity with the other arenas.
5. **Audit.** Update UNSAFE-2026-0012: status → `Removed` with commit SHA. If the ADR identifies a residual window, narrow the entry instead with explicit invariants.
6. **Tests.** Re-run host tests. Run QEMU smoke; confirm trace matches A6. Add any regression test the ADR recommends.

## Definition of done

- [x] `cargo fmt --all -- --check` clean.
- [x] `cargo host-clippy` clean with `-D warnings`.
- [x] `cargo kernel-clippy` clean.
- [x] `cargo host-test` passes (109 tests green).
- [x] `cargo kernel-build` clean; QEMU smoke matches the A6 trace.
- [x] ADR-0021 Accepted before the implementation commit (2026-04-22, commit `3b8aa34`).
- [x] UNSAFE-2026-0012 audit entry status updated to `Removed — 2026-04-22, commit f9b72f8` (commit `a1310ae`).
- [x] New `unsafe` audited — UNSAFE-2026-0013 (`StaticCell::as_mut_ptr`) and UNSAFE-2026-0014 (scheduler free-function momentary `&mut` pattern).
- [x] Commit messages follow [`commit-style.md`](../../../standards/commit-style.md) with `Refs: ADR-0021` and `Audit: UNSAFE-2026-0012` trailers.
- [x] Task status updated to `In Review`. Transition to `Done` awaits maintainer sign-off (and ideally a brief security re-review of the aliasing story, even if optional in v1 solo phase).
- [x] [`docs/roadmap/current.md`](../../../roadmap/current.md) updated — commit `92e5acd`.

## Design notes

- **Why raw pointers and not a bigger redesign?** The IPC bridge's lifetime story is fundamentally driven by the cooperative context switch: the scheduler needs to drive state after the *other* task resumes, without carrying a compiler-visible `&mut` across that boundary. Raw pointers are the minimum change that dissolves the aliasing while keeping every other invariant (single-core cooperative, bounded state, no heap, IPC state machine unchanged). A bigger refactor — e.g. a message-passing scheduler API where tasks enqueue state transitions — is Phase C's concern, once preemption makes the cooperative-cover invariant no longer available anyway. Option B in the approach section is the second-best alternative; ADR-0021 weighs it explicitly.
- **Why bundle `TaskArena` migration here?** [phase-b.md §B0](../../../roadmap/phases/phase-b.md) bundles K3-11 with this task because both touch the BSP's static-cell surface. Doing both in one commit avoids two rounds of BSP churn and keeps the static-cell footprint consistent. The migration is also what makes the post-B0 world ready for task destruction or status-query APIs without another BSP rewrite.
- **What if UNSAFE-2026-0012 cannot be fully removed?** The residual case is worth recording. If a single `&mut Scheduler` remains live across `context_switch` inside the scheduler's own call frame (because the scheduler *is* the caller), UNSAFE-2026-0012's scope narrows to "the scheduler's own `&mut self` across its own call" rather than disappearing. That is still a strict improvement (removes BSP-task aliasing, leaves only a kernel-internal aliasing window inside code the kernel team authors and reviews), and the audit entry narrows accordingly. Full removal is the goal; partial narrowing with a documented invariant is the acceptable fallback.
- **Coupling with `Scheduler::start` / `Scheduler::yield_now`.** `yield_now` already uses a split-borrow with raw-pointer arithmetic on `self.contexts` ([sched/mod.rs:310-316](../../../../kernel/src/sched/mod.rs#L310-L316)); that pattern is sound because the two indices are provably distinct. The UNSAFE-2026-0012 problem is specifically the *external* `&mut` to `EP_ARENA / IPC_QUEUES / TABLE_*` that the bridge currently consumes. The split-borrow inside `Scheduler` is unaffected by this task.
- **Projected commit sequence.**
  1. `docs(roadmap): open T-006 — raw-pointer scheduler API refactor` (this opening commit — task file + current.md + phase-b/README.md).
  2. `docs(adr): propose ADR-0021 — raw-pointer scheduler IPC-bridge API`.
  3. `docs(adr): accept ADR-0021` (after review).
  4. `feat(sched): raw-pointer IPC bridge API (T-006)`.
  5. `feat(bsp): TaskArena → StaticCell; adopt raw-pointer bridge in task_a/b (T-006)`.
  6. `docs(audits): retire UNSAFE-2026-0012`.
  7. `docs(roadmap): T-006 → Done; T-007 next`.

## References

- [UNSAFE-2026-0012 audit entry](../../../audits/unsafe-log.md) — the concrete aliasing hazard this task closes.
- [Security review of Phase A exit](../../reviews/security-reviews/2026-04-21-tyrne-to-phase-a.md) — §1 and §3 enumerate the blocker.
- [Code review of Phase A exit](../../reviews/code-reviews/2026-04-21-tyrne-to-phase-a.md) — §Correctness (Scheduler bullet 2) and §Integration (`Kernel is zero-\`unsafe\``) flag the same surface.
- [Phase B plan](../../../roadmap/phases/phase-b.md) — §B0 sub-items 1 and 7 (this task's scope); §How to start Phase B (ordering).
- [ADR-0013: Roadmap and planning process](../../../decisions/0013-roadmap-and-planning.md).
- [ADR-0016: Kernel object storage](../../../decisions/0016-kernel-object-storage.md) — TaskArena migration conforms to this ADR's global-ownership framing.
- [ADR-0019: Scheduler shape](../../../decisions/0019-scheduler-shape.md) — the IPC bridge is part of the scheduler per this ADR.
- [ADR-0020: Cpu trait v2 / context-switch extension](../../../decisions/0020-cpu-trait-v2-context-switch.md) — the context switch around which the aliasing window currently opens.
- [`unsafe-policy.md`](../../../standards/unsafe-policy.md) — the discipline every new `unsafe` block upholds.
- [`justify-unsafe`](../../../../.claude/skills/justify-unsafe/SKILL.md) — the skill that writes the audit entry.

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-04-22 | @cemililik (+ Claude Opus 4.7 agent) | opened; status `In Progress`. ADR-0021 not yet written — writing it is the first step inside this task per phase-b.md's "How to start Phase B". `current.md` updated; T-006 is now the active task. |
| 2026-04-22 | @cemililik (+ Claude Opus 4.7 agent) | [ADR-0021 — Raw-pointer scheduler IPC-bridge API](../../../decisions/0021-raw-pointer-scheduler-ipc-bridge.md) drafted at status `Proposed`. Four options considered (raw-pointer parameters, scheduler-owns-arenas, continuation-passing, per-task `TaskContext` extensions); raw-pointer parameters chosen. Awaiting maintainer sign-off → `Accepted` before implementation begins. |
| 2026-04-22 | @cemililik (+ Claude Opus 4.7 agent) | ADR-0021 revised on maintainer request. Self-review caught a flaw in the initial draft: the bridge's `&mut self` receiver produces a `&mut Scheduler` that crosses `cpu.context_switch`, which is the same aliasing hazard UNSAFE-2026-0012 describes — merely relocated from the arenas to the scheduler. The revision promotes Option A to "all-pointers including `self`": bridge entry points are `unsafe fn`s over `*mut Scheduler<C>`, not `&mut self` methods. Consequence sections and Option A / Option B pros-cons updated; a `Revision notes` section records the correction. Status remains `Proposed`; still awaiting Accepted sign-off. |
| 2026-04-22 | @cemililik | ADR-0021 → `Accepted`. Implementation phase cleared to begin: (1) raw-pointer bridge in `kernel/src/sched/mod.rs`, (2) BSP adoption in `bsp-qemu-virt/src/main.rs` with `StaticCell::as_mut_ptr` helper + `TaskArena` → `StaticCell` migration, (3) 11 scheduler host tests updated to pass raw pointers, (4) UNSAFE-2026-0012 audit-log entry moved to `Removed` + any new audit entries for the two helper patterns. Each step lands as its own commit; task moves to `In Review` when all six projected commits are in and host tests + QEMU smoke are green. |
| 2026-04-22 | @cemililik (+ Claude Opus 4.7 agent) | Implementation complete. Four commits landed: `f9b72f8` (scheduler refactor + BSP adoption + `StaticCell::as_mut_ptr` helper), `1746bc8` (TaskArena → StaticCell global migration, K3-11), `a1310ae` (UNSAFE-2026-0012 → `Removed`, new UNSAFE-2026-0013 / 0014 audit entries). Verification: `cargo fmt` clean, `cargo host-clippy` + `cargo kernel-clippy` clean, 109 host tests green, `cargo kernel-build` clean, QEMU smoke reproduces the A6 trace unchanged. Status → `In Review`; awaiting maintainer sign-off to promote to `Done`. |
| 2026-04-22 | @cemililik (+ Claude Opus 4.7 agent) | Post-In-Review second-read pass landed as commit `7eaa10a`. Three silent-failure gaps closed: `Scheduler::start` was still a `&mut self` method (the only residual ADR-0021 violation) and is now a raw-pointer free function; `let _ = self.ready.enqueue(...)` on two sites (yield_now re-enqueue and unblock_receiver_on) became invariant-panic on failure so regression would be loud instead of silently losing a task; `sync_generation` (renamed `reset_if_stale_generation`) gained a `debug_assert!` that fires if a future destroy path ever leaves a `SendPending/RecvComplete { cap: Some(_) }` behind, preventing silent `Capability` drops. Also: `debug_assert_ne!(current_idx, next_idx)` before both context-switch sites. ADR-0021 gained an in-place *Revision notes* rider documenting the `start()` reshape + the global-invariant language in the shared safety contract. UNSAFE-2026-0012 retirement entry gained a matching post-review rider. All 75 kernel + 34 test-hal = 109 host tests remain green; QEMU smoke still matches A6. Mini-retro filed at `docs/analysis/reviews/business-reviews/2026-04-22-T-006-mini-retro.md` (commit `bd919bd`). |
| 2026-04-22 | @cemililik (+ Claude Opus 4.7 agent) | Close-out second-read (after T-007 landed on top of T-006). Independent-agent review walked every call site of the four ADR-0021-guarded types and confirmed no `&mut` crosses `cpu.context_switch`. UNSAFE-2026-0014's citation list covers all current call sites (including T-007's `idle_entry` which only derives raw pointers, never materialising its own momentary `&mut`). ADR-0021's *Decision outcome* matches the shipped code (all four scheduler entry points are raw-pointer free functions). Four small doc cleanups applied in the close-out commit: ADR-0021 Revision notes reordered, stale "Status unchanged — Proposed" line removed, this review-history row added, and the DoD "(pending)" parenthetical on `current.md` resolved. T-006 is ready to close. |
