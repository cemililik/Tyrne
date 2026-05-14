# 0028 — Address-space data structure (B3 — kernel-object + capability-gated `Mmu::map` wrappers + activation-on-context-switch)

- **Status:** Accepted
- **Date:** 2026-05-11
- **Deciders:** @cemililik

## Context

Phase B3's milestone goal is **address-space abstraction**: per-task translation tables, capability-gated [`Mmu::map`](../../hal/src/mmu/mod.rs) / [`Mmu::unmap`](../../hal/src/mmu/mod.rs) wrappers, and activation-on-context-switch wiring. [Phase-b.md §B3](../roadmap/phases/phase-b.md#milestone-b3--address-space-abstraction) items 2–7 enumerate the unblockers. Item 1 (the Physical Memory Manager prerequisite) was settled by [ADR-0035](0035-physical-memory-manager.md) and shipped by [T-017](../analysis/tasks/phase-b/T-017-physical-memory-manager.md). Item 2 is the design ADR for the address-space data structure itself; this ADR settles item 2 and forward-stages items 3–7 into the implementation task ([T-018](../analysis/tasks/phase-b/T-018-address-space-kernel-object.md), opened `Draft` in the same commit).

The kernel needs a typed object that:

1. **Owns the root translation-table frame** that [`Mmu::create_address_space`](../../hal/src/mmu/mod.rs) constructed from a [`PhysFrame`](../../hal/src/mmu/mod.rs).
2. **Lives in an arena alongside [`TaskHandle`](../../kernel/src/obj/task.rs) / [`EndpointHandle`](../../kernel/src/obj/endpoint.rs) / [`NotificationHandle`](../../kernel/src/obj/notification.rs)** so capabilities (per [ADR-0014](0014-capability-representation.md)) can grant access to it the same way they grant access to other kernel objects.
3. **Plugs into the [`Mmu`](../../hal/src/mmu/mod.rs) trait's [`AddressSpace`](../../hal/src/mmu/mod.rs) associated type** — the BSP defines what an `AddressSpace` *contains* (page-table topology), the kernel defines what an `AddressSpace` *is* as a kernel-object (capability-bearing arena slot with a generation tag).
4. **Activates on context switch** — when [`yield_now`](../../kernel/src/sched/mod.rs) dispatches a task whose `AddressSpace` differs from the outgoing task's, the kernel invokes [`Mmu::activate`](../../hal/src/mmu/mod.rs) before the architectural context-switch instruction sequence.

