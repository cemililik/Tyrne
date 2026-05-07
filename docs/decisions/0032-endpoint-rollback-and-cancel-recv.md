# 0032 — Endpoint state rollback on `ipc_recv_and_yield` Deadlock + `ipc_cancel_recv` primitive

- **Status:** Accepted
- **Date:** 2026-05-07
- **Deciders:** @cemililik

## Context

The 2026-05-06 comprehensive code review (Track A non-blocker) and the 2026-05-07 B1 closure security review (forward-flagged item) both surfaced an **asymmetric rollback** in `ipc_recv_and_yield`'s Deadlock path:

- Phase 1 (`ipc_recv` non-blocking try) atomically transitions the endpoint state from `Idle` to `RecvWaiting` (recording the calling task as the sole waiter).
- Phase 2 (block + dequeue + switch) checks whether any other task is Ready; if not (`s.ready.dequeue().or(s.idle)` returns `None`), it returns `Err(SchedError::Deadlock)`. The Deadlock path **rolls back the scheduler state** (`s.current` → caller; `s.task_states[caller]` → Ready) but does **not** reverse Phase 1's endpoint transition. The endpoint stays in `RecvWaiting`.
- Consequence: a subsequent `ipc_recv_and_yield` from the same caller on the same endpoint observes `IpcError::QueueFull` (the endpoint already has a registered receiver — itself — but Phase 2 cannot tell the registered receiver is the same caller).

In v1 this is benign: with `register_idle` having installed the BSP idle task per [ADR-0026](0026-idle-dispatch-fallback.md), Phase 2's dispatch fallback always finds *some* task to switch to (idle, if no real Ready task), so the Deadlock path is structurally unreachable. The endpoint state asymmetry is a latent gap, not a live bug.

The latent gap becomes a **live bug** under any of:

- **Phase B2's first userspace-driven endpoint destroy.** If userspace destroys an endpoint that holds a stuck `RecvWaiting { caller }` slot, the slot's destruction silently drops the registration; the caller (still alive) holds a stale handle to a now-non-existent registration. Subsequent `ipc_recv` on a re-created endpoint at the same slot index could be confused by `IpcQueues::reset_if_stale_generation` if the generation increment is missed.
- **Multi-waiter endpoints (post-ADR-0019 §Open questions).** If endpoints support multiple waiters in a future scheduler shape, the asymmetry compounds: a Deadlock-then-recover path could leave multiple waiters registered who never actually entered Phase 2's blocking step.
- **Preemption (B5+).** A preempted Phase-2-bound task whose Phase 1 has already transitioned the endpoint, but whose Phase 2 has not yet executed, leaves an endpoint in an "ambiguous registration" state across the preemption boundary. Same shape as the Deadlock case, with similar consequences.

The fix is a single primitive that v1 can supply but does not need to call: `ipc_cancel_recv(ep_handle)` reverses an `Idle → RecvWaiting` transition for the calling task, returning the endpoint to `Idle` (or, when multi-waiter lands, removing the calling task from the waiter list). The Deadlock path then becomes a simple `ipc_cancel_recv → SchedError::Deadlock` sequence; preemption-side rollback uses the same primitive; userspace endpoint destroy can call it as part of a "drain receivers" sweep.

## Decision drivers

- **Symmetry.** Phase 1's `ipc_recv` transitions the endpoint; the Deadlock-rollback path should reverse the *same* transition. The Phase A invariant ("error path leaves observable state unchanged") was the implicit contract; the current implementation honours it for the *scheduler* state but breaks it for the *endpoint* state.
- **Forward compatibility with multi-waiter and preemption.** Both planned futures (ADR-0019 §Open questions; B5+ preemption ADR) need the same primitive. Adding it now means later ADRs build on it rather than re-derive it.
- **Cost.** The primitive is small (~30 LOC kernel + a host test). The implementation task (T-015) is bounded.
- **No userspace caller in v1.** No syscall surface exists yet, so `ipc_cancel_recv` lives as a kernel-internal helper that the Deadlock path calls. When syscalls land in B5, the syscall-ABI ADR can decide whether to expose the primitive to userspace directly.
- **Zero new `unsafe`.** The primitive operates on the same `EndpointArena` + `IpcQueues` raw-pointer surface the existing IPC bridge already uses; no new audit-log entry needed.

