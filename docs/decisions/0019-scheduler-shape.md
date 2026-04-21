# 0019 — Scheduler shape

- **Status:** Accepted
- **Date:** 2026-04-21
- **Deciders:** @cemililik

## Context

[T-004](../analysis/tasks/phase-a/T-004-cooperative-scheduler.md) opens Milestone A5: the first real scheduler in Umbrix. The IPC layer (T-003, [ADR-0017](0017-ipc-primitive-set.md)) records waiter state in `IpcQueues` but has no mechanism to suspend a calling task or resume it when the other side arrives. A5 wires that mechanism.

Before any implementation lands, four inter-related questions must be settled:

1. **Queue structure.** What data structure holds the set of tasks that are ready to run?
2. **Yield semantics.** What does "yield" mean — give up the CPU to anyone, or to a specific task?
3. **Blocked-task representation.** How is a task that is waiting on IPC recorded, and how is it unblocked?
4. **IPC bridge ownership.** Which layer is responsible for calling `ipc_send`/`ipc_recv` and reacting to `SendOutcome::Delivered` / `RecvOutcome::Pending`?

**Constraints inherited from earlier ADRs.**

- All kernel state is statically bounded — no heap ([ADR-0016](0016-kernel-object-storage.md)).
- The kernel crate is `no_std`, HAL-dependent only through traits ([ADR-0006](0006-workspace-layout.md), [ADR-0008](0008-cpu-trait.md)).
- IPC is cooperative in A4/A5; preemption is Phase B ([ADR-0017](0017-ipc-primitive-set.md)).
- Context-switch assembly lives in the BSP, exposed through the `ContextSwitch` trait (ADR-0020); the scheduler calls the trait, not the assembly.
- Only one CPU core is in scope (single-core aarch64 QEMU `virt`).

**A5 scale.** Two kernel-level task stubs, cooperative yield, no timer tick. Complexity should match scale: a design that works perfectly for 2 tasks and is straightforward to extend later is better than a general scheduler written before there is a workload to validate it against.

## Decision drivers

- **Simplest correct first.** Single-core, cooperative, two tasks — the scheduler should be the least complex structure that satisfies the A5 and A6 acceptance criteria.
- **Bounded and heap-free.** The ready queue must have a compile-time capacity, just like `CapabilityTable` and the kernel-object arenas.
- **No circular module dependency.** `kernel::ipc` imports from `kernel::cap` and `kernel::obj`. A `kernel::sched` that imports from `kernel::ipc` is fine. The reverse (`ipc` importing `sched`) is not — it would create a cycle.
- **Auditable `unsafe` boundary.** The context-switch primitive will carry `unsafe`; the scheduler's data structures and logic must be safe Rust so the `unsafe` surface stays minimal and localised to the BSP wrapper.
- **Host-testable scheduler logic.** Queue operations (enqueue, dequeue, block, unblock) must be exercisable without QEMU, assembly, or a running loop.
- **Forward compatibility.** The chosen shape should not require a rewrite to add priority queues or a third task in A6 or Phase B.

## Considered options

### Queue structure

**Option A — Single bounded FIFO (`SchedQueue<N>`).**
One array-backed ring buffer of `TaskHandle`s with capacity `N = TASK_ARENA_CAPACITY`. `enqueue` appends to the back; `dequeue` pops from the front. All ready tasks are treated equally — no priority.

- Pro: minimal complexity; consistent with the arena/bounded-table pattern established across the codebase.
- Pro: O(1) enqueue and dequeue; no sorting, no heap.
- Pro: the only failure mode is "queue full" (more than `N` ready tasks simultaneously), which cannot happen when `N` equals the total task capacity.
- Con: no priority differentiation. A latency-sensitive task cannot cut the queue.
- Neutral: adding per-priority queues later is additive (a new ADR supersedes this one); nothing in the FIFO design precludes it.

**Option B — Per-priority bounded FIFOs.**
Multiple queues, one per priority level; `dequeue` picks the highest non-empty queue.

- Pro: enables latency differentiation.
- Con: no A5/A6 scenario requires it; adds complexity (priority assignment, multiple queues) before there is a workload that exposes the need.
- Con: priority inversion is a known hazard that requires protocol-level mitigation (priority inheritance, ceiling); introducing priority before those mitigations are in place is premature.

**Option C — Round-robin ring buffer (no explicit FIFO order).**
Each task gets a fixed slot; `yield_now` advances a cursor.

- Pro: deterministic per-task slot; no reordering.
- Con: slots are allocated statically regardless of whether a task exists, wasting capacity.
- Con: blocking a task leaves a gap in the ring; handling gaps adds complexity.

### Yield semantics

**Option D — Yield to next ready (FIFO order).**
`yield_now` moves the current task to the back of the ready queue and runs the task at the front. The caller does not specify a target.

