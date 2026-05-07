# 0023 — Cross-table capability revocation policy

- **Status:** Deferred
- **Date:** 2026-04-27 (deferred at B0 closure); placeholder body landed 2026-05-07 (B1 closure trio)
- **Deciders:** @cemililik

> **Deferred placeholder.** This ADR's slot is reserved for a future cross-table capability-derivation-tree (CDT) decision. The deferral was recorded at the [B0 closure security review (2026-04-27)](../analysis/reviews/security-reviews/2026-04-27-B0-closure.md) and the [B0 closure retrospective (2026-04-27)](../analysis/reviews/business-reviews/2026-04-27-B0-closure.md) per the *accept-deferred* path called out in [`docs/roadmap/phases/phase-b.md`](../roadmap/phases/phase-b.md) §B0 ledger. The body below records why the slot is reserved, the conditions under which a real ADR-0023 should be authored, and what shape it would likely take. Until a real ADR lands, no implementation task references this ADR; the slot exists so the in-tree references (glossary, retros, phase-b ledger) resolve to a citable artefact rather than a dead link.

## Context

[ADR-0014](0014-capability-representation.md) defines the capability table as a per-task data structure: each task owns its own `CapabilityTable` (kernel-wide arena allocator under [ADR-0016](0016-kernel-object-storage.md)). [ADR-0017](0017-ipc-primitive-set.md) defines `ipc_send` / `ipc_recv` with capability transfer: the sender's `ipc_send` atomically removes a capability from its own table, embeds it in the message, and `ipc_recv` deposits it into the receiver's table. Capability *derivation* (`cap_derive`) is supported within a single table — children inherit a strict subset of the parent's rights and live in a parent / child / sibling tree under [`kernel/src/cap/table.rs`](../../kernel/src/cap/table.rs)'s linked-list invariants.

Revocation today is **per-table transitive only**: `cap_revoke(handle)` walks the BFS over the local derivation tree starting at `handle` and frees every descendant. After a capability has been *transferred* via IPC, the original sender's local derivation-tree node is gone (atomic removal) and the receiver's freshly-installed copy lives in a separate table with no parent / child link back. Therefore: a sender that *transferred* a capability cannot subsequently revoke the receiver's copy via the local CDT. The receiver's copy survives until the receiver itself revokes it or the receiver task is destroyed.

The seL4 reference design solves this with a *whole-system* CDT: every cap derivation, including cross-table transfers, contributes a parent / child link spanning tables. That gives a sender post-transfer revocation authority but pays a cost in (a) shared CDT storage / lookup, (b) cross-task dependency in the revoke fast path, (c) more elaborate locking for a future SMP world.

The Phase A code review (2026-04-21) flagged the local-only revocation as **security blocker #2** — a sender's perspective on capability lifecycle is incomplete in v1. The B0 closure security review (2026-04-27) classified the gap as *accept-deferred*: v1 has no multi-task server arc that would exercise the missing semantics, so the gap is benign for the current workload, but a future multi-task pattern must surface the question explicitly. This ADR is the placeholder that records the deferral and the trigger for re-opening it.

## Why deferred (the conditions for reopening)

A real ADR-0023 should be drafted when **any** of the following becomes true:

1. **A B-phase or later task introduces a userspace pattern that grants a capability to another task with revocation expectations.** Example: a logging server hands a sub-capability to each client; sender expects to revoke a misbehaving client. v1's cooperative IPC demo does not exercise this (the demo's two tasks share their own caps, no transfers).
2. **The first userspace driver lands** (Phase B6 or later) and needs to delegate sub-resources to client tasks with the option to claw them back when the client misbehaves.
3. **A formal threat-model item escalates this gap** above its current "accept-deferred" classification. The [`docs/architecture/security-model.md`](../architecture/security-model.md) "Open questions" section would be the natural surfacing point.

Until any of these triggers fires, the v1 cooperative workload is genuinely safe under per-table-only revocation: tasks that transfer capabilities are accepting that the receiver's copy survives the sender's revoke. This is documented behaviour, not a silent gap.

## What a real ADR-0023 would have to settle

When the trigger fires, the replacement ADR will need a Decision outcome that picks among:

- **Option A — Whole-system CDT (seL4-style).** All capabilities, regardless of which table they live in, share one global CDT. Cross-table revocation becomes the same `cap_revoke` walk. Costs: shared CDT storage scaling with global capability count; per-derivation lock; SMP cross-core cache traffic on the CDT.
- **Option B — Per-table CDT + cross-table back-pointer.** Local CDT stays per-table; cross-table transfers add a back-pointer from receiver's entry to sender's *original* local entry (which must outlive the transfer). Sender retains revocation authority via the back-pointer. Costs: a back-pointer field on every capability slot; receiver's entry depends on sender's slot lifetime (a destroyed sender either cancels the back-pointer or invalidates the receiver's entry — TBD).
- **Option C — Explicit `revoke_transferred(token)` syscall.** Sender records a transfer token at transfer time; cancel-token primitive walks every table and revokes any entry matching the token. Lightweight per-transfer; expensive at revoke time (linear scan over all tables).
- **Option D — Defer indefinitely with userspace responsibility** (what v1 implements). Userspace patterns that need post-transfer revocation must implement it in protocol (e.g., periodic re-authentication; supervisor-mediated indirection).

A real ADR-0023 will need a *Simulation* table per the [Decision outcome](#decision-outcome-not-applicable-deferred) discipline introduced by [ADR-0026](0026-idle-dispatch-fallback.md) and codified in the [write-adr skill](../../.claude/skills/write-adr/SKILL.md): walk the worst-case (sender transfers cap to receiver A → A re-derives a sub-cap to B → sender revokes original → both A's copy and B's sub-derivation must die under the chosen Option). The Simulation is what surfaces the cross-table-CDT-vs-back-pointer-vs-token tradeoff that prose alone hides.

## Decision drivers

These are the drivers a real ADR-0023 will need to weigh; recorded here so the placeholder is not just a status note:

- **Per-revoke cost.** Whole-system CDT (Option A) is O(descendants) regardless of which table they live in; back-pointer (Option B) is O(1) per cross-table edge but requires walking a per-edge list; token-revoke (Option C) is O(total capabilities) per revoke — fine for rare admin operations, bad for fine-grained delegation.
- **Per-derivation cost.** Whole-system CDT requires a global lock on the CDT for every `cap_derive` (per-task today); back-pointer adds one pointer write at transfer time; token requires no derive-time work.
- **SMP scalability.** Phase C work; but Option A's global CDT is a known SMP scaling pain point (seL4 has accumulated multiple per-core CDT optimisations); Options B and C are inherently more local.
- **Storage overhead.** Capability slot is currently 32 bytes per [ADR-0014](0014-capability-representation.md). Option A: no per-slot growth (CDT links are out-of-band). Option B: +8 bytes for back-pointer (or +4 for slot index + 4 for table index). Option C: no per-slot growth.

## Decision outcome (not applicable — Deferred)

No decision today. The ADR is a placeholder; when a trigger fires (see *Why deferred* above), a real ADR-0023 supersedes this body via the [supersede-adr skill](../../.claude/skills/supersede-adr/SKILL.md), or — preferred for placeholders — this body is rewritten in place with a Status flip from `Deferred` to `Proposed` (then `Accepted`). The Status flip is not subject to the append-only rule that protects original Accepted bodies, because a `Deferred` placeholder is not a load-bearing decision artefact.

### Simulation

Not applicable — this ADR is a Deferred placeholder; no decision to simulate. The eventual replacement ADR will need the discipline.

### Dependency chain

Not applicable while Status is `Deferred`. When the placeholder is replaced by a real ADR, that ADR's *Dependency chain* will list the implementation task(s) per [ADR-0025 §Rule 1](0025-adr-governance-amendments.md).

## Consequences

### Positive

- The in-tree references that mention ADR-0023 ([`docs/glossary.md`](../glossary.md), [`docs/roadmap/phases/phase-b.md`](../roadmap/phases/phase-b.md) §B0 ledger, [`docs/decisions/README.md`](README.md) index, the B0 closure security review) now resolve to a citable artefact instead of a 404.
- The deferral conditions are recorded at the slot itself rather than scattered across review prose, making it easier to recognise when the trigger fires.
- The "options A/B/C/D" sketch gives a future ADR author a starting analysis instead of a blank page.

### Negative

- The placeholder is itself maintenance overhead — when the project's threat model evolves, this body may need updates even before a trigger fires. *Mitigation:* the body is short and structured around the deferral conditions; substantive updates correspond to a real Status flip, not casual revisions.
- A reader scanning the ADR index for `Accepted` decisions could mistake a `Deferred` row for an oversight. *Mitigation:* the README index already shows `Deferred` in the Status column; this body's leading callout is unambiguous.

### Neutral

- The ADR slot 0023 is now consumed; it does not become available for unrelated reuse. ADR-0026's repurpose-of-T-012-reservation pattern is still available for other reserved-but-unused slots, but ADR-0023 is no longer one of them.

## Pros and cons of the options

(To be authored when a real ADR-0023 supersedes this placeholder. The four-option sketch under *What a real ADR-0023 would have to settle* above is a starting point but not a Pros / Cons treatment.)

## References

- [ADR-0014 — Capability representation](0014-capability-representation.md) — defines the per-table capability data structure this ADR's deferral rests on.
- [ADR-0016 — Kernel object storage](0016-kernel-object-storage.md) — defines the per-task arena placement.
- [ADR-0017 — IPC primitive set](0017-ipc-primitive-set.md) — defines `ipc_send` / `ipc_recv` capability transfer.
- [ADR-0026 — Idle dispatch via separate fallback slot](0026-idle-dispatch-fallback.md) — the Simulation discipline a real ADR-0023 will need to follow.
- [`docs/architecture/security-model.md`](../architecture/security-model.md) — the threat-model document where a real trigger event would surface.
- [`docs/roadmap/phases/phase-b.md`](../roadmap/phases/phase-b.md) — the phase plan that records the accept-deferred decision.
- [B0 closure security review (2026-04-27)](../analysis/reviews/security-reviews/2026-04-27-B0-closure.md) — the source artefact for the deferral.
- Klein, G., et al. *"seL4: Formal Verification of an OS Kernel."* SOSP 2009 — the whole-system CDT reference.
- Shapiro, J., Smith, J., Farber, D. *"EROS: A Fast Capability System."* SOSP 1999 — alternative capability lifecycle treatment.
