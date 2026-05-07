# T-015 — Endpoint rollback / `ipc_cancel_recv` primitive

- **Phase:** B
- **Milestone:** B2 prep (lands before any B2 implementation task that touches userspace endpoint destroy; not gating B2's MMU work itself)
- **Status:** Draft
- **Created:** 2026-05-07
- **Author:** @cemililik (+ Claude Opus 4.7 agent)
- **Dependencies:** [ADR-0032](../../../decisions/0032-endpoint-rollback-and-cancel-recv.md) — must be `Accepted` before code lands.
- **Informs:** Unblocks the userspace-driven endpoint destroy path the B2+ MMU activation arc may surface; preconditions multi-waiter wake (ADR-0019 §Open questions) and preemption-rollback (B5+) by providing the cancel primitive both arcs need.
- **ADRs required:** [ADR-0032](../../../decisions/0032-endpoint-rollback-and-cancel-recv.md). No supersession; ADR-0017 (IPC primitive set) gains a §Revision notes rider rather than a full supersede.

---

## User story

As the kernel's IPC subsystem, I want a primitive that reverses an `Idle → RecvWaiting` endpoint transition for the calling task, so that `ipc_recv_and_yield`'s Deadlock path leaves both *scheduler* and *endpoint* state in their pre-call shape — restoring the Phase A "error path leaves observable state unchanged" invariant on both axes.

## Context

The 2026-05-06 comprehensive code review (Track A non-blocker) and the 2026-05-07 B1 closure security review (forward-flagged item) flagged the asymmetric rollback in `ipc_recv_and_yield`'s Deadlock path: Phase 1's `ipc_recv` transitions the endpoint from `Idle` to `RecvWaiting`; Phase 2's Deadlock path rolls back the scheduler state but does *not* reverse the endpoint transition. v1 hides the asymmetry behind ADR-0026's `register_idle` (Deadlock is structurally unreachable when idle is registered as the dispatcher's fallback), but the gap becomes a live bug under any of:

- Userspace-driven endpoint destroy (Phase B2+) needing a "drain receivers" sweep.
- Multi-waiter endpoints (ADR-0019 §Open questions).
- Preemption (B5+) where Phase 1 has run but Phase 2 has not.

[ADR-0032](../../../decisions/0032-endpoint-rollback-and-cancel-recv.md)'s *Decision outcome* settles on **Option A — `ipc_cancel_recv` primitive**: a small (~30 LOC kernel) addition that provides the cancel semantics every future caller will need, replacing the per-site duplication Option B would have created. T-015 is the implementation of that decision per the ADR's *Dependency chain*.

## Acceptance criteria

- [ ] **ADR-0032 Accepted** before code lands. Same-day Accept after careful re-read is permitted per [ADR-0025 §Revision notes](../../../decisions/0025-adr-governance-amendments.md); Propose commit is separate from the Accept commit.
- [ ] **`ipc_cancel_recv` primitive added** in [`kernel/src/ipc/mod.rs`](../../../../kernel/src/ipc/mod.rs). Signature: `pub fn ipc_cancel_recv(ep_arena: &mut EndpointArena, queues: &mut IpcQueues, ep_cap: CapHandle, caller_table: &CapabilityTable) -> Result<(), IpcError>`. Reverses an `Idle → RecvWaiting { waiter: caller }` transition; no-op if endpoint is in any other state (returns `Ok(())` for `Idle`, `RecvWaiting { waiter: other }` — though the latter is impossible in v1 single-waiter — and any `Send*` state, since those mean Phase 1 already advanced past `RecvWaiting`).
- [ ] **`ipc_recv_and_yield`'s Phase 2 Deadlock branch updated** in [`kernel/src/sched/mod.rs`](../../../../kernel/src/sched/mod.rs) to call `ipc_cancel_recv` before returning `Err(SchedError::Deadlock)`. The call uses the existing `*mut EndpointArena` / `*mut IpcQueues` raw-pointer discipline (no new `unsafe` site; UNSAFE-2026-0014 covers).
- [ ] **`SchedError::Deadlock` doc-comment updated** to record the symmetric rollback (the *Rollback scope* sub-section gains one sentence: "the endpoint state is also restored to `Idle` via `ipc_cancel_recv`; both halves of the rollback are atomic relative to the caller's observation").
- [ ] **`docs/architecture/ipc.md`** §"State machine" gains a one-line note about the cancel primitive and the symmetric rollback.
- [ ] **ADR-0017 §Revision notes rider** records the addition of `cancel_recv` as a recovery primitive (does not extend the user-observable IPC surface; v1 syscalls are not yet defined).
- **Tests.**
  - [ ] **New: `ipc_recv_and_yield_deadlock_rolls_back_endpoint_state`** in `kernel/src/sched/mod.rs::tests`. Constructs a scheduler with one regular task A (no idle registered to force Deadlock), endpoint at `Idle`, A as current. Calls `ipc_recv_and_yield(ep)`. Expected: `Err(SchedError::Deadlock)`; scheduler state restored to pre-call (existing assertion); endpoint state restored to `Idle` (new assertion). The test is the empirical form of the ADR's Simulation-table row 3b.
  - [ ] **Existing T-007 `ipc_recv_and_yield_returns_deadlock_when_ready_queue_empty` test updated** to also assert endpoint state restoration alongside scheduler state restoration. Same shape, additional assertion.
- [ ] **Verification gates.**
  - [ ] `cargo fmt --all -- --check` clean.
  - [ ] `cargo host-clippy` clean (`-D warnings`).
  - [ ] `cargo kernel-clippy` clean.
  - [ ] `cargo host-test` passes — 152 + ~1 new = 153 total (or whatever the final count comes out to).
  - [ ] `cargo +nightly miri test` passes on the same set.
  - [ ] `cargo kernel-build` clean.
  - [ ] **QEMU smoke unchanged.** T-015 does not add any v1-reachable code path; the smoke trace should match the post-T-014 baseline byte-for-byte.

## Out of scope

- **Userspace exposure of `cancel_recv`.** v1 has no syscalls; the primitive lands as kernel-internal. The B5+ syscall-ABI ADR (currently pencilled as ADR-0030 per phase-b.md ledger) decides whether to expose it.
- **Multi-waiter wake-up semantics** (ADR-0019 §Open questions). The cancel primitive's signature is single-waiter-shaped today; a future multi-waiter ADR may extend it to "remove caller from waiter list", but that extension is out of scope for T-015.
- **Preemption-rollback path** (B5+). T-015 implements the cancel primitive; the preemption-rollback path that calls it is B5+ work.
- **Userspace-driven endpoint destroy drain** (B2+). T-015 makes the cancel primitive available; the destroy-drain code path that calls it lands with B2's first userspace-destroy implementation task.
- **`IpcQueues::reset_if_stale_generation`'s `Some(cap)` payload silent-drop** (Phase A non-blocker, Track A inheritance). Different shape: that's a generation-mismatch destroy hazard, not a Deadlock-rollback hazard. Cleanup of that path comes with B2's userspace-destroy work.

## Approach

The implementation is roughly 30 LOC across two files plus tests.

1. **`kernel/src/ipc/mod.rs::ipc_cancel_recv`** — new function. Mirrors the shape of `ipc_recv`'s validate-then-mutate pattern: validate `ep_cap` via `Scheduler::resolve_ep_cap` semantics (look up endpoint handle; check the endpoint exists and has `RECV` rights — hmm actually the cancel doesn't need RECV rights since it's reversing a previous `recv` registration; it needs the same rights as the original `recv` did, so the caller's table must still hold the same handle). Then on the `IpcQueues` slot for the endpoint: if the state matches `RecvWaiting { waiter: caller }` (where caller is `s.current` — but we don't have access to scheduler from `kernel::ipc`; the bridge layer in `kernel::sched` passes the caller's TaskHandle as a parameter? — no, `ipc_recv` doesn't take TaskHandle either; the `RecvWaiting` state stores the caller via the `IpcQueues` mutation in Phase 1; let me re-check the actual `IpcQueues::set_recv_waiting` signature). Implementation detail to nail down during code-write: the cancel must only reverse a transition the *same* caller initiated; `ipc_recv` may not record the caller in `RecvWaiting` today (it's single-waiter so the slot itself implies "the only waiter"); v1 cancel can simply check `state == RecvWaiting` and reset to `Idle`. When multi-waiter lands, the cancel signature grows a `caller: TaskHandle` parameter.
2. **`kernel/src/sched/mod.rs::ipc_recv_and_yield`** Phase 2 Deadlock branch. Currently:
   ```rust
   let Some(next_handle) = s.ready.dequeue().or(s.idle) else {
       // Restore so Err(Deadlock) leaves the scheduler unchanged.
       s.task_states[current_idx] = prior_state;
       s.current = Some(current_handle);
       return Err(SchedError::Deadlock);
   };
   ```
   New shape — call `ipc_cancel_recv` on the endpoint before returning. Need to materialise momentary `&mut EndpointArena` + `&mut IpcQueues` inside the rollback block (the existing momentary-`&mut Scheduler<C>` block will need to grow to include the IPC arenas, OR the cancel call drops outside the scheduler block and re-acquires them — Phase 1 already had the pattern). The ADR-0021 raw-pointer discipline applies; UNSAFE-2026-0014's audit covers the new momentary `&mut`s.
3. **Doc-comment updates** for `SchedError::Deadlock` (one sentence in *Rollback scope*) and `docs/architecture/ipc.md` §State machine (one line on the cancel primitive).
4. **ADR-0017 §Revision notes rider** records the addition of `cancel_recv` as a recovery primitive.
5. **Tests** — one new + one updated, both in `kernel/src/sched/mod.rs::tests`.

## Definition of done

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo host-clippy` clean with `-D warnings`.
- [ ] `cargo kernel-clippy` clean.
- [ ] `cargo host-test` passes — expected ~153 (was 152 at B1 closure; +1 from new Deadlock-rollback test).
- [ ] `cargo +nightly miri test` passes on the same set.
- [ ] `cargo kernel-build` clean.
- [ ] **QEMU smoke** byte-for-byte identical to post-T-014 baseline (T-015 adds no v1-reachable code path).
- [ ] No new `unsafe` block without an audit entry; UNSAFE-2026-0014's existing umbrella covers the new `&mut EndpointArena` / `&mut IpcQueues` momentary materialisation in `ipc_recv_and_yield`'s extended rollback block.
- [ ] Commit messages follow [`commit-style.md`](../../../standards/commit-style.md) with `Refs: ADR-0032` trailers.
- [ ] Task status updated to `In Review` then `Done`; phase-b.md ADR ledger gains the ADR-0032 row (added with the ADR's Propose commit) + sub-breakdown bullet flagging this work as B2-prep follow-on; [`docs/roadmap/current.md`](../../../roadmap/current.md) updated.

## Design notes

- **Why an explicit primitive rather than inline?** Per ADR-0032 §Decision outcome, the inline rollback (Option B) would be smaller today but accumulates duplication at every future caller. Discrete primitive scales to multi-waiter wake (ADR-0019 §Open questions) and preemption-rollback (B5+) without re-deriving the cancel logic.
- **Why `Result<(), IpcError>` rather than `()`?** Future-proofing: if multi-waiter lands, the cancel may surface a "caller was not the registered waiter" error. v1's single-waiter shape returns `Ok(())` on no-op for any state other than `RecvWaiting`; the typed return matches the rest of the IPC surface.
- **Why no syscall ABI exposure?** v1 has no syscalls. ADR-0030 (B5 syscall ABI) decides whether to expose `cancel_recv` to userspace. The kernel-internal placement matches v1's "kernel surface only" posture.
- **Why land before B2 rather than alongside B2?** B2's first ADR (ADR-0027 kernel virtual memory layout) does not directly need `cancel_recv`. But B2's *first userspace-destroy implementation task* (whichever of B2-B6 lands it) will. Landing T-015 alongside ADR-0032 keeps the scheduler / IPC subsystem internally consistent for the entire B2-B6 arc; it does not block ADR-0027.

## References

- [ADR-0032 — Endpoint state rollback on `ipc_recv_and_yield` Deadlock + `ipc_cancel_recv` primitive](../../../decisions/0032-endpoint-rollback-and-cancel-recv.md) — the policy this task implements.
- [ADR-0017 — IPC primitive set](../../../decisions/0017-ipc-primitive-set.md) — the existing surface gaining a §Revision notes rider.
- [ADR-0021 — Raw-pointer scheduler IPC-bridge API](../../../decisions/0021-raw-pointer-scheduler-ipc-bridge.md) — the `*mut` discipline `ipc_cancel_recv` follows.
- [ADR-0022 — Idle task and typed scheduler deadlock error](../../../decisions/0022-idle-task-and-typed-scheduler-deadlock.md) — the original Deadlock typing decision; its scheduler-rollback shape gains the symmetric endpoint-rollback this task adds.
- [ADR-0026 — Idle dispatch via separate fallback slot](../../../decisions/0026-idle-dispatch-fallback.md) — the `register_idle` discipline that makes `SchedError::Deadlock` structurally unreachable in v1; explains why this gap is benign at HEAD.
- [Comprehensive code review (2026-05-06)](../../reviews/code-reviews/2026-05-06-full-tree-comprehensive.md) — Track A non-blocker that surfaced the asymmetry.
- [B1 closure security review (2026-05-07)](../../reviews/security-reviews/2026-05-07-B1-closure.md) — forward-flagged item.
- [`kernel/src/sched/mod.rs::ipc_recv_and_yield`](../../../../kernel/src/sched/mod.rs) — the call site to update.
- [`kernel/src/ipc/mod.rs`](../../../../kernel/src/ipc/mod.rs) — where `ipc_cancel_recv` lands.
- [`docs/architecture/ipc.md`](../../../architecture/ipc.md) — gains §State machine line on the cancel primitive.

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-05-07 | @cemililik (+ Claude Opus 4.7 agent) | Opened with status `Draft`, paired with ADR-0032 (`Proposed`) per [ADR-0025 §Rule 1](../../../decisions/0025-adr-governance-amendments.md) (forward-reference contract) — ADR-0032's *Dependency chain* requires a real T-NNN file for the implementation step; this task is that file. Will move to `In Progress` only after ADR-0032 is `Accepted`. |
