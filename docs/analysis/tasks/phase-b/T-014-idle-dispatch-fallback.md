# T-014 — Idle dispatch via separate fallback slot

- **Phase:** B
- **Milestone:** B1 — Drop to EL1 in boot, install exception infrastructure (reopened 2026-05-06 by [B1 smoke-regression mini-retro](../../reviews/business-reviews/2026-05-06-B1-smoke-regression.md))
- **Status:** In Progress
- **Created:** 2026-05-06
- **Author:** @cemililik (+ Claude Opus 4.7 agent)
- **Dependencies:** [ADR-0026 — Idle dispatch via separate fallback slot](../../../decisions/0026-idle-dispatch-fallback.md) — must be `Accepted` before the kernel scheduler refactor lands.
- **Informs:** Closes the B1 smoke regression. Unblocks B2 prep (ADR-0027 kernel virtual memory layout drafting), which the [2026-04-28 B1 closure retro](../../reviews/business-reviews/2026-04-28-B1-closure.md) had authorised in parallel with the (then-pending) maintainer-side smoke. Future preemption work (Phase B5+) inherits the cleaner dispatch shape.
- **ADRs required:** [ADR-0026](../../../decisions/0026-idle-dispatch-fallback.md). No other ADR change — ADR-0022's typed-error axis (Option G) is preserved; ADR-0019 / ADR-0020 / ADR-0021 are unaffected.

---

## User story

As the Tyrne cooperative scheduler, I want idle to be dispatched **only** when no real task is Ready, so that the cooperative IPC demo (and every future cooperative workload) cannot silently hang in `WFI` after a context switch picks idle from the FIFO ahead of a just-unblocked receiver — the regression the 2026-05-06 QEMU smoke surfaced at HEAD `214052d`.

## Context

[ADR-0022](../../../decisions/0022-idle-task-and-typed-scheduler-deadlock.md) (Accepted 2026-04-22) chose Option A — idle as a regular task in the FIFO ready queue — on the assumption that `yield_now`'s "only one ready task" early-return path would collapse the solo-idle case to an inline WFI loop. That assumption holds when idle is the *sole* Ready task; it fails the moment three tasks (`task_b` unblocked by `unblock_receiver_on`, `task_a` re-enqueued by `yield_now`, `idle_entry` perpetually Ready) sit in the FIFO simultaneously. Round-robin then dispatches whichever is at the head — and by `kernel_entry`'s `add_task(B, A, idle)` ordering, that is **idle**. Idle's body issues `WFI`; v1's demo never arms a deadline; no IRQ ever fires; the kernel hangs.

The first end-to-end QEMU smoke at HEAD `214052d` (2026-05-06) reproduced this hang. Six prior review/closure artefacts had cleared the code path (Phase A code review, B0 closure trio, B1 closure trio); none of them simulated the demo's queue-state machine. See [`2026-05-06-B1-smoke-regression.md`](../../reviews/business-reviews/2026-05-06-B1-smoke-regression.md) for the full incident report and [ADR-0026](../../../decisions/0026-idle-dispatch-fallback.md) for the structural fix this task implements.

This task is the *implementation* half of ADR-0026's chosen Option B. The ADR's *Decision outcome* presents a queue-state simulation table proving Option B fixes the demo flow structurally. T-014 implements that fix with ~30-50 LOC of kernel diff plus one BSP `kernel_entry` line.

## Acceptance criteria

