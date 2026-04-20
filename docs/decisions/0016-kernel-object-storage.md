# 0016 — Kernel object storage

- **Status:** Proposed
- **Date:** 2026-04-21
- **Deciders:** @cemililik

## Context

[T-002](../analysis/tasks/phase-a/T-002-kernel-object-storage.md) opens the kernel-object subsystem in `umbrix-kernel`. Before any implementation code lands, the concrete shape of a kernel object needs to be settled: **how it is stored, who owns it, how its lifecycle is managed, and how capabilities refer to it**. [ADR-0014](0014-capability-representation.md) deferred these questions by representing the object-reference field as an opaque `CapObject(u64)` placeholder; [Milestone A3 in phase-a.md](../roadmap/phases/phase-a.md) is the place where the placeholder gets replaced.

The decision compounds. Every Phase A subsystem that follows — IPC (A4), scheduler (A5), two-task demo (A6) — will reference kernel objects through the representation chosen here. Changing the representation once callers exist is painful: the capability system, the IPC path, and the scheduler all depend on the handle shape.

The guiding context:

- [ADR-0001](0001-microkernel-architecture.md) commits Umbrix to a capability-based microkernel — **every kernel resource is an object, every action against it goes through a capability**. There is no "kernel-global implicit state" kernel objects can live in; they must be explicit, typed, and countable.
- [ADR-0014](0014-capability-representation.md) established the shape of the capability table: per-task fixed-size arena with generation-tagged handles, `unsafe`-free. Its pattern is already audited and covered by 29 host tests.
- [`architectural-principles.md`](../standards/architectural-principles.md) mandates **bounded kernel state** — no unbounded growth, no heap surprises.
- **Single-core v1.** No cross-core concurrency yet; per-core state and atomics are deferred to Phase C.
- The WOSR-derived pattern note in [phase-a.md §A3](../roadmap/phases/phase-a.md) flags fixed-size-block per kernel-object kind as the pattern to weigh first.

T-002 introduces the minimum set of kernel-object kinds Phase A requires: `Task` (placeholder for what the scheduler will run), `Endpoint` (rendezvous target for A4), `Notification` (signal target for A4). `MemoryRegion` is deferred to Phase B, where the MMU work makes it concrete.

## Decision drivers

- **Consistency with the capability table.** The capability subsystem already uses a per-table index-based arena with generation-tagged handles and zero `unsafe`. Using the same shape for kernel objects means one audited pattern instead of two, and the audit cost of "does this storage keep its invariants?" is paid once.
- **Bounded storage, no heap.** The kernel still has no allocator. Kernel-object arenas must be fixed-size arrays inside the kernel crate.
- **Use-after-destroy prevention.** A handle whose object has been destroyed must fail lookup cleanly. The representation must detect staleness structurally, not through programmer discipline.
- **Typed handles over untyped indices.** A `TaskHandle` must not be passable where an `EndpointHandle` is expected. The type system makes this cheap; a raw index handle does not.
- **Per-type cost accounting.** Different kernel-object kinds have different sizes (`Task` carries scheduler state and eventually a register frame; `Endpoint` has an IPC queue; `Notification` is a single saturating word). A shared arena pays the cost of the largest variant for every slot — a per-type arena grows each kind independently.
- **Deterministic lifecycle.** Capabilities are transferable; many capabilities may name the same object across tasks. Tying object destruction to "last capability drop" requires reference counting and revocation-aware decrement — non-trivial, and a footgun under concurrent revocation. v1 can do better with **explicit destruction** through a kernel-internal destroy path, deferring ref-counting to a later ADR if a real use-case forces it.
- **Global ownership.** A single kernel object is reachable from many capabilities in many tasks. Arenas therefore belong to the kernel, not to a specific task's state. Single-core v1 needs no synchronization around them.
- **No `unsafe`.** Following [ADR-0014](0014-capability-representation.md)'s precedent: a subsystem this security-sensitive should stay in safe Rust wherever possible. Any `unsafe` goes through [`unsafe-policy.md`](../standards/unsafe-policy.md) audit.
- **Room to grow.** `MemoryRegion` lands in Phase B; future kinds (IrqCap, TaskCap-with-address-space, TEE-session) land in their respective phases. The representation must accept new kinds without reshaping the existing ones.

## Considered options

### Option A — Shared arena of `KernelObject` enum values

