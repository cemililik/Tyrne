# 0017 — IPC primitive set

- **Status:** Accepted
- **Date:** 2026-04-21
- **Deciders:** @cemililik

## Context

[T-003](../analysis/tasks/phase-a/T-003-ipc-primitives.md) opens the IPC layer for Milestone A4. The kernel-object layer (T-002, [ADR-0016](0016-kernel-object-storage.md)) delivers `Endpoint` and `Notification` objects with placeholder queue fields but no send/recv logic. Before any implementation lands, the shape of the IPC interface must be settled: which operations exist in v1, how blocking is represented, what a message looks like, and how capabilities move with a message.

The decision has downstream consequences for every phase that follows:

- **A5 (scheduler)** must know which data structures to drain when a task blocks on IPC. The waiter-queue shape must be decided here.
- **A6 (two-task demo)** demonstrates IPC round-trip; its acceptance criteria depend on the exact operations v1 exposes.
- **Phase B (userspace)** will expose these operations through a syscall surface. Changing the kernel-side primitive set then is painful.

Three related sub-questions share this ADR:

1. **Operation set.** Pure rendezvous (`send` + `recv` + `notify`) or include a `reply_recv` fastpath from the start?
2. **Message format.** Fixed-size register-sized struct, or variable-length, or something else?
3. **Capability transfer.** Up to one capability per message (v1), multiple, or none?

**Single-core, no real scheduler in A4.** The kernel has no context-switch primitive until A5. IPC "blocking" in A4 means enqueuing a task stub on a wait-queue; the scheduler (A5) drains the queue when the partner arrives. Host tests exercise IPC by constructing two in-process parties that exchange messages without a real scheduler loop.

## Decision drivers

- **Correctness before optimization.** Phase A is about getting the kernel's internal structure right. An IPC fastpath is an optimization; it should follow measurements, not precede them.
- **Minimal stable surface.** Operations added in A4 become part of the syscall ABI in Phase B. Fewer operations in v1 means fewer commitments to evolve.
- **Composability with the capability system.** Every IPC operation is gated by a capability on the target object ([ADR-0014](0014-capability-representation.md)). The operation set must compose cleanly with rights bits already defined.
- **Fixed-size, bounded, heap-free.** All kernel state must be statically bounded ([ADR-0016](0016-kernel-object-storage.md) precedent). Messages and waiter queues must not allocate.
- **Atomic capability transfer.** Moving a capability from sender to receiver must be all-or-nothing. Partial transfer leaves both parties in an inconsistent authority state — a capability-system invariant violation.
- **Consistency with the arena / bounded-table pattern.** Waiter queues should follow the same index-based bounded pattern as `CapabilityTable` and the kernel-object arenas; one shape, lower audit cost.
- **Testability in isolation.** Operations must be exercisable as host-side Rust tests without QEMU, assembly, or a running scheduler loop.

## Considered options

### Operation set

1. **Option A — Pure rendezvous + notify** (`send`, `recv`, `notify`): three orthogonal primitives; no fastpath.
2. **Option B — Rendezvous + `reply_recv` fastpath** (`send`, `recv`, `reply_recv`, `notify`): adds a combined "reply to current client + wait for next client" operation common in server-pattern microkernels.
3. **Option C — Asynchronous queued send** (`send` never blocks; messages are buffered; `recv` drains the queue): sender and receiver are fully decoupled.

### Message format

4. **Option D — Fixed-size 4-word struct** (`label: u64`, `params: [u64; 3]`): 32 bytes, passed by value, no allocation.
5. **Option E — Variable-length buffer**: flexible, but requires a heap or a shared memory region the kernel does not have in Phase A.
6. **Option F — Single untyped `u64` payload**: simplest possible, but too narrow for real use; every non-trivial message needs at least a label and data words.

### Capability transfer per message

7. **Option G — Up to one capability per message (v1)**: exactly 0 or 1 cap transferred with delivery; simplest atomic-transfer path.
8. **Option H — Multiple capabilities per message** (e.g., up to 4): more expressive, but requires a small array and multiple atomic cap-moves; adds complexity before there is a use case.
9. **Option I — No capability transfer in A4**: defer entirely to A5 or A6; simplest A4 implementation, but leaves the atomicity story untested until the demo.

