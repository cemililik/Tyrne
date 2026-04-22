# T-007 — Idle task + typed `SchedError::Deadlock` + resume-path hardening

- **Phase:** B
- **Milestone:** B0 — Phase A exit hygiene
- **Status:** In Review
- **Created:** 2026-04-22
- **Author:** @cemililik (+ Claude Opus 4.7 agent)
- **Dependencies:** [T-006](T-006-raw-pointer-scheduler-api.md) (`In Review`, ADR-0021 Accepted — the raw-pointer bridge shape this task's idle-registration path inherits).
- **Informs:** [T-011](.) (Missing-tests bundle — adds the direct unit test for `SchedError::Deadlock` once this task lands). Unblocks every B1+ milestone that depends on a panic-free scheduler.
- **ADRs required:** [ADR-0022 — Idle task and typed scheduler deadlock error](../../../decisions/0022-idle-task-and-typed-scheduler-deadlock.md) *(Accepted 2026-04-22)*.

---

## User story

As the Tyrne kernel scheduler, I want a registered idle task and typed error variants for the two current kernel-liveness panics — so that a userspace-caused IPC graph with no runnable task cannot panic the kernel, and the resume-path `debug_assert!` in `ipc_recv_and_yield` becomes a release-mode typed return instead of a silent fall-through that the caller decodes as a panic one frame up.

## Context

The A5 scheduler shipped with three hard-panic paths documented in [ADR-0022](../../../decisions/0022-idle-task-and-typed-scheduler-deadlock.md):

1. `sched::ipc_recv_and_yield` panics with `"deadlock: all tasks blocked on IPC and no idle task available"` when every task is blocked and the ready queue is empty.
2. `sched::start` panics when called without any registered task.
3. A `debug_assert!(!matches!(result, Ok(RecvOutcome::Pending)))` in `ipc_recv_and_yield`'s resume path is stripped in release builds, allowing a `RecvOutcome::Pending` to propagate to the caller — where the BSP's `let RecvOutcome::Received { … } = outcome else { panic!(…) }` turns it into a downstream panic.

The 2026-04-21 Phase-A-exit security review flagged path (1) as §4 (liveness); the code review flagged all three. ADR-0022 resolves the design: idle-as-regular-task (Option A) + two new error surfaces (Option G: `SchedError::Deadlock` and `IpcError::PendingAfterResume`). Path (2)'s `start` empty-queue panic is **kept** — it is a boot-time programming error, not a runtime condition.

T-007 lands the implementation: register an idle task during `kernel_entry`, extend `SchedError` and `IpcError`, and wire the typed returns through `ipc_recv_and_yield`. Because ADR-0022 chose "idle as regular task", there is no new scheduler API — idle uses `sched::add_task` + `sched::yield_now` unchanged.

## Acceptance criteria

- [x] **ADR-0022 Accepted** (2026-04-22) — idle-as-regular-task + typed `SchedError::Deadlock` + `IpcError::PendingAfterResume`; `start`'s empty-queue panic kept.
- [x] **`SchedError::Deadlock` variant added** to `kernel::sched::SchedError` and returned by `ipc_recv_and_yield` in place of the former `panic!("deadlock: …")`; scheduler state is restored before the return. Commit `029d066`.
- [x] **`IpcError::PendingAfterResume` variant added** to `kernel::ipc::IpcError` and returned through `SchedError::Ipc(…)` in place of the former release-mode fall-through. The companion `debug_assert!` is removed per the ADR-0022 *Revision notes* second rider — the typed return is the observable contract. Commit `029d066` (variant) / `8110cc5` (assert removal).
- [x] **BSP idle task registered.** `bsp-qemu-virt/src/main.rs` adds `TASK_IDLE_STACK`, `fn idle_entry() -> !`, and a `create_task` + `add_task` for idle in `kernel_entry` *after* A and B so FIFO dispatch still runs task B first. Idle body is `spin_loop + yield_now` for v1 (WFI deferred to T-009 per ADR-0022 first rider — no IRQ source yet). Commit `25cfaf4`.
- [x] **Tests:** `ipc_recv_and_yield_returns_deadlock_when_ready_queue_empty` (blocks sole task, asserts `Err(SchedError::Deadlock)`, verifies state restored) and `ipc_recv_and_yield_resume_pending_returns_typed_err` (uses a `ResetQueuesCpu` test-helper to force `Ok(Pending)` resume, asserts typed `Err`). Commit `8110cc5`.
- [x] **Tests stay green.** 77 kernel + 34 test-hal = 111 host tests (+2 from T-007). QEMU smoke reproduces the A6 five-line trace byte-for-byte.
- [x] **No new `unsafe` blocks introduced.** Idle reuses the existing `TaskStack` + `fn() -> !` + raw-pointer-bridge patterns; `ResetQueuesCpu`'s `Send`/`Sync` impls are test-only and scoped to the test module.
- [x] **Documentation:** `ipc_recv_and_yield`'s `# Errors` section documents `SchedError::Deadlock` and `SchedError::Ipc(IpcError::PendingAfterResume)`; `IpcError::PendingAfterResume` and `SchedError::Deadlock` carry inline doc-comments citing ADR-0022; the scheduler module doc gained an *Idle task* section. `idle_entry` documents the WFI deferral.

## Out of scope

- Idle task priority / per-priority queues — priorities are not on the B0/B1 roadmap (ADR-0019); idle-as-lowest-priority is a follow-up ADR if priorities land later.
- Kernel-owned idle entry function (Option D in ADR-0022) — explicitly rejected; BSPs own their idle entry.
- Timer IRQ wiring that would let idle wake up on tick — T-009.
- `const { assert!(N > 0) }` on `SchedQueue::new` / `CapabilityTable::new` — routed to T-010's hardening bundle unless it falls out naturally from this work.
- `TASK_ARENA_CAPACITY` bump — not needed; 16 minus idle = 15 is plenty for v1.
- Any code outside `kernel/src/sched/mod.rs`, `kernel/src/ipc/mod.rs`, `bsp-qemu-virt/src/main.rs`, and the audit log / docs it touches.

## Approach

Settled in ADR-0022 §Decision outcome. At sketch level, in commit order:

1. **Extend `IpcError` with `PendingAfterResume`.** One-line variant addition plus doc.
2. **Extend `SchedError` with `Deadlock`.** One-line variant addition plus doc; keep the existing `Ipc(IpcError)` variant — `PendingAfterResume` propagates through it.
3. **Rewire `ipc_recv_and_yield`:**
   - Replace the `panic!("deadlock: …")` in the Phase 2 block with `return Err(SchedError::Deadlock)` — and restore `s.current` + `s.task_states[current_idx]` to their pre-block state before returning so the caller is not left in an inconsistent scheduler state.
   - In Phase 3 (resume), replace the `debug_assert!` + `result.map_err(SchedError::Ipc)` pattern with an explicit match that converts `Ok(RecvOutcome::Pending)` into `Err(SchedError::Ipc(IpcError::PendingAfterResume))`. (During implementation the `debug_assert!` was found redundant with the typed return and blocking the test; it was dropped per ADR-0022 *Revision notes* second rider — the typed `Err` is the observable contract.)
4. **BSP idle registration.**
   - Add `static TASK_IDLE_STACK: TaskStack = TaskStack::new();` next to the existing stacks in [`bsp-qemu-virt/src/main.rs`](../../../../bsp-qemu-virt/src/main.rs).
   - Add `fn idle_entry() -> !` that loops `core::hint::spin_loop()` then `sched::yield_now(SCHED.as_mut_ptr(), cpu)`; `.expect`s the `yield_now` Result since it can only return `Err(NoCurrentTask)`, impossible once the scheduler has started. (WFI is deferred to T-009 — see ADR-0022 *Revision notes* first rider: v1 has no IRQ source, so `cpu.wait_for_interrupt()` would suspend the core indefinitely the moment FIFO dispatches idle between two ready application tasks.)
   - In `kernel_entry`: `create_task` three times in order `A → B → idle` (so `handle_idle` is the highest slot index), then call `sched.add_task` in order `B → A → idle`. Idle is added **last** so FIFO dispatch runs Task B first (matching the A6 trace) and idle only reaches the head of the ready queue after both application tasks have yielded at least once.
5. **Tests.**
   - `sched::tests::ipc_recv_and_yield_returns_deadlock_when_ready_queue_empty` — builds a `Scheduler<FakeCpu>`, adds one task with a RECV capability, calls `ipc_recv_and_yield` via the raw-pointer bridge, asserts `Err(SchedError::Deadlock)`, and verifies the scheduler state (`current`, `task_states[i]`, `ready`) is identical to its pre-call shape.
   - `sched::tests::ipc_recv_and_yield_resume_pending_returns_typed_err` — uses a dedicated `ResetQueuesCpu` test helper whose `context_switch` rewrites `IpcQueues::new()` to force `ipc_recv`'s Phase 3 re-call into `Ok(Pending)`, then asserts `Err(SchedError::Ipc(IpcError::PendingAfterResume))`.
   - Both tests run under `cargo host-test`; neither requires QEMU.
6. **QEMU smoke.** Boot under QEMU, capture the five-line trace, confirm byte-for-byte match with the A6 baseline.
7. **Task status → In Review**, `current.md` updated, commit sequence matches the projected list in §Design notes.

## Definition of done

- [x] `cargo fmt --all -- --check` clean.
- [x] `cargo host-clippy` clean with `-D warnings`.
- [x] `cargo kernel-clippy` clean.
- [x] `cargo host-test` passes — 77 kernel + 34 test-hal = 111 host tests (+2 from T-007).
- [x] `cargo kernel-build` clean; QEMU smoke reproduces the A6 five-line trace unchanged.
- [x] ADR-0022 Accepted before the implementation commit (2026-04-22, commit `2895360` accepted via the T-007 opening commit).
- [x] No new `unsafe` blocks introduced — no audit-log changes required.
- [x] Commit messages follow [`commit-style.md`](../../../standards/commit-style.md) with `Refs: ADR-0022` trailers.
- [x] Task status updated to `In Review`; [`docs/roadmap/current.md`](../../../roadmap/current.md) updated.

## Design notes

- **Why restore state before returning `Err(SchedError::Deadlock)`?** The current `panic!` path leaves `s.current = None` and `s.task_states[current_idx] = Blocked { on: ep_handle }` because it never returns. A typed return that *doesn't* restore these leaves the caller's scheduler view inconsistent with the task's actual state (the task is still running — it's the one that just observed the `Err`). The restore block is small (one line for `current`, one for `task_states`), and it keeps the bridge's invariants composable: after `Err(Deadlock)` the scheduler state is exactly what it was before the bridge was called. The test asserts this directly.
- **Why two variants across two enums instead of one?** ADR-0022 §Option G weighs this. Short form: scheduler faults live in `SchedError`; IPC faults live in `IpcError`. `PendingAfterResume` is semantically an IPC invariant ("sender failed to deliver before unblocking receiver"); it propagates through `SchedError::Ipc(…)` exactly like `IpcError::InvalidCapability` does today.
- **Why is `start`'s empty-queue panic kept?** Boot-time programming error. The only way to reach it is `kernel_entry` that forgot to `add_task`. Converting to `Err` means `kernel_entry` has to panic one frame up instead — strictly more code and less informative. Panicking where the invariant is violated is correct.
- **Idle registration order.** FIFO dispatch runs the first-added task first, so the BSP's [`kernel_entry`](../../../../bsp-qemu-virt/src/main.rs) calls `sched.add_task` in the order **B → A → idle**. Idle added last means B is dispatched first (matching the A6 trace), A runs after B blocks, and idle only reaches the head of the ready queue after both application tasks have yielded into it.
- **Idle-yields-to-itself path.** When idle is the only ready task, `yield_now`'s "only one ready task" early-return fires (the dequeue after the idle re-enqueue hands back idle's own handle, and the bridge returns `Ok(())` without calling `cpu.context_switch`). The idle body — `spin_loop()` then `yield_now(...)` — therefore degrades to a tight kernel-resident spin without any scheduler round-trip. When T-009 wires a timer IRQ, the body becomes `wait_for_interrupt()` + `yield_now(...)` and the same fast-path applies, but the spin becomes a low-power WFI wait — matching ADR-0022 §Option A's long-term efficiency claim.
- **Projected commit sequence.**
  1. `docs(roadmap): open T-007 — idle task + typed scheduler deadlock (B0)` (this opening commit — task file + current.md + phase-b/README.md + phase-b.md + ADR-0022 accept).
  2. `feat(sched,ipc): SchedError::Deadlock + IpcError::PendingAfterResume; wire typed returns (T-007)`.
  3. `feat(bsp): register idle task in kernel_entry (T-007)`.
  4. `test(sched): deadlock and pending-after-resume return Err (T-007)`.
  5. `docs(roadmap): T-007 → In Review`.

