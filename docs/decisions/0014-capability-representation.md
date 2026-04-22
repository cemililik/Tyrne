# 0014 — Capability representation

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

[T-001](../analysis/tasks/phase-a/T-001-capability-table-foundation.md) opens the capability table subsystem in `tyrne-kernel`. Before any implementation code lands, the concrete shape of a capability needs to be settled: how it is stored, how callers refer to it (handles), how the derivation tree is tracked, how revocation works, what "rights" means, what "scope" means for v1, and what errors operations can produce.

These decisions compound. Every future kernel subsystem ([Milestone A3 — kernel objects](../roadmap/phases/phase-a.md), [A4 — IPC](../roadmap/phases/phase-a.md), later memory and scheduling) will build on this representation, and changing the representation after it has callers is painful. Hence a dedicated ADR before T-001's code.

The guiding context:

- [ADR-0001](0001-microkernel-architecture.md) commits Tyrne to a capability-based microkernel.
- [architectural-principles.md P1](../standards/architectural-principles.md#p1--no-ambient-authority) forbids ambient authority.
- [security-model.md](../architecture/security-model.md) describes the capability system at the architectural level.
- T-001 is **single-core, no-heap, no-userspace** — concurrency and userspace ABI concerns are explicitly deferred.

## Decision drivers

- **Move-only discipline, enforced by Rust.** A `Capability` is not `Copy`, not `Clone`. Duplication is an explicit operation. The compiler enforces this.
- **Handle-based access.** Callers reference capabilities by an opaque `CapHandle`. Raw capability bits are never exposed.
- **Use-after-revoke prevention.** A handle whose slot has been freed and reused must fail lookup cleanly. The representation must detect staleness, not silently alias.
- **Bounded storage, no heap.** A per-task capability table of fixed capacity, allocated inside whatever struct holds it (eventually the `Task` kernel object). Exhaustion is a typed error.
- **Cascading revocation.** Revoking a capability invalidates its entire derivation subtree. The representation must record parent / children relationships well enough to walk the subtree in bounded time.
- **Safe Rust.** No `unsafe` in the capability implementation if it can be avoided — this subsystem is too security-sensitive to accept avoidable `unsafe`.
- **Cache-friendly layout.** All slots live in a single array; derivation links are indices into that array, not allocated pointers. Revocation walk touches contiguous memory.
- **Placeholder object references for v1.** Until [Milestone A3](../roadmap/phases/phase-a.md) introduces real kernel objects, the variant-specific object reference is a placeholder `u64`. This ADR keeps the door open for a typed replacement without a breaking change.
- **Single-core v1.** Concurrent access across cores is a later problem; the representation does not yet need atomics or a mutex.

## Considered options

### Option A — intrusive doubly-linked list on `Capability` itself

Each `Capability` carries `prev` / `next` pointers; the list lives in a separately allocated pool (`Box<Capability>` or a custom allocator). Flexible and textbook.

### Option B — index-based arena with generation-tagged handles

A fixed-size array of `Slot`s per capability table. Each slot holds either a `Capability` + derivation-tree indices (parent, first_child, next_sibling) or nothing. A `CapHandle` is `(index, generation)`; looking it up checks the generation to detect staleness. Revocation walks the subtree and frees slots, bumping each slot's generation.

### Option C — flat table with parent-pointer only, revocation by full-table scan

Same slot array as B, but only a `parent` index — no sibling / child links. Cascading revocation walks the whole table looking for descendants. Simpler data structure; worse revocation complexity.

### Option D — epoch-based lookup invalidation

Each slot has an `epoch` field. Revocation bumps the epoch of every slot in the subtree; every lookup checks the epoch. Subsumes Option B's generation check. Adds 8 bytes per slot for something single-core v1 does not benefit from beyond the generation check.

## Decision outcome

**Chosen: Option B — index-based arena with generation-tagged handles and explicit derivation-tree walks.**

### Core types

```rust
/// Compile-time bound on the per-task capability table.
/// Revisit when a real use-case demands more; 64 is a defensible v1 default.
pub const CAP_TABLE_CAPACITY: usize = 64;

/// Hard cap on derivation-tree depth per root, preventing pathological trees
/// from consuming the entire table through a single derivation chain.
pub const MAX_DERIVATION_DEPTH: usize = 16;

/// Opaque handle. `(index, generation)` — the generation is checked on
/// every lookup and detects use-after-revoke.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CapHandle {
    index: u16,
    generation: u32,
}

/// Bitfield of rights a capability confers. Narrowing-only: cap_copy and
/// cap_derive may drop rights, never add them.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct CapRights(u32);

impl CapRights {
    pub const EMPTY: Self = Self(0);
    pub const DUPLICATE: Self = Self(1 << 0); // may be cap_copy-ed
    pub const DERIVE:    Self = Self(1 << 1); // may be cap_derive-d from
    pub const REVOKE:    Self = Self(1 << 2); // may be cap_revoke-d (subtree)
    pub const TRANSFER:  Self = Self(1 << 3); // may be moved via IPC (placeholder; A4)
    // further bits reserved for future ADRs (read/write/execute on
    // MemoryRegionCap once kernel objects land)
}

/// Placeholder for the object the capability points at. Milestone A3
/// replaces this with a typed enum over real kernel objects. The inner
/// identifier is kept private; callers construct through `CapObject::new`
/// and read through `CapObject::raw`, so every touch site is auditable
/// when the typed replacement lands.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CapObject(u64);

impl CapObject {
    pub const fn new(id: u64) -> Self { Self(id) }
    pub const fn raw(self) -> u64 { self.0 }
}

/// The kinds v1 supports — all currently stubs pointing at `CapObject`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CapKind {
    Task,
    Endpoint,
    Notification,
    MemoryRegion,
}

/// A capability. **Move-only** — neither `Copy` nor `Clone`. Duplication
/// only through `cap_copy`, which consumes an input with `DUPLICATE` rights.
pub struct Capability {
    kind: CapKind,
    rights: CapRights,
    object: CapObject,
}

/// Errors from capability-table operations. `#[non_exhaustive]` so new
/// variants (added by future ADRs) are not breaking changes.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CapError {
    /// The table is full; no free slot.
    CapsExhausted,
    /// Handle points to a free or stale slot.
    InvalidHandle,
    /// Attempt to broaden rights on copy or derive.
    WidenedRights,
    /// Caller's rights on the source cap do not include the needed authority
    /// (DUPLICATE / DERIVE / REVOKE).
    InsufficientRights,
    /// Derivation would exceed MAX_DERIVATION_DEPTH.
    DerivationTooDeep,
}
```

### Slot layout

```rust
struct Slot {
    /// `None` when the slot is free.
    entry: Option<SlotEntry>,
    /// Bumped on every free. Makes stale handles detectable.
    generation: u32,
    /// Free-list link when `entry` is `None`; unused otherwise.
    next_free: Option<u16>,
}

