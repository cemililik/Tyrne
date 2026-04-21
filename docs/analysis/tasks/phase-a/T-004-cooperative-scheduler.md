# T-004 — Cooperative scheduler

- **Phase:** A
- **Milestone:** A5 — Cooperative scheduler and context switch
- **Status:** Draft
- **Created:** 2026-04-21
- **Author:** @cemililik
- **Dependencies:** T-003 — IPC primitives (Done)
- **Informs:** T-005 (two-task IPC demo — A6)
- **ADRs required:** ADR-0019 (Scheduler shape) — must be Accepted before implementation; ADR-0020 (`Cpu` trait v2 / context-switch extension) — must be Accepted before assembly or context-switch code lands.

---

## User story

As the Umbrix kernel, I want a cooperative, yield-based scheduler that can switch register context between two kernel-level task stubs and can unblock a task that was waiting on IPC — so that the A6 two-task IPC demo can run both tasks on a single CPU core with observable console output from inside QEMU.

## Context

T-003 gave us `ipc_send` / `ipc_recv` / `ipc_notify` with correct waiter-state management. What the A4 IPC layer cannot do is actually suspend a calling task and resume it when the other side arrives: "blocking" in A4 is a state recorded in `IpcQueues`; the caller's execution still continues. A5 wires the scheduler so that blocking means the task is removed from the ready queue, the CPU switches to another ready task, and the blocked task is re-enqueued when its condition is satisfied.

The scheduler itself is deliberately simple: cooperative yield (no preemption, no timer tick in A5). Two design decisions drive the shape:

- **ADR-0019** settles the scheduler's data structure (queue type, yield semantics, blocked-task representation).
- **ADR-0020** extends the [`umbrix-hal::Cpu`](../../../hal/src/cpu.rs) trait with `save_context` / `restore_context` and a `TaskContext` associated type, so the context-switch assembly lives in the BSP rather than the kernel crate.

The actual assembly for saving and restoring aarch64 register state (callee-saved registers, SP, LR/PC) lives in `bsp-qemu-virt` behind a safe Rust wrapper with a documented `# Safety` contract. The kernel crate calls the HAL trait and stays `unsafe`-free.

**Scope constraint.** A5 has no timer: preemption, time-sliced round-robin, and real-time guarantees are Phase B work. The QEMU smoke test is deliberately minimal — two tasks that yield back and forth and print to the console, proving context switch works.

## Acceptance criteria

- [ ] **ADR-0019 Accepted** before any scheduler implementation lands. Settles: queue structure (single FIFO vs. priority queues), yield semantics, blocked-task lifecycle.
- [ ] **ADR-0020 Accepted** before any context-switch code lands. Settles: `Cpu` trait v2 shape (`save_context`, `restore_context`, `TaskContext` associated type), safety contract.
- [ ] **`Cpu` trait v2** lands in `umbrix-hal`; the BSP `QemuVirtCpu` implements it.
- [ ] **Context-switch assembly** in `bsp-qemu-virt`, behind a safe Rust wrapper; `unsafe` block audited per [`unsafe-policy.md`](../../../standards/unsafe-policy.md).
- [ ] **Scheduler queue** in `kernel::sched`: bounded, heap-free. Shape decided by ADR-0019.
- [ ] **`yield_now` kernel operation**: moves the current task to the back of the ready queue and switches to the head.
- [ ] **IPC integration**: when `ipc_recv` finds no sender (returns `RecvOutcome::Pending`), the scheduler removes the calling task from the ready queue and parks it. When `ipc_send` delivers to a waiting receiver (returns `SendOutcome::Delivered`), the scheduler re-enqueues the receiver.
- [ ] **Host tests** for scheduler data structures (enqueue, dequeue, block, unblock).
- [ ] **QEMU smoke test**: two kernel-level tasks yield back and forth; console shows alternating output from each task.
- [ ] **No new `unsafe`** beyond the context-switch wrapper. If any additional `unsafe` lands, audit entry per [`unsafe-policy.md`](../../../standards/unsafe-policy.md).

