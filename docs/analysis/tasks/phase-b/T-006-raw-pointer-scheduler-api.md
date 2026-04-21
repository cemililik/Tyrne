# T-006 — Raw-pointer scheduler API refactor

- **Phase:** B
- **Milestone:** B0 — Phase A exit hygiene
- **Status:** In Progress
- **Created:** 2026-04-22
- **Author:** @cemililik (+ Claude Opus 4.7 agent)
- **Dependencies:** Phase A complete (T-001..T-005 all Done). No sibling task dependencies within B0 — T-006 may run in parallel with T-008 (architecture docs) and T-009 (timer init).
- **Informs:** [T-007 — Idle task + typed scheduler deadlock](T-007-idle-task-typed-deadlock.md) *(not yet opened)*; every subsequent B1..B6 milestone depends on the aliasing story this task settles.
- **ADRs required:** [ADR-0021 — Raw-pointer scheduler IPC-bridge API](../../../decisions/0021-raw-pointer-scheduler-ipc-bridge.md) *(Proposed 2026-04-22; must be Accepted before implementation commits land)*.

---

## User story

As the Umbrix kernel's cooperative scheduler, I want an IPC-bridge API whose calling convention never requires the caller to hold a live `&mut` reference to any shared kernel state across a `cpu.context_switch` — so that the Rust aliasing model is not violated when a suspended task resumes and another task accesses the same statics, closing **UNSAFE-2026-0012** before Phase B adds preemption, SMP, or any additional shared-state path where the cooperative cover no longer masks the hazard.

## Context

[UNSAFE-2026-0012](../../../audits/unsafe-log.md) records that the BSP's `task_a` / `task_b` functions hold `&mut` references to `SCHED`, `EP_ARENA`, `IPC_QUEUES`, and `TABLE_{A,B}` live across `cpu.context_switch`. When the other task resumes, it derives its own `&mut` references to the same `UnsafeCell` interiors. Under Rust's strict aliasing model two live `&mut` references to the same storage are immediately UB — the compiler is entitled to optimise as if each of them were uniquely aliased, regardless of whether the accesses actually occur simultaneously. Umbrix v1's single-core cooperative invariant (no two tasks execute at once) covers the operational case, but not the language-level contract.

The 2026-04-21 [security review of Phase A exit](../../reviews/security-reviews/2026-04-21-umbrix-to-phase-a.md) made this the **#1 Phase-B blocker**. Every milestone after B0 (EL drop in B1, MMU-on in B2, address spaces in B3, syscall boundary in B5, userspace in B6) introduces more shared state and more paths where the cooperative cover no longer holds. Preemption and SMP — even if they remain out of scope through Phase B — will eventually expose the same aliasing hazard in a form where the compiler's assumption of no-aliasing becomes directly observable. Fixing the API now costs one B0 task; deferring costs the same task plus a cascading set of band-aids across subsequent milestones.

T-006 is therefore the first Phase B task on the critical path. T-007 (idle task + typed deadlock) and the rest of B0 can land behind it but the API shape decided here determines what the scheduler calling convention looks like for every subsequent IPC caller.

## Acceptance criteria

- [ ] **ADR-0021 Accepted** — settles the raw-pointer calling convention, states the invariants callers uphold, and notes which (if any) aliasing window survives (it should not; the goal is full removal).
- [ ] **Scheduler IPC-bridge API.** `Scheduler::ipc_send_and_yield` and `Scheduler::ipc_recv_and_yield` take raw pointers (`*mut EndpointArena`, `*mut IpcQueues`, `*mut CapabilityTable`) across the `cpu.context_switch` call, or are redesigned such that no caller-visible `&mut` is live across the switch. Internal code inside the scheduler may still materialise `&mut` *momentarily* (strictly before the switch is entered and strictly after it returns).
- [ ] **BSP adoption.** `task_a` / `task_b` in [`bsp-qemu-virt/src/main.rs`](../../../../bsp-qemu-virt/src/main.rs) no longer hold `&mut` references alive across `cpu.context_switch`. Every `assume_init_mut()` site either disappears or is reduced to a raw-pointer acquisition followed by a narrow, non-crossing use.
- [ ] **`TaskArena` migration.** `TaskArena` moves from a local `kernel_entry` variable to a `StaticCell<TaskArena>` global, matching the `EP_ARENA` / `TABLE_{A,B}` pattern (phase-b.md §B0 sub-item 7; Kova 3 K3-11). No functional change — the scheduler still only needs `TaskHandle`s — but the storage shape becomes uniform with the other kernel-object arenas per [ADR-0016](../../../decisions/0016-kernel-object-storage.md).
- [ ] **Audit retirement.** `docs/audits/unsafe-log.md` UNSAFE-2026-0012 entry marked `Removed — <commit SHA>` with the resolution date, or explicitly narrowed to any residual window the ADR identifies (with a documented invariant that makes that window sound).
- [ ] **Tests.** `cargo host-test` reports 109+ green (any new scheduler tests added; no regressions). `cargo kernel-build` clean. `tools/run-qemu.sh` produces the A6 trace unchanged.
- [ ] **No new `unsafe` without audit.** Each `unsafe` block introduced by the refactor (raw pointer derefs in the BSP task bodies; any additions inside `Scheduler`) carries a conforming `// SAFETY:` comment per [`unsafe-policy.md`](../../../standards/unsafe-policy.md) §1 and a new `UNSAFE-2026-NNNN` audit entry.

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

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo host-clippy` clean with `-D warnings`.
- [ ] `cargo kernel-clippy` clean.
- [ ] `cargo host-test` passes (109+ tests).
- [ ] `cargo kernel-build` clean; QEMU smoke matches the A6 trace.
- [ ] ADR-0021 Accepted before the implementation commit.
- [ ] UNSAFE-2026-0012 audit entry status updated (Removed or narrowed) with commit SHA.
- [ ] Any new `unsafe` has an audit entry per [`unsafe-policy.md`](../../../standards/unsafe-policy.md) / [`justify-unsafe`](../../../../.claude/skills/justify-unsafe/SKILL.md).
- [ ] Commit messages follow [`commit-style.md`](../../../standards/commit-style.md) with `Refs: ADR-0021` and, where applicable, `Audit: UNSAFE-2026-0012`, `Security-Review: ...` trailers.
- [ ] Task status updated to `In Review` after implementation, then `Done` after a security re-review of the aliasing story.
- [ ] [`docs/roadmap/current.md`](../../../roadmap/current.md) updated on each status transition.

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
- [Security review of Phase A exit](../../reviews/security-reviews/2026-04-21-umbrix-to-phase-a.md) — §1 and §3 enumerate the blocker.
- [Code review of Phase A exit](../../reviews/code-reviews/2026-04-21-umbrix-to-phase-a.md) — §Correctness (Scheduler bullet 2) and §Integration (`Kernel is zero-\`unsafe\``) flag the same surface.
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