One global array of `Slot<KernelObject>` where `KernelObject` is an enum with a variant per kind. Single storage location, single `ObjectHandle` type indexing into it.

### Option B — Per-type fixed-size-block arenas with generation-tagged typed handles (chosen)

One global arena per kernel-object kind — `TaskArena`, `EndpointArena`, `NotificationArena` — each a fixed-size array of `Slot<KindSpecificFields>` with the same generation-tagged-handle pattern as [`CapabilityTable`](../../kernel/src/cap/table.rs). Each kind has its own typed handle (`TaskHandle`, `EndpointHandle`, …), so the compiler enforces that a `TaskHandle` cannot be passed where an `EndpointHandle` is expected. The `CapObject` enum wraps whichever handle its `CapKind` dictates.

### Option C — Heap-allocated per-type linked lists (intrusive)

Each kernel object carries `prev` / `next` pointers; the kernel maintains a per-kind head pointer. Flexible, textbook — but requires a heap or custom allocator the kernel does not have, and exhibits poor cache behaviour on iteration.

### Option D — Slab-ish split: raw byte arena + typed capability wrappers

A "slab" of raw bytes the kernel carves into kind-specific structures on demand, with the capability system handling the typed view separately. Powerful — allows retyping — but complex, introduces indirection, and brings seL4's untyped-memory design burden for no Phase A benefit.

### Option E — Generation-tagged shared arena with an external type tag

Like Option A, but the `Slot` stores a type discriminator separately from the inner union, avoiding the "size of the largest variant" penalty by using untyped storage plus per-kind reinterpretation. Closer to Option D in spirit; still opens type-confusion paths that safe Rust would disallow.

## Decision outcome

**Chosen: Option B — per-type fixed-size-block arenas with generation-tagged typed handles, global ownership, and explicit-destruction lifecycle.**

The decision follows directly from the drivers. Option B is **the same shape as [`CapabilityTable`](../../kernel/src/cap/table.rs), instantiated three times**: `TaskArena`, `EndpointArena`, `NotificationArena`. One audited pattern, three instances. Per-type cost accounting falls out of the design. Typed handles prevent whole classes of misuse at compile time. Lifecycle is explicit (destroy via a kernel path); reference counting is deferred to a later ADR if and when a concrete use-case demands it.

### Core types (sketch)

```rust
/// Compile-time bound per kernel-object kind. Conservatively small for
/// v1; revisit when a real use-case demands more.
pub const TASK_ARENA_CAPACITY:         usize = 16;
pub const ENDPOINT_ARENA_CAPACITY:     usize = 16;
pub const NOTIFICATION_ARENA_CAPACITY: usize = 16;

/// Typed handles. Each one is specifically not interchangeable with the
/// others; the compiler forbids passing a `TaskHandle` where an
/// `EndpointHandle` is expected.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TaskHandle { index: u16, generation: u32 }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct EndpointHandle { index: u16, generation: u32 }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct NotificationHandle { index: u16, generation: u32 }

/// Task v1 — placeholder fields; scheduler state lands in A5.
pub struct Task {
    id: u32,
    // scheduler & context fields arrive in A5
}

/// Endpoint v1 — IPC queue fields present but unwired.
pub struct Endpoint {
    // waiter queues land in A4
}

/// Notification v1 — a saturating bit-word target.
pub struct Notification {
    word: u64,
    // waiter list lands in A4
}

/// Errors from kernel-object operations. `#[non_exhaustive]` so new
/// variants (introduced as kinds land) are not breaking changes.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ObjError {
    /// The arena of this kind is full.
    ArenaFull,
    /// The handle is stale (object destroyed or slot reused).
    InvalidHandle,
    /// Destruction refused because at least one capability still names
    /// the object. The caller must cap_revoke the subtree or cap_drop
    /// every copy first.
    StillReachable,
}
```

### `CapObject` rewiring

`CapObject` transitions from an opaque `u64` to a typed enum paralleling `CapKind`:

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CapObject {
    Task(TaskHandle),
    Endpoint(EndpointHandle),
    Notification(NotificationHandle),
    // MemoryRegion arrives in Phase B
}

impl CapObject {
    pub const fn kind(self) -> CapKind { /* pattern-match */ }
}
```

The outer API of [`CapabilityTable`](../../kernel/src/cap/table.rs) does not change — [ADR-0014](0014-capability-representation.md) reserved room for this migration, and it is delivered here additively.