## Out of scope

- Timer interrupts and preemption — Phase B.
- Priority scheduling beyond what ADR-0019 decides for v1 (single FIFO is acceptable).
- SMP / multi-core — deferred indefinitely.
- Userspace task creation and switching — Phase B or later.
- `reply_recv` fastpath integration — deferred to when A6 reveals a concrete need.

## Approach

Design is delegated to ADR-0019 and ADR-0020. At a sketch level:

1. **Queue.** A `SchedQueue<N>` in `kernel::sched` — bounded, heap-free, holding task handles. ADR-0019 picks N and whether per-priority buckets are needed for v1.
2. **`TaskContext`.** An associated type on `Cpu` (ADR-0020) holding callee-saved registers + SP + PC/LR. The BSP implements the concrete type and the save/restore assembly.
3. **`yield_now(scheduler, cpu)`.** Saves the current task's context, moves the task handle to the back of the ready queue, pops the head, restores its context.
4. **IPC bridge.** After `ipc_recv` returns `Pending`, the caller's scheduler-layer wrapper parks the task (removes from ready queue, records it as waiting on the endpoint). After `ipc_send` returns `Delivered`, the scheduler-layer wrapper unparks the previously waiting receiver.
5. **QEMU smoke.** Two tasks created in `kernel_main`; each prints its ID and calls `yield_now`; loop runs until both have printed N times.

## Definition of done

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo host-clippy` clean.
- [ ] `cargo kernel-clippy` clean.
- [ ] `cargo host-test` passes with new scheduler unit tests.
- [ ] QEMU smoke test runs and prints alternating task output (manual check or CI run).
- [ ] `unsafe` in context-switch wrapper has a `# Safety` section and an audit entry.
- [ ] Commit(s) follow [`commit-style.md`](../../../standards/commit-style.md).
- [ ] [`current.md`](../../../roadmap/current.md) updated on each status transition.

## Design notes

- **Why cooperative-only?** Preemption requires a timer IRQ and safe IRQ entry/exit, which pulls in interrupt handling before the scheduler is even proven. Starting cooperative keeps the first context switch auditable and testable without hardware interrupt complexity.
- **Why `Cpu` trait extension rather than a separate trait?** The context-switch primitive is a fundamental CPU operation, like `write_bytes` on `Console`. Extending `Cpu` keeps the HAL surface minimal and avoids a proliferation of single-method traits. ADR-0020 may decide otherwise if the extension is large or awkward.
- **Safety of context switch.** The save/restore assembly is the first `unsafe` in the kernel that is not structurally impossible to make safe. The invariants (stack pointer valid, registers stable, interrupts disabled during switch) must be stated explicitly and checked in review.
- **IPC bridge complexity.** Parking a task on `RecvOutcome::Pending` requires knowing which task is the caller — in A5, "task" is a kernel-level stub with an ID and a `TaskContext`; the scheduler maps task ID to ready/blocked state. This is the first time the kernel has a concept of "current task."

## References

- [ADR-0017: IPC primitive set](../../../decisions/0017-ipc-primitive-set.md) — Accepted; A5 wires its blocking semantics to the scheduler.
- [ADR-0019: Scheduler shape](../../../decisions/0019-scheduler-shape.md) *(to be written before implementation)*.
- [ADR-0020: Cpu trait v2 / context-switch extension](../../../decisions/0020-cpu-trait-v2.md) *(to be written before context-switch code lands)*.
- [Phase A plan](../../../roadmap/phases/phase-a.md) — A5 sub-breakdown and acceptance criteria.
- [T-003](T-003-ipc-primitives.md) — delivers the IPC waiter states this task wires to the scheduler.
- seL4 scheduler model — priority-based, cooperative within a priority band (prior art; full model deferred).

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-04-21 | @cemililik | opened; status Draft — ADR-0019 and ADR-0020 not yet written; A5 blocked until both Accepted. |