## Considered options

1. **Option A — Add `ipc_cancel_recv` primitive; Deadlock path calls it.** Reverses the Phase 1 endpoint transition before returning `Err(SchedError::Deadlock)`. Symmetric, future-proof, bounded in scope.
2. **Option B — Inline the rollback inside `ipc_recv_and_yield`'s Deadlock branch.** No new primitive; the Deadlock branch directly mutates the endpoint's `IpcQueues` slot back to `Idle`. Smaller diff but the cancel logic is duplicated when preemption / multi-waiter need it; refactoring later is more disruptive than authoring the primitive now.
3. **Option C — Defer cleanup to endpoint destroy.** Leave the asymmetry; rely on a future "drain endpoints on destroy" sweep to clear stuck `RecvWaiting` slots. Smallest diff today; pushes the entire cost forward to whoever lands the userspace-destroy code path.
4. **Option D — Document the asymmetry; do nothing.** Treat the gap as an explicit v1 limitation. Smallest diff; smallest forward burden; but the gap becomes a load-bearing invariant that B-phase userspace work must navigate around.

## Decision outcome

Chosen option: **Option A — add `ipc_cancel_recv` primitive; Deadlock path calls it.**

The primitive is small enough that its scope cost is in the same order of magnitude as Option B's inline rollback, but the *future surface* is dramatically better: multi-waiter wake (ADR-0019 §Open questions) needs `ipc_cancel_recv` for fair-wake semantics; preemption (B5+) needs it for the preempted-mid-Phase-2 case; userspace endpoint destroy (B2+) needs it for drain-before-free. Option B duplicates the logic at every site; Option A places the discipline once.

Option C (defer to destroy) is rejected because it conflates two orthogonal cleanup events (Deadlock-recovery vs endpoint-destroy) and implicitly moves Phase 1's atomicity contract from "rollback on error" to "best-effort-with-cleanup-eventually". The current code's *scheduler* rollback already follows the rollback-on-error pattern; matching the *endpoint* side is the right symmetry.

Option D (document only) is rejected because the asymmetry is benign in v1 only by accident — `ipc_recv_and_yield`'s Deadlock path is structurally unreachable with idle registered. As soon as multi-waiter or preemption lands, the path becomes reachable and the asymmetry becomes a live bug. Documenting a hazard as a v1 limitation is acceptable when v1 proves the limitation is bounded; this hazard is not bounded — it grows with each future ADR that touches the IPC state machine.

### Simulation

Phase 2 Deadlock path under v1 + Option A:

| Step | State pre | Action | State post | Switch target / observable effect |
|------|-----------|--------|------------|-----------------------------------|
| 0 | endpoint `Idle`; caller `Ready` | caller calls `ipc_recv_and_yield(ep)` | unchanged | — |
| 1 | endpoint `Idle`; caller `Ready`, current | Phase 1: `ipc_recv` returns `Pending` (no sender); transition `Idle → RecvWaiting { waiter: caller }` | endpoint `RecvWaiting { waiter: caller }`; caller still `Ready`, current | — |
| 2 | endpoint `RecvWaiting`; caller `Ready`, current | Phase 2: `s.task_states[caller] = Blocked { on: ep }`; `s.current = None`; try `s.ready.dequeue().or(s.idle)` | endpoint `RecvWaiting`; caller `Blocked { on: ep }`; `s.current = None` | — |
| 3a | `dequeue.or(s.idle) = Some(idle)` (v1 path) | switch to idle | endpoint `RecvWaiting`; caller `Blocked`; `s.current = idle` | switch to idle (existing v1 behaviour; not the Deadlock path) |
| 3b | `dequeue.or(s.idle) = None` (no idle; Deadlock path) | scheduler rollback: `s.current = caller`; `s.task_states[caller] = Ready`; **NEW: call `ipc_cancel_recv(ep)`** → endpoint `RecvWaiting → Idle` | endpoint `Idle`; caller `Ready`, current | return `Err(SchedError::Deadlock)`; subsequent `ipc_recv` on same endpoint sees clean `Idle` state (correct symmetry; Phase A "error path leaves observable state unchanged" invariant holds for both scheduler *and* endpoint) |

