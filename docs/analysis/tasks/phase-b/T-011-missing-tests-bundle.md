# T-011 — Missing tests bundle

- **Phase:** B
- **Milestone:** B0 — Phase A exit hygiene
- **Status:** Draft
- **Created:** 2026-04-23
- **Author:** @cemililik (+ Claude Opus 4.7 agent)
- **Dependencies:** [T-006](T-006-raw-pointer-scheduler-api.md), [T-007](T-007-idle-task-typed-deadlock.md) (both `In Review`). Both settle the code shape T-011 writes tests against; T-011 should not land until both have been promoted to `Done`.
- **Informs:** Closes the remaining Phase-A-exit code-review items (*Test coverage* section); unblocks B0's exit criterion "109+ host tests green".
- **ADRs required:** none — this task only adds tests against shapes already settled by ADR-0017, ADR-0019, ADR-0021, ADR-0022.

---

## User story

As the Tyrne kernel, I want the specific test gaps the 2026-04-21 code review and the 2026-04-23 coverage baseline identified to be closed by host tests — so that each invariant documented in an ADR is either directly exercised or explicitly marked defensively-dead, and so that any future regression breaks a test rather than slipping past review.

## Context

Four distinct sources flagged missing tests:

1. **Phase A code-review §Test coverage** — `IpcError::ReceiverTableFull` is reachable but not asserted; slot-reuse with a pending transfer `Capability` is not exercised.
2. **ADR-0022 §Consequences/Negative** — `SchedError::Deadlock` is a defensive return variant; without a direct unit test it drifts into dead-variant rot. T-007 landed the test for this one case; the ADR's concern is more general.
3. **T-007 deferred follow-ups** — `ipc_send_and_yield` has no symmetric state-restore test equivalent to T-007's `ipc_recv_and_yield` Deadlock test.
4. **R2 coverage baseline** ([2026-04-23-coverage-baseline.md](../../reviews/business-reviews/../reports/2026-04-23-coverage-baseline.md)) — `sched::start` body (lines 426-473) entirely uncovered; `ipc_send_and_yield` entire body (lines 589-645) entirely uncovered; 40 error-return branches in `cap/table.rs` uncovered.

T-011 bundles these items into a single task so the test-writing discipline lands once, with one commit arc, rather than trickled in across later milestones where it would compete with feature work.

## Acceptance criteria

- [ ] **`IpcError::ReceiverTableFull` test** — set up a `CapabilityTable` filled to capacity, invoke `ipc_recv` against a pending send that carries a `Capability`, assert `Err(ReceiverTableFull)` **and** that the capability remains in the endpoint's `RecvComplete` / `SendPending` state (not silently dropped). Code review §Test coverage bullet 1.
- [ ] **Slot-reuse with pending transfer cap test** — two tests, paired:
  1. A `#[cfg(debug_assertions)]` + `#[should_panic(expected = "endpoint slot must be drained")]` test that queues a `SendPending` with `cap: Some(_)` on an endpoint, destroys the endpoint (bumping its generation), allocates a new endpoint at the same slot, and triggers `reset_if_stale_generation`. The test confirms the `debug_assert!` (added by `7eaa10a` at [`kernel/src/ipc/mod.rs`](../../../../kernel/src/ipc/mod.rs) line ~215) fires as designed when an in-flight `Capability` would otherwise be silently dropped.
  2. A variant **without** `cap: Some(_)` (i.e. `SendPending { cap: None }` or `RecvWaiting`) that is not gated by `#[should_panic]`: the `debug_assert!` must *not* fire because nothing would be leaked. This confirms the assert's predicate is not over-broad.

  Both tests must exercise the actual `reset_if_stale_generation` function by name, not a local copy. The paired form (should-panic + must-not-panic) protects against the assert rotting into a blanket panic or into a no-op. Code review §Test coverage bullet 2.
