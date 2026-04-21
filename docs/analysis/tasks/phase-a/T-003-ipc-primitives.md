# T-003 — IPC primitives

- **Phase:** A
- **Milestone:** A4 — IPC primitives
- **Status:** In Review
- **Created:** 2026-04-21
- **Author:** @cemililik
- **Dependencies:** T-002 — Kernel object storage foundation (Done)
- **Informs:** T-004 (cooperative scheduler / context switch — A5); T-005 (two-task IPC demo — A6)
- **ADRs required:** ADR-0017 (IPC primitive set) — must be Accepted before code lands; ADR-0018 (Badge scheme) — Accepted or explicitly deferred before A4 closes.

---

## User story

As the Umbrix kernel, I want synchronous rendezvous `send` / `recv` operations on `Endpoint` objects and a non-blocking `notify` path on `Notification` objects — both gated by capabilities — so that two kernel-level task stubs can exchange a capability-gated message and both resume, proving the IPC contract that the A6 two-task demo will exercise end-to-end.

## Context

[T-002](T-002-kernel-object-storage.md) delivered the kernel-object storage layer: `Endpoint` and `Notification` live in bounded arenas, capabilities name them through typed handles, and the create / destroy lifecycle is explicit. What the A3 objects lack is *behaviour*: an `Endpoint` has waiter-queue placeholder fields but no `send` or `recv` logic; a `Notification` can set and consume bits but has no `notify` or `wait` path.

T-003 wires the behaviour. The design decisions — pure rendezvous vs. rendezvous + reply-recv fastpath, blocking semantics, message format, whether badges are needed in v1 — are delegated to [ADR-0017](../../../decisions/0017-ipc-primitive-set.md) (and conditionally [ADR-0018](../../../decisions/0018-badge-scheme.md)). ADR-0017 must be Accepted before any implementation code lands.

**Scope constraint.** Phase A still has no real scheduler: tasks are kernel-level stubs without context switching. A4 must therefore implement IPC *without* a live scheduler: blocking is represented as a wait-queue that the scheduler (A5) will drain, but in A4 host tests the "block" state is exercised by constructing two task stubs that hand-deliver to each other. The scheduler integration happens in A5, not here.

This mirrors how A3 delivered `Endpoint` and `Notification` objects without IPC logic — A4 delivers the logic without scheduler integration. Each layer builds on the previous and defers the next.

## Acceptance criteria

- [x] **ADR-0017 Accepted** before implementation code lands. Settles: synchronous rendezvous vs. reply-recv fastpath, blocking semantics for sender and receiver, message format (register-sized fixed fields for v1), capability-transfer atomicity.
- [x] **ADR-0018 Accepted or explicitly deferred.** ADR-0017 explicitly defers the badge scheme; ADR-0018 to be written or confirmed-deferred before A4 closes.
- [x] **`send` operation.** Validates that the sender holds a send-right capability on the target `Endpoint`. If a receiver is waiting, delivers the message (and any transferred caps) and resumes both parties. If no receiver waits, enqueues the sender on the endpoint's blocked-sender list.
- [x] **`recv` operation.** Symmetric: validates a recv-right capability. Dequeues a waiting sender and delivers, or enqueues the receiver on the endpoint's blocked-receiver list.
- [x] **`notify` operation.** ORs a caller-supplied bitmask into a `Notification`'s saturating word. Wakes any waiter (waiter-wake integration with A5 is deferred; in A4 the bit is set and a future scheduler step drains waiters).
- [x] **Capability transfer with message.** Moving a capability from sender to receiver is atomic with message delivery: either the message is delivered with the capability, or neither is transferred (no partial state).
- [x] **Host tests** covering:
  - Round-trip: sender-first and receiver-first orderings both deliver correctly.
  - No-receiver: `send` with no waiting receiver enqueues the sender.
  - Capability transfer: moved cap is gone from sender, present in receiver — `cap_copy` is not acceptable as the mechanism (it must be a move).
  - `notify` delivery and bit saturation (OR semantics, not overwrite).
  - Blocked-sender wake: a subsequent `recv` drains the queue and delivers.
- [x] **No new `unsafe`** beyond what A3 introduced. If any lands, audit entry per [`unsafe-policy.md`](../../../standards/unsafe-policy.md).

## Out of scope

