# 0018 — Badge scheme and `reply_recv` fastpath: formal deferral

- **Status:** Accepted
- **Date:** 2026-04-21
- **Deciders:** @cemililik

## Context

[ADR-0017](0017-ipc-primitive-set.md) settled the v1 IPC primitive set (`send`, `recv`, `notify`) and explicitly left two questions open for this ADR:

1. **Badge scheme.** Should the kernel inject a per-derivation discriminator into `Message::label` automatically? A badge is a small integer stamped onto a capability at derivation time; when the bearer sends a message through that capability, the kernel replaces (or augments) `label` with the badge value. The receiver can therefore distinguish which derived capability a message arrived on — useful for servers that share a single endpoint across many clients.

2. **`reply_recv` fastpath.** Should the kernel expose a combined "reply-to-current-client + wait-for-next-client" operation (`reply_recv`)? This eliminates one context switch per server-pattern round trip by merging the outbound `send` and the next inbound `recv` into a single kernel entry.

Both features are well-established in the L4 / seL4 lineage. Neither is required by the Phase A or A6 acceptance criteria. ADR-0017 required that this ADR either accept them or record a principled deferral.

**State of the codebase at decision time.**

- `Message::label` is a caller-controlled `u64`; the kernel does not inspect or modify it.
- The capability derivation tree tracks `parent` / `first_child` / `next_sibling` indices but stores no badge value.
- `ipc_send` / `ipc_recv` / `ipc_notify` are implemented; the `CapabilityTable` has no "badge" field.
- No scheduler exists yet (A5 is next); `reply_recv` would require a live scheduler to be meaningful.
- The A6 two-task demo involves exactly two tasks, one endpoint, and one direction of reply — no multiplexed server pattern.

## Decision drivers

- **No scenario requires either feature in Phase A or A6.** Badge injection is useful when a single endpoint serves multiple clients distinguished by their derivation path; A6 has exactly one client and one server. `reply_recv` optimises away a context switch that does not yet exist.
- **Both features modify the security-critical capability derivation path.** Adding a badge field to `Capability` or `SlotEntry` and making the kernel stamp it on message delivery changes how the kernel propagates authority. Introducing that change without a concrete use case that can validate correct behaviour is a risk that does not pay off in Phase A.
- **`reply_recv` requires scheduler concepts that do not exist until A5.** The operation must atomically reply to the current caller and suspend waiting for the next; this is meaningless without a running scheduler and task identities. The earliest it can be implemented correctly is A5; the earliest it can be *measured* is A6.
- **The `label` field is forward-compatible.** A badge scheme that injects into `label` is additive: current code that writes `label` explicitly continues to work if the scheme is introduced later (the badge would OR into or replace the value — the exact semantics are an ADR-0018-successor question). No ABI break.
- **Deferral preserves design freedom.** A badge scheme for a pure rendezvous kernel may look different from one designed alongside `reply_recv`. Deciding them together — after A6 produces a concrete use case — yields a more coherent design.

## Considered options

### Badge scheme

**Option A — Kernel-injected badge (defer).** Add a `badge: u64` field to `SlotEntry`; set it at `cap_derive` time; stamp it into `Message::label` on `ipc_send`. Defer to a successor ADR once A6 or Phase B produces a server-with-multiple-clients scenario.

**Option B — User-space badge emulation.** The server stores the badge in its own data structure keyed on `CapHandle`. No kernel change needed. Always available without a new ADR.

**Option C — Kernel-injected badge now.** Add the badge immediately alongside T-003. Rejected: no test case in Phase A validates correct badge injection; adding a kernel-side mutation of a user-visible message field before it is tested is unsafe practice for a high-assurance kernel.

### `reply_recv` fastpath

**Option D — Defer `reply_recv`.** Implement it in a successor ADR once A5 gives us a running scheduler and A6 lets us measure the context-switch cost it would eliminate.

