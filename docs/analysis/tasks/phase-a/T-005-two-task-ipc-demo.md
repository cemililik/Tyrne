# T-005 — Two-task IPC demo

- **Phase:** A
- **Milestone:** A6 — Two-task IPC demo
- **Status:** Done
- **Created:** 2026-04-21
- **Author:** @cemililik
- **Dependencies:** T-004 — Cooperative scheduler (Done)
- **Informs:** Phase B — first Phase B task (not yet opened)
- **ADRs required:** none — ADR-0017 (IPC), ADR-0019 (scheduler), ADR-0020 (context switch) all Accepted.

---

## User story

As the Phase A exit gate, I want a deterministic QEMU-verified scenario where Task A sends a capability-gated message to Task B through an endpoint and B replies, so that the Phase A claim — "two kernel tasks exchange IPC under capability control, scheduled cooperatively" — is concretely proven before Phase B begins.

## Context

A5 (T-004) delivered a cooperative scheduler and a working context switch; the smoke test shows two tasks yielding back and forth using `yield_now`. What A5's smoke test does **not** demonstrate is actual IPC: the tasks never call `ipc_send`, `ipc_recv`, or capability operations. A6 replaces the yield-loop stub with a real end-to-end flow:

- Task A holds an endpoint capability and calls `ipc_send_and_yield`.
- Task B calls `ipc_recv_and_yield`; it receives A's message, logs it, and calls `ipc_send_and_yield` to reply.
- Task A resumes and receives the reply, logs it, and exits cleanly.

The scheduler's IPC-bridge operations (`ipc_send_and_yield`, `ipc_recv_and_yield`) were written in A5 but have not been exercised end-to-end on real hardware/QEMU. A6 is the first complete run of the capability → IPC → scheduler → context-switch stack.

A6 also closes the phase with two mandatory review artifacts: a baseline performance snapshot and a business retrospective covering A2–A6.

## Acceptance criteria

- [x] **Deterministic QEMU trace.** Running `tools/run-qemu.sh` produces (in order, allowing additional lines):
  ```text
  tyrne: hello from kernel_main
  tyrne: starting cooperative scheduler
  tyrne: task B — waiting for IPC
  tyrne: task A -- sending IPC
  tyrne: task B — received IPC (label=0xaaaa); replying
  tyrne: task A — received reply (label=0xbbbb); done
  tyrne: all tasks complete
  ```