- [ ] **ADR-0026 Accepted** before any code lands. Same-day Accept after careful re-read is permitted per [ADR-0025 §Revision notes](../../../decisions/0025-adr-governance-amendments.md); the Propose commit is separate from the Accept commit.
- [ ] **`Scheduler<C>` field added.** `kernel/src/sched/mod.rs` gains `idle: Option<TaskHandle>` on the `Scheduler<C>` struct, default `None`, written exclusively by the new `register_idle` free function.
- [ ] **`register_idle` raw-pointer free function added** in the [ADR-0021](../../../decisions/0021-raw-pointer-scheduler-ipc-bridge.md) shape: `pub unsafe fn register_idle<C: ContextSwitch + Cpu>(sched: *mut Scheduler<C>, cpu: &C, handle: TaskHandle, entry: fn() -> !, stack_top: *mut u8) -> Result<(), SchedError>`. Internally calls `init_context` + writes `s.idle = Some(handle)` inside a momentary `&mut Scheduler<C>` block per the Shared safety contract; the function is `unsafe` for the same `*mut Scheduler<C>` reasons as `add_task`.
- [ ] **Dispatch sites consult `idle` as a fallback.** In `start_prelude`, `yield_now`, and `ipc_recv_and_yield`, the dequeue chain becomes `s.ready.dequeue().or_else(|| s.idle)` (or equivalent). Idle is selected only when the ready queue is genuinely empty.
- [ ] **`SchedError::Deadlock` semantics tightened.** The defensive return now fires only when both halves are unavailable: ready queue empty AND `idle.is_none()`. The variant remains; its meaning becomes more precise.
- [ ] **`bsp-qemu-virt::kernel_entry` updated.** The `add_task(idle_entry, ...)` call for idle becomes `register_idle(idle_entry, ...)`. The `add_task(B, ...)` and `add_task(A, ...)` calls are unchanged.
- [ ] **UNSAFE-2026-0014 Amendment.** Append a 2026-05-06+ Amendment naming `register_idle` as a new sanctioned site of the momentary-`&mut`-from-`*mut`-Scheduler pattern, with the same shared-safety-contract reasoning the existing entry covers.
- **Tests.**
  - [ ] **Existing host tests still pass.** All 149 currently-green tests must remain green; `start_prelude_panics_on_empty_ready_queue` may need its setup adjusted to avoid registering idle (its panic path is now "ready queue empty AND idle is None"; the test's intent is unchanged).
  - [ ] **New: `register_idle_stores_handle_in_idle_slot`.** Direct unit test asserting `register_idle` writes to `idle` and does not enqueue.
  - [ ] **New: `dispatcher_picks_idle_only_when_ready_empty`.** Constructs a scheduler with one regular task and idle registered; asserts that `start_prelude` and `yield_now` never select idle while the regular task is Ready, and select idle only when the regular task transitions to Blocked.
  - [ ] **New: `unblock_after_yield_dispatches_unblocked_not_idle`.** Reproduces the demo flow's failing step in a host test: `register_idle`, register two tasks A and B, mark B Blocked-on-EP, run A's `unblock_receiver_on(ep)` + `yield_now`, assert next dispatched task is B (not idle).
  - [ ] **`SchedError::Deadlock` test still fires** — the existing T-011 test that constructs a scheduler without idle and asserts `Err(SchedError::Deadlock)` continues to pass with the new dispatch chain.
- [ ] **QEMU smoke pass.** End-to-end smoke at the post-T-014 HEAD produces the full demo trace through `tyrne: all tasks complete` plus the boot-to-end timing line. Maintainer runs and posts the trace; trace lands in T-014's review-history table verbatim.
- [ ] **UNSAFE-2026-0019 / 0020 final Amendment.** Once the smoke passes, append a 2026-05-06+ Amendment to UNSAFE-2026-0019 and UNSAFE-2026-0020 lifting the partial-verification status by recording that the smoke now reaches the same setup sites *and* completes the demo without hanging. UNSAFE-2026-0021 remains `Pending QEMU smoke verification` because v1's demo still does not arm a deadline (the timer-write site's contract remains unexercised).
- [ ] **Documentation.**
  - [ ] `docs/architecture/scheduler.md` updated to reflect the dispatch-with-fallback shape; the existing prose describing idle's role as "always Ready in the FIFO" needs a §Revision rider noting the supersession by ADR-0026 and pointing at the new shape.
  - [ ] [`docs/roadmap/current.md`](../../../roadmap/current.md) — *Active task* and *Last reviews* lines updated by T-014's status transitions.
  - [ ] [`docs/analysis/tasks/phase-b/README.md`](README.md) — new index row added for T-014.
- [ ] **B1 milestone-level closure.** Once the smoke passes and T-014 is `Done`, a fresh B1 closure trio (business + consolidated security + performance) replaces the 2026-04-28 trio's load-bearing role. The 2026-04-28 trio remains historical record. Trigger handled separately; T-014's DoD includes "B1 closure retro can be opened" but not "T-014 writes the retro itself".

## Out of scope

- **Per-CPU idle (Phase C SMP).** ADR-0026 is forward-compatible with per-CPU `idle: [Option<TaskHandle>; NCPU]`, but v1 is single-core; the array shape lands when SMP does.
- **Idle priority class beyond fallback semantics.** ADR-0019's open question on multi-priority classes stays open; T-014 is "lowest priority = fallback only", not "explicit priority levels".
- **Removing idle's `yield_now` call after `wait_for_interrupt`.** ADR-0026 §Consequences (Negative bullet 4) notes that idle's body could simplify (drop `yield_now`) since idle never sits in the FIFO. T-014 keeps the `wait_for_interrupt() + yield_now` shape uniform with v1's existing `idle_entry` body — the `yield_now` becomes a no-op (ready queue empty + idle is current → fallback returns idle again → fast path returns) but costs nothing and preserves the option to grow idle's body later.
- **B2 prep (ADR-0027).** Paused until B1 closes for real (post-T-014 + smoke pass). Reactivated by the new B1 closure retro's *Pathfinder* output.
- **The seven Track-E doc-drift blockers** from the 2026-05-06 comprehensive code review. Tracked separately in [`2026-05-06-full-tree-comprehensive.md`](../../reviews/code-reviews/2026-05-06-full-tree-comprehensive.md); orthogonal to T-014.