## Decision outcome

**Chosen:**
- Operation set: **Option A — pure rendezvous + notify**.
- Message format: **Option D — fixed-size 4-word struct**.
- Capability transfer: **Option G — up to one capability per message**.

### Operation set rationale

`send` + `recv` + `notify` is the minimal correct IPC surface. The `reply_recv` fastpath (Option B) is an optimization for the server loop pattern — receive a request, process it, reply while simultaneously listening for the next request — and reduces context switches by one per round trip. That is a measurable benefit, but it requires:

- A second distinguished "reply endpoint" concept or a way to name the caller's return channel.
- Scheduler awareness of which task is the current client.
- A combined atomic operation across three distinct state machines (reply capability, endpoint queue, task context).

None of these exist in Phase A. Without a running scheduler and real context switches, there is nothing to measure, and therefore no evidence the complexity is worth carrying. The fastpath is not rejected — it is deferred: ADR-0018 (or a successor) will introduce it when A5/A6 produce benchmarks that motivate it. Phase A's `send` + `recv` + `notify` is the foundation on which the fastpath is layered.

Option C (asynchronous send) is a fundamentally different model. Asynchronous delivery decouples sender and receiver but requires a message buffer large enough to hold all in-flight messages — a heap or shared-memory region Tyrne does not have in Phase A. It also complicates the capability-transfer atomicity invariant: when does authority transfer if the receiver hasn't consumed the message yet? Rendezvous avoids this by transferring authority only at the moment both parties are present.

### Message format rationale

Option D (4-word struct) matches the register-file model of real syscall ABIs: on aarch64, a syscall passes arguments in x0–x7; four words fit in x0–x3 plus a label in x4 (or similar). The struct is stack-allocated and copied by value — no pointer chasing, no allocation. Option E (variable-length) requires infrastructure Tyrne does not have in Phase A. Option F (single u64) is too narrow: a `label` word and at least one data word are needed for the A6 two-task demo to carry meaningful content.

The 4-word message type:

```rust
/// Fixed-size IPC message body. Passed by value — no heap, no pointers.
///
/// `label` is a caller-defined discriminator (opcode, tag, error code on
/// reply). `params` carries up to three arbitrary-width data words.
/// Content interpretation is entirely the caller's responsibility; the
/// kernel does not inspect or validate fields beyond delivering them.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Message {
    /// Caller-defined discriminator. The kernel does not interpret this.
    pub label: u64,
    /// Up to three general-purpose data words.
    pub params: [u64; 3],
}
```

### Capability transfer rationale

Option G (≤ 1 cap per message) is sufficient for the A6 demo and exercises the full atomicity story: validate the handle, remove it from the sender's table, deliver it to the receiver's table — all in one kernel-internal step that either fully completes or is never started (pre-validation ensures no partial state). Option H (multiple caps) adds a loop around the same operation and is straightforward to add later, but there is no A4 or A6 scenario that requires more than one cap per message. Introducing the loop before there is a test case that drives it is premature. Option I (no transfer in A4) would leave the most security-sensitive property of IPC — capability atomicity — untested until the demo; we test it in A4 instead.

The optional capability is represented as:

```rust
/// The optional capability transferred with a message.
///
/// `None` means no capability is transferred. `Some(handle)` names
/// exactly one capability in the sender's `CapabilityTable` to be moved
/// atomically with delivery.
pub type CapTransfer = Option<crate::cap::CapHandle>;
```

### Blocking model

Each `Endpoint` carries two bounded wait-queues: one for blocked senders, one for blocked receivers. Each queue holds at most `ENDPOINT_QUEUE_DEPTH` entries (decided below). In A4, "blocking" means inserting a task-stub descriptor into the appropriate queue. The scheduler (A5) inspects the queue when it selects the next runnable task; the `recv` / `send` path wakes the partner when a rendezvous completes.

