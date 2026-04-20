# T-001 — Capability table foundation

- **Phase:** A
- **Milestone:** A2 — Capability table foundation
- **Status:** Done
- **Created:** 2026-04-20
- **Author:** @cemililik
- **Dependencies:** none (Milestone A1 — Bootable skeleton — is complete)
- **Informs:** future tasks under Milestones A3 (kernel objects) and A4 (IPC primitives)
- **ADRs required:** ADR-0014 (capability representation) — **Accepted** 2026-04-20.

---

## User story

As the Umbrix kernel, I want a per-task capability table with first-class copy / derive / revoke / drop operations, so that authority is explicit and enforceable rather than implicit — eliminating the "confused deputy" class of bug by construction and providing the substrate that every future subsystem (tasks, IPC, memory, interrupts) will use to express who may do what.

## Context

[ADR-0001](../../../decisions/0001-microkernel-architecture.md) commits Umbrix to a capability-based microkernel. [Architectural principle P1](../../../standards/architectural-principles.md#p1--no-ambient-authority) makes this non-negotiable. [`security-model.md`](../../../architecture/security-model.md) describes the capability system in architectural terms: unforgeable kernel-held tokens, handle-based access from userspace, derivation trees, cascading revocation.

What does not yet exist is a concrete implementation. The kernel boots and says hello; it has no objects to protect and no callers to protect them from. T-001 is the first subsystem in the kernel beyond "hello world" and the foundation on which every later kernel subsystem will rest.

This task deliberately stops short of introducing the kernel objects capabilities refer to (that is Milestone A3) or the IPC path that transfers them (Milestone A4). The capability table must be correct and well-tested *as a data structure* before other subsystems start using it.

## Acceptance criteria

- [ ] **ADR-0014 Accepted.** Defines: in-kernel capability representation (struct layout, rights bits, object-reference encoding), handle type exposed to callers, derivation-tree storage (intrusive vs. index-based), per-task bound on table size, and the error type for operations.
- [ ] **`CapabilityTable` type** in a new `umbrix_kernel::cap` module. Bounded capacity (compile-time or per-instance), no heap allocation.
- [ ] **`Capability` type** (enum or struct with a kind field) covering the v1 placeholder variants. Concrete object references are placeholders until Milestone A3 replaces them — the point is the *table's* correctness, not the objects'.
- [ ] **Rights** (`CapRights` or similar) represented as a bitfield with the operations exposed so far: duplicate, derive, revoke, transfer-on-IPC (placeholder — no IPC yet).
- [ ] **Handle-based access.** Callers receive a `CapHandle` (opaque index); raw capability bits are never exposed.
- [ ] **Four operations** implemented:
  - `cap_copy(src, narrower_rights) -> Result<CapHandle, CapError>` — install a peer in the caller's table with the same or narrower rights.
  - `cap_derive(src, narrower_scope) -> Result<CapHandle, CapError>` — install a child capability whose scope is strictly narrower; record the parent-child relationship.
  - `cap_revoke(src) -> Result<(), CapError>` — invalidate the derivation subtree rooted at `src`.
  - `cap_drop(handle) -> Result<(), CapError>` — release a capability from the caller's table with no effect on others.
- [ ] **Move-only discipline.** The `Capability` type must not be `Copy` or `Clone`. Duplication is strictly through `cap_copy` or an explicit in-kernel duplication operation; the Rust type system enforces this.
- [ ] **Rights narrowing invariant.** `cap_copy` and `cap_derive` cannot broaden rights; a test demonstrates that attempting to widen returns an error.
- [ ] **Revocation cascade.** A test constructs a derivation tree of depth ≥ 3 and verifies that revoking a parent invalidates all descendants atomically.
- [ ] **Bounded state.** A test fills the capability table to capacity and confirms the next insert returns `CapError::CapsExhausted` rather than panicking or allocating. See [architectural-principles.md — bounded kernel state](../../../standards/architectural-principles.md) and [security-model.md — Bounded kernel resources](../../../architecture/security-model.md).
- [ ] **Documentation:** new rustdoc on every public item; no `missing_docs` warnings.
- [ ] **Tests:** unit tests in the kernel module (using `#[cfg(test)]`), plus any integration tests that need `test-hal` fakes.
- [ ] **No new `unsafe`** if achievable; if any is required, audit-log entry per [`unsafe-policy.md`](../../../standards/unsafe-policy.md).

## Out of scope

- Kernel objects that capabilities refer to (Task, Endpoint, MemoryRegion) — that is Milestone A3.
- IPC and capability transfer across tasks — Milestone A4 onward.
- Badge / discriminator semantics — separate ADR (likely in A4).
- Capability persistence across reboot — long-term future work.
- Capability table storage *in a real per-task structure* — Phase A still uses kernel-level stand-ins for tasks; the table will be wired into real tasks in Phase B.
- Revocation semantics under concurrent use across cores — single-core v1.

## Approach

Two top-level shape decisions will be pinned in ADR-0014:

1. **Storage:** intrusive doubly-linked list per derivation node vs. index-based parent/children arrays. Index-based is cache-friendlier and easier to bound statically; intrusive is more flexible. Expected outcome: index-based, following Hubris conventions.
2. **Revocation representation:** per-entry "epoch" bump vs. explicit subtree walk on revoke. Epoch is O(1) per revoke and O(1) per access (check the epoch on lookup) but needs an extra word per entry; explicit walk is O(descendants) per revoke but simpler. Expected outcome: explicit walk for v1, epoch added later if benchmarks justify.

Implementation order within the task:

1. Write ADR-0014.
2. Introduce the module skeleton: `umbrix_kernel::cap` with just the type definitions and docs.
3. Implement `cap_drop` first (simplest; touches only the table).
4. Implement `cap_copy` (narrowing of rights).
5. Implement `cap_derive` (narrowing of scope, parent/child linkage).
6. Implement `cap_revoke` (subtree walk).
7. Tests at each step; the cascade revocation test is the headline.

Every step keeps `cargo host-test` green.

## Definition of done

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo host-clippy -- -D warnings` clean.
- [ ] `cargo kernel-clippy` clean (the kernel builds for aarch64 with the new code).
- [ ] `cargo host-test` passes with the new tests; coverage-of-contract is readable from the test names.
- [ ] Any new `unsafe` has an audit entry per [`unsafe-policy.md`](../../../standards/unsafe-policy.md). Ideally none.
- [ ] Commit(s) follow [`commit-style.md`](../../../standards/commit-style.md). At minimum: ADR-0014 as one commit, implementation as one commit. Trailers `Refs: ADR-0014, ADR-0001`.
- [ ] [`../../../roadmap/current.md`](../../../roadmap/current.md) updated on status transitions (to `In Progress`, then `In Review`, then `Done`).
- [ ] Milestone A2 business review written after this task is Done, per [`conduct-review`](../../../../.claude/skills/conduct-review/SKILL.md).

## Design notes

- The per-task table capacity is a configuration. A sensible v1 default is 64 entries; it is changeable without source edits only if we expose it through the kernel's config surface (which does not yet exist — a future task). For T-001, a compile-time constant is fine and the number will be revisited.
- The `CapRights` bitfield should leave room for unknown bits. Passing an unknown bit from userspace (eventually) should be a caller-visible error, not a silent ignore.
- Derivation trees have a natural depth limit in any bounded system. The ADR should pick a policy: hard cap (e.g., 16), soft cap (warn and degrade), or unbounded with explicit per-task budget. Preference leans toward a hard cap in v1 for simplicity.
- This task deliberately does not introduce kernel-object placeholders. Using `u64` or `()` for the object reference is fine; Milestone A3 replaces them with real types.

## References

- [ADR-0001: Capability-based microkernel architecture](../../../decisions/0001-microkernel-architecture.md)
- [Architectural principle P1 — No ambient authority](../../../standards/architectural-principles.md#p1--no-ambient-authority)
- [Architecture — security model](../../../architecture/security-model.md)
- [Phase A plan](../../../roadmap/phases/phase-a.md)
- [seL4 capability paper](https://sel4.systems/)
- [Hubris task / capability model](https://hubris.oxide.computer/)

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-04-20 | @cemililik | opened; status Ready |
| 2026-04-20 | @cemililik | ADR-0014 Accepted; status → In Progress; work begins on `development` branch |
| 2026-04-20 | @cemililik | implementation landed on `development`; status → In Review. 27 new host tests green on top of the existing 34 (61/61 total). |
| 2026-04-21 | @cemililik | review-round code/doc fixes landed; CapRights masks reserved bits, `cap_drop` rejects interior nodes with `HasChildren`, `CapObject` encapsulated (`new`/`raw`), BFS uses `debug_assert` for invariants, `cap_derive` cleaned up; two new host tests added (29 kernel + 34 test-hal = 63/63 green). |
| 2026-04-21 | @cemililik | PR #1 merged to `main`; status → Done. |
