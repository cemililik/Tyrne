# 0035 — Physical Memory Manager (B3 prerequisite — bitmap allocator)

- **Status:** Accepted
- **Date:** 2026-05-09
- **Deciders:** @cemililik

## Context

Phase B3 ("Address space abstraction" per [phase-b.md §B3](../roadmap/phases/phase-b.md#milestone-b3--address-space-abstraction)) introduces per-task translation tables and capability-gated `Mmu::map` / `Mmu::unmap` wrappers. Both depend on a runtime physical-frame allocator that the kernel does not yet have. [ADR-0027 §"Frame allocation discipline"](0027-kernel-virtual-memory-layout.md) and [`memory-management.md` §"Frame allocation discipline"](../architecture/memory-management.md#frame-allocation-discipline) both flag this gap explicitly: v1's bootstrap satisfies the [`Mmu::map`](../../hal/src/mmu/mod.rs)'s `&mut dyn FrameProvider` parameter via static reservation in [`bsp-qemu-virt/linker.ld`](../../bsp-qemu-virt/linker.ld) `.boot_pt`; for any post-bootstrap mapping the kernel needs a Physical Memory Manager (PMM) that implements `FrameProvider` over the live physical RAM.

The decision is load-bearing for B3's full sub-breakdown ([phase-b.md §B3](../roadmap/phases/phase-b.md#milestone-b3--address-space-abstraction) items 2 / 3 / 5 / 6): the per-task `AddressSpace` kernel object needs frames for its root translation table; the capability-gated `Map / Unmap operations` need frames for intermediate L1/L2/L3 tables; the activation-on-context-switch path consumes the frames the prior steps allocated; the isolation tests need to run against a real allocator rather than the host-test `VecFrameProvider`.

The PMM's design surface — allocation discipline, metadata layout, reservation tracking — is independent enough from the address-space data structure that it warrants a separate ADR (per the methodical-pace principle of CLAUDE.md non-negotiable #6). The ADR-0028 slot is reserved for the address-space data structure (per [ADR-0027 §Context](0027-kernel-virtual-memory-layout.md#context); no file today, opens with the second B3 ADR); ADR-0035 settles the layer below it.

The constraints v1 imposes on the PMM:

1. **Single allocation size.** The HAL's [`PhysFrame`](../../hal/src/mmu/mod.rs) is `PAGE_SIZE`-aligned (4 KiB) per [ADR-0009](0009-mmu-trait.md). v1 has no caller for variable-size allocations — buddy / arena / slab disciplines are overkill for the actual demand.
2. **Bounded RAM.** QEMU `virt` v1 ships with 128 MiB at PA `0x4000_0000..0x4800_0000` (per [ADR-0012](0012-boot-flow-qemu-virt.md) + [`bsp-qemu-virt/linker.ld`](../../bsp-qemu-virt/linker.ld)). 32 768 frames managed; no NUMA, no hot-plug, no over-commit.
3. **Single-core, cooperative.** No concurrent allocators in v1; no need for atomic bitmap operations or per-CPU caches.
4. **Reserved regions exist before the PMM init.** The kernel image (`.text` / `.rodata` / `.data` / `.bss`), the `.boot_pt` reservation, the boot stack, and the future capability-system grants need to be marked allocated at PMM init so they can never be handed out. The init API must accept a list of reserved ranges.
5. **`FrameProvider` contract** ([ADR-0009](0009-mmu-trait.md), [`hal/src/mmu/mod.rs`](../../hal/src/mmu/mod.rs)). Returned frames must be **page-aligned** (the type system enforces this via `PhysFrame::from_aligned`) and **zero-initialised**. The PMM must zero allocated frames before returning them. Returning `None` propagates as `MmuError::OutOfFrames`.
6. **Forward-readiness.** The B5+ high-half ADR-0033 placeholder will move kernel `.text` to a high VA; the PMM should not bake in identity-only assumptions (e.g., reading next-pointers stored in-frame would require a `PA → VA` translation step that the v1 identity layout makes vacuous but the future high-half regime would need a helper for).
7. **Bounded `unsafe` surface.** The PMM should be writable in safe Rust. The MMU's `unsafe` discipline (UNSAFE-2026-0022 through 0025) covers the page-table-walker; the PMM is a level above that and should be `unsafe`-free in its body.

Out of scope of ADR-0035 (deferred by reference, not relitigated): variable-size allocations / huge-page support ([ADR-0009 §Open questions](0009-mmu-trait.md#open-questions)), per-CPU caching, NUMA awareness, ASLR, copy-on-write fast-path optimisation, mlock / mprotect, swapping, transparent-huge-page promotion. These all sit on top of the v1 PMM if and when they're needed.

## Decision drivers

- **Methodical pace** (CLAUDE.md non-negotiable #6 — "minimum required surface per milestone"). v1 needs *runtime frame alloc + reservation tracking + zero-fill discipline*; it does **not** need *fragmentation handling, per-CPU caches, or variable sizes*. Choices that defer those complexities without locking us in are preferred.
- **Bounded `unsafe` surface.** Page-table writes already audit at UNSAFE-2026-0022 / 0025; piling on PMM-internal `unsafe` adds review weight for no v1 win. The PMM body should be safe Rust; metadata lives in regular `[u8; N]` / `&mut` arrays.
- **Reproducible bootstrap.** The PMM's reservation API must be called exactly once at boot, with the kernel-image / `.boot_pt` / stack ranges pre-computed by the BSP. No dynamic discovery; no DTB parsing today (deferred per [ADR-0012 §Open questions](0012-boot-flow-qemu-virt.md)).
- **Forward-compat with B5+ high-half.** [ADR-0033 placeholder](0027-kernel-virtual-memory-layout.md) will move kernel mappings to `TTBR1_EL1`; the PMM's metadata addressing should not depend on the translation regime. Bitmap-style metadata (one bit per frame, indexed by frame number) is regime-independent. In-frame next-pointer linking is regime-dependent (requires `PA → VA` translation post-MMU when the frame's PA is no longer identity-mapped).
- **Capability-aware extension point.** B5+ introduces `MemoryRegionCap` (capability-gated frame ownership). The v1 PMM is kernel-internal; the future capability-system grants will sit on top of `Pmm::alloc_frame` + a reservation-tracking layer. The PMM's reservation API should be designed so the future capability layer can mark frames as "owned by capability X" without the PMM caring about the cap object.
- **Compatibility with [ADR-0009](0009-mmu-trait.md)'s `FrameProvider` trait.** v1 trait surface: `fn alloc_frame(&mut self) -> Option<PhysFrame>`. The PMM impls this directly; no surface change. A future revision could grow `alloc_frame_zeroed` / `alloc_frames_contiguous` etc., but those are not v1 concerns.
- **Audit-discipline minimisation.** The PMM today should add zero new audit-log entries. The frame-zeroing step is the only operation that touches kernel-owned memory the caller doesn't yet own; it requires a single `unsafe` block (`core::ptr::write_bytes` over a `*mut u8` derived from the allocated `PhysFrame`) — but the operation extends UNSAFE-2026-0001's existing umbrella ("kernel-static-buffer raw-pointer write to identity-mapped memory") via Amendment rather than introducing a new entry. Materialising a `&mut [u8; 4096]` slice from the raw pointer (to enable `core::slice::fill` instead) would *itself* be an `unsafe` step (`core::slice::from_raw_parts_mut`); raw `write_bytes` is the more honest expression of what the PMM is doing.
- **Honest `v1` scope.** Like [ADR-0009](0009-mmu-trait.md), this ADR commits to what v1 can express (bounded-RAM, single-size, single-core) and names what it cannot (NUMA, hot-plug, swap), so later ADRs land against a known baseline.

## Considered options

1. **Option A — Bitmap allocator (one bit per frame; external metadata; hint pointer).** Metadata: 32 768 frames / 8 bits = 4 KiB for 128 MiB. Alloc: linear scan from a hint pointer for a 0 bit; O(N) worst-case, O(1) typical. Free: O(1) bit-clear + hint rewind.
2. **Option B — External free-list (LIFO stack of frame indices).** Metadata: stack of 32 K × 4 bytes = 128 KiB (frame-number indices). Alloc: O(1) pop. Free: O(1) push.
3. **Option C — In-frame free-list (linked list using free frames themselves as next-pointers).** Metadata: 1 head pointer (8 bytes) + 1 free-count gauge (8 bytes) = 16 bytes. Alloc / free: O(1). Frame contents act as the linkage.
4. **Option D — Buddy allocator (recursive splitting + buddy merging).** Metadata: log₂(32 K) = 15 levels of bitmaps; total ~16 KiB. Alloc: O(log N) split. Free: O(log N) merge.

## Decision outcome

Chosen option: **Option A — Bitmap allocator with hint pointer.**

The four constraints that drove the choice:

1. **Bounded RAM (32 K frames) makes O(N) scan acceptable.** Worst-case scan of 4 KiB of bitmap is ~32 K bit reads — sub-microsecond on Cortex-A72, negligible at v1's allocation rate (page-table frames + per-task stacks + capability tables — tens of allocations per task creation, not thousands per second). Under the actual v1 traffic, the hint pointer makes amortised cost O(1) for forward-scanning patterns.
2. **Lowest metadata footprint (4 KiB) of any option.** Fits cleanly in `.bss` next to the `.boot_pt` reservation; no constraint pressure.
3. **Forward-portable to high-half kernel.** Bitmap address is computed from `frame_number / 8 + bitmap_base`; bitmap base is a kernel-static address that the high-half migration can relocate without touching the algorithm. In-frame linked free-list (Option C) requires reading the frame's contents as a pointer, which post-high-half-migration would need a `PA → VA` translation helper; we'd be locking in the translation discipline today for a v1 use case that doesn't need it.
4. **Reservation tracking is trivial.** `init` walks the reserved-range list and `set`s every covered frame's bit. The same bitmap then naturally tracks "what's free for future allocs" without dual data structures.

The hint pointer (last successful allocation index) makes amortised cost O(1) for forward-scanning patterns, with O(N) worst case for fragmented patterns. v1 has no fragmentation pressure (no destructive frees from short-lived owners; B3+ task-creation arrives and sticks for the lifetime of the kernel). When B5+ introduces `MemoryRegionCap` revocation and frame churn, the worst-case scan cost may surface as a hot-path concern — at which point the PMM grows a free-frame counter + occupancy hint pair, *not* a structural rewrite.

Zero new `unsafe` audit entries. The bitmap is a `&mut [u8]` over a `[u8; PMM_BITMAP_BYTES]` static; bit-set / bit-clear is safe-Rust integer arithmetic. The frame-zeroing step is the only memory-touching operation: implemented via `core::ptr::write_bytes` on a `*mut u8` derived from the allocated `PhysFrame` (which post-MMU is identity-mapped to a kernel-readable VA per ADR-0027). The zeroing is `unsafe`, but it sits on top of the existing UNSAFE-2026-0001 "MMIO + kernel-static-buffer raw-pointer" umbrella and gains an Amendment rather than a new entry. The PMM body itself — bitmap init, scan, set, clear, hint maintenance — is entirely safe Rust.

### Simulation

The PMM is a four-state state machine per frame: `Reserved` (set at init, never freed), `Free` (cleared, available for alloc), `Allocated` (set by alloc), `Reserved → Free` is forbidden (init is one-way). The table walks the worst-case interaction across init + alloc + free + exhaustion + post-exhaustion-recovery, under the chosen Option A shape.

| Step | State pre | Action | State post | Switch target / observable effect |
|------|-----------|--------|------------|-----------------------------------|
| 0 | Bitmap zero-initialised by `.bss` zero-fill at `_start`; `hint = 0`; `free_count = 0` | `Pmm::new(extent: PhysFrameRange, reserved: &[PhysFrameRange]) -> Self` walks the reserved-range list, marks every covered frame's bit `1` (Reserved), and sets `free_count = total - reserved_count`. Sets `hint` to the first frame *not* in any reserved range. | Bitmap reflects "Reserved at every reserved-region frame, Free elsewhere"; `hint = first_free_frame`; `free_count` = correct | The PMM is now ready to accept `alloc_frame`. The reservation discipline is one-shot at boot; no later call moves a Reserved frame to Free. |
| 1 | Bitmap with reserved bits set; `hint = first_free_frame`; `free_count = 32_768 - reserved_count` | `alloc_frame()` scans bitmap from `hint` forward, finds bit `0` at index `i`, sets bit `i` to `1`, writes-zero through `core::ptr::write_bytes` over the 4 KiB at PA = `extent.start + i × 4096`, advances `hint = i + 1`, decrements `free_count`, returns `Some(PhysFrame::from_aligned(PhysAddr(extent.start + i × 4096)).unwrap())` | Bit `i` = 1 (Allocated); `hint = i + 1`; `free_count` decremented | Frame is now caller-owned; subsequent allocs skip it. The unwrap of `from_aligned` is provably-correct because `extent.start` is page-aligned by the BSP-init contract and `i × 4096` preserves that. |
| 2 | Bit `i` = 1 (Allocated); `hint = i + 1`; `free_count` reduced | `free_frame(PhysFrame(...))` computes `i` from the frame's PA (`(pa - extent.start) / 4096`); reads the bit — if already `0` (already Free) returns `Err(PmmError::DoubleFree)` without mutating; clears bit `i`; if `i < hint`, sets `hint = i` (rewind); increments `free_count` | Bit `i` = 0 (Free); `hint = min(prev_hint, i)`; `free_count` incremented | Frame is reclaimable; the rewind discipline keeps the hint pointing at the lowest known free frame, preventing fragmentation from accumulating linearly with alloc / free interleaving. **Critical row:** the bitmap collapses Reserved + Allocated into a single "1" bit. A `free_frame` of a Reserved frame *cannot* be detected by the bitmap alone (the bit is `1` either way) — but it cannot occur in well-formed kernel code: no caller ever holds a `PhysFrame` whose PA falls in a reserved range (reservation is one-way at init; reserved-frame `PhysFrame` values are never minted by the PMM). The kernel-discipline contract is that `free_frame` is only called with frames returned from `alloc_frame`. A misbehaving caller passing a Reserved-frame PA would corrupt the bitmap by clearing a Reserved bit — handled by **defensive validation**: `free_frame` cross-checks that the index `i` is *not* in any reserved range (PMM caches the reserved-range list for this check) and returns `Err(PmmError::DoubleFree)` if it is. The check is O(R) in the reserved-range count (R ≤ a small constant for v1's BSP); the bitmap itself stays single-bit. |
| 3 | Bitmap fully `1`; `free_count = 0`; `hint = N` | `alloc_frame()` scans from `hint`, wraps to start (the hint-rewind discipline makes the wrap a separate scan from the initial `hint..N` pass), scans `0..hint` looking for any `0` bit, finds none | Bitmap unchanged; `hint` unchanged; `free_count = 0` | Returns `None`; caller propagates as `MmuError::OutOfFrames` per the `FrameProvider` contract. The two-pass scan (forward from hint, then wrap) ensures correctness even under the hint pointing past the last free frame. |
| 4 | Some frames Allocated, some Free, some Reserved; alloc / free interleaving across many calls | `Pmm::stats() -> PmmStats { total_frames, reserved_frames, allocated_frames, free_frames }` reads the cached counters | No state change | Diagnostic / debug surface; useful for invariant assertions in host tests + future runtime self-checks. The cached `free_count` is the source of truth; the bitmap's bit-count is the cross-check (host-tested for parity against the counters). |

The 5-row shape mirrors the [ADR-0027 §Simulation](0027-kernel-virtual-memory-layout.md#simulation) discipline applied here: row 0 captures init; rows 1 / 2 the steady-state alloc / free; row 3 the failure-class moment (`OutOfFrames`); row 4 documents the steady-state observability contract that B3+ allocators inherit. The Reserved-vs-Allocated single-bit collapse in row 2 is the **only** correctness subtlety the design carries forward; host tests must pin the `free_frame(reserved_frame)` rejection path so a future B-phase task that adds reservation removal cannot land without breaking a test.

### Dependency chain

For this decision to be fully in effect:

```text
1. New `kernel/src/mm/` module — top-level memory-management subsystem
   parent. Contains the PMM today; will host the `AddressSpace` kernel
   object for B3 §2 (ADR-0028) tomorrow. — T-017 (Draft, opens with
   this ADR)
2. `kernel/src/mm/pmm.rs` — Bitmap allocator implementation:
   `Pmm::new(extent, reserved)`, `Pmm::alloc_frame`, `Pmm::free_frame`,
   `Pmm::stats`, `impl FrameProvider for Pmm`. Plus `PmmError` enum
   (DoubleFree, OutOfRange, etc.). — T-017
3. `bsp-qemu-virt/src/main.rs` — BSP wires the PMM at boot: computes
   the kernel-image / `.boot_pt` / boot-stack ranges from linker
   symbols, calls `Pmm::new(extent, &reserved)`, publishes the PMM in
   a `StaticCell<Pmm>`. The `kernel_entry` order gains one new step:
   `mmu_bootstrap()` → "tyrne: mmu activated" print → **PMM init →
   "tyrne: pmm initialized (N frames available; M reserved)" print**
   → GIC init → demo. — T-017
4. Host tests for the bitmap allocator: alloc / free round-trip,
   reservation marking pinned, `OutOfFrames` after exhaustion,
   `DoubleFree` rejection, hint-pointer rewind under interleaved alloc
   / free patterns, `Pmm::stats` parity with bit-count. Tests live in
   `kernel/src/mm/pmm.rs::tests` under `#[cfg(test)]`. — T-017
5. **No new audit-log entry.** The frame-zeroing step is the only new
   memory-touching operation; it joins UNSAFE-2026-0001's umbrella
   ("kernel-static raw-pointer + MMIO + zero-init") via a new
   Amendment naming the PMM site. T-017 lands the Amendment in the
   audit log. — T-017
6. Update [`docs/architecture/memory-management.md`](../architecture/memory-management.md)
   §"Frame allocation discipline" — replace "v1 satisfies this for
   the bootstrap moment via static reservation; for post-MMU mappings
   the kernel needs an actual PMM" with the post-T-017 reality (PMM
   live; bitmap; reservation list). Cross-link from
   [ADR-0009](0009-mmu-trait.md), [ADR-0027](0027-kernel-virtual-memory-layout.md),
   and ADR-0035 (this file). — T-017
7. Update `current.md` headline + `phase-b.md` ADR ledger row.
   Closure trio is **not** required for T-017 in isolation; T-017
   `Done` flips on (cargo gates + miri + smoke unchanged plus the new
   `tyrne: pmm initialized (...)` line). The B3 *milestone* closure
   trio runs when the milestone closes (after T-018's address-space
   abstraction lands). — T-017
```

The first task (T-017) covers steps 1 through 7. No further task is opened by this ADR. T-017 is a **single bundled task** — same shape as T-016 (which bundled HAL trait extension + BSP impl + bootstrap routine + audit entries + cross-references in one task). The implementation may land across multiple commits within T-017's scope.

T-017's `Done` flip gates only on its own DoD (host-tests + miri + clippy + kernel-build + smoke-trace-byte-stable-plus-one-new-`tyrne: pmm initialized` line); it does not require a closure trio. The B3 milestone-level closure trio runs when T-017 + T-018 (address-space abstraction, opened by ADR-0028 in a future commit) both land Done.

## Consequences

### Positive

- **Runtime `Mmu::map` callers can now allocate intermediate page-table frames.** Unblocks B3 §3 ("Map / unmap operations") and B3 §2 ("`AddressSpace` kernel object" — needs frames for its root translation table).
- **Reservation tracking is structurally correct.** The single-bitmap design makes "kernel image cannot be freed by a runtime caller" a design property: a `free_frame` of a Reserved frame returns `PmmError::DoubleFree` without corrupting the bitmap, because the discrimination between Reserved and Allocated lives in the *kernel* code that calls `Pmm::new` (no caller ever holds a `PhysFrame` covering a Reserved range; reservation is one-way at init).
- **Bounded metadata.** 4 KiB of `.bss` for the bitmap; all other state (hint, free_count) is `usize` × 2. Total PMM footprint: ~4 KiB of `.bss` + ~16 bytes of cached counters.
- **Zero new `unsafe` audits.** The PMM body is safe Rust; the frame-zeroing site joins UNSAFE-2026-0001's umbrella via an Amendment rather than a new entry. Audit-log surface stays bounded at the project's "minimum required" pace.
- **Forward-compat with B5+ high-half kernel.** Bitmap addressing is regime-independent; the PMM's metadata doesn't sit inside frame contents, so the high-half migration relocates the bitmap base address but does not require a `PA → VA` translation discipline at the algorithm level.
- **Capability-system-ready extension.** Future `MemoryRegionCap` grants sit on top of `Pmm::alloc_frame`; the PMM stays unaware of capability identity. The reservation-list API already supports "pre-allocate N frames and hand them to layer X" without the PMM caring about X's nature.

### Negative

- **O(N) worst-case alloc scan under fragmentation pressure.** v1 has no fragmentation pressure (no destructive frees from short-lived owners); the worst-case is ~32 K bit reads, sub-microsecond on Cortex-A72. *Mitigation:* the hint pointer makes amortised cost O(1) for forward-scanning. When B5+ revocation introduces churn, the worst-case may surface; at that point a free-frame-counter + region-occupancy hint pair extends the design without rewriting (a new ADR can settle that growth — ADR-0035 does not commit to a single hint forever).
- **Bitmap conflates Reserved and Allocated into a single bit.** A future operation that needs to distinguish them at run-time (e.g., for diagnostic dumps, or for a future `Pmm::release_reserved_range` recovery) cannot do so without a second bitmap or external metadata. *Mitigation:* the cached `reserved_count` + `allocated_count` counters give the *aggregate* discrimination; per-frame discrimination is genuinely deferred. The closing-this-gap ADR is forward-flagged in §References.
- **The PMM is single-core only (cooperative).** No atomic bitmap operations; no per-CPU caches. *Mitigation:* v1 is single-core by ADR-0008; a future SMP ADR will need to extend the PMM with per-core caches + atomic bit-set / clear (or with a global lock + IRQ disable, both bounded acceptable extensions).
- **Bitmap of 32 K bits is fixed at compile time** for the v1 BSP. Future BSPs (Pi 4 with 4–8 GiB) need a larger bitmap; the BSP-init API supports it (the bitmap size is parameterised), but the storage backing must be sized at compile time per BSP. *Mitigation:* every BSP carries its own `PMM_BITMAP_BYTES` constant; the kernel crate's PMM is generic over the bitmap-size const.

### Neutral

- **No change to ADR-0017's IPC primitive set.** The PMM is internal infrastructure; user-observable IPC primitives (`send` / `recv` / `notify`) are untouched.
- **No change to `SchedError` / `IpcError` taxonomies.** PMM failures surface as `MmuError::OutOfFrames` (already in the enum) when called via `FrameProvider`; direct PMM API failures surface as `PmmError` (a new enum, scoped to PMM-internal callers).
- **The `FrameProvider` trait is unchanged.** v1's ADR-0009 surface accepts the new PMM impl without any Revision rider. The PMM is the first real `FrameProvider` impl outside the host-test `VecFrameProvider`; integration is via the existing trait method, no new HAL surface.
- **No new ADR governance burden.** This ADR follows the [`write-adr` skill §Simulation](../../.claude/skills/write-adr/SKILL.md) discipline (codified in commit `77a578a`); the §Dependency chain section satisfies [ADR-0025 §Rule 1](0025-adr-governance-amendments.md) (every forward-reference is grounded in T-017 which opens with this ADR's Propose commit).

## Pros and cons of the options

### Option A — Bitmap allocator with hint pointer (chosen)

- Pro: lowest metadata footprint (4 KiB for 128 MiB).
- Pro: regime-independent (high-half-portable).
- Pro: trivial reservation tracking (init walks the list, sets bits).
- Pro: zero new `unsafe` audit entries.
- Pro: deterministic worst-case (O(N) bounded by RAM size).
- Con: O(N) worst-case scan; under fragmentation pressure beyond v1's reach, may need extension.
- Con: single-bit Reserved-vs-Allocated collapse forecloses certain future per-frame discrimination operations without a second bitmap.

### Option B — External free-list (LIFO stack of frame indices)

- Pro: O(1) alloc / free, zero scan.
- Pro: regime-independent (stack lives in `.bss`).
- Con: 128 KiB metadata for 128 MiB — 32× the bitmap. Out of proportion to the v1 win.
- Con: reservation tracking awkward — must enumerate every Free frame into the stack at init (32 K writes), versus the bitmap's per-range bit-set.
- Con: no random-access "is frame X allocated?" without traversing the stack; debug + `free_frame` validation become O(N).

### Option C — In-frame free-list (linked list using free frames as next-pointers)

- Pro: O(1) alloc / free.
- Pro: minimal metadata (16 bytes).
- Con: regime-dependent — high-half migration requires `PA → VA` translation when reading next-pointers; we'd lock in the translation discipline today for a v1 use case that doesn't need it.
- Con: every alloc / free reads / writes the frame contents, adding `unsafe` writes — increases audit-log surface for no v1 benefit.
- Con: reservation tracking awkward — must thread reserved frames into the list initially and skip them on alloc; bookkeeping is more complex than the bitmap's per-bit walk.
- Con: validation ("is this PA a valid PMM frame?") requires traversing the list, O(N).

### Option D — Buddy allocator

- Pro: variable-size support; fragmentation handling.
- Pro: O(log N) alloc / free.
- Con: massive overkill for v1 (single-size 4 KiB allocations only).
- Con: complex implementation (~500 LOC of split / merge logic) versus bitmap's ~150 LOC.
- Con: requires bitmap + level metadata; more storage than Option A despite the algorithmic appeal.
- Con: pre-pays the cost of variable-size support without obtaining the benefit until a future ADR introduces a caller. Conflicts with CLAUDE.md non-negotiable #6 (methodical pace).

## References

- [ADR-0009 — `Mmu` HAL trait signature (v1)](0009-mmu-trait.md) — the trait this PMM implements (`FrameProvider`).
- [ADR-0012 — Boot flow and memory layout for `bsp-qemu-virt`](0012-boot-flow-qemu-virt.md) — physical RAM extent + reserved-region origins.
- [ADR-0027 — Kernel virtual memory layout (B2 — identity-mapped MMU activation)](0027-kernel-virtual-memory-layout.md) — the MMU layer above this PMM; `Mmu::map`'s `&mut dyn FrameProvider` is the v1 caller.
- [`docs/architecture/memory-management.md`](../architecture/memory-management.md) §"Frame allocation discipline" — the chapter this ADR resolves an open question for.
- [`docs/audits/unsafe-log.md`](../audits/unsafe-log.md) — UNSAFE-2026-0001 will gain an Amendment for the PMM frame-zeroing site (T-017).
- [`docs/standards/unsafe-policy.md`](../standards/unsafe-policy.md) — the audit-discipline contract.
- xv6 PMM (in-frame linked-list of free frames) — prior art for Option C; rejected for forward-compat reasons above.
- Linux's `bootmem` allocator (`mm/bootmem.c` / `mm/memblock.c`) — direct prior art for the bitmap shape Option A adopts; Linux replaces bootmem with `buddy` post-init, which is the natural future ADR slot if v1's bitmap surfaces a fragmentation hot path.
- seL4's untyped-region model (`src/object/untyped.c`) — capability-mediated frame ownership; forward-flag for B5+ `MemoryRegionCap` work that sits on top of this PMM.
- Hubris's allocator-less stance (`hubris/lib/userlib/src/lib.rs`) — Cortex-M target with no MMU; direct comparison shows that the PMM only matters when the architecture has an MMU active, which Tyrne does post-T-016.