## Approach

The implementation is roughly 30-50 lines across three files. ADR-0026's *Decision outcome* §Dependency chain enumerates the seven steps; the high-level shape:

1. **`Scheduler<C>` struct gains the field.** Add `pub(crate) idle: Option<TaskHandle>` to the struct definition. `Default` impl sets it to `None`.
2. **`register_idle` mirrors `add_task`'s shape.** Same `*mut Scheduler<C>` parameter discipline, same momentary `&mut` pattern, same reliance on `init_context`. The only difference is the destination of the resulting `TaskHandle` (`s.idle` vs. `s.ready.enqueue`).
3. **Dispatch chain in three sites:**
   - `start_prelude`: `s.ready.dequeue().or_else(|| s.idle).ok_or_else(|| panic!("..."))` — empty-queue + no-idle is still a boot-time programming error.
   - `yield_now`: `s.ready.dequeue().or_else(|| s.idle)` — when this returns the *current* task (idle yielding to itself), the existing fast-path early-return handles it. Otherwise switches.
   - `ipc_recv_and_yield`: `s.ready.dequeue().or_else(|| s.idle)` — when this returns `None`, return `Err(SchedError::Deadlock)` (rolling back the caller's Blocked-state mutation per the existing rollback discipline).
4. **`bsp-qemu-virt::kernel_entry`:** the third `add_task` call (the one for idle) becomes a `register_idle` call. The `TaskHandle` allocation pattern is unchanged — idle still consumes a `TaskArena` slot (the slot stores idle's `TaskContext`); only the registration target changes.
5. **Audit-log Amendment for UNSAFE-2026-0014.** New `register_idle` site listed; same momentary-`&mut` discipline.
6. **Host tests** (per Acceptance criteria §Tests). The new tests live in `kernel/src/sched/mod.rs`'s existing `#[cfg(test)] mod tests` block; the three new tests use the existing `FakeCpu` test harness.

The simulation table in [ADR-0026 §Decision outcome](../../../decisions/0026-idle-dispatch-fallback.md#decision-outcome) is the authoritative description of the post-fix demo flow. T-014's implementation must produce exactly that flow when run; the new `unblock_after_yield_dispatches_unblocked_not_idle` host test mechanically verifies it.

## Definition of done

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo host-clippy` clean with `-D warnings`.
- [ ] `cargo kernel-clippy` clean.
- [ ] `cargo host-test` passes — 149 + 3 new = **152 tests** (or whatever the final count comes out to after the three new tests + any test adjustments).
- [ ] `cargo +nightly miri test` passes on the same set.
- [ ] `cargo kernel-build` clean.
- [ ] **QEMU smoke produces the full demo trace** through `tyrne: all tasks complete` plus the boot-to-end timing line; trace pasted into the review-history row.
- [ ] No new `unsafe` block without an audit entry; UNSAFE-2026-0014 gains an Amendment naming `register_idle`.
- [ ] UNSAFE-2026-0019 / 0020 gain final Amendments lifting their partial-verification status; UNSAFE-2026-0021 keeps its `Pending QEMU smoke verification` (v1 demo unchanged on the deadline-arming axis).
- [ ] Commit messages follow [`commit-style.md`](../../../standards/commit-style.md) with `Refs: ADR-0026` and `Audit: UNSAFE-2026-0014` trailers.
- [ ] Task status updated to `In Review` then `Done`; phase-b.md §B1 status update points at this task; [`docs/roadmap/current.md`](../../../roadmap/current.md) updated.
- [ ] B1 milestone-level closure trio (business + security + performance) trigger registered for the maintainer to schedule (T-014 does *not* itself produce the trio; the trio's *Pathfinder* output owns the next-task pointer to ADR-0027 / B2 prep).

## Design notes

- **Why not patch the FIFO ordering (ADR-0026 Option E)?** Considered and rejected in ADR-0026 §Pros and cons. Local hack vs. structural fix: local hacks hit the same problem on every future workload that introduces an additional priority class.
- **Why is the simulation table in ADR-0026 and not duplicated here?** ADR-0026 owns the design rationale; T-014 implements it. Duplicating the table here would invite drift.
- **Why does idle's body keep `yield_now` after `wait_for_interrupt`?** Cost is nothing (the call becomes a fast-path no-op when idle is the only task or when the dispatcher picks idle as fallback again). Removing the call would diverge from the v1 implementation pattern and require a doc update in `docs/architecture/scheduler.md` for no observable benefit. Keeping it preserves "idle's body is a normal cooperative loop" as a teaching point for future BSP authors.
- **Why is the `start_prelude` panic preserved instead of becoming `SchedError::QueueEmpty`?** Same reasoning as ADR-0022 §Decision outcome. Boot-time programming errors panic; runtime conditions return typed errors. ADR-0026 inherits this without re-litigating it.
- **What about the Track-E doc-drift fixes?** Orthogonal; the doc fixes (GIC v2/v3 confusion, `scheduler.md` idle-prose, `hal.md` Timer status, `security-model.md` DAIF closure, glossary ADR-0023 dead link, etc.) are pure documentation work and can land in their own commit cluster either before, alongside, or after T-014. The `scheduler.md` rider this task adds about ADR-0026 supersession is the only doc change in T-014's scope.

## References

- [ADR-0026 — Idle dispatch via separate fallback slot](../../../decisions/0026-idle-dispatch-fallback.md) — the policy this task implements; supersedes ADR-0022 §Decision outcome's idle-task-location axis.
- [ADR-0022 — Idle task and typed scheduler deadlock error](../../../decisions/0022-idle-task-and-typed-scheduler-deadlock.md) — the superseded ADR; its typed-error axis (Option G) is preserved unchanged.
- [ADR-0021 — Raw-pointer scheduler IPC-bridge API](../../../decisions/0021-raw-pointer-scheduler-ipc-bridge.md) — the raw-pointer convention `register_idle` follows.
- [ADR-0019 — Scheduler shape](../../../decisions/0019-scheduler-shape.md) — defines `SchedError::Deadlock` and the FIFO ready queue.
- [B1 smoke-regression mini-retro (2026-05-06)](../../reviews/business-reviews/2026-05-06-B1-smoke-regression.md) — the empirical event motivating ADR-0026 + T-014.
- [Comprehensive code review (2026-05-06)](../../reviews/code-reviews/2026-05-06-full-tree-comprehensive.md) — multi-agent review running the same day; Track A (Kernel correctness) Approve illustrates the static-analysis gap that the ADR-0026 simulation table closes.
- [T-007 task file](T-007-idle-task-typed-deadlock.md) — the task that introduced ADR-0022's Option A; T-014 does not reopen T-007 (its DoD was satisfied; the structural failure is in the design ADR-0022 chose, not in T-007's execution).
- [`kernel/src/sched/mod.rs`](../../../../kernel/src/sched/mod.rs) — the dispatch sites this task modifies.
- [`bsp-qemu-virt/src/main.rs`](../../../../bsp-qemu-virt/src/main.rs) — the `kernel_entry` line that switches from `add_task(idle_entry, ...)` to `register_idle(idle_entry, ...)`.
- [`docs/audits/unsafe-log.md`](../../../audits/unsafe-log.md) — UNSAFE-2026-0014 (Amendment), UNSAFE-2026-0019 / 0020 (final Amendments).

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-05-06 | @cemililik (+ Claude Opus 4.7 agent) | Opened with status `Draft`, paired with ADR-0026 (`Proposed`) per [ADR-0025 §Rule 1](../../../decisions/0025-adr-governance-amendments.md) (forward-reference contract) — ADR-0026's *Dependency chain* requires a real T-NNN file for the implementation step; this task is that file. Will move to `In Progress` only after ADR-0026 is `Accepted`. |
| 2026-05-06 | @cemililik (+ Claude Opus 4.7 agent) | Promoted `Draft → In Progress`. ADR-0026 was Accepted same-day after careful re-read per [ADR-0025 §Revision notes](../../../decisions/0025-adr-governance-amendments.md) (substance-of-the-step gate replaces calendar cool-down). Implementation: `kernel/src/sched/mod.rs` adds `Scheduler::idle: Option<TaskHandle>` field + `register_idle` raw-pointer free function + dispatch-chain updates in `start_prelude` / `yield_now` / `ipc_recv_and_yield` (each consults `s.idle` only when `s.ready.dequeue()` returns `None`); `bsp-qemu-virt/src/main.rs` switches the third `add_task(idle_entry, ...)` call to `register_idle(...)`; `idle_entry` doc-comment + inline comments updated for ADR-0026. Three new host tests added: `register_idle_stores_handle_in_idle_slot_and_not_in_ready_queue`, `dispatcher_picks_idle_only_when_ready_queue_empty`, `unblock_after_yield_dispatches_unblocked_receiver_not_idle` (the third is the regression guard). Module-level docstring §"Idle task" rewritten for the supersession; `SchedError::Deadlock` doc-comment updated. |