Queue depth: **1** for v1. The A4 host tests and the A6 demo involve exactly one sender and one receiver. A queue of depth 1 is sufficient and matches the "minimal bounded" principle. A successor ADR raises the depth when a concrete use case demands it.

```rust
/// Maximum number of task stubs that can be simultaneously blocked on a
/// single endpoint waiting to send or receive. Revisit when A5/A6
/// produce a scenario that requires depth > 1.
pub const ENDPOINT_QUEUE_DEPTH: usize = 1;
```

The `notify` operation is non-blocking: it ORs bits into the `Notification` word and records that there may be a waiter to wake. The waiter-wake path is wired into the scheduler in A5; in A4 the bits are set and any waiter descriptor is stored but not immediately run.

## Consequences

### Positive

- **Minimal stable syscall surface.** Three operations (`send`, `recv`, `notify`) are easier to specify formally and less likely to need breaking changes in Phase B.
- **Atomic capability transfer in v1.** The single-cap transfer path is tested in A4; the invariant is known correct before A6 depends on it.
- **Fixed message size enables copy-by-value.** No pointer arguments, no aliasing hazards, no heap pressure in the IPC hot path.
- **Waiter-queue shape unblocks A5.** A5's scheduler knows exactly what it must drain: a bounded array of task-stub descriptors per endpoint, one for senders and one for receivers.
- **Composable with existing rights.** `send` requires a send-right cap on the endpoint; `recv` requires a recv-right; `notify` requires a notify-right. These map cleanly onto `CapRights` bits to be added in T-003.

### Negative

- **No `reply_recv` fastpath.** Server-pattern tasks (receive request → process → reply → listen) incur an extra context switch per round trip compared to a fastpath-equipped design. Mitigation: the fastpath is additive; it can be introduced in a future ADR without changing `send` or `recv`. The cost is not paid until A5 measures it.
- **Single-cap transfer per message.** Protocols that need to transfer multiple capabilities in one atomic step must either use multiple messages (breaking atomicity) or wait for a later ADR that extends `CapTransfer` to a small array. Mitigation: no Phase A scenario requires multi-cap transfer; the extension is additive to the message struct.
- **Waiter queue depth 1.** A second blocked sender is refused (the operation returns an error rather than blocking indefinitely). Mitigation: depth 1 is correct for all A4/A6 scenarios; the constant lives in one place and a successor ADR raises it with a concrete test.

### Neutral

- **`notify` waiter wake deferred to A5.** In A4, `notify` sets the bits and records a pending wake, but the waiter does not actually run until A5 wires it into the scheduler. This is correct — there is no scheduler to run in A4 — but it means the full `wait` / `notify` round trip is not exercisable in A4 host tests alone. A5 completes the picture.
- **No badge scheme in v1 (ADR-0018 decides).** The `Message` struct has a `label` field that callers may use as a discriminator. A badge scheme would have the kernel inject a per-derivation tag into `label` automatically. ADR-0018 decides whether v1 needs this or defers it; the `label` field is forward-compatible with either outcome.
- **Message content is opaque to the kernel.** The kernel copies `Message` fields verbatim; it does not validate, interpret, or sanitize them. This is standard for microkernel IPC and is the right call for v1, but means the receiver bears full responsibility for input validation.

## Pros and cons of the options

### Option A — Pure rendezvous + notify (chosen)

- Pro: minimal operation count; easiest to specify and formally reason about.
- Pro: sufficient for the A6 demo; the two-task scenario needs exactly `send`, `recv`, and optionally `notify`.
- Pro: no scheduler dependencies beyond waiter-queue management.
- Con: server-pattern tasks pay an extra context switch per round trip vs. a `reply_recv` fastpath. Accepted cost — no benchmarks yet to quantify this.

### Option B — Rendezvous + `reply_recv` fastpath

- Pro: eliminates a context switch per server-pattern request; well-established in seL4 and L4 family.
- Con: requires a "reply cap" or equivalent mechanism that does not yet exist in the capability system.
- Con: adds a fourth operation with non-trivial scheduler interaction before A5 exists.
- Con: the optimization cannot be measured until A5/A6 produce real timings — premature.