- Pro: simple, composable; the scheduler owns the scheduling decision.
- Pro: with a FIFO queue, two tasks alternate deterministically — exactly the A5 smoke-test scenario.
- Con: a server task cannot yield specifically to a known client (needed for `reply_recv` fastpath, which is deferred to ADR-0018).

**Option E — Yield to specific task handle.**
`yield_to(target: TaskHandle)` moves the current task to the back of the ready queue and specifically moves `target` to the front (or directly runs it).

- Pro: supports the `reply_recv` pattern without a scheduler round-trip.
- Con: the caller must know the target's `TaskHandle`; this couples IPC and scheduling tightly.
- Con: if `target` is blocked, the call fails or silently becomes a `yield_to_next` — ambiguous semantics.
- Neutral: can be added later as a `yield_to` extension without changing the base `yield_now` semantics.

**Option F — Yield to highest priority ready.**
`yield_now` runs the highest-priority ready task regardless of order.

- Con: requires per-priority queues (Option B); excluded while Option A is chosen.

### Blocked-task representation

**Option G — Per-task state enum in the scheduler.**
The scheduler owns an array `task_states: [TaskState; TASK_ARENA_CAPACITY]` where `TaskState` is:

```rust
enum TaskState {
    Ready,
    Blocked { on: EndpointHandle },
    Idle,  // slot not in use
}
```

When `ipc_recv` returns `RecvOutcome::Pending`, the scheduler sets the task's state to `Blocked { on: ep_handle }` and removes it from the ready queue. When `ipc_send` returns `SendOutcome::Delivered`, the scheduler finds the task blocked on that endpoint, sets it back to `Ready`, and enqueues it.

- Pro: all scheduling state in one place; the scheduler is the single source of truth.
- Pro: O(N) unblock scan is acceptable at A5 scale (N ≤ 16).
- Pro: consistent pattern with `IpcQueues` (parallel state array indexed by slot index).
- Con: the blocked-state scan is O(N); a priority-indexed blocked list would be faster but unnecessary at A5 scale.

**Option H — Blocked tasks stored inside `IpcQueues`.**
`IpcQueues` tracks which `TaskHandle` is waiting at each endpoint alongside the `EndpointState`.

- Con: creates a `kernel::ipc` → `kernel::obj::task` dependency for `TaskHandle`. `kernel::cap` already imports from `kernel::obj`; adding `TaskHandle` to `ipc` does not create a cycle, but it couples two subsystems that are currently clean of each other.
- Con: the scheduler would need to query `IpcQueues` to learn which task to unblock — mixing IPC state and scheduling state in one structure.

**Option I — Blocked tasks on a per-endpoint wait list inside `Endpoint`.**
`obj::Endpoint` grows a `waiting_task: Option<TaskHandle>` field.

- Con: `obj` would need to import from `cap` (for `TaskHandle`) and potentially `sched`, creating circular dependencies.
- Con: the `Endpoint` struct already deliberately defers waiter state to `IpcQueues` (see ADR-0017 rationale); adding a task handle there reverses that decision.

### IPC bridge ownership

**Option J — Scheduler as the IPC orchestration layer.**
`kernel::sched` calls `ipc_send` / `ipc_recv` on behalf of tasks, inspects the outcomes, and performs the ready-queue and state transitions. Tasks do not call `ipc_*` directly; they request IPC through the scheduler.

- Pro: clean ownership hierarchy — `sched` is the top-level orchestrator; `ipc` is a pure state-machine layer with no scheduling concerns.
- Pro: no circular dependency (`sched` → `ipc` → `cap`, `obj`).
- Pro: `ipc_send` and `ipc_recv` remain pure, side-effect-free with respect to scheduling — easy to test independently.

**Option K — IPC functions call scheduler callbacks.**
`ipc_send` / `ipc_recv` accept a callback (trait object or function pointer) invoked on `Delivered` / `Pending`.

- Con: introduces dynamic dispatch or a generic parameter that threads through every IPC call site; adds complexity for no benefit in A5.
- Con: the IPC layer was deliberately designed without scheduling knowledge (ADR-0017); adding a callback parameter couples the two.

## Decision outcome

**Chosen:**

| Axis | Choice |
|------|--------|
| Queue structure | **Option A — Single bounded FIFO** |
| Yield semantics | **Option D — Yield to next ready** |
| Blocked-task state | **Option G — Per-task state enum in scheduler** |
| IPC bridge | **Option J — Scheduler as orchestration layer** |

**Rationale.** The four choices form a coherent, minimal design:

- A single FIFO ready queue mirrors the arena/bounded-table pattern already used throughout the codebase. Capacity equals `TASK_ARENA_CAPACITY` (currently 16), so the queue can never be "full" relative to the number of tasks that can exist.
- "Yield to next ready" is the simplest semantics that produces the A5 smoke-test behaviour (two tasks alternating deterministically). `reply_recv`-style targeted yield is deferred to ADR-0018.
- Keeping all scheduler state (ready queue + per-task `TaskState`) in `kernel::sched` avoids polluting `IpcQueues` or `obj::Endpoint` with scheduling concerns. The O(N) unblock scan is negligible at N ≤ 16.
- Making the scheduler the IPC orchestration layer keeps `kernel::ipc` free of scheduling knowledge, preserving its independent testability and the existing module boundary established in ADR-0017.

### Public API sketch

```rust
// kernel/src/sched/mod.rs

pub struct Scheduler {
    ready: SchedQueue<TASK_ARENA_CAPACITY>,
    task_states: [TaskState; TASK_ARENA_CAPACITY],
    current: Option<TaskHandle>,
}

pub enum TaskState {
    Idle,   // slot not occupied by a live task
    Ready,  // in the ready queue
    Blocked { on: EndpointHandle },
}

impl Scheduler {
    pub fn add_task(&mut self, handle: TaskHandle);
    pub fn yield_now<C: Cpu>(&mut self, cpu: &mut C, contexts: &mut TaskContexts<C>);
    pub fn ipc_send_and_yield<C: Cpu>(
        &mut self, cpu: &mut C, contexts: &mut TaskContexts<C>,
        ep_arena: &mut EndpointArena, queues: &mut IpcQueues,
        caller_table: &mut CapabilityTable, ep_cap: CapHandle,
        msg: Message, transfer: Option<CapHandle>,
    ) -> Result<SendOutcome, IpcError>;
    pub fn ipc_recv_and_yield<C: Cpu>(
        &mut self, cpu: &mut C, contexts: &mut TaskContexts<C>,
        ep_arena: &mut EndpointArena, queues: &mut IpcQueues,
        caller_table: &mut CapabilityTable, ep_cap: CapHandle,
    ) -> Result<RecvOutcome, IpcError>;
}
```

`TaskContexts<C>` is a bounded array of `C::TaskContext` values, indexed by raw task slot index — the same parallel-array pattern used by `IpcQueues`. It depends on ADR-0020 for the `C::TaskContext` associated type.

## Consequences

**Positive:**

- The scheduler data structures are safe Rust; only the context-switch call (ADR-0020) crosses the `unsafe` boundary.
- `kernel::ipc` remains testable in isolation with no scheduler dependency — existing 64 tests are unaffected.
- A5's QEMU smoke test (two tasks yielding back and forth) falls directly out of `yield_now` on a two-element FIFO.
- Adding priority queues in a future ADR is additive: `SchedQueue<N>` can be replaced by `[SchedQueue<N>; PRIORITY_LEVELS]` without changing the `yield_now` / `add_task` signatures.

**Negative:**

- O(N) unblock scan. With N ≤ 16 this is negligible; at larger N a hash map or endpoint-indexed wait list would be more efficient. Accepted as a known limitation at A5 scale.
- `yield_to(specific_task)` is not supported. The `reply_recv` fastpath (ADR-0017 open question, ADR-0018) requires it; until ADR-0018 is written, server-pattern tasks pay a scheduler round-trip on every reply.

**Neutral:**

- The `Scheduler` struct owns `current: Option<TaskHandle>`. In a real preemptive kernel this would be per-CPU; for single-core A5 a single field is sufficient and straightforward to change when SMP arrives.
- `TaskContexts<C>` is a parallel array to `IpcQueues` — same indexing discipline. This is consistent with the established pattern but means a third parallel array joins the two already present.

## Open questions

- **`yield_to(target)` for `reply_recv`.** Deferred to ADR-0018 (badge/reply scheme). If ADR-0018 adds `reply_recv`, it must decide whether to implement it as `yield_to(sender)` or as a scheduler-side fastpath.
- **Task context initialisation.** How is a new task's initial `TaskContext` set up before it runs for the first time? The first `restore_context` must point at a known entry function. The ADR-0020 and the T-004 implementation will specify the initialisation convention; it is not settled here.
- **Idle task.** With two tasks and cooperative scheduling, if both block on IPC simultaneously (a deadlock), the ready queue is empty and `yield_now` has nothing to run. A5 will panic in this case; a real idle task (spin-loop or WFI) is Phase B work.

## References

- [ADR-0017: IPC primitive set](0017-ipc-primitive-set.md) — the IPC layer this scheduler wires up.
- [ADR-0008: `Cpu` HAL trait v1](0008-cpu-trait.md) — ADR-0020 introduces the separate `ContextSwitch` trait alongside it.
- [T-004: Cooperative scheduler](../analysis/tasks/phase-a/T-004-cooperative-scheduler.md) — the task this ADR gates.
- seL4 scheduler — strict priority, bounded queues; Phase B reference.
- Hubris task-dispatch model — cooperative, bounded task set; closest prior art to the A5 design.
