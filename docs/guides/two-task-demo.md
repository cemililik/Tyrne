# Guide: Two-task IPC demo

This guide explains what the two-task IPC demo proves, how to run it, and how to interpret its output. It corresponds to Milestone A6, [T-005](../analysis/tasks/phase-a/T-005-two-task-ipc-demo.md).

## What the demo proves

Phase A's exit bar states: *"two kernel tasks exchange IPC messages under capability control, scheduled cooperatively, running on QEMU `virt` aarch64."* The demo satisfies this bar end-to-end:

- **Capability discipline.** Each task holds its own `CapabilityTable` with a `SEND | RECV` capability on a shared endpoint. Neither task has a raw pointer to the endpoint object; all access is through the capability system (ADR-0014, ADR-0016).
- **IPC semantics.** Task A sends a message; Task B receives it and replies. Both directions go through `ipc_send` / `ipc_recv` (ADR-0017), not bare `yield_now`.
- **Cooperative scheduling.** The scheduler's `ipc_send_and_yield` and `ipc_recv_and_yield` bridge (ADR-0019) parks blocked tasks and resumes them when the matching operation arrives. No timer, no preemption.
- **Context switch.** Every yield invokes the `#[unsafe(naked)]` aarch64 context-switch assembly (ADR-0020) that saves and restores x19–x28, fp, lr, sp, and d8–d15.

## How to run

```
cargo kernel-build          # build debug image (once)
tools/run-qemu.sh           # run — terminate with Ctrl-A x
```

To run the release build:

```
tools/run-qemu.sh --release
```

To capture the exception log when debugging a silent hang:

```
tools/run-qemu.sh --int-log
# after the run:
grep "Taking exception" /tmp/qemu_int.log
```

## Expected output

```
umbrix: hello from kernel_main
umbrix: starting cooperative scheduler
umbrix: task B — waiting for IPC
umbrix: task A -- sending IPC
umbrix: task B — received IPC (label=0xaaaa); replying
umbrix: task A — received reply (label=0xbbbb); done
umbrix: all tasks complete
```

After printing "all tasks complete", Task A enters a `core::hint::spin_loop()` which compiles to `wfe`. The kernel halts in this state; QEMU continues running but produces no further output. Terminate with **Ctrl-A x** (QEMU monitor quit).

## Execution trace

The scheduler adds **Task B first**, then Task A, so B runs first:

1. **Task B** starts, prints "waiting for IPC", calls `ipc_recv_and_yield`. No sender is ready → endpoint transitions to `RecvWaiting` → B is marked `Blocked`, B's context is saved, Task A is dequeued and restored.

2. **Task A** starts, prints "sending IPC", calls `ipc_send_and_yield`. A sender finds a registered receiver → endpoint advances to `RecvComplete` (the message is staged for B) → `unblock_receiver_on` re-enqueues B → `yield_now` re-enqueues A, dequeues B, context-switches to B.

3. **Task B** resumes inside `ipc_recv_and_yield`. A second `ipc_recv` call collects the staged message (`RecvComplete → Idle`). B prints "received IPC; replying", constructs a reply message, calls `ipc_send_and_yield`. A is not yet blocked on recv → endpoint transitions to `SendPending` → outcome is `Enqueued` (no auto-yield). B calls `yield_now` explicitly to give A the CPU, then prints "done; spinning".

4. **Task A** resumes from `ipc_send_and_yield`, calls `ipc_recv_and_yield`. The endpoint is in `SendPending` → `ipc_recv` returns `Received` immediately (no blocking needed). A prints "received reply; done" and "all tasks complete", then enters the spin loop.

## What each line tells you

| Line | What it confirms |
|------|-----------------|
| `hello from kernel_main` | Boot succeeded; PL011 console is operational. |
| `starting cooperative scheduler` | Capability tables, endpoint arena, IPC queues, and scheduler are all initialised. |
| `task B — waiting for IPC` | Task B's entry function ran; `ipc_recv_and_yield` was invoked. |
| `task A -- sending IPC` | Context switch from B to A worked; A's stack is intact. |
| `task B — received IPC (label=0xaaaa); replying` | Context switch back to B worked; B received A's message with correct label. |
| `task A — received reply (label=0xbbbb); done` | Context switch from B to A worked a second time; IPC reply delivered with correct label. |
| `all tasks complete` | Phase A exit bar met. |

## Capability setup

`kernel_entry` creates one `Endpoint` in the `EndpointArena`, then derives two capabilities — one in `TABLE_A`, one in `TABLE_B` — each with `SEND | RECV | DUPLICATE` rights. The rights needed per operation are:

| Operation | Right checked |
|-----------|--------------|
| `ipc_send_and_yield` | `CapRights::SEND` |
| `ipc_recv_and_yield` | `CapRights::RECV` |

No capability escapes its owner's table. The tasks never access the `EndpointArena` directly.

## Known limitations (Phase A)

- **No preemption.** Task B must call `yield_now` after sending the reply; without it, A never runs again.
- **One endpoint, depth-1 queue.** A second concurrent sender would hit `IpcError::QueueFull`.
- **`&mut` aliasing.** The BSP uses `UnsafeCell` statics for shared kernel state. The aliasing across context switches is documented and justified in [UNSAFE-2026-0012](../audits/unsafe-log.md). It will be resolved by a raw-pointer API refactor in a future ADR.
- **No timer.** IPC latency is not measured in Phase A; see the [baseline performance review](../analysis/reviews/performance-optimization-reviews/2026-04-21-A6-baseline.md).

## References

- [ADR-0017: IPC primitive set](../decisions/0017-ipc-primitive-set.md)
- [ADR-0019: Scheduler shape](../decisions/0019-scheduler-shape.md)
- [ADR-0020: Cpu trait v2 / context-switch extension](../decisions/0020-cpu-trait-v2-context-switch.md)
- [T-005: Two-task IPC demo](../analysis/tasks/phase-a/T-005-two-task-ipc-demo.md)
- [BSP boot checklist](../standards/bsp-boot-checklist.md)