## References

- [ADR-0022 — Idle task and typed scheduler deadlock error](../../../decisions/0022-idle-task-and-typed-scheduler-deadlock.md) — the design this task implements.
- [ADR-0019 — Scheduler shape](../../../decisions/0019-scheduler-shape.md) — defines `SchedError`.
- [ADR-0017 — IPC primitive set](../../../decisions/0017-ipc-primitive-set.md) — defines `IpcError`.
- [ADR-0021 — Raw-pointer scheduler IPC-bridge API](../../../decisions/0021-raw-pointer-scheduler-ipc-bridge.md) — the bridge shape idle's `add_task` + idle_entry's `yield_now` calls inherit.
- [Security review — Tyrne → Phase A exit](../../reviews/security-reviews/2026-04-21-tyrne-to-phase-a.md) — §4 flags the deadlock panic.
- [Code review — Tyrne → Phase A exit](../../reviews/code-reviews/2026-04-21-tyrne-to-phase-a.md) — *Correctness (Scheduler bullets 2, 4)* flags all three panics.
- [T-006 mini-retro](../../reviews/business-reviews/2026-04-22-T-006-mini-retro.md) — "post-In-Review second-read" adjustment: schedule a preventative-assert pass at `In Review` before promoting to `Done`.
- [Phase B plan](../../../roadmap/phases/phase-b.md) — §B0 item 2 bundles this work.