### Lifecycle

- **Creation.** `create_task(args) -> Result<TaskHandle, ObjError>` — allocates a slot, fills it, returns the handle. A companion kernel path installs the initial capability in the creator's `CapabilityTable`.
- **Destruction.** `destroy_task(handle) -> Result<(), ObjError>`:
  - If any capability still names this object (reachability query), return `ObjError::StillReachable`. v1's reachability check is a simple "is there any live `CapObject` in any known `CapabilityTable` that points at this handle?" — an acceptable O(n) cost at Phase A's scale, replaceable by a per-object refcount in a later ADR if it earns its place.
  - Otherwise, free the slot (bump the generation, return it to the arena's free list). All stale handles fail their generation check on next lookup.
- **No drop-on-last-cap-drop in v1.** Capability drops / revocations do not reach into kernel-object arenas; the arenas are separately managed. A future ADR may couple them, but v1 keeps the two concerns distinct.

### Global state

Per-type arenas are kernel-global singletons (conceptually; the actual kernel struct that owns them is a parameter passed through the call stack so that host tests can construct an isolated instance). Single-core v1 accesses them without synchronization.

### Why Option B over alternatives

- **Over Option A (shared enum arena):** avoids "size of largest variant" tax. More importantly, keeps typed handles at compile-time level; Option A erases the kind into an enum tag that the compiler does not check at handle use.
- **Over Option C (intrusive linked list):** the kernel has no heap yet and will not have one for months. Arena-based bounded storage matches current capability in both senses.
- **Over Option D (slab + capability wrappers):** slab storage is strictly more powerful but strictly more complex. seL4's untyped-memory model gives retyping freedom at the cost of an entire security story around untyped-to-typed transitions. Umbrix does not need retyping in v1 and would pay complexity for an unused feature.
- **Over Option E (shared arena + external type tag):** trades the type-safety Option B gets from typed handles for a runtime discriminator. If the discriminator is ever wrong, the caller reinterprets bytes as the wrong type. Option B's compile-time separation is strictly safer at equivalent space cost.

## Consequences

### Positive

- **One audited pattern.** The per-type arena shape is already audited once (in [`CapabilityTable`](../../kernel/src/cap/table.rs)). A3 adds three instances of a known pattern rather than a new one.
- **Typed handles at compile time.** A `TaskHandle` is not substitutable for an `EndpointHandle`. Whole categories of "passed the wrong object" bug become compile errors.
- **Use-after-destroy is structurally impossible.** Generation counter on every arena slot, same as the capability table.
- **Zero `unsafe`.** The arenas are plain safe Rust, following the capability-table model.
- **Deterministic lifecycle.** No reference counting; destruction is an explicit kernel operation with a typed error.
- **Independent per-kind bounds.** Growing `Task` doesn't force `Endpoint` to grow.
- **Cap-wiring is additive.** [ADR-0014](0014-capability-representation.md)'s outer API stays stable; `CapObject` becomes typed without breaking callers.

### Negative

- **Compile-time per-kind bounds.** Raising `TASK_ARENA_CAPACITY` requires a rebuild. v1 accepts this — the number lives in one place and is revisited when a real deployment asks for more. No worse than `CAP_TABLE_CAPACITY`.
- **Three near-identical modules.** `task_arena.rs`, `endpoint_arena.rs`, `notification_arena.rs` share shape; three nearly-identical implementations risk drift. Mitigation: factor the arena into a generic `Arena<T, const N: usize>` shared by all three kinds, parameterised over the slot's payload type. This earns the consistency without the copy-paste.
- **Reachability check on destroy is O(n).** v1 scans every live capability to decide whether destruction is safe. At Phase A's scale (one `CapabilityTable` of 64 slots per task, two tasks in the A6 demo) this is trivial. When the system has hundreds of tasks, a per-object reference count replaces the scan — to be decided by a later ADR.
- **No reclamation on capability drop.** A kernel object survives until explicitly destroyed, even if every capability to it is gone. For v1 this is acceptable; a "reclaim on unreachable" operation can be added without changing the outer API.
- **Generic-over-const-generic arena requires one `unsafe`-free pattern we haven't yet written.** Known-solvable in stable Rust; expect one iteration during T-002.

### Neutral

- **`MemoryRegion` is absent here.** Phase B lands it when the MMU work does. The representation is additive — a new `CapKind::MemoryRegion` variant and a new arena module, no change to the existing three.
- **Task / Endpoint / Notification are v1 skeletons.** They carry the fields A3 needs and leave room for A4 / A5 additions. Not every field each kind will eventually carry exists at A3 close.
- **Handle size.** `{u16 index, u32 generation}` = 8 bytes (with padding). Same as `CapHandle`; consistency.

## Pros and cons of the options

### Option A — Shared `KernelObject` enum arena

- Pro: one storage location; simpler mental model.
- Pro: one `ObjectHandle` type; uniform capability-to-object lookup.
- Con: every slot is the size of the largest variant — `Task` dominates, so `Endpoint` and `Notification` slots waste space on padding.
- Con: loss of compile-time kind separation; runtime match on every access.
- Con: harder to compute per-kind bounds; exhaustion of one kind starves the others.

### Option B — Per-type arenas with typed handles (chosen)

- Pro: mirrors the audited capability-table pattern; one shape, three instances.
- Pro: typed handles are compile-time guarantees.
- Pro: per-kind storage is right-sized.
- Pro: zero `unsafe`, no heap.
- Con: three arenas to initialise, test, and evolve — mitigated by a shared generic arena implementation.
- Con: exhaustion is per-kind — arguably a feature (you know which kind ran out), arguably a cost (you can't borrow a free slot from another kind).

### Option C — Intrusive linked lists per kind

- Pro: dynamic size per kind; no compile-time bound.
- Pro: textbook kernel pattern.
- Con: requires a heap or custom allocator the kernel does not have.
- Con: cache-unfriendly iteration.
- Con: pointer soup; harder to audit.

### Option D — Slab-ish retyping store

- Pro: maximum flexibility; enables future "untyped memory → typed kernel object" operations without rework.
- Con: massive complexity for v1; introduces an entire sub-story around retyping invariants that Umbrix does not need yet.
- Con: the security model for retyping is non-trivial (seL4's is formally verified; we would not match that in v1).

### Option E — Shared arena + external type tag

- Pro: avoids the "size of largest variant" cost of Option A.
- Con: hands safety to a runtime type tag instead of to the type system; a single wrong discriminator produces a type-confusion bug the compiler cannot catch.
- Con: adds complexity without matching Option B's compile-time guarantees.

## Open questions

- **Generic arena shape.** Whether the three per-kind arenas are three copies of the pattern or one `Arena<T, const N: usize>` instantiated three times. Preference: the generic — decided during T-002 implementation.
- **Per-object refcount for destruction safety.** v1 uses an O(n) reachability scan. When measurement shows this cost in a realistic workload (probably around F or G), a successor ADR introduces refcounts behind the existing outer API.
- **Task ownership of kernel objects.** Phase B adds the "a Task owns an AddressSpace, which owns MemoryRegion caps" chain. That ADR decides whether an object can be arbitrarily transferred between tasks or is tied to a creator — out of scope for A3.
- **Initial-capability install.** Creating a kernel object produces a capability; *which* `CapabilityTable` that capability lands in (the creator's, a specified target's, or the kernel's boot-task) is a Phase-A detail T-002 picks for v1 and a future ADR formalises if it changes.
- **Cross-core safety.** Single-core v1 uses no atomics or locks. Phase C's multi-core ADR extends this — likely per-CPU arenas with a stealing policy, or a global arena with a spinlock depending on contention.

## References

- [ADR-0001 — Capability-based microkernel architecture](0001-microkernel-architecture.md).
- [ADR-0014 — Capability representation](0014-capability-representation.md).
- [ADR-0013 — Roadmap and planning process](0013-roadmap-and-planning.md).
- [architectural-principles.md](../standards/architectural-principles.md) — bounded kernel state, no ambient authority.
- [security-model.md](../architecture/security-model.md) — kernel objects as the unit of authority.
- [`kernel/src/cap/table.rs`](../../kernel/src/cap/table.rs) — existing audited per-type-arena pattern to mirror.
- [T-002 — Kernel object storage foundation](../analysis/tasks/phase-a/T-002-kernel-object-storage.md) — the implementing task.
- seL4 kernel-object model — https://sel4.systems/ (prior art; retyping not adopted here).
- Hubris kernel task/cell storage — https://hubris.oxide.computer/ (direct shape parallel: compile-time-bounded per-type arenas).
