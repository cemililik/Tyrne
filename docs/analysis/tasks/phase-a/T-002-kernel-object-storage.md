# T-002 — Kernel object storage foundation

- **Phase:** A
- **Milestone:** A3 — Kernel objects
- **Status:** In Review
- **Created:** 2026-04-21
- **Author:** @cemililik
- **Dependencies:** T-001 — Capability table foundation (must reach `Done`; A3 is blocked on A2 closure per [phase-a.md](../../../roadmap/phases/phase-a.md))
- **Informs:** future tasks under Milestone A4 (IPC primitives) — IPC needs `Endpoint` and `Notification` kernel objects to dispatch against.
- **ADRs required:** ADR-0016 (kernel object storage) — **Accepted** 2026-04-21.

---

## User story

As the Umbrix kernel, I want typed kernel-object arenas (`Task`, `Endpoint`, `Notification`) reachable from the capability table, so that capabilities point at real in-kernel entities with well-defined lifecycle — replacing the opaque `CapObject(u64)` placeholder from [ADR-0014](../../../decisions/0014-capability-representation.md) with structure that the IPC path (Milestone A4) can dispatch against.

## Context

[T-001](T-001-capability-table-foundation.md) landed the capability table but left `CapObject` as a placeholder `u64`. [ADR-0014](../../../decisions/0014-capability-representation.md) explicitly defers the "what does a capability point at" question to A3. Until kernel objects exist, a capability asserts authority over *nothing concrete*: every later subsystem — IPC endpoints, notifications, tasks, memory regions — needs an object to name.

T-002 introduces the minimum set of kernel-object kinds Phase A requires: `Task` (so future scheduler work has something to schedule), `Endpoint` (so A4's IPC has a rendezvous point), `Notification` (so A4's async signalling has a target). Memory regions are deferred to Phase B's MMU work.

This task is **structure and lifecycle only**, not behaviour. The scheduler (A5), the context switch (A5), and the IPC send/recv paths (A4) all build on the storage A3 introduces, but do not land inside A3.

## Acceptance criteria

- [x] **ADR-0016 Accepted.** Settles storage strategy (per-type arena vs. shared pool), handle type (generation-tagged or equivalent), ownership (global vs. per-task), and explicit-destruction semantics.
- [ ] **Kernel-object module** `umbrix_kernel::obj` (or equivalent) with three types: `Task`, `Endpoint`, `Notification`. Minimal fields per ADR-0016; no scheduler / IPC logic.
- [ ] **Per-type arenas** with bounded capacity (compile-time constants, revisited when a real use case demands more).
- [ ] **Handle types** (`TaskHandle`, `EndpointHandle`, `NotificationHandle`) or a unified typed handle — whichever ADR-0016 chooses. Use-after-destroy structurally impossible (generation check, typed arena, or equivalent).
- [ ] **`CapObject` wiring.** The placeholder `CapObject` becomes a typed reference to a kernel object (enum over the three kinds, carrying the appropriate handle). [ADR-0014](../../../decisions/0014-capability-representation.md) said the outer API of the capability table does not change; this task verifies that claim holds.
- [ ] **Create / destroy APIs.**
  - `create_task(initial_state) -> Result<TaskHandle, ObjError>`
  - `destroy_task(handle) -> Result<(), ObjError>` (symmetric; fails if already destroyed or if any capability still names it — exact policy per ADR-0016).
  - Symmetric pair for `Endpoint` and `Notification`.
- [ ] **Capability flow.** Creating a kernel object produces the capability that names it (with all v1 rights on that kind); dropping the last such capability's behaviour is per ADR-0016 (explicit destruction vs. reference-counted).
- [ ] **Host tests** covering: create / lookup / destroy happy path; handle invalidation after destroy; arena exhaustion returns typed error; capability-to-object lookup resolves correctly; dropping capabilities vs. destroying objects produces the ADR-0016-documented outcome.
- [ ] **No new `unsafe`** if achievable. If any lands, an audit entry per [`unsafe-policy.md`](../../../standards/unsafe-policy.md).
- [ ] **Move-only where correctness demands.** Kernel-object types that encode ownership (e.g., "the endpoint's blocked-sender list") should not be `Copy` / `Clone`; the compiler enforces the ownership story.

## Out of scope