- [ ] **`ipc_send_and_yield` three-case bundle**:
  - `Ok(SendOutcome::Delivered)` with a registered receiver → sender's unblock-and-yield path runs; receiver ends up in `Ready`; scheduler state is consistent post-call.
  - `Ok(SendOutcome::Enqueued)` with no receiver → no yield; scheduler state unchanged.
  - `Err(SchedError::Ipc(_))` propagated from an `ipc_send` failure (e.g. invalid transfer cap) → scheduler state unchanged pre- and post-call (symmetric to T-007's `ipc_recv_and_yield_returns_deadlock_when_ready_queue_empty` state-restore assertion). T-007 deferred follow-up §Approach bullet 5.
- [ ] **`start()` prelude refactor + test** — extract a `start_prelude(sched: *mut Scheduler<C>) -> usize` helper that performs the dequeue + state-mutation; `start` becomes `start_prelude` + `IrqGuard` + `context_switch` (semantically unchanged). Add a direct test for `start_prelude` that asserts the expected `next_idx`, `task_states[next_idx] == Ready`, and `s.current == Some(next_handle)`. Addresses R2 baseline's highest-value sched gap.
- [ ] **`cap/table.rs` targeted sweep** — five or fewer host tests covering the most-reached of the 40 uncovered error branches (likely `cap_derive` with exhausted table, `cap_take` on stale handle, `cap_drop` on root-only path, etc.). Accept that 100 % coverage is not the goal.
- [ ] **Tests stay green.** Expected new count: 77 + 5–7 = 82–84 kernel tests; total 116–118 host tests.
- [ ] **R2 coverage re-run** — `cargo llvm-cov --workspace --exclude tyrne-bsp-qemu-virt --summary-only` pushes `sched/mod.rs` past 90 % regions and the workspace past 96 %. Updated baseline report committed under `docs/analysis/reports/`.
- [ ] **Miri stays clean** — `cargo +nightly miri test -p tyrne-kernel` still passes after the new tests (the R3 discipline applies to new test helpers too).

## Out of scope

- Branch coverage for every `cap/table.rs` error return — diminishing returns; five targeted tests is the cap.
- BSP coverage — T-011 does not touch `bsp-qemu-virt`; BSP-side measurement is a follow-up for [T-009](T-009-timer-init-cntvct.md) (Timer init / perf measurement) and the IRQ-wiring task [T-012](T-012-exception-and-irq-infrastructure.md) (exception infrastructure, B1 Draft).
- `ResetQueuesCpu` or `FakeCpu` API changes — existing test-harness shapes are sufficient.
- Performance tests — measurements are the scope of [T-009](T-009-timer-init-cntvct.md) (Timer init / perf measurement, `In Review` since 2026-04-23).
- New ADRs — the code shapes already exist; T-011 only writes tests.
- Test-hal crate coverage improvements — 95 %+ already; the remaining gaps are no-op stubs.

## Approach

In commit order:

1. **`ReceiverTableFull` test** (`kernel/src/ipc/mod.rs` tests module) — smallest diff, isolated to IPC.
2. **Slot-reuse-with-cap test** (`kernel/src/ipc/mod.rs`) — builds on #1's setup.
3. **`start_prelude` refactor + test** (`kernel/src/sched/mod.rs`) — small refactor, one test.
4. **`ipc_send_and_yield` three-case bundle** (`kernel/src/sched/mod.rs`) — largest single commit; closes the single biggest coverage gap.
5. **`cap/table.rs` targeted sweep** (`kernel/src/cap/table.rs` tests) — last, because the specific branches to target are easier to choose once a fresh llvm-cov run narrows them.
6. **Coverage re-run + updated report** — file a new `docs/analysis/reports/YYYY-MM-DD-coverage-rerun.md` citing deltas.
7. **Miri re-run** — confirm no new Stacked Borrows violations.

## Definition of done

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo host-clippy` clean with `-D warnings`.
- [ ] `cargo kernel-clippy` clean.
- [ ] `cargo host-test` passes with at least 116 host tests.
- [ ] `cargo +nightly miri test --workspace --exclude tyrne-bsp-qemu-virt` passes.
- [ ] `cargo llvm-cov` re-run: `sched/mod.rs` regions ≥ 90 %, workspace regions ≥ 96 %.
- [ ] `cargo kernel-build` clean; QEMU smoke reproduces the A6 five-line trace.
- [ ] Commit messages follow [`commit-style.md`](../../../standards/commit-style.md); each commit is focused on one criterion; `Refs:` trailers cite the ADR(s) that motivated each test.
- [ ] Task status updated to `In Review`; [`docs/roadmap/current.md`](../../../roadmap/current.md) updated.

## Design notes

- **Why `start_prelude` rather than a divergent-function test harness?** The alternative — running `start` with a `FakeCpu` whose `context_switch` longjmps back — would need OS-specific scaffolding and brings signal-handling complexity into the test harness for essentially one piece of coverage. A prelude helper is three lines of refactor that makes the uncovered logic directly testable, and — if the idle-task design ever grows (e.g. priorities) — the prelude becomes the natural place for that logic to live. The trade-off is a one-step indentation change in `start()`.
- **Why cap `cap/table.rs` sweep at five tests?** R2 found 40 uncovered regions. Most are error-return branches; five focused tests cover the `cap_derive(table_full)`, `cap_take(stale)`, `cap_take(has_children)`, `cap_drop(root_with_children)`, and `insert_root(full)` paths — the remaining 20-ish are duplications or trivially symmetric. Diminishing returns beyond five is sharp.
- **Why not convert `start()`'s empty-queue panic to a typed error (take the T-010 bait)?** ADR-0022 §Decision outcome keeps the panic deliberately; that decision stands. `start_prelude`'s test covers the non-panic path; the panic remains a boot-time programming error.
- **Projected commit sequence.**
  1. `docs(roadmap): open T-011 — missing tests bundle` (this opening commit).
  2. `test(ipc): ReceiverTableFull asserts cap retention (T-011)`.
  3. `test(ipc): slot-reuse with pending transfer cap (T-011)`.
  4. `refactor(sched): extract start_prelude for testability (T-011)` + `test(sched): start_prelude dequeue + state (T-011)`.
  5. `test(sched): ipc_send_and_yield Delivered/Enqueued/Err (T-011)`.
  6. `test(cap): targeted error-branch sweep on CapabilityTable (T-011)`.
  7. `docs(analysis): coverage re-run — sched ≥ 90 %, workspace ≥ 96 %`.
  8. `docs(roadmap): T-011 → In Review`.

## References

- [Code review — Tyrne → Phase A exit](../../reviews/code-reviews/2026-04-21-tyrne-to-phase-a.md) — §Test coverage.
- [ADR-0022 — Idle task and typed scheduler deadlock error](../../../decisions/0022-idle-task-and-typed-scheduler-deadlock.md) — §Consequences/Negative (dead-variant concern).
- [T-007 — Idle task + typed SchedError::Deadlock](T-007-idle-task-typed-deadlock.md) — §Deferred follow-ups.
- [Coverage baseline 2026-04-23](../../reports/2026-04-23-coverage-baseline.md) — R2.
- [Miri validation 2026-04-23](../../reports/2026-04-23-miri-validation.md) — R3.
- [ADR-0017 — IPC primitive set](../../../decisions/0017-ipc-primitive-set.md) — defines the IPC state machine the ReceiverTableFull test exercises.
- [ADR-0019 — Scheduler shape](../../../decisions/0019-scheduler-shape.md) — defines the ready-queue / state invariants tested.

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-04-23 | @cemililik (+ Claude Opus 4.7 agent) | Opened with status `Draft`. Scope bundles (a) Phase A code-review §Test coverage items, (b) T-007 deferred `ipc_send_and_yield` state-restore test, (c) R2 coverage-baseline gap routing (start-prelude + cap/table sweep), and (d) an explicit re-run of R2 llvm-cov + R3 miri after the new tests land. Will move to `In Progress` once T-006 and T-007 are promoted to `Done`. |