Under Option B (inline rollback) row 3b's "NEW" cell would inline the `IpcQueues` slot reset rather than call a primitive. Same observable shape; differs only in code organisation.

Under Option C / D row 3b's "NEW" cell is empty; endpoint stays `RecvWaiting`. A subsequent `ipc_recv_and_yield(ep)` from the same caller would observe `IpcError::QueueFull` because `RecvWaiting` already names the caller as the registered receiver but Phase 2 cannot recognise the registration as its own.

### Dependency chain

For this decision to be fully in effect:

```text
1. Add `ipc_cancel_recv(ep_arena, queues, ep_cap, table) -> Result<(), IpcError>`
   primitive in `kernel/src/ipc/mod.rs`. Reverses an `Idle → RecvWaiting`
   transition for the calling task; no-op if endpoint is not in
   `RecvWaiting { waiter: caller }`. — T-015 (Draft, opens with this ADR)
2. Modify `kernel/src/sched/mod.rs::ipc_recv_and_yield`'s Phase 2 Deadlock
   branch to call `ipc_cancel_recv` before returning `Err(SchedError::Deadlock)`. — T-015
3. Add a host test that triggers Deadlock and asserts both scheduler AND
   endpoint state are restored to pre-call shape. — T-015
4. Update [`SchedError::Deadlock`'s doc-comment](../../kernel/src/sched/mod.rs)
   to record the symmetric rollback. — T-015
5. Update [`docs/architecture/ipc.md`](../architecture/ipc.md) §"State machine"
   with the rollback symmetry. — T-015
```

T-015 opens as `Draft` in the same commit as this ADR per [ADR-0025 §Rule 1](0025-adr-governance-amendments.md). All five steps fall inside T-015's scope; no separate task needed.

T-015's `Done` flip gates only on its own DoD (host test + miri + clippy + smoke) — it does not require a closure trio because it is a discrete change inside an already-closed milestone.

## Consequences

### Positive

- **Symmetric rollback.** The Phase A invariant ("error path leaves observable state unchanged") now holds for both scheduler *and* endpoint state.
- **Future-proof for multi-waiter (ADR-0019 §Open questions) and preemption (B5+).** Both arcs reuse the same primitive instead of duplicating the rollback logic.
- **Forward-cleanup primitive for endpoint destroy.** Userspace-driven endpoint destroy (B2+) can call `ipc_cancel_recv` as part of a "drain receivers" sweep, eliminating the silent-drop hazard `IpcQueues::reset_if_stale_generation`'s `debug_assert!` currently catches in tests but tolerates in release.
- **Zero new `unsafe`.** The primitive shares the same raw-pointer arena / queues surface; UNSAFE-2026-0014's discipline applies as-is.
- **Tested via a single host test.** The Deadlock path becomes structurally test-equivalent to the post-rollback "fresh `ipc_recv` on same endpoint" expectation; one test covers both halves.

### Negative

- **One additional primitive in the IPC surface.** `ipc_cancel_recv` joins `ipc_send` / `ipc_recv` / `ipc_notify` as the fourth IPC entry point. Marginal cognitive load for kernel-IPC readers; documented in the same module. *Mitigation:* the primitive's name + signature are self-explanatory; the "no-op if not in RecvWaiting" semantics keep its contract simple.
- **`SchedError::Deadlock`'s doc-comment becomes slightly more precise** (the Rollback-scope sub-section gains one sentence on endpoint state). Trivial maintenance cost.
- **Adding the primitive without an immediate userspace consumer** means the cancel path is exercised only by the Deadlock host test. *Mitigation:* the path is structurally unreachable in v1 (idle registered); the test is the only meaningful exerciser today, and that is enough to prevent regressions.

### Neutral