- Scheduler interaction with `Task` (A5).
- IPC send / recv against `Endpoint` and `Notification` (A4).
- Capability transfer through IPC (A4).
- `MemoryRegion` kernel object (Phase B — lands with the MMU).
- Per-task ownership of kernel objects (Phase B introduces the "task owns an address space owns memory-region caps" chain).
- Reference counting on the hot path if ADR-0016 picks explicit destruction.

## Approach

Sketch; real design in ADR-0016.

1. **Storage.** Per-type fixed-size-block arena — one array of `Task`, one of `Endpoint`, one of `Notification`, each with index-based generation-tagged handles mirroring the [`CapabilityTable`](../../../kernel/src/cap/table.rs) pattern from T-001. Consistency with the already-audited capability table is valuable: one shape, three instances. A shared pool alternative is weighed in the ADR and should be rejected unless it earns its place.
2. **Ownership.** Arenas are global (kernel-scope), not per-task, because a single kernel object is reachable from many capabilities across many tasks. Single-core v1 needs no synchronization.
3. **Lifecycle.** Explicit destruction via a `Destroy`-righted capability (or a dedicated kernel path). The cascade "last capability drop destroys the object" is seductive but introduces reference counting that v1 does not need; defer until a concrete use case forces it.
4. **CapObject wiring.** `CapObject` becomes an enum variant per `CapKind`, each carrying its typed handle:
   ```rust
   enum CapObject {
       Task(TaskHandle),
       Endpoint(EndpointHandle),
       Notification(NotificationHandle),
       // MemoryRegion lands in Phase B
   }
   ```
   [ADR-0014](../../../decisions/0014-capability-representation.md) reserved room for this migration; A3 makes it real.

## Definition of done

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo host-clippy` clean.
- [ ] `cargo kernel-clippy` clean.
- [ ] `cargo host-test` passes with the new tests.
- [ ] No new `unsafe` without an audit entry.
- [ ] Commit(s) follow [`commit-style.md`](../../../standards/commit-style.md); at minimum: ADR-0016 as one commit, `umbrix_kernel::obj` module as one commit, `CapObject` wiring as one commit.
- [ ] [`current.md`](../../../roadmap/current.md) updated on each status transition.
- [ ] Milestone A3 is **not** closed by this task alone — A3 closes when subsequent work (if any) lands. Current plan: T-002 covers A3 in one task.

## Design notes

- **Why not a shared `KernelObject` enum arena?** A single enum carries the size of its largest variant for every slot; `Task` is likely to grow faster than `Endpoint` (scheduler state, context frame). Per-type arenas let each grow independently. The consistency of "one arena shape, three instances" also lowers audit cost.
- **Why explicit destroy instead of drop-on-last-ref?** Reference counting on capabilities needs a revoke-aware decrement path that is non-trivial to get right under concurrent use (multi-core Phase C). Explicit destruction is deterministic, auditable, and can be extended to ref-counted later behind the existing outer API if warranted.
- **Sizing.** v1 arenas pick conservative compile-time bounds — `16` each is enough for Phase A smoke tests. Revisit when A4 / A5 / A6 have real numbers.
- **Handle safety.** Typed handles prevent the most common misuse: passing a `TaskHandle` where an `EndpointHandle` is expected. The compile-time distinction is cheap and catches entire classes of bug.

## References

- [ADR-0014: Capability representation](../../../decisions/0014-capability-representation.md)
- [ADR-0016: Kernel object storage](../../../decisions/0016-kernel-object-storage.md) *(Proposed — to be Accepted before code lands)*
- [Phase A plan](../../../roadmap/phases/phase-a.md)
- [seL4 kernel-object model](https://sel4.systems/) — per-kind kernel-object design, untyped-to-typed retyping (not adopted here; referenced for comparison).
- [Hubris task/cell storage](https://hubris.oxide.computer/) — compile-time-bounded per-type arenas; direct shape parallel.

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-04-21 | @cemililik | opened; status Draft (A3 blocked until A2 Done) |
| 2026-04-21 | @cemililik | A2 Done + A2 business review committed; status → Ready. ADR-0016 Accepted the same day; implementation may begin. |
| 2026-04-21 | @cemililik | implementation landed on `development`; status → In Review. `umbrix_kernel::obj` module (generic `Arena<T, N>`, `Task`/`Endpoint`/`Notification` + typed handles + create/destroy APIs); `CapObject` rewired to a typed enum; `Capability` loses its redundant `kind` field (derived from object). 14 new host tests on top of the 63 T-001 baseline (77/77 total). |
