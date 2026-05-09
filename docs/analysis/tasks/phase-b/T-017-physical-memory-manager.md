# T-017 — Physical Memory Manager (PMM): bitmap allocator + reservation tracking + `FrameProvider` impl

- **Phase:** B
- **Milestone:** B3 — Address space abstraction (PMM is the prerequisite layer; opens with this task, AddressSpace data structure follows in ADR-0028 + T-018)
- **Status:** Draft
- **Created:** 2026-05-09
- **Author:** @cemililik (+ Claude Opus 4.7 agent)
- **Dependencies:** [ADR-0035](../../../decisions/0035-physical-memory-manager.md) — must be `Accepted` before code lands.
- **Informs:** Unblocks [phase-b.md §B3 §3 "Map / unmap operations"](../../../roadmap/phases/phase-b.md#milestone-b3--address-space-abstraction) — runtime `Mmu::map` callers can now obtain frames for intermediate page tables. Unblocks B3 §2 (`AddressSpace` kernel object — needs frames for its root translation table). Sets up the future `MemoryRegionCap` capability layer (B5+) without locking it in today.
- **ADRs required:** [ADR-0035](../../../decisions/0035-physical-memory-manager.md). Touches no prior ADR's §Revision notes (the [`FrameProvider`](../../../../hal/src/mmu/mod.rs) trait surface from [ADR-0009](../../../decisions/0009-mmu-trait.md) is unchanged — PMM is the first real impl outside the host-test `VecFrameProvider`). No supersession.

---

## User story

As the Tyrne kernel, I want a Physical Memory Manager (PMM) that tracks the 32 768 frames of QEMU virt's 128 MiB RAM via a bitmap, marks the kernel image / `.boot_pt` / boot stack as reserved at boot, and implements [`tyrne_hal::FrameProvider`](../../../../hal/src/mmu/mod.rs) so that any post-bootstrap caller of [`Mmu::map`](../../../../hal/src/mmu/mod.rs) can obtain page-aligned, zero-initialised frames for intermediate translation tables and (in B3+) per-task user mappings.

## Context

[ADR-0035](../../../decisions/0035-physical-memory-manager.md) settles the PMM design: bitmap allocator with hint pointer, one bit per frame, reservation tracking via `Pmm::new(extent, &reserved)`, no `unsafe` in the PMM body, frame-zeroing under a UNSAFE-2026-0001 Amendment (no new audit-log entry). The four constraints driving the choice — bounded RAM (32 K frames) makes O(N) scan acceptable; lowest metadata footprint (4 KiB) of any option; forward-portable to high-half kernel; trivial reservation tracking — are recorded in the ADR's §Decision drivers + §Decision outcome.

T-017 is the implementation of those decisions per the ADR's §Dependency chain: a single bundled task covering the new `kernel/src/mm/pmm.rs` module + BSP wiring + host tests + audit-log Amendment + companion architecture-doc cross-references. The task's shape mirrors [T-016](T-016-mmu-activation.md) (which bundled HAL trait extension + BSP impl + bootstrap routine + audit entries + cross-references in one task); the implementation may land across multiple commits within the same task.

## Acceptance criteria

- [ ] **ADR-0035 Accepted** before code lands. Same-day Accept after careful re-read is permitted per [ADR-0025 §Revision notes](../../../decisions/0025-adr-governance-amendments.md); Propose commit is separate from the Accept commit per [`write-adr` skill §10](../../../../.claude/skills/write-adr/SKILL.md).

### PMM module (`kernel/src/mm/pmm.rs`)

- [ ] **New module `kernel/src/mm/`** at the top level of the kernel crate. Contains `pmm.rs` today; will host `address_space.rs` for B3 §2 (ADR-0028 / T-018) tomorrow. The `kernel/src/mm/mod.rs` parent declares the submodules and re-exports the public surface.
- [ ] **`Pmm` struct** in `kernel/src/mm/pmm.rs`, parameterised as `Pmm<const N: usize, const R: usize>` (consistent with the existing `PMM_BITMAP_BYTES` per-BSP-const-generic pattern; reviewer Finding 8.2). Fields:
  - `bitmap: [u8; N]` — one bit per frame; bit `i` set ⇔ frame `i` is Allocated or Reserved (the bitmap collapses Reserved + Allocated into a single bit per ADR-0035 §Negative consequences).
  - `extent: PhysFrameRange` — the BSP-provided physical-RAM range this PMM manages.
  - `reserved_ranges: [Option<PhysFrameRange>; R]` — fixed-size cached copy of the BSP-provided reservation list, so `free_frame` can defensively reject Reserved-frame-PA inputs (per [ADR-0035 §Simulation §Step 2 Critical row](../../../decisions/0035-physical-memory-manager.md#simulation)). The BSP picks `R` to fit its reservation list; v1's `bsp-qemu-virt` picks `R = 8`. The defensive scan in `free_frame` iterates only the `Some(_)` slots (O(populated-entries), not O(R)).
  - `hint: usize` — the next frame index to scan from on alloc.
  - `free_count: usize` — cached count of frames in `Free` state.
  - `reserved_count: usize` — cached count of frames in `Reserved` state (set at init; never changes post-init).
  - `allocated_count: usize` — cached count of frames in `Allocated` state.
- [ ] **`PMM_BITMAP_BYTES` const** parameterised per-BSP over the frame count. For QEMU virt's 128 MiB / 4 KiB = 32 768 frames, `PMM_BITMAP_BYTES = 4096`. The kernel crate exposes the type as `Pmm<const N: usize, const R: usize>`; each BSP picks both consts at instantiation time.
- [ ] **`R` (reservation-list capacity) per-BSP const generic.** v1's `bsp-qemu-virt` picks `R = 8` to cover its 3 ranges (kernel image + `.boot_pt` + boot stack) plus headroom. The per-BSP-const-generic shape (rather than a kernel-wide constant) accommodates future BSPs whose reservation list could include DTB-reserved + ATF / ARM Trusted Firmware secure-world reservations + ACPI tables + initrd + framebuffer-reservation regions (typical aarch64 boot stacks land 7–9 ranges; a kernel-wide const would force every BSP onto v1's choice). The BSP-side panic on `Pmm::new(...).expect("reserved-range list exceeds R")` is structurally unreachable when `R` is sized correctly per the BSP's static reservation list.
- [ ] **`Pmm::new(extent, reserved)`** constructor — `pub fn new(extent: PhysFrameRange, reserved: &[PhysFrameRange]) -> Result<Self, PmmError>`. Validates `reserved.len() <= R` (returns `Err(TooManyReservedRanges)` otherwise); walks the reserved-range list and sets every covered frame's bit to 1 (Reserved); copies the list into the `reserved_ranges` array (remaining slots `None`) for `free_frame`'s defensive validation; sets `hint` to the first frame *not* in any reserved range; computes `free_count` / `reserved_count` / `allocated_count` initial values. **Safe Rust**; no `unsafe`. Per [ADR-0035 §Simulation §Step 0](../../../decisions/0035-physical-memory-manager.md#simulation).
- [ ] **`Pmm::alloc_frame() -> Option<PhysFrame>`** — scans bitmap from `hint` forward for a 0 bit; on hit, sets the bit, zero-fills the 4 KiB frame contents via `core::ptr::write_bytes`, advances `hint`, decrements `free_count`, increments `allocated_count`, returns `Some(PhysFrame)`. On miss (forward + wrap scan both empty), returns `None`. Per [ADR-0035 §Simulation §Steps 1, 3](../../../decisions/0035-physical-memory-manager.md#simulation).
- [ ] **`Pmm::free_frame(frame: PhysFrame) -> Result<(), PmmError>`** — computes frame index from PA; defensively rejects via a scan of the `Some(_)` slots in `reserved_ranges` (iterates only populated entries — `iter().flatten()` over the `[Option<PhysFrameRange>; R]` array; cost is O(populated-entries), not O(R)) — if the frame falls in any reserved range, returns `Err(PmmError::DoubleFree)` without mutation; reads the bit — if `0` (already Free), returns `Err(PmmError::DoubleFree)`; if `1` (Allocated), clears the bit; rewinds `hint = min(hint, i)`; increments `free_count`; decrements `allocated_count`. Per [ADR-0035 §Simulation §Step 2](../../../decisions/0035-physical-memory-manager.md#simulation). The Reserved-vs-Allocated bitmap-collapse plus the explicit reserved-range check is the v1 trade-off: ~128 bytes of cached range metadata (R × 16 bytes) buys defensive `free_frame(reserved_pa)` rejection without doubling the bitmap.
- [ ] **`Pmm::stats() -> PmmStats`** — returns `{ total_frames, reserved_frames, allocated_frames, free_frames }`. Diagnostic surface for host tests + future runtime self-checks. Per [ADR-0035 §Simulation §Step 4](../../../decisions/0035-physical-memory-manager.md#simulation).
- [ ] **`PmmError` enum** — `#[non_exhaustive]`. Variants: `OutOfRange` (frame PA outside the managed `extent`), `MisalignedAddress` (frame PA not 4 KiB-aligned — should be unreachable through `PhysFrame` construction but defensively named), `DoubleFree` (frame was already Free or its PA falls in a reserved range), `TooManyReservedRanges` (caller passed more than `MAX_RESERVED_RANGES` to `Pmm::new`).
- [ ] **`PhysFrameRange` struct** — `(start: PhysAddr, end: PhysAddr)` half-open range. Lives in `kernel/src/mm/mod.rs` (or `tyrne_hal::mmu` if a future ADR generalises it). v1: kernel-internal type.
- [ ] **`impl FrameProvider for Pmm`** — `fn alloc_frame(&mut self) -> Option<PhysFrame>` delegates to `Pmm::alloc_frame`. The trait surface is unchanged from [ADR-0009](../../../decisions/0009-mmu-trait.md).

### BSP wiring (`bsp-qemu-virt/src/main.rs`)

- [ ] **Compute reserved-range list at boot.** From linker symbols:
  - `[__bss_start, __bss_end)` covers the kernel image's `.bss` (which already contains `.boot_pt` per T-016's linker-script extension).
  - `[__stack_top - 64K, __stack_top)` covers the boot stack.
  - The kernel image's `.text` / `.rodata` / `.data` lives in PA `0x4008_0000..__bss_start`; this is also reserved.
  - Total: 1 to 3 `PhysFrameRange` entries depending on linker-script layout (ranges may merge if contiguous).
- [ ] **`PMM` `StaticCell`** — published before any `Mmu::map`-using code path. Initialised in `kernel_entry` immediately after `mmu_bootstrap()` returns and before GIC init (so PMM-allocated frames are usable for any post-bootstrap mapping). The `Pmm::new` call uses `.expect("reserved-range list exceeds MAX_RESERVED_RANGES")` because the BSP-side reservation list is statically known at compile time; the `expect` is structurally unreachable in v1 (3 ranges < 8 limit) but documents the kernel-discipline contract. Order:
  ```
  cpu.now_ns() snapshot
  → mmu_bootstrap()
  → "tyrne: mmu activated" print
  → PMM init (Pmm::new(extent, &reserved))
  → "tyrne: pmm initialized (N frames available; M reserved)" print
  → GIC init + DAIF unmask
  → timer banner
  → demo
  ```
- [ ] **No use of the PMM by the v1 demo.** The cooperative IPC demo continues to ride the bootstrap mappings; the PMM is published but unused at runtime in v1. T-018 (B3 §2 AddressSpace) is the first runtime caller. The PMM init succeeds on every boot and its stats are reported in the banner; integration health is verified by the smoke trace's new line.

### Host tests (`kernel/src/mm/pmm.rs::tests`)

- [ ] **`new_marks_reserved_ranges_and_initialises_counters`** — pin the §Simulation §Step 0 contract: a `Pmm::new` with a reserved range marks every covered frame's bit set, sets `hint` to the first non-reserved frame, computes `free_count` / `reserved_count` correctly.
- [ ] **`alloc_frame_returns_first_free_and_zeroes_payload`** — pin §Simulation §Step 1: alloc returns the lowest-indexed free frame; the returned frame's contents are all zeroes (memset-derived); subsequent allocs skip the just-allocated frame.
- [ ] **`free_frame_clears_bit_and_rewinds_hint`** — pin §Simulation §Step 2 happy path: free of an Allocated frame rewinds the hint and reclaims the frame for the next alloc.
- [ ] **`free_frame_rejects_double_free_and_reserved`** — pin §Simulation §Step 2 critical row: `free_frame(reserved_frame)` returns `PmmError::DoubleFree` and leaves the bitmap unchanged; `free_frame(already_free_frame)` likewise.
- [ ] **`alloc_frame_returns_none_when_exhausted`** — pin §Simulation §Step 3: after allocating every Free frame, `alloc_frame` returns `None`; the cached `free_count` is 0; the bitmap is fully set.
- [ ] **`alloc_frame_recovers_after_free_under_exhaustion`** — interleave: allocate-all → free-one → alloc-one returns the just-freed frame (the rewind discipline prevents the wrap from re-handing-out a different frame).
- [ ] **`stats_parity_with_bitmap_bit_count`** — for a fully-instantiated PMM under randomised alloc / free patterns, `Pmm::stats().free_frames` matches the bitmap's 0-bit count. Cross-check against the cached counter.
- [ ] **`alloc_frame_implements_frame_provider`** — exercise the trait method via `&mut dyn FrameProvider` to confirm the impl integration.
- [ ] **`new_rejects_too_many_reserved_ranges`** — `Pmm::new(extent, &[range × (MAX_RESERVED_RANGES + 1)])` returns `Err(TooManyReservedRanges)`; the bitmap is not partially mutated.
- [ ] **`free_frame_reserved_check_iterates_only_populated_slots`** — pin the [§Simulation §Step 2 Critical row](../../../decisions/0035-physical-memory-manager.md#simulation) defensive scan's contract that it ignores the `None` slots in the `[Option<PhysFrameRange>; R]` array (i.e., is genuinely O(populated-entries), not O(R)). Strategy: instantiate `Pmm<N, 8>` with only 1 populated reserved range; call `free_frame` against a frame whose PA falls *outside* every populated range but whose computed index would happen to fall inside an uninitialised-`None`-slot's "interpreted-as-range" interpretation if the scan accidentally treated `None` as a wildcard. The test passes if the call returns `Ok(())` (the `None` slots are correctly skipped). Combines with test #4 to cover both the "reserved-PA correctly rejected" path (test #4) and the "non-reserved-PA correctly accepted under partially-populated array" path (this test).

### Audit-log update (`docs/audits/unsafe-log.md`)

- [ ] **UNSAFE-2026-0001 Amendment** — adds the PMM frame-zeroing site as a sanctioned operation under the existing umbrella ("kernel-static raw-pointer + MMIO + zero-init"). The Amendment names: (a) location (`kernel/src/mm/pmm.rs::Pmm::alloc_frame`); (b) operation (`core::ptr::write_bytes` over a 4 KiB region whose PA is identity-mapped to a kernel-readable VA per ADR-0027); (c) invariants (frame is exclusively-owned by the PMM at this moment because the bitmap has just been atomically set; the PA is page-aligned by `PhysFrame::from_aligned`'s contract; the 4 KiB region is entirely within the BSP-provided extent which is verified to fit in the identity-mapped RAM range); (d) rejected alternatives (a `&mut [u8; 4096]` materialised from a raw pointer would *be* the audited operation — wrapping it in safe-looking syntax would obscure the audit point). **No new entry.**
- [ ] **All `unsafe` discipline** ([unsafe-policy.md §1](../../../standards/unsafe-policy.md)) holds: the `unsafe` block in `Pmm::alloc_frame` carries a `// SAFETY:` comment naming the audit reference (UNSAFE-2026-0001 Amendment).

### Documentation

- [ ] **[`docs/architecture/memory-management.md`](../../../architecture/memory-management.md) §"Frame allocation discipline"** — replace the "PMM is not part of T-016" paragraph with the post-T-017 reality: PMM is live; bitmap; reservation list; `FrameProvider` impl. Cross-link to ADR-0035.
- [ ] **[`docs/architecture/boot.md`](../../../architecture/boot.md)** — extend the Stage-3 sequence diagram with the new PMM-init step. Cross-link to ADR-0035.
- [ ] **No update needed for [`docs/architecture/hal.md`](../../../architecture/hal.md) §Mmu** — the `FrameProvider` trait surface is unchanged; the existing `Memory allocation for page tables is not the HAL's job — the kernel owns a physical-frame allocator and hands the HAL frames to fill in.` line is now true at runtime as well as at boot.

### Verification gates

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo host-clippy` clean (`-D warnings`).
- [ ] `cargo kernel-clippy` clean (`-D warnings`).
- [ ] `cargo host-test` passes — expected ~195 (current 185 + 10 new PMM tests: 8 core scenarios + 2 reserved-range guards added per ADR-0035 §Simulation §Step 2 Critical row).
- [ ] `cargo +nightly miri test` passes on the same set.
- [ ] `cargo kernel-build` clean.
- [ ] **QEMU smoke unchanged plus one new line.** The post-T-017 trace adds `tyrne: pmm initialized (N frames available; M reserved)` immediately after `tyrne: mmu activated` and before `tyrne: timer ready (...)`. Concrete numbers for QEMU virt's 128 MiB layout: total = 32 768 frames; reserved = ~30 frames (kernel image ~6 frames + `.boot_pt` 4 frames + `.bss` non-`.boot_pt` ~4 frames + 64 KiB stack = 16 frames + alignment); free = ~32 738. Exact numbers depend on linker-script layout — pin the expected values in a host test that mocks the BSP's reserved list.
- [ ] **`-d int,unimp,guest_errors` window unchanged.** Adds at most +21 PL011 "data written to disabled UART" instances per the new banner line (one per byte of the new UART message). No new fault classes.

## Out of scope

- **Variable-size allocations.** v1's PMM hands out single 4 KiB frames; ADR-0009 commits to single-size at the trait surface. A future ADR introduces variable-size if needed (likely paired with a buddy-style allocator extension).
- **Per-CPU caching / lock-free atomic bitmap operations.** v1 is single-core + cooperative; no concurrency hazard. SMP extension is post-Phase-C work.
- **NUMA awareness.** Single-node v1; future Pi 4 BSP is also UMA. NUMA awareness lands when a multi-node target arrives.
- **Hot-plug / dynamic memory addition.** RAM extent is fixed at boot; `Pmm::new` is one-shot.
- **Swap / paging-out.** v1 has no backing store; allocated frames stay in RAM until freed.
- **`MemoryRegionCap` capability-system integration.** v1's PMM is kernel-internal; the future capability layer (B5+) wraps `Pmm::alloc_frame` with a capability check before handing out the frame to userspace.
- **`Pmm::release_reserved_range` recovery API.** ADR-0035 §Negative consequences flags this; introducing it requires a second bitmap or external metadata to distinguish Reserved-from-Allocated. Out of v1 scope.
- **`Pmm::alloc_contiguous(n)`.** No v1 caller needs > 1 frame contiguous; future DMA buffer / huge-page work introduces it via a separate ADR.
- **Free-frame defragmentation.** No fragmentation pressure in v1; defrag is a future B5+ concern.

## Approach

The implementation lands in roughly four independently-bisectable commits. Each commit ends green (`cargo host-test`, `kernel-clippy`); QEMU smoke is verified at the *end* of the chain (after step 4) because intermediate commits leave the PMM module wired but not yet booted.

1. **`kernel/src/mm/pmm.rs` — `Pmm` struct + bitmap arithmetic + `Pmm::new` constructor.** Reservation marking; counter init; hint-pointer init; reserved-range cache copy; `R`-cap validation. Pure safe Rust; bitmap operations are `[u8]` integer arithmetic. Host tests added: `new_marks_reserved_ranges_and_initialises_counters` + `new_rejects_too_many_reserved_ranges` (both pin `Pmm::new`, so they land in the constructor commit). No `unsafe` in this commit.
2. **`kernel/src/mm/pmm.rs` — `alloc_frame` / `free_frame` / `stats`.** Adds the frame-zeroing `unsafe` block (covered by UNSAFE-2026-0001 Amendment, OR a new audit entry per the §Dependency-chain-step-5 adjudication-deferred caveat — security-review verdict at this commit's review). Host tests added: alloc/free round-trip + `OutOfFrames` + `DoubleFree` rejection (free + reserved-range) + populated-slots-only defensive scan + recovery-after-free + `stats` parity. The Amendment (or new entry) lands in `docs/audits/unsafe-log.md` in this commit.
3. **`kernel/src/mm/pmm.rs` — `impl FrameProvider for Pmm`** + the `kernel/src/mm/mod.rs` parent module. Host test: `alloc_frame_implements_frame_provider`. Trivial layer-on-top.
4. **`bsp-qemu-virt/src/main.rs` — PMM publication + smoke verification.** Computes reserved ranges from linker symbols; instantiates the `Pmm`; publishes it in a `StaticCell`; prints the banner. Run QEMU smoke; verify `tyrne: pmm initialized (...)` appears in the expected position; verify `-d int,unimp,guest_errors` window is unchanged.

## Definition of done

- [ ] All Acceptance criteria checked.
- [ ] All four commits are independently bisectable (each ends with green host-tests + clippy + kernel-build).
- [ ] QEMU smoke trace matches the post-T-016 baseline plus one new `tyrne: pmm initialized (N frames available; M reserved)` line.
- [ ] `cargo +nightly miri test` is green.
- [ ] No new `unsafe` block lacks a `// SAFETY:` comment naming UNSAFE-2026-0001's Amendment per [unsafe-policy.md §1](../../../standards/unsafe-policy.md).
- [ ] [`docs/audits/unsafe-log.md`](../../../audits/unsafe-log.md) UNSAFE-2026-0001 gains the PMM frame-zeroing Amendment.
- [ ] No new audit-log entry — confirms ADR-0035's "zero new audit entries" promise byte-for-byte.
- [ ] [`docs/architecture/memory-management.md`](../../../architecture/memory-management.md) §"Frame allocation discipline" reflects the post-T-017 reality (PMM live; bitmap; reservation list).
- [ ] [`docs/architecture/boot.md`](../../../architecture/boot.md) Stage-3 sequence diagram extended with the new PMM-init step.
- [ ] [`docs/roadmap/current.md`](../../../roadmap/current.md) updated: T-017 → Done; ADR-0035 → Accepted; B3 status → "PMM live; AddressSpace abstraction (T-018) next".
- [ ] [`docs/roadmap/phases/phase-b.md`](../../../roadmap/phases/phase-b.md) ADR ledger row for ADR-0035 → Accepted; T-017 marked Done; B3 §1 (PMM) flipped to ✅.
- [ ] Commit messages follow [`commit-style.md`](../../../standards/commit-style.md) with `Refs: ADR-0035, T-017` trailers and `Audit: UNSAFE-2026-0001` trailer where applicable.
- [ ] PR description includes the §Simulation table from [ADR-0035 §Decision outcome / §Simulation](../../../decisions/0035-physical-memory-manager.md#simulation) and the post-PMM smoke trace verbatim.

## Design notes

- **Why one bundled task instead of T-017a + T-017b + T-017c?** Mirrors [T-016](T-016-mmu-activation.md) which bundled HAL trait extension + BSP impl + bootstrap routine + audit entries + cross-references in one task. The split would be artificial: PMM module + BSP wiring + audit Amendment + doc updates are tightly coupled at the implementation level. A single task with four bisectable commits is cleaner than three tasks each with one or two commits.
- **Why bitmap and not free-list?** Per [ADR-0035 §Decision outcome](../../../decisions/0035-physical-memory-manager.md#decision-outcome). Forward-portable to high-half kernel; lowest metadata; trivial reservation tracking; zero new `unsafe` audits. The trade-off (O(N) worst-case scan) is microseconds-class for v1's bounded RAM.
- **Why `Pmm::new(extent, reserved)` and not `Pmm::new(extent)` + `Pmm::reserve(range)` separately?** One-shot init is a structural property the design relies on (the §Simulation row 0 contract). Allowing `reserve(range)` after `new` would require deciding "what if a Reserved frame is currently Allocated to a runtime caller?" — which has no good answer in v1. Forcing all reservations into the constructor is the simpler discipline.
- **Why frame-zeroing in `alloc_frame` and not lazy / explicit?** [ADR-0009](../../../decisions/0009-mmu-trait.md) `FrameProvider` contract requires zero-initialised frames. Enforcing the zero at alloc time (via `core::ptr::write_bytes`) is the safe-Rust honest implementation. Lazy zeroing (zero on first use) requires page-fault routing into the PMM, which v1 doesn't have. Explicit zeroing (caller-driven `Pmm::alloc_frame_uninit` + `frame.zero()`) doubles the API surface for no v1 win.
- **Why is the PMM kernel-internal, not HAL-level?** The `FrameProvider` trait is HAL (it abstracts over platform-specific frame sources, e.g., a v1 PMM vs. a future Pi 4 PMM with different reserved regions). The PMM impl itself is kernel-level because it owns the bitmap storage and the kernel-side reservation discipline. BSPs vary the *list* of reserved ranges + the *extent*; the bitmap algorithm is portable.
- **Why is `PhysFrameRange` in `kernel/src/mm/` and not `tyrne_hal::mmu`?** v1 only kernel callers exist; the kernel mm module is the natural host. If a future BSP wants to expose its own reserved-range type to the HAL, this gets generalised — but generalising before the second caller exists is premature per CLAUDE.md non-negotiable #6.
- **Why no separate Reserved-vs-Allocated bit?** [ADR-0035 §Negative consequences](../../../decisions/0035-physical-memory-manager.md#negative). Single-bit collapse forces the discrimination into the *kernel* code that calls `Pmm::new` (no caller ever holds a `PhysFrame` covering a Reserved range; reservation is one-way at init); a future operation that needs per-frame discrimination will require a second bitmap or external metadata. The cost of carrying that overhead today (an extra 4 KiB of `.bss`) outweighs the v1 benefit.

## References

- [ADR-0035 — Physical Memory Manager (B3 prerequisite — bitmap allocator)](../../../decisions/0035-physical-memory-manager.md) — the load-bearing design document.
- [ADR-0009 — `Mmu` HAL trait signature (v1)](../../../decisions/0009-mmu-trait.md) — the trait this task's `Pmm` impls (`FrameProvider`).
- [ADR-0012 — Boot flow and memory layout for `bsp-qemu-virt`](../../../decisions/0012-boot-flow-qemu-virt.md) — the static image layout this task's reservation list inherits.
- [ADR-0027 — Kernel virtual memory layout (B2 — identity-mapped MMU activation)](../../../decisions/0027-kernel-virtual-memory-layout.md) — the MMU layer above this PMM; identity mapping is what makes the frame-zeroing safe (PA is identity-mapped to a kernel-readable VA).
- [`docs/architecture/memory-management.md`](../../../architecture/memory-management.md) — companion architecture chapter; §"Frame allocation discipline" is the section this task resolves an open question for.
- [`docs/audits/unsafe-log.md`](../../../audits/unsafe-log.md) — UNSAFE-2026-0001 gains the PMM frame-zeroing Amendment.
- [`docs/standards/unsafe-policy.md`](../../../standards/unsafe-policy.md) — audit-discipline contract.
- [`hal/src/mmu/mod.rs`](../../../../hal/src/mmu/mod.rs) — `FrameProvider` trait the PMM implements.
- [T-016 — Activate MMU with identity-mapped kernel + `MapperFlush` token discipline](T-016-mmu-activation.md) — task-shape precedent for bundling multiple-concern work in one task.
- xv6 PMM (`kalloc.c`) — in-frame linked-list prior art (rejected for forward-compat reasons in ADR-0035 §Pros and cons).
- Linux's `bootmem` allocator (`mm/bootmem.c`) — direct prior art for the bitmap shape.
- seL4 untyped-region model — capability-mediated frame ownership; forward-flag for B5+ MemoryRegionCap layer.

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-05-09 | @cemililik (+ Claude Opus 4.7 agent) | Opened with status `Draft`, paired with ADR-0035 (`Proposed`) per [ADR-0025 §Rule 1](../../../decisions/0025-adr-governance-amendments.md) (forward-reference contract) — ADR-0035's *Dependency chain* requires a real T-NNN file for the implementation step; this task is that file. Will move to `In Progress` only after ADR-0035 is `Accepted`. |