- [x] **Capability discipline exercised.** `kernel_entry` creates one `Endpoint` in a global `EndpointArena`, then builds two separate `CapabilityTable`s (one per task) and inserts a root capability in each with `CapRights::SEND | CapRights::RECV`. Neither task ever dereferences the endpoint object directly; all IPC goes through each task's own cap handle (`EP_CAP_A` / `EP_CAP_B`) resolved via its table. No raw object access.
- [x] **IPC round-trip through scheduler.** The flow uses `Scheduler::ipc_send_and_yield` and `Scheduler::ipc_recv_and_yield` (no bare `yield_now` for the IPC itself — B's explicit `yield_now` after sending the reply is the scheduler-level companion call for the Enqueued path, documented in-code). The scheduler parks the blocked receiver until the sender arrives.
- [x] **Clean exit.** Both tasks complete without panic; after the IPC round-trip each task enters a `core::hint::spin_loop()` ("all tasks complete" is printed first). No "deadlock panic" from an empty ready queue. (The spin loop is an implementation hint — the specific instruction emitted is LLVM's choice — a dedicated idle-task / `wfe` path is Phase B work per ADR-0019's open questions.)
- [x] **Guide committed** at `docs/guides/two-task-demo.md` explaining what the demo proves and how to run it.
- [x] **Baseline performance review committed** at `docs/analysis/reviews/performance-optimization-reviews/2026-04-21-A6-baseline.md` with measured values for: kernel image size (stripped release ELF), idle memory footprint, context-switch instruction count, boot time, plus a note that IPC round-trip latency cannot be measured without a timer (Phase B).
- [x] **Business review committed** at `docs/analysis/reviews/business-reviews/2026-04-21-A6-completion.md` covering A2–A6 retrospective and Phase B readiness.

## Out of scope

- Userspace tasks — still Phase B.
- `reply_recv` fastpath — deferred by ADR-0018.
- Multi-message protocols or multiple rounds of IPC — the A6 demo is a single send/reply exchange.
- Performance optimization — A6's review records numbers; no optimization targets yet.
- SMP or preemption — Phase B.
- Real hardware (RPi4) — Phase B.

## Approach

The A5 BSP (`bsp-qemu-virt/src/main.rs`) already sets up `StaticCell`-backed stacks and a scheduler. A6 replaces the two yield-loop tasks with the IPC scenario:

1. **`kernel_entry`** creates one `Endpoint` via the kernel-object arena; derives a `Send` cap (for Task A) and a `Recv` cap (for Task B) using the capability table operations from A2/A3.
2. **Task A** calls `sched.ipc_send_and_yield(ep_send_cap, msg, ...)`, which invokes the A4 `ipc_send` under the hood; if Task B is not yet waiting, A parks until B arrives.
3. **Task B** calls `sched.ipc_recv_and_yield(ep_recv_cap, ...)`, receives A's message, logs it, then calls `sched.ipc_send_and_yield` on a reply endpoint to deliver the reply. (If a dedicated reply endpoint is cumbersome to set up without `reply_recv`, Task B may reuse the same endpoint in the opposite direction — the ADR-0017 rendezvous model supports both orderings.)
4. **Task A** resumes and receives the reply via a second `ipc_recv_and_yield`, logs "received reply; done", and returns.
5. **`kernel_entry`** detects both tasks complete (or the scheduler's ready queue is empty with all tasks `Idle`) and prints "all tasks complete" before entering `wfe`.

If the "clean idle" detection requires scheduler changes, a small `Scheduler::is_all_idle()` predicate is acceptable — no new ADR needed for a one-liner.

The guide and review documents are written after the QEMU smoke confirms the trace.

## Definition of done

- [x] `cargo fmt --all -- --check` clean.
- [x] `cargo host-clippy` clean with `-D warnings`.
- [x] `cargo kernel-clippy` clean.
- [x] `cargo host-test` passes (109 tests green; no scheduler tests broken by BSP changes).
- [x] QEMU smoke trace matches acceptance criterion 1 (confirmed 2026-04-21).
- [x] Any new `unsafe` has an audit entry per `unsafe-policy.md` (UNSAFE-2026-0012 extended to cover IPC statics aliasing).
- [x] Guide and both review documents committed.
- [x] Commit message follows `commit-style.md`.
- [x] Task status updated to `Done`; `docs/roadmap/current.md` updated.

## Design notes

- **Reply endpoint design.** ADR-0017's rendezvous model supports sender-first and receiver-first orderings on the same endpoint. The simplest A6 implementation reuses the single endpoint for both directions: A sends → B receives and replies → A receives on the same endpoint. This avoids allocating a second endpoint and keeps the capability setup minimal. If this proves confusing in the trace or the guide, a second endpoint is a straightforward extension.
- **Idle detection.** After both tasks return (they are `fn() -> !` in A5, but A6 tasks may `loop { wfe }` after printing "done" — or the task entry wrapper can call a scheduler method to deregister itself). The simplest approach: tasks spin on `wfe` and `kernel_entry` polls `sched.ready_count() == 0` in a loop — acceptable for a demo.
- **Performance review methodology.** IPC round-trip latency can be measured by reading CNTVCT_EL0 before `ipc_send_and_yield` and after the matching `ipc_recv_and_yield`. Context-switch overhead requires two CNTVCT reads bracketing `yield_now`. These are coarse but sufficient for a v0.0.1 baseline. The review doc notes the measurement method alongside the numbers so future reviews can be compared apples-to-apples.
- **No new `unsafe` expected.** The IPC path and capability operations are safe Rust; the scheduler's context switch already has all its audit entries. If any new `unsafe` surfaces, it must be audited per the usual policy.

## References

- [ADR-0017: IPC primitive set](../../../decisions/0017-ipc-primitive-set.md) — the `ipc_send` / `ipc_recv` semantics this demo exercises.
- [ADR-0019: Scheduler shape](../../../decisions/0019-scheduler-shape.md) — `ipc_send_and_yield` / `ipc_recv_and_yield` API used by both tasks.
- [ADR-0020: Cpu trait v2 / context-switch extension](../../../decisions/0020-cpu-trait-v2-context-switch.md) — the context-switch primitive invoked on every yield.
- [T-004: Cooperative scheduler](T-004-cooperative-scheduler.md) — delivers the scheduler and IPC-bridge this task exercises.
- [Phase A plan](../../../roadmap/phases/phase-a.md) — A6 acceptance criteria and Phase A exit bar.
- [bsp-qemu-virt/src/main.rs](../../../../bsp-qemu-virt/src/main.rs) — BSP entry point this task modifies.
- [kernel/src/sched/mod.rs](../../../../kernel/src/sched/mod.rs) — scheduler IPC-bridge operations.

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-04-21 | @cemililik | opened; T-004 Done → T-005 In Progress; A6 work begins. |
| 2026-04-21 | @cemililik | implementation complete; QEMU trace confirms IPC round-trip; guide, perf review, and business review committed. Status → Done. Phase A exit bar met. |