**Option E — Implement `reply_recv` now (A5).** The operation would be part of T-004 / A5. Rejected: A5 is already complex (context-switch assembly, HAL extension, scheduler data structures); adding a combined IPC-plus-scheduling operation before the base scheduler is proven stable adds risk without a measured motivation.

**Option F — Replace `send` + `recv` with `reply_recv` as the primary primitive.** Adopted by seL4 as a design choice. Rejected: it couples IPC and scheduling more tightly than the current architecture allows; it would require revisiting ADR-0017's operation set wholesale.

## Decision outcome

**Chosen:**
- Badge scheme: **Option A — defer to a successor ADR.**
- `reply_recv` fastpath: **Option D — defer to a successor ADR.**

**Rationale.**

Both features share the same deferral logic: no Phase A or A6 scenario requires them, both touch security-critical code paths that should be introduced with a concrete test case, and both are genuinely additive — the existing `Message` struct, `CapabilityTable`, and IPC operations are forward-compatible with either feature landing later.

User-space badge emulation (Option B) is available today at zero kernel cost and is sufficient for the A6 demo. If the A6 demo reveals that the server pattern genuinely needs kernel-injected badges, the evidence will be in the code and can motivate a focused successor ADR with a real test.

`reply_recv` is explicitly gated on A5 being complete: the operation is nonsensical without a scheduler and unmeasurable without real context switches. The right time to write its ADR is after A5/A6 produce timings.

### Revisit triggers

A successor ADR superseding this one should be written when **any one** of the following is true:

1. A6 or Phase B introduces a server that multiplexes a single endpoint across more than one client, and distinguishing clients at the receiver side requires kernel-level badge injection (rather than user-space keying on `CapHandle`).
2. A5/A6 QEMU measurements show that the extra context switch in the server loop is a meaningful bottleneck (e.g., it dominates the round-trip latency in a benchmark that Phase B cares about).
3. A formal security argument for Tyrne requires that badge injection happen inside the kernel TCB rather than in user space.

## Consequences

**Positive:**

- No change to `Capability`, `SlotEntry`, `Message`, or any IPC operation. T-003's implementation is complete and unaffected.
- The deferral is now formally recorded; future contributors will not re-open the question without a concrete trigger.
- User-space badge emulation is available immediately: a server allocates one `CapHandle` per client and stores the client identity in a local table keyed by handle. This pattern works correctly today.

**Negative:**

- Multiplexed-server protocols that rely on kernel-injected badges cannot be written until a successor ADR lands. This is not a Phase A or A6 concern.
- The server-pattern round-trip has one extra context switch compared to a `reply_recv` design. This cost is not measurable until A5; it is not paid in Phase A.

**Neutral:**

- `Message::label` remains a plain caller-supplied `u64`. A badge scheme that writes into `label` is additive; a badge scheme that adds a separate `badge` field is also additive (a new public field in `Message` with a default of `0` is backwards-compatible). Both options remain open to the successor ADR.
- The `CapabilityTable::cap_derive` signature is unchanged. A badge-aware `cap_derive` would gain a `badge: u64` parameter; that is an additive API change that does not break existing call sites (they can pass `0`).

## References

- [ADR-0017: IPC primitive set](0017-ipc-primitive-set.md) — the ADR that opened these questions.
- [T-003: IPC primitives](../analysis/tasks/phase-a/T-003-ipc-primitives.md) — the implemented task; `Message::label` is the badge-compatible field.
- [T-004: Cooperative scheduler](../analysis/tasks/phase-a/T-004-cooperative-scheduler.md) — prerequisite for any `reply_recv` implementation.
- seL4 badge scheme — https://sel4.systems/ — kernel-injected per-endpoint badge; the design this ADR defers borrowing from.
- seL4 `seL4_ReplyRecv` — combined reply + receive fastpath; the operation Option D defers.
- Hubris IPC — no badge concept; server distinguishes callers by task identity rather than capability derivation. A simpler model worth revisiting when the use case is clearer.