### Option C — Asynchronous queued send

- Pro: sender and receiver are fully decoupled; neither blocks on the other's availability.
- Con: requires per-endpoint message buffers (a heap or shared memory region that Phase A does not have).
- Con: capability transfer atomicity is ill-defined when a message sits in a buffer between send and receive.
- Con: fundamentally different model from the rendezvous that Phase B's syscall surface would expose; picking it now commits Phase B to it.

### Option D — Fixed-size 4-word struct (chosen)

- Pro: copy by value, no allocation, no pointers, no aliasing.
- Pro: maps directly to the aarch64 register ABI (x0–x3 or similar) for the eventual real syscall path.
- Con: applications that need to transfer large data must either use multiple messages or a shared-memory region (Phase B). Accepted — large transfers belong in Phase B.

### Option E — Variable-length buffer

- Pro: flexible; applications can transfer arbitrary amounts of data in one call.
- Con: requires a heap or a shared-memory frame the kernel does not have in Phase A.
- Con: buffer lifetime and ownership semantics are non-trivial; a full design adds significant complexity.

### Option F — Single untyped u64

- Pro: simplest possible; no design decisions about field layout.
- Con: too narrow for real protocols; a label and at least one data word are both needed for the A6 demo to carry meaningful content.

### Option G — Up to one capability per message (chosen)

- Pro: sufficient for all A4 and A6 scenarios.
- Pro: atomic-transfer path is a straight-line: validate → remove from sender → insert in receiver.
- Con: protocols needing to transfer multiple caps atomically must wait for a later ADR.

### Option H — Multiple capabilities per message

- Pro: more expressive; covers protocols that hand off a set of capabilities atomically.
- Con: no A4/A6 scenario requires it; adding a loop and array before there is a test case is premature complexity.

### Option I — No capability transfer in A4

- Pro: simplest A4 implementation; defer the atomicity story to A6.
- Con: the most security-sensitive IPC property (capability atomicity) is untested until the demo; a bug found in A6 would require reopening the A4 module. Testing it in A4 is strictly better.

## Open questions carried into T-003

- **`reply_recv` fastpath.** Explicitly deferred. ADR-0018 or a later ADR introduces it when A5/A6 benchmarks motivate it.
- **Badge scheme.** ADR-0018 decides whether the kernel injects a per-derivation discriminator into `Message::label` automatically. The `label` field is reserved for this; the scheme is additive.
- **`wait` operation on Notification.** A task that calls `wait` on a `Notification` blocks until `notify` fires. The blocking half arrives in A5 (the scheduler must run the waiter). A4 wires the queue; A5 drains it.
- **CapRights bits for send / recv / notify.** T-003 adds `SEND`, `RECV`, and `NOTIFY` to `CapRights`. Their bit positions are implementation detail; they must not overlap existing bits (`DUPLICATE` = 0x1, `DERIVE` = 0x2, `REVOKE` = 0x4, `TRANSFER` = 0x8).
- **Endpoint queue depth > 1.** `ENDPOINT_QUEUE_DEPTH = 1` for v1; a later ADR raises it if a real workload demands it.

## References

- [ADR-0001 — Capability-based microkernel architecture](0001-microkernel-architecture.md).
- [ADR-0014 — Capability representation](0014-capability-representation.md) — rights bitfield this ADR extends.
- [ADR-0016 — Kernel object storage](0016-kernel-object-storage.md) — `Endpoint` and `Notification` objects this ADR wires up.
- [T-003 — IPC primitives](../analysis/tasks/phase-a/T-003-ipc-primitives.md) — the implementing task.
- seL4 IPC model — https://sel4.systems/ — synchronous rendezvous with cap transfer and badge scheme (prior art; `reply_recv` fastpath and badges not adopted in v1).
- L4 microkernel family — https://os.inf.tu-dresden.de/L4/overview.html — origin of synchronous rendezvous IPC semantics.
- Hubris IPC — https://hubris.oxide.computer/ — fixed-size message passing, capability gating (shape parallel).