struct SlotEntry {
    capability: Capability,
    parent: Option<u16>,
    first_child: Option<u16>,
    next_sibling: Option<u16>,
    depth: u8, // 0 for a root; parent.depth + 1 otherwise
}
```

### Table layout

```rust
pub struct CapabilityTable {
    slots: [Slot; CAP_TABLE_CAPACITY],
    free_head: Option<u16>,
}

impl CapabilityTable {
    pub const fn new() -> Self { … }
    pub fn insert_root(&mut self, cap: Capability) -> Result<CapHandle, CapError>;
    pub fn cap_copy(&mut self, src: CapHandle, new_rights: CapRights) -> Result<CapHandle, CapError>;
    pub fn cap_derive(&mut self, src: CapHandle, new_rights: CapRights, new_object: CapObject) -> Result<CapHandle, CapError>;
    pub fn cap_revoke(&mut self, src: CapHandle) -> Result<(), CapError>;
    pub fn cap_drop(&mut self, handle: CapHandle) -> Result<(), CapError>;
    // Internal lookup returning a &SlotEntry with a generation check.
}
```

### Revocation

Explicit subtree walk, iterative using a small local stack buffer sized for `MAX_DERIVATION_DEPTH`. Each visited slot is freed: `entry` becomes `None`, `generation` is bumped (wrapping), the slot is pushed onto the free list. Existing `CapHandle`s to any freed slot will fail their generation check on next lookup.

### Narrowing invariants

- `cap_copy(src, new_rights)`: `new_rights ⊆ src.rights` (enforced by `CapRights::contains`).
- `cap_derive(src, new_rights, new_object)`: same rights rule; `new_object` is opaque for v1 — per-variant scope narrowing arrives in A3 when real kernel objects land.

### Why option B over alternatives

- **Over Option A (intrusive list):** avoids a separate allocator or heap. The kernel's capability system is a bounded-memory construct by design.
- **Over Option C (parent-only, O(n) revoke):** worst-case revocation is O(n) in the table size; with 64 entries that is cheap today but degrades poorly if the table grows. The sibling / child pointers cost two `Option<u16>` per slot (4 bytes) — a good trade.
- **Over Option D (epoch-based):** generations already cover use-after-revoke cleanly; epochs add complexity to cover concurrent revocation, which v1 does not have. Option D becomes attractive in a multi-core world; we can migrate without a breaking change by adding epoch checks behind the existing `CapHandle` façade.

## Consequences

### Positive

- **Zero `unsafe`** in the capability core. The subsystem whose correctness matters most stays in safe Rust.
- **Bounded memory.** The table is a fixed-size array inside whatever owns it.
- **Use-after-revoke is structurally impossible.** A stale handle fails its generation check on lookup.
- **Cache-friendly.** All slots in contiguous memory; revocation walks the array, not heap-scattered nodes.
- **Derivation depth hard-capped.** `MAX_DERIVATION_DEPTH = 16` prevents pathological trees.
- **Simple testing.** Pure data structure; host tests cover the whole surface.

### Negative

- **Compile-time table size.** Changing `CAP_TABLE_CAPACITY` requires a rebuild. Mitigation: the constant is exposed and revisited when a real use-case demands more; for v1 `64` is adequate.
- **Generation overflow.** A `u32` generation counter wraps after ~4 × 10⁹ free-reuse cycles of the same slot. v1 does *not* implement a slot-poisoning mechanism — the current `Slot` layout (`entry: Option<SlotEntry>`, `generation: u32`, `next_free`) has no dedicated poison indicator, and `free_slot` wraps the generation without checking for overflow. Mitigation is therefore deferred: a v2 addition will either add a sentinel `generation == u32::MAX` convention (with `pop_free` and the allocation path skipping any slot whose next-bump would hit the sentinel), or introduce an explicit `poisoned: bool` field. Either way the contract is "once a slot can no longer produce a fresh generation, it is removed from future allocation." If in practice the overflow becomes reachable in any realistic workload, an earlier ADR raises the generation to `u64` instead.
- **Hard depth cap (16).** Some future subsystem may want deeper. At that point, the cap is loosened in a follow-up ADR with a stated reason.
- **Three tree-link pointers per slot (`parent`, `first_child`, `next_sibling`) add 2 bytes each as `Option<u16>`.** Per-slot overhead sums to roughly 32 bytes including the rest of `SlotEntry`; sixty-four slots per task land near 2 KiB. Acceptable for v1.

### Neutral

- **Placeholder `CapObject(u64)`.** Migration to real typed objects in A3 is additive — the outer API does not change. New `CapKind` variants in A3 get their own object types behind the opaque wrapper.
- **`CapRights` reserves bits** for future rights (memory read/write/execute, future protocol-specific rights). The bitfield is `u32`, which is ample.

## Pros and cons of the options

### Option A — intrusive doubly-linked list

- Pro: classic, flexible.
- Pro: natural `Rc`/`Arc` / custom-allocator fit.
- Con: requires a heap or a custom allocator the kernel does not yet have.
- Con: pointer chasing on revocation; worse cache behaviour than arena.

### Option B — index-based arena, generation-tagged handles (chosen)

- Pro: no heap; bounded memory.
- Pro: `unsafe`-free.
- Pro: stale-handle detection for free.
- Pro: good cache locality.
- Con: compile-time size; per-slot bookkeeping adds a few bytes.

### Option C — parent-only, full-table scan on revoke

- Pro: smaller per-slot overhead.
- Con: revocation is O(n); a 64-slot table is cheap but a larger future table is not.
- Con: sibling / child queries are linear scans, bad for diagnostics.

### Option D — epoch-based lookup invalidation

- Pro: cleaner semantics under concurrent revocation.
- Con: 8 bytes per slot of overhead for a benefit v1 cannot use.
- Con: migration from Option B to Option D is additive, so we start simple and add epochs when they earn their place.

## Open questions

- **Per-task capability-table allocation.** Currently the table is a struct the kernel allocates wherever. When [Milestone A3](../roadmap/phases/phase-a.md) introduces a `Task` kernel object, the table becomes one of its fields. How task lifetimes compose with the table's lifetime is decided there, not here.
- **Multi-core safety.** Single-core v1 does not need atomics on the table. [Phase C](../roadmap/phases/phase-c.md) will extend this (likely per-table mutex or per-core tables; ADR when we get there).
- **Badge semantics.** Per-derivation discriminators for IPC endpoints are out of scope here; their bits, if any, will live inside `CapObject` or a sibling field per a future ADR (to be written in [A4](../roadmap/phases/phase-a.md) — see the A4 ledger in phase-a.md for the reserved number).
- **Rights inheritance under parent-revoke.** Today a parent-revoke nukes descendants cascadingly. If a future subsystem wants "narrow parent rights without nuking children," that is a new operation with its own ADR.
- **Serialization / persistence.** Capabilities do not persist across reboots in v1. Future work.
- **Raising `CAP_TABLE_CAPACITY` and `MAX_DERIVATION_DEPTH`.** Both are revisited when a concrete use-case demands more. For now, both are `const` and documented.
- **Adopting the `bitflags` crate.** `CapRights` is hand-rolled to keep the kernel dependency-free. [ADR-0009](0009-mmu-trait.md) has the same open question for `MappingFlags`; both may migrate together in a future ADR.

## References

- [ADR-0001 — Capability-based microkernel architecture](0001-microkernel-architecture.md).
- [ADR-0013 — Roadmap and planning process](0013-roadmap-and-planning.md).
- [architectural-principles.md P1](../standards/architectural-principles.md#p1--no-ambient-authority).
- [security-model.md](../architecture/security-model.md) — capability system at the architecture level.
- [T-001 — Capability table foundation](../analysis/tasks/phase-a/T-001-capability-table-foundation.md).
- seL4 capability model — https://sel4.systems/ (prior art for derivation trees and revocation).
- Hubris capability design — https://hubris.oxide.computer/ (prior art for compile-time-bounded kernel-object tables).
- *EROS: A Fast Capability System* (Shapiro et al., 1999) — prior art for handle-based access and revocation semantics.