The HAL trait surface already crosses the BSP boundary at five `Mmu::AddressSpace`-related call sites: [`Mmu::create_address_space`](../../hal/src/mmu/mod.rs#L325) (constructs an `Mmu::AddressSpace` value from a `PhysFrame`), [`Mmu::address_space_root`](../../hal/src/mmu/mod.rs#L328) (takes `&Mmu::AddressSpace`, returns the root frame), [`Mmu::activate`](../../hal/src/mmu/mod.rs#L331) (takes `&Mmu::AddressSpace`), and [`Mmu::map`](../../hal/src/mmu/mod.rs#L354) / [`Mmu::unmap`](../../hal/src/mmu/mod.rs#L375) (take `&mut Mmu::AddressSpace`; post-T-016 with [`MapperFlush`](../../hal/src/mmu/mod.rs)). The associated type is constrained only by `Send` — its inline shape is BSP-defined. The kernel must decide **how to wrap this associated type into a typed kernel-object** that participates in the capability system without leaking BSP details into capability-handling code.

The decision is load-bearing for the rest of B3 (items 3–7 in [phase-b.md §B3](../roadmap/phases/phase-b.md#milestone-b3--address-space-abstraction)) and for the future ADR-0033 (high-half kernel migration; placeholder per [ADR-0027 §Decision outcome](0027-kernel-virtual-memory-layout.md#decision-outcome)). The shape chosen here is the **first kernel object the BSP and the kernel share by type** (Endpoint / Notification / Task have no BSP-side analogue), so the precedent extends to future BSP-tied kernel objects (e.g., a hypothetical InterruptCap that wraps a GIC-specific [`IrqController`](../../hal/src/irq_controller.rs) per-line state).

The constraints v1 imposes:

1. **Capability-gated.** Every `Mmu::map` / `Mmu::unmap` / `Mmu::activate` call site must resolve a capability first. No ambient authority — the kernel never holds an `&mut AddressSpace` without a capability invocation that minted it. (Per CLAUDE.md non-negotiable #1.)
2. **Generic over `M: Mmu`.** The kernel is already generic over `M: Mmu` at the scheduler surface (per [ADR-0019](0019-scheduler-shape.md) / [ADR-0020](0020-cpu-trait-v2-context-switch.md)); extending that to the address-space object is consistent rather than novel.
3. **No allocation during context switch.** Activation on the hottest path must be a pointer dereference + `Mmu::activate` call — no arena resize, no capability resolution beyond the just-dequeued [`TaskHandle`](../../kernel/src/obj/task.rs)'s pre-cached `AddressSpaceHandle`.
4. **One bootstrap AddressSpace.** [T-016](../analysis/tasks/phase-b/T-016-mmu-activation.md) activated the MMU using bootstrap page-table frames in [`.boot_pt`](../../bsp-qemu-virt/linker.ld) (per [ADR-0027](0027-kernel-virtual-memory-layout.md) §Decision outcome). The kernel must **wrap the already-active topology** into the first arena slot at boot — no `Mmu::create_address_space` call for slot 0 (the topology is live; we name what already exists).
5. **Forward-compat to ADR-0033 (high-half).** When B5+ surfaces per-task `TTBR0_EL1` swap, the kernel needs to track per-AS metadata (ASID, possibly per-AS table-walk bookkeeping). The chosen shape must accommodate additive fields without a HAL trait surface change.
6. **Bounded `unsafe` surface.** The arena + capability layer is safe Rust; the only `unsafe` is the existing [`Mmu`](../../hal/src/mmu/mod.rs) trait-surface contract (UNSAFE-2026-0023 / 0024 / 0025) which this ADR consumes without extending. The `Mmu::create_address_space` zero-fill of the root frame is already covered by [UNSAFE-2026-0026](../audits/unsafe-log.md) (T-018 is its first runtime exerciser per [T-017 §Review history](../analysis/tasks/phase-b/T-017-physical-memory-manager.md#review-history)).

Out of scope of ADR-0028 (deferred by reference, not relitigated): per-AS destroy / `cap_revoke(AddressSpaceCap)` (B4+, when first userspace destroy lands); ASID allocation (ADR-0033 placeholder); copy-on-write or shared-mapping reference-counting (post-B5); reverse-mapping (PFN → AS list) for revocation efficiency (Phase C); SMP per-core MMU / per-core TLB-shootdown (Phase C); huge-page promotion (ADR-0009 §Open questions). The v1 demo touches one bootstrap AS and (in T-018's tests) constructs a second AS for isolation verification — beyond that the address-space layer is dormant until B5+ userspace work activates it.

## Decision drivers

- **HAL trait stability after T-016.** [ADR-0009](0009-mmu-trait.md) defined the [`Mmu::AddressSpace`](../../hal/src/mmu/mod.rs) associated type; [ADR-0027](0027-kernel-virtual-memory-layout.md) + T-016 stabilised `create_address_space` / `address_space_root` / `activate` / `map` / `unmap` signatures with the [`MapperFlush`](../../hal/src/mmu/mod.rs) token discipline. Any option that re-opens this surface this soon after stabilisation costs more than it buys in v1.
- **Kernel BSP-agnosticism at the object surface.** Capability-handling code in [`kernel/src/cap/`](../../kernel/src/cap/) and IPC fast-paths in [`kernel/src/ipc/`](../../kernel/src/ipc/) should not see BSP-specific types. The kernel is already generic over `M: Mmu` at the scheduler surface; the question is whether the *address-space-object* surface picks up the same generic axis or hides it via opaqueness / trait-object dispatch.
- **Zero-cost activation hook.** Context switch is the kernel's hottest fast-path (per [yield_now](../../kernel/src/sched/mod.rs)'s ADR-0021 raw-pointer discipline). Activation must be a struct-field read + monomorphised `Mmu::activate` call — no virtual dispatch, no arena reallocation.
- **Forward-compat to ADR-0033 (high-half).** When TTBR0/TTBR1 split lands, kernel-side metadata (ASID, per-AS table-walk bookkeeping) grows. The shape that accommodates additive fields without forcing a HAL trait revision is preferred.
- **Capability-system integration cost.** All viable options add `CapKind::AddressSpace` + `CapObject::AddressSpace(AddressSpaceHandle)` to the [capability enum](../../kernel/src/cap/mod.rs). The cost is mechanical and identical across options; this driver does not discriminate.
- **Audit-discipline minimisation.** Zero new `unsafe` audit-log entries (the existing UNSAFE-2026-0023/0024/0025/0026 cover the underlying MMU + PMM surface; the address-space object is pure safe Rust over those). Choices that would require new `unsafe` (e.g., trait-object dispatch through raw pointers to dodge the generic spread) lose.
- **Methodical pace** (CLAUDE.md non-negotiable #6). The minimum-required-surface principle says: don't add a generic axis the kernel doesn't already use, *unless* the alternative is structurally worse. The kernel already uses `M: Mmu` generic spread; the address-space object inherits it rather than introducing parallel infrastructure.

## Considered options

1. **Option A — Generic `AddressSpace<M: Mmu>` wrapping `M::AddressSpace` inline; per-type arena `AddressSpaceArena<M>`.** Zero runtime indirection. The kernel object holds the BSP-specific [`Mmu::AddressSpace`](../../hal/src/mmu/mod.rs) value directly as a field. Forward-compat fields (ASID, reverse-mapping pointers) land additively via Amendment when ADR-0033 / Phase-C surfaces them.

2. **Option B — Opaque non-generic `AddressSpace { root: PhysFrame, gen: Generation }`.** Kernel object holds only the root translation-table frame; the HAL trait gains a `reconstruct(&self, root: PhysFrame) -> Self::AddressSpace` shim so every `Mmu::map` / `Mmu::activate` call site rebuilds the BSP-specific value on demand. Kernel stays BSP-agnostic at the object surface.

3. **Option C — Trait-object dispatch via a kernel-defined `MmuObject` trait the BSP implements per-AddressSpace.** The kernel holds `&dyn MmuObject` (or a typed-arena equivalent); every map/unmap/activate is a virtual call. Frees the kernel from generic spread entirely.

## Decision outcome

Chosen option: **Option A — Generic `AddressSpace<M: Mmu>` wrapping `M::AddressSpace` inline; per-type `AddressSpaceArena<M>`.**

The reasoning, connected to the drivers:

1. **HAL trait stability** rules out Option B. Option B requires a new HAL trait method (`reconstruct`) and reverses the post-T-016 stabilisation. The trait signature `Mmu::create_address_space(&self, root: PhysFrame) -> Self::AddressSpace` already crosses the BSP boundary at exactly the right place; pretending the BSP-specific value doesn't exist inside the kernel object would mean discarding the value the BSP just constructed and rebuilding it on every call — wasted work for no real BSP-agnosticism gain (the kernel still needs to know `M` at the trait-method call sites; hiding `M` from the *object surface* doesn't hide it from the *call surface*).

2. **Zero-cost activation hook** rules out Option C. The `dyn MmuObject` route adds an indirect call to every `Mmu::activate` — on context switch, on every map/unmap. Phase-C SMP work makes this path even hotter (cross-core preempt). Trait-object dispatch is genuinely valuable when the implementation set is open and runtime-variable; the kernel statically links exactly one `Mmu` impl per BSP, so the runtime-variability premise of trait objects doesn't apply.

3. **Kernel BSP-agnosticism** holds under Option A because the *capability-handling layer* never touches `M::AddressSpace` directly — it operates on `AddressSpaceHandle` (a `Handle<AddressSpace<M>>` newtype) and delegates the inner-value operations to wrappers that take `(&mut Mmu, &mut AddressSpace<M>, ...)`. The generic `M` propagates through the scheduler (already done per ADR-0019/0020), the capability invocation site (`cap_invoke` is already monomorphised per task), and the arena (`AddressSpaceArena<M>` is a static of the BSP's chosen M). Capability storage itself stays non-generic — `CapObject::AddressSpace(AddressSpaceHandle)` carries the handle, not the typed value; resolution into the typed `&mut AddressSpace<M>` happens at the typed-arena boundary, identical to how `CapObject::Endpoint(EndpointHandle)` resolves to `&mut Endpoint` today.

4. **Forward-compat to ADR-0033** holds under Option A because additive fields (`asid: Option<Asid>`, `reverse_map_head: Option<RevMapHandle>`, etc.) land by Amendment to the `AddressSpace<M>` struct definition. No HAL trait revision; no capability variant change. ADR-0033's §Simulation table will walk through how the new fields participate in the high-half activation sequence.

5. **Bootstrap AddressSpace** (constraint 4) is handled by a one-shot `AddressSpace::wrap_bootstrap(inner: M::AddressSpace) -> AddressSpace<M>` constructor that **does not** call `Mmu::create_address_space` (the topology is already active per T-016). It wraps a pre-constructed BSP-side `Mmu::AddressSpace` value with the kernel-side metadata (generation tag = 0; the bootstrap AS is "always-here" and capabilities to it never go stale via revoke in v1). The BSP is responsible for materialising the inner value from the already-live root frame, via a BSP-side `Mmu::wrap_existing_root(root: PhysFrame) -> Self::AddressSpace` companion (or an equivalent free function on the BSP's `Mmu` impl — T-018 picks the exact shape). Separating "produce the BSP-specific inner value" (BSP-side, knows the page-table topology) from "wrap with kernel-side metadata" (kernel-side, generation + capability-system bookkeeping) keeps the kernel-side `wrap_bootstrap` method pure (no HAL trait call); calling `Mmu::create_address_space` from inside `wrap_bootstrap` would re-zero the live L0 frame and break the running translation tables. The BSP calls `wrap_bootstrap` exactly once at boot, after `mmu_bootstrap()` and after `Pmm::new` (so the bootstrap root frame is already marked Reserved in the PMM, and `wrap_bootstrap` does not consume a PMM allocation). All subsequent `AddressSpace` values are minted via `cap_create_address_space` → `PMM.alloc_frame()` → `Mmu::create_address_space(root)`.

The forward-flag fields chosen for v1: **only `inner: M::AddressSpace` + `generation: Generation`.** ASID / reverse-mapping / per-AS table-walk bookkeeping are named in §"Forward-compat to ADR-0033" but not added today. Adding them speculatively would violate CLAUDE.md non-negotiable #6 (don't design for hypothetical future requirements).

### Simulation

The `AddressSpace<M>` object is a three-state machine: `Live` (in arena, capability-reachable), `Bootstrap` (a specialised `Live` whose `inner` value was constructed by `wrap_bootstrap` rather than `Mmu::create_address_space`), and `Stale` (arena slot freed; capability generation mismatch). The table walks the worst-case interaction across bootstrap, create, map, and activation-on-context-switch under the chosen Option A shape. (Destroy / cap_revoke transitions are deferred to B4+ per §Context; row 0 is sufficient for the v1 demo + T-018's two-AS isolation test.)

| Step | State pre | Action | State post | Switch target / observable effect |
|------|-----------|--------|------------|-----------------------------------|
| 0 | Bootstrap MMU active (T-016); `AddressSpaceArena<M>` slot 0 empty; no kernel `AddressSpace<M>` value exists yet. PMM is initialised (T-017); bootstrap page-table frames in `.boot_pt` are already marked Reserved per [T-017 commit 4](../analysis/tasks/phase-b/T-017-physical-memory-manager.md#approach). | The BSP first materialises an `M::AddressSpace` value naming the already-live root via its own `Mmu::wrap_existing_root(root: PhysFrame) -> Self::AddressSpace` companion (where `root` is the L0 frame address that `mmu_bootstrap` wrote into `TTBR0_EL1` per [`bsp-qemu-virt/src/mmu_bootstrap.rs`](../../bsp-qemu-virt/src/mmu_bootstrap.rs)). `kernel_init` then calls `AddressSpace::wrap_bootstrap(inner: M::AddressSpace)` to wrap that value with kernel-side metadata. **No** `Mmu::create_address_space` call is made on the bootstrap root — that would re-zero the live L0 frame. Arena slot 0 is occupied; `CapabilityTable[0]` mints a `CapKind::AddressSpace + CapObject::AddressSpace(AddressSpaceHandle { index: 0, generation: 0 })` cap. | Bootstrap AS exists in arena slot 0; root cap held by kernel-init's parent context. `PMM.stats()` unchanged (no allocation). | One new smoke trace line `tyrne: address-space-arena ready (1 / N slots used; bootstrap AS root = 0x<pa>)` immediately after the existing `tyrne: pmm initialized (...)` banner. |
| 1 | Bootstrap AS in slot 0; some `parent_cap` held by kernel-init that grants AS creation authority. | `cap_create_address_space(parent_cap) -> Result<AddressSpaceHandle, CapError>`. The wrapper: (i) validates `parent_cap.kind == CapKind::AddressSpace` (or future `Untyped` per Phase-C); (ii) `PMM.alloc_frame()` for the new root — returns `Err(OutOfFrames)` on PMM exhaustion; (iii) calls `unsafe { Mmu::create_address_space(root) }` to materialise the BSP-specific value (the `unsafe` is the existing HAL contract per UNSAFE-2026-0023 — the caller must guarantee `root` is page-aligned + zero-filled; PMM's `alloc_frame` zero-fills per UNSAFE-2026-0026, and `PhysFrame::from_aligned` enforces alignment statically); (iv) `arena.alloc(AddressSpace { inner, generation: <new> })` returns the typed `AddressSpaceHandle`; (v) `CapabilityTable.mint(CapKind::AddressSpace, CapObject::AddressSpace(handle))`. | New AS in arena slot 1+; PMM `free_count - 1`, `allocated_count + 1`; CapabilityTable gains a new cap entry. | UNSAFE-2026-0026 zero-fill exercised at runtime for the first time (lifts its `Pending QEMU smoke verification` note per [T-017 §Review history](../analysis/tasks/phase-b/T-017-physical-memory-manager.md#review-history) once T-018's smoke trace shows the alloc path executing). |
| 2 | Two AS in arena (bootstrap + new); cap held by caller for the new AS. | `cap_invoke(addr_space_cap, MapOp { va, pa, flags })`. The wrapper resolves the cap → `&mut AddressSpace<M>` from the arena → calls `Mmu::map(&mut as_.inner, va, pa, flags, &mut pmm)` → discharges the returned `MapperFlush` token via `.flush(&mmu)` (which invokes `Mmu::invalidate_tlb_address(va)`). Intermediate L1/L2/L3 frames are allocated from PMM via the `&mut dyn FrameProvider` parameter. | Mapping installed in the new AS's translation tables; PMM free_count may decrement (intermediate-table allocs); TLB entry for `va` invalidated. | UNSAFE-2026-0025 first post-bootstrap exerciser (lifts its `Pending QEMU smoke verification` note per [ADR-0027 §Decision outcome](0027-kernel-virtual-memory-layout.md#decision-outcome)); the cap-gated `Mmu::map` wrapper is the **first** non-bootstrap caller of the trait method since T-016 wrote it. |
| 3 | Current task `T_a` runs against bootstrap AS (slot 0); scheduler's ready queue dequeues `T_b` whose `TaskHandle.address_space_handle != T_a.address_space_handle`. | In `yield_now`: just before `cpu.context_switch(cur_ctx, nxt_ctx)` (per ADR-0021's raw-pointer discipline — momentary borrow), check `next.address_space_handle != current.address_space_handle`; if so, resolve `next.address_space_handle` → `&AddressSpace<M>` from `AddressSpaceArena<M>` → `Mmu::activate(&as_.inner)`. The activate call: (1) writes `TTBR0_EL1` with the new root + `ISB`; (2) `DSB ISHST` to ensure prior page-table stores are globally observable; (3) `TLBI VMALLE1` to invalidate stale TLB entries for the outgoing AS; (4) `DSB ISH` + `ISB` to drain the invalidation and pipeline. The borrow of `&AddressSpace<M>` ends before `context_switch` per ADR-0021; the next task's view is consistent. | `TTBR0_EL1` swapped to `T_b`'s root; TLB **flushed** (`TLBI VMALLE1` in `QemuVirtMmu::activate` — more conservative than the "no auto-flush" note in the original design; single-core v1 with `TCR_EL1.AS = 0` per ADR-0027 §"ASID" has no per-task ASID isolation, so the global flush is the safe choice). | `T_b`'s instruction stream loads its first user-VA via the swapped TTBR0_EL1; smoke trace gains the `T_b`-side outputs after the existing `T_a`-side outputs without interleaving (the test fixture chooses well-separated VAs in T-018). |

The 4-row shape mirrors the [ADR-0027 §Simulation](0027-kernel-virtual-memory-layout.md#simulation) discipline applied here: row 0 captures the bootstrap-AS wrap (the load-bearing "no `Mmu::create_address_space` call for slot 0" decision; missing this would either skip cap minting for the live topology, or double-zero-fill the in-use L0 frame); row 1 the steady-state create; row 2 the cap-gated map; row 3 the activation-on-context-switch decision-point. The single-core no-ASID assumption in row 3 is the **only** correctness subtlety the v1 design carries forward — `TCR_EL1.AS = 0` means no per-task ASID tagging, so every AS switch must globally invalidate the TLB (`TLBI VMALLE1`) to prevent stale-AS hits; T-018 host tests must pin "two AS, activate, observe distinct views" so a future Phase-C task that adds ASIDs cannot regress the contract.

### Dependency chain

For this decision to be fully in effect:

```text
1. New `kernel/src/mm/address_space.rs` module — `AddressSpace<M>` struct,
   `AddressSpace::wrap_bootstrap`, the generation tag, the
   `AddressSpaceHandle` newtype. Lives alongside the T-017 PMM under
   `kernel/src/mm/`. — T-018 (Draft, opens with this ADR)
2. `kernel/src/mm/address_space.rs` — `AddressSpaceArena<M, const N: usize>`
   following the [ADR-0016](0016-kernel-object-storage.md) per-type
   fixed-size-block pattern (mirrors `TaskArena` / `EndpointArena`). — T-018
3. `kernel/src/cap/mod.rs` — `CapKind::AddressSpace` variant +
   `CapObject::AddressSpace(AddressSpaceHandle)` variant. The
   capability-resolution path gains a new arm; existing typed-handle
   discipline (per [ADR-0014](0014-capability-representation.md)) extends
   unchanged. — T-018
4. `kernel/src/mm/address_space.rs` — `cap_create_address_space`,
   `cap_map`, `cap_unmap` capability-gated wrappers around
   `Mmu::create_address_space` / `Mmu::map` / `Mmu::unmap`. Each wrapper
   validates the cap kind, resolves to the typed handle, takes a
   momentary `&mut AddressSpace<M>` per [ADR-0021](0021-raw-pointer-scheduler-ipc-bridge.md)'s
   discipline, calls the underlying `Mmu` method, discharges the
   `MapperFlush` token. — T-018
5. `kernel/src/sched/mod.rs::yield_now` — activation-on-context-switch
   hook: pre-`cpu.context_switch` check + momentary `Mmu::activate`
   call when the outgoing and incoming tasks have different
   `address_space_handle`. The check is a single field comparison;
   no allocation, no cap resolution beyond the just-dequeued
   `TaskHandle`'s pre-cached `address_space_handle`. — T-018
6. `bsp-qemu-virt/src/main.rs::kernel_entry` — bootstrap-AS wrap +
   `AddressSpaceArena<QemuVirtMmu>` `StaticCell` publication, in the
   order: `pmm initialized → address-space-arena ready → ...` per
   the §Simulation row-0 banner expectation. — T-018
7. `kernel/src/obj/task.rs::Task` struct — gains an
   `address_space_handle: AddressSpaceHandle` field. Existing tasks
   on the bootstrap AS get `AddressSpaceHandle { index: 0, generation: 0 }`
   at construction. — T-018

The implementation lands across T-018's bisectable commit chain. Steps 1–3
are commits 1–3 (kernel-internal struct + arena + cap-variant landing in
order so each commit ends green); steps 4–5 are commits 4–5 (wrapper
surface + scheduler hook); steps 6–7 are commit 6 (BSP wiring + Task struct
extension + smoke verification).
```

Forward-flag (not blocking Accept, per [ADR-0025 §Rule 1](0025-adr-governance-amendments.md) interpretation — these are existing-ADR-placeholders or future-phase tasks, not undefined T-NNNs):

- **[ADR-0033 placeholder](0027-kernel-virtual-memory-layout.md#decision-outcome)** — high-half kernel migration. When B5+ surfaces per-task `TTBR0_EL1` swap, ADR-0033 opens and revises ADR-0028 by Amendment: `AddressSpace<M>` gains an `asid: Option<Asid>` field; the activation hook (step 5 above) writes `TTBR0_EL1.ASID` alongside the base address; `Mmu::activate` signature may gain an `Asid` parameter or absorb it via the `AddressSpace` value.
- **B4 `MemoryRegionCap` work** — adds `cap_revoke(AddressSpaceCap)` which walks the page-table tree, frees every L3 mapping via `Mmu::unmap`, then frees L3/L2/L1/L0 frames back to PMM. The destroy path is non-trivial enough to warrant its own ADR or §B4 task ledger row.
- **Phase-C SMP** — per-core `Mmu` impl + cross-core TLB shootdown via `Mmu::invalidate_tlb_address` on every CPU. The `AddressSpace<M>` shape accommodates this without revision; the scheduler's per-core view of `current.address_space_handle` does the work.

## Consequences

### Positive

- **Zero new `unsafe` audit-log entries.** The existing UNSAFE-2026-0023 / 0024 / 0025 (MMU surface) + UNSAFE-2026-0026 (PMM frame-zeroing) cover the underlying operations; the address-space-object layer is pure safe Rust on top.
- **Zero HAL trait surface change.** [`Mmu::AddressSpace`](../../hal/src/mmu/mod.rs) + `create_address_space` / `address_space_root` / `activate` / `map` / `unmap` signatures are unchanged. The trait surface stabilised in T-016 stays stable; ADR-0009 §Revision notes gets no new rider for this ADR.
- **Capability-gated by construction.** Every `Mmu::map` / `Mmu::unmap` / `Mmu::activate` call site in the kernel goes through a `cap_*` wrapper that resolves a cap first; no ambient authority path exists for an attacker (or a misbehaving kernel subsystem) to bypass cap checks.
- **Forward-compat to ADR-0033 by Amendment.** ASID + reverse-mapping fields land additively in the `AddressSpace<M>` struct; no struct rewrite, no HAL revision, no capability variant change.
- **Reuses the [ADR-0016](0016-kernel-object-storage.md) per-type arena pattern.** Reviewers and future contributors recognise the shape immediately — same generation-tagged handles, same fixed-size-block storage, same arena-slot recycling discipline as Endpoint / Notification / Task.

### Negative

- **The kernel grows another `M: Mmu` generic axis at the address-space-object surface.** Every kernel function that takes `&mut AddressSpace<M>` is monomorphised per BSP. *Mitigation:* the scheduler already propagates `M: Mmu` per [ADR-0019](0019-scheduler-shape.md); the address-space-object inherits the same axis. The monomorphisation cost is bounded (one BSP per kernel build today) and the alternative (Option C trait-object dispatch) is genuinely worse on the activation hot path.
- **Capability invocation gains a new arm.** Every cap-resolving site that pattern-matches `CapObject` gains an `AddressSpace(handle) => ...` arm. *Mitigation:* the cost is mechanical; the new arm is exercised in T-018's tests; the typed-handle discipline prevents wrong-kind invocation at compile time.
- **The bootstrap-AS wrap is a special case.** `wrap_bootstrap` does **not** call `Mmu::create_address_space`; the BSP must expose `wrap_existing_root` (or recognise an already-active topology inside `create_address_space`). This is a one-shot path that a future BSP author could miss. *Mitigation:* T-018 §Acceptance criteria pins the bootstrap-wrap path as a load-bearing test (without it, slot 0 either gets re-zeroed-mid-execution or never gets a cap minted for it); the path is named in this ADR's §Simulation row 0 as a load-bearing decision so a future BSP can't skip it inadvertently.
- **No per-AS ASID isolation in v1.** With `TCR_EL1.AS = 0` there is no per-task ASID tagging in the TLB, so every AS switch must globally invalidate the TLB to prevent stale-AS hits. The `QemuVirtMmu::activate` path does this explicitly (`MSR TTBR0_EL1; ISB; DSB ISHST; TLBI VMALLE1; DSB ISH; ISB` — the original ADR description called this row "activation-without-TLB-flush", which was advisory; the implementation landed strictly more conservative in T-018 commit 2 to close the stale-TLB risk unconditionally). The cost is a full TLB flush on every AS switch — acceptable for v1's two-AS demo; ADR-0033 will revisit when B5+ introduces ASID-based isolation and per-task `TTBR0_EL1` swap on hot context-switch paths. The §Simulation row 3 cell captures the current sequence verbatim.

### Neutral

- **Type-driven discoverability of address-space methods.** `<AddressSpace<M>>::wrap_bootstrap` / `<AddressSpace<M>>::root_frame` are discoverable from the type; the IDE / `cargo doc` surface mirrors how Endpoint / Notification / Task methods surface. Neither a clear win (the project is small enough that contributors find methods by reading the crate) nor a clear cost.
- **The `AddressSpaceHandle` shape exactly mirrors `EndpointHandle` / `TaskHandle`.** Generation tag + arena index, same generic-over-target shape. Reduces conceptual surface; not a structural innovation.

## Pros and cons of the options

### Option A — Generic `AddressSpace<M: Mmu>` wrapping `M::AddressSpace` inline

- Pro: Zero new HAL trait method; zero new `unsafe` audit entries.
- Pro: Zero-cost activation hook — monomorphised `Mmu::activate` call, no virtual dispatch.
- Pro: Forward-compat fields land by Amendment (ASID, reverse-mapping); no struct rewrite at ADR-0033 time.
- Pro: Reuses [ADR-0016](0016-kernel-object-storage.md) per-type arena pattern; recognisable shape.
- Con: Adds another `M: Mmu` generic axis at the address-space-object surface; capability-handling code gains an `M` parameter at sites that resolve `CapObject::AddressSpace(handle)` to `&mut AddressSpace<M>`. *Mitigation:* the scheduler already propagates `M`; the address-space-object inherits.
- Con: The bootstrap-AS wrap is a special-case path the BSP must implement (`wrap_existing_root` or equivalent). *Mitigation:* §Simulation row 0 names it; T-018 host-tests pin it.

### Option B — Opaque `AddressSpace { root: PhysFrame, gen: Generation }` + HAL `reconstruct` shim

- Pro: Kernel object stays non-generic; capability-handling code never sees `M`.
- Pro: BSP-specific value is recomputed on demand; no stale-cache hazard.
- Con: Re-opens the HAL trait surface T-016 just stabilised. New trait method `reconstruct(&self, root: PhysFrame) -> Self::AddressSpace` with `unsafe` contract; existing UNSAFE-2026-0023 either gets an Amendment or a new entry.
- Con: Forces every `Mmu` impl to be **stateless w.r.t. AddressSpace** — the impl can't cache anything per-AS (e.g., a free-list of recently-freed intermediate L3 frames for that AS, useful for future copy-on-write work). v1 doesn't need this, but locking it out architecturally is a real future-cost.
- Con: `reconstruct` cost on the activation hot path — even if cheap (typically `Self::AddressSpace::from_root(root)`), it's per-`Mmu::activate`-call work the kernel can't elide.

### Option C — `dyn MmuObject` trait-object dispatch

- Pro: Kernel completely free from `M: Mmu` generic spread at the address-space-object surface; capability code stays simplest.
- Pro: Heterogeneous-Mmu kernels (Phase-C SMP with per-core different Mmu impls?) trivially supported.
- Con: Virtual dispatch on every `Mmu::activate` / `Mmu::map` / `Mmu::unmap` — context-switch path takes an indirect call. v1 doesn't measure this cost, but it's a real Phase-C concern.
- Con: Either heap-allocation (rejected; kernel has no heap) or a typed-arena-of-trait-objects pattern that [ADR-0016](0016-kernel-object-storage.md) doesn't have today. The typed-arena variant would require either `Box<dyn MmuObject>` boxed-into-arena-slot (heap) or non-trivial dyn-trait-arena infrastructure (new audit weight).
- Con: The trait-object design premise — "the implementation set is open and runtime-variable" — is wrong for the kernel: exactly one `Mmu` impl is statically linked per BSP. Trait objects are the wrong tool for the static-dispatch use case.

## References

- [ADR-0009 — `Mmu` HAL trait signature (v1)](0009-mmu-trait.md) — the trait this ADR consumes; defines the [`AddressSpace`](../../hal/src/mmu/mod.rs) associated type and the `create_address_space` / `address_space_root` / `activate` / `map` / `unmap` surface.
- [ADR-0014 — Capability representation](0014-capability-representation.md) — the capability enum this ADR extends with `CapKind::AddressSpace` + `CapObject::AddressSpace(AddressSpaceHandle)`.
- [ADR-0016 — Kernel-object storage](0016-kernel-object-storage.md) — the per-type fixed-size-block arena pattern with generation-tagged handles that `AddressSpaceArena<M>` follows.
- [ADR-0019 — Scheduler shape](0019-scheduler-shape.md) — the source of the kernel-level `M: Mmu` generic spread the address-space-object inherits.
- [ADR-0021 — Raw-pointer scheduler IPC-bridge API](0021-raw-pointer-scheduler-ipc-bridge.md) — the momentary-borrow discipline the cap-gated wrappers and the activation hook follow.
- [ADR-0025 — ADR governance amendments](0025-adr-governance-amendments.md) — §Rule 1 (forward-reference contract) requires T-018 to open `Draft` in the same commit as this ADR's Propose.
- [ADR-0027 — Kernel virtual memory layout](0027-kernel-virtual-memory-layout.md) — defines the identity-only v1 layout the activation hook operates within, and names the ADR-0033 placeholder this ADR forward-flags for high-half migration.
- [ADR-0035 — Physical Memory Manager (bitmap allocator)](0035-physical-memory-manager.md) — the [`FrameProvider`](../../hal/src/mmu/mod.rs) impl `cap_create_address_space` consumes for root-frame allocation; the load-bearing prerequisite for B3 item 2.
- [phase-b.md §B3](../roadmap/phases/phase-b.md#milestone-b3--address-space-abstraction) — milestone breakdown items 2–7.
- [T-016 — MMU activation](../analysis/tasks/phase-b/T-016-mmu-activation.md) — the prior task that stabilised the [`Mmu`](../../hal/src/mmu/mod.rs) trait surface this ADR consumes.
- [T-017 — Physical Memory Manager](../analysis/tasks/phase-b/T-017-physical-memory-manager.md) — the prior task that delivered the PMM this ADR consumes.
- [T-018 — `AddressSpace` kernel object](../analysis/tasks/phase-b/T-018-address-space-kernel-object.md) — the implementation task this ADR drives.
- [`docs/architecture/memory-management.md`](../architecture/memory-management.md) — companion architecture chapter; §"Frame allocation discipline" is what T-017 closed; the §"Address-space objects" section is what T-018 will add.
- [`hal/src/mmu/mod.rs`](../../hal/src/mmu/mod.rs) — the trait this ADR consumes; lines 290–400 are the load-bearing surface.
- seL4 untyped-region model — capability-mediated frame ownership; B5+ `MemoryRegionCap` forward-flag.
- Hubris `addr_of_kernel_image` / kernel-image-mapping discipline — prior art for naming kernel-side virtual addresses; relevant when ADR-0034 (kernel-image section permissions placeholder) opens.