- `reply_recv` fastpath — ADR-0017 decides; if kept in v1 it is an acceptance criterion, if deferred it is a named open question in the ADR.
- Badge / discriminator semantics (ADR-0018; may be deferred).
- Scheduler integration — blocking state is represented, but the wake-up path that runs the unblocked task waits for A5.
- Real per-task context (register save / restore) — that is A5.
- Capability transfer through IPC across tasks with different `CapabilityTable` instances — A4 operates on kernel-level stubs sharing the test harness; cross-table transfer is the A6 demo scenario.
- `MemoryRegion` capabilities in messages (Phase B).

## Approach

Design is pinned in ADR-0017. At a sketch level:

1. **Message format.** Fixed-size, register-sized (e.g., four `u64` words) for v1. No variable-length buffers — those land with userspace in Phase B. Format is decided by ADR-0017.
2. **Endpoint waiter queues.** The `Endpoint` object (already in `kernel::obj::endpoint`) gains a blocked-sender queue and a blocked-receiver queue, each bounded (size decided by ADR-0017; a small fixed array mirrors the arena pattern). A5's scheduler drains them when a partner arrives.
3. **Capability transfer.** The sender names a set of `CapHandle`s to transfer. The kernel validates each, atomically moves them from the sender's `CapabilityTable` to the receiver's via `cap_drop` + `insert_root` (or an equivalent atomic swap). Partial failure is prevented by pre-validating all handles before modifying any table.
4. **`notify`.** `Notification::set(bits)` already exists. The `notify` kernel path validates the send-right cap, calls `set`, and records any pending waiter for A5 to wake.
5. **Host-test strategy.** Create two `CapabilityTable`s and two `Endpoint` stubs; call `send` and `recv` in each ordering; assert message fields and transferred caps.

## Definition of done

- [x] `cargo fmt --all -- --check` clean.
- [x] `cargo host-clippy` clean.
- [x] `cargo kernel-clippy` clean.
- [x] `cargo host-test` passes with the new tests (55 tests green; 11 new IPC tests).
- [x] No new `unsafe` without an audit entry.
- [x] Commit(s) follow [`commit-style.md`](../../../standards/commit-style.md).
- [x] [`current.md`](../../../roadmap/current.md) updated on each status transition.

## Design notes

- **Why no reply-recv fastpath in v1 by default?** A pure rendezvous (`send` + `recv`) is the minimal correct primitive; the fastpath is an optimization for the common server pattern (receive, process, reply) that adds complexity before we have benchmarks to justify it. ADR-0017 decides; the preference leans toward deferral unless the two-task demo in A6 clearly benefits.
- **Capability transfer atomicity.** The simplest safe implementation: (a) validate all source handles exist in the sender's table, (b) remove them in order, (c) install them in the receiver's table. If step (b) fails mid-way (shouldn't happen with pre-validation), roll back by reversing the removes. A simpler alternative: validate then swap in one pass — depends on whether `CapabilityTable` will expose a swap primitive (probably not in v1).
- **Waiter queue sizing.** An `Endpoint` waiter queue of depth 1 is enough for the A4 and A6 scenarios (one sender, one receiver). ADR-0017 picks the bound; larger queues are an A4-extension or A5/A6 concern.
- **Badge deferral.** If ADR-0018 defers badges, the message header has a reserved discriminator field that is always zero in v1. The receiver sees it; no semantic is attached. This makes ADR-0018 additive.

## References

- [ADR-0016: Kernel object storage](../../../decisions/0016-kernel-object-storage.md) — the A3 foundation this task extends.
- [ADR-0017: IPC primitive set](../../../decisions/0017-ipc-primitive-set.md) *(to be written before code lands)*.
- [ADR-0018: Badge scheme](../../../decisions/0018-badge-scheme.md) *(to be written or explicitly deferred in A4)*.
- [Phase A plan](../../../roadmap/phases/phase-a.md) — A4 sub-breakdown and acceptance criteria.
- [T-002](T-002-kernel-object-storage.md) — delivers the `Endpoint` and `Notification` objects this task wires up.
- seL4 IPC model — synchronous rendezvous with capability transfer (prior art; badge scheme not adopted in v1).

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-04-21 | @cemililik | opened; status Draft — ADR-0017 not yet written; A4 blocked until ADR-0017 Accepted. |
| 2026-04-21 | @cemililik | ADR-0017 Accepted; status → Ready. Implementation may begin. |
| 2026-04-21 | @cemililik | status → In Progress; implementation begins on `development`. |
| 2026-04-21 | @cemililik | status → In Review; 55/55 tests pass, all clippy clean. |