- **No new audit-log entry.** The primitive operates on the existing `EndpointArena` + `IpcQueues` raw-pointer interfaces under UNSAFE-2026-0014's umbrella.
- **No public-API change.** v1 has no userspace surface, so `ipc_cancel_recv` is kernel-internal. The B5 syscall-ABI ADR can later decide whether to expose it.
- **No change to ADR-0017's IPC primitive set.** ADR-0017 enumerated `send` / `recv` / `notify` as the v1 set; `cancel_recv` is a *recovery primitive* that does not extend the user-observable surface, so ADR-0017 does not need supersession. A §Revision notes rider on ADR-0017 records the addition.

## Pros and cons of the options

### Option A — `ipc_cancel_recv` primitive (chosen)

- Pro: structural symmetry; Phase A invariant restored.
- Pro: future-proof for multi-waiter and preemption.
- Pro: drain primitive available for userspace endpoint destroy.
- Pro: bounded scope (~30 LOC kernel + 1 host test + 1 doc-comment update).
- Con: one new IPC entry point; small cognitive load.
- Con: kernel-internal-only in v1; no userspace observer.

### Option B — Inline Deadlock rollback (no new primitive)

- Pro: smallest diff today.
- Pro: no new IPC entry point.
- Con: rollback logic gets duplicated at every future caller (preemption rollback path; multi-waiter wake path; userspace destroy drain).
- Con: each duplication site invites local-shape drift; the symmetry property becomes harder to enforce as the IPC state machine grows.

### Option C — Defer cleanup to endpoint destroy

- Pro: smallest diff today.
- Pro: no new primitive.
- Con: conflates Deadlock-recovery with endpoint-destroy as cleanup events.
- Con: implicitly weakens Phase 1's atomicity contract from "rollback on error" to "best-effort with eventual cleanup".
- Con: pushes the cost forward to whoever lands userspace endpoint destroy (B2+); that PR's reviewer must reason about every Deadlock-stranded `RecvWaiting` slot the kernel may have accumulated.

### Option D — Document only

- Pro: zero diff.
- Pro: zero forward burden today.
- Con: documents a hazard whose v1-benign property depends on accidental structural unreachability of `SchedError::Deadlock`.
- Con: B-phase ADRs that touch the IPC state machine (multi-waiter, preemption, syscall-exposed recv) all surface the asymmetry independently and re-discover the need for a fix.

## References

- [ADR-0017 — IPC primitive set](0017-ipc-primitive-set.md) — the existing send / recv / notify surface; gains a §Revision notes rider when this ADR Accepts.
- [ADR-0019 — Scheduler shape](0019-scheduler-shape.md) — `SchedError::Deadlock` is documented here; multi-waiter wake remains an open question that this ADR's primitive supports.
- [ADR-0021 — Raw-pointer scheduler IPC-bridge API](0021-raw-pointer-scheduler-ipc-bridge.md) — `ipc_cancel_recv` follows the same raw-pointer discipline.
- [ADR-0022 — Idle task and typed scheduler deadlock error](0022-idle-task-and-typed-scheduler-deadlock.md) — the original Deadlock typing decision; this ADR adds the symmetric endpoint rollback to the scheduler rollback ADR-0022 already records.
- [ADR-0026 — Idle dispatch via separate fallback slot](0026-idle-dispatch-fallback.md) — the `register_idle` discipline that makes `SchedError::Deadlock` structurally unreachable in v1, which is what makes the asymmetry benign at HEAD.
- [Comprehensive code review (2026-05-06)](../analysis/reviews/code-reviews/2026-05-06-full-tree-comprehensive.md) — Track A non-blocker that surfaced the asymmetry.
- [B1 closure security review (2026-05-07)](../analysis/reviews/security-reviews/2026-05-07-B1-closure.md) — forward-flagged item recording the same gap from the security axis.
- [`kernel/src/sched/mod.rs::ipc_recv_and_yield`](../../kernel/src/sched/mod.rs) — the call site T-015 modifies.
- [`kernel/src/ipc/mod.rs`](../../kernel/src/ipc/mod.rs) — where `ipc_cancel_recv` lands.