## Deferred follow-ups

Items raised during T-007's post-In-Review second-read that are intentionally out of scope for this task and routed elsewhere:

- **Symmetric state-restore test for `ipc_send_and_yield`.** T-007 verifies that `ipc_recv_and_yield` leaves the scheduler state unchanged on `Err(SchedError::Deadlock)`. The send-side bridge has the same implicit contract (on `Err` from the underlying `ipc_send`, `unblock_receiver_on` is not called and `needs_yield` stays false — state is not mutated), but no direct test asserts this. Routed to **T-011** (missing-tests bundle) alongside the `ReceiverTableFull` + slot-reuse coverage.
- **Idle `yield_now` failure mode under preemption.** `idle_entry` uses `.expect("idle: yield_now failed")`. In the v1 cooperative kernel `yield_now` can only return `NoCurrentTask`, which is impossible once the scheduler has started — the expect is safe today. When preemption lands (late Phase B or Phase C), the call's failure modes may widen; revisit and prefer `match + debug_assert` so idle degrades to spin instead of panicking. Routed to the **preemption design ADR** whenever it is written.

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-04-22 | @cemililik (+ Claude Opus 4.7 agent) | opened; status `In Progress`. ADR-0022 Accepted earlier today (commit `7fb74bb`). Implementation cleared to begin: `IpcError::PendingAfterResume` + `SchedError::Deadlock` variants → `ipc_recv_and_yield` rewire → BSP idle registration → two new host tests. Current.md pointed at T-007; T-006 moves off `Active task`, remains `In Review` pending maintainer promotion. |
| 2026-04-22 | @cemililik (+ Claude Opus 4.7 agent) | Implementation complete. Four commits landed: `029d066` (typed error variants + `ipc_recv_and_yield` rewire with state restore), `25cfaf4` (BSP idle task registration; WFI deferred to T-009 per ADR-0022 first rider after QEMU hang reproduced empirically), `8110cc5` (two new host tests + resume-path `debug_assert!` dropped as redundant with typed error; ADR-0022 gains a second rider documenting the decision). 77 kernel + 34 test-hal = 111 host tests green; QEMU smoke matches A6 byte-for-byte; fmt/clippy all clean. Status → `In Review`. T-006 mini-retro's "post-In-Review second-read" gate applies before promoting to `Done`. |
