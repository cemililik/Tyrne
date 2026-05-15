//! Physical Memory Manager (PMM) — bitmap allocator per [ADR-0035].
//!
//! The PMM tracks the kernel's physical-RAM extent via a bitmap (one
//! bit per [`PAGE_SIZE`]-frame), reserves regions handed at init
//! (kernel image / `.boot_pt` / boot stack), and implements
//! [`tyrne_hal::FrameProvider`] for runtime [`Mmu::map`] callers.
//!
//! See [ADR-0035] for the design (bitmap vs. free-list trade-offs,
//! reservation tracking, forward-portability to high-half kernel) and
//! [T-017] for the implementation arc this file lands across four
//! bisectable commits.
//!
//! Commit 1 (this file, initial landing): `Pmm` struct + bitmap
//! arithmetic + `Pmm::new` constructor + four host tests pinning
//! `Pmm::new`'s contract. No `unsafe`. The next commit adds
//! `alloc_frame` / `free_frame` / `stats`.
//!
//! [ADR-0035]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0035-physical-memory-manager.md
//! [T-017]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-b/T-017-physical-memory-manager.md
//! [`Mmu::map`]: tyrne_hal::Mmu::map

use tyrne_hal::{FrameProvider, PhysAddr, PhysFrame, PAGE_SIZE};

use crate::mm::PhysFrameRange;

/// Errors returned by the Physical Memory Manager.
///
/// `#[non_exhaustive]` so future variants (e.g., `Fragmented` for a
/// `alloc_contiguous` extension, or per-cap-grant errors when
/// `MemoryRegionCap` lands in B5+) can be added without breaking
/// existing match patterns.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PmmError {
    /// `Pmm::new` rejected an `extent` whose `start` or `end` is not
    /// [`PAGE_SIZE`]-aligned, or `free_frame` rejected a frame whose
    /// PA is not aligned (defensive — `PhysFrame::from_aligned`
    /// already enforces alignment for any value reachable through
    /// the trait, but the variant exists for completeness).
    MisalignedAddress,
    /// `Pmm::new` rejected a reserved range that does not fit inside
    /// `[extent.start, extent.end)`, or `free_frame` rejected a frame
    /// whose PA falls outside the managed extent.
    OutOfRange,
    /// `Pmm::new` rejected a reserved-list whose length exceeds the
    /// per-BSP `R` const-generic capacity of the cached
    /// `[Option<PhysFrameRange>; R]` array.
    TooManyReservedRanges,
    /// `Pmm::new` rejected a reserved-list whose entries overlap
    /// pairwise. Overlap is detected as `a.start < b.end && b.start
    /// < a.end` over every (i, j) pair with `i < j`. Without this
    /// check, overlapping ranges would double-count `reserved_count`
    /// vs. the bitmap (each overlapping frame contributes once to
    /// the bit-set but twice to the cached counter), leaving
    /// `stats()` inconsistent with the bitmap and risking
    /// `free_count = 0` while `alloc_frame()` still has free
    /// frames. See [PR #26 review-round 1](https://github.com/cemililik/Tyrne/pull/26).
    OverlappingReservedRanges,
    /// `free_frame` rejected an attempt to free a frame that is
    /// already Free, or whose PA falls in a Reserved range (the
    /// bitmap collapses Reserved + Allocated into a single bit;
    /// the cached reserved-range list is the discrimination
    /// mechanism, per [ADR-0035 §Simulation §Step 2 Critical row]).
    ///
    /// [ADR-0035 §Simulation §Step 2 Critical row]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0035-physical-memory-manager.md#simulation
    DoubleFree,
}

/// Diagnostic snapshot of PMM state.
///
/// Counters are cached inside `Pmm`; `stats()` reads them in O(1).
/// Useful for runtime self-checks, host-test invariants, and the
/// `tyrne: pmm initialized (...)` boot-banner format.
#[allow(
    clippy::struct_field_names,
    reason = "the `_frames` postfix on every field is intentional — it disambiguates \
              the four counter-classes (total / reserved / allocated / free) when \
              destructured at a `stats()` call site (e.g. `let PmmStats { free_frames, .. } = ...`); \
              renaming fields would lose the type-level signal"
)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PmmStats {
    /// Total number of frames in the managed extent.
    pub total_frames: usize,
    /// Frames marked Reserved at init (kernel image / `.boot_pt` /
    /// boot stack — never available for runtime alloc).
    pub reserved_frames: usize,
    /// Frames currently Allocated (handed to a caller via
    /// `alloc_frame`, not yet `free_frame`d).
    pub allocated_frames: usize,
    /// Frames currently Free (available for the next alloc).
    pub free_frames: usize,
}

/// Bitmap-based Physical Memory Manager.
///
/// `Pmm<N, R>` is generic over:
/// - `N` (`PMM_BITMAP_BYTES`): the bitmap-storage byte count, sized
///   per BSP to cover that BSP's physical-RAM extent (1 bit per
///   [`PAGE_SIZE`]-frame). For QEMU virt's 128 MiB, `N = 4096`.
/// - `R`: the per-BSP capacity of the cached reserved-range array.
///   `bsp-qemu-virt` picks `R = 8` to cover its 3 v1 reservations
///   (kernel image / `.boot_pt` / boot stack) plus headroom.
///
/// Per [ADR-0035 §Decision outcome][adr-0035].
///
/// [adr-0035]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0035-physical-memory-manager.md#decision-outcome
pub struct Pmm<const N: usize, const R: usize> {
    /// One bit per frame; bit `i` set ⇔ frame `i` is Allocated or
    /// Reserved (single-bit collapse per [ADR-0035 §Negative
    /// consequences]).
    bitmap: [u8; N],
    /// Managed physical-RAM extent.
    extent: PhysFrameRange,
    /// Cached copy of the BSP-provided reservation list (populated
    /// slots up to `reserved.len()`; remainder `None`). `free_frame`
    /// iterates only the `Some(_)` slots for its defensive scan.
    reserved_ranges: [Option<PhysFrameRange>; R],
    /// Forward-scan starting index for `alloc_frame`. Rewinds to
    /// the freed frame's index on `free_frame` to keep
    /// fragmentation from accumulating linearly.
    hint: usize,
    /// Cached `Free`-state counter (kept in sync with bitmap by
    /// every `alloc_frame` / `free_frame`).
    free_count: usize,
    /// Cached `Reserved`-state counter (set at init; never changes
    /// post-init).
    reserved_count: usize,
    /// Cached `Allocated`-state counter.
    allocated_count: usize,
    /// **Test-only failure injection.** When `Some(n)`, `alloc_frame`
    /// returns `None` after `n` further successful calls; production
    /// code has no setter for this field. Used by host tests in
    /// `obj::task_loader::tests` to drive the structurally-unreachable-
    /// in-v1 `LoadError::OutOfFrames` rollback path that is otherwise
    /// guarded out by the loader's frame-budget preflight.
    #[cfg(test)]
    alloc_failure_after: Option<usize>,
}

impl<const N: usize, const R: usize> Pmm<N, R> {
    /// Construct a PMM over `extent` with the given reserved ranges.
    ///
    /// Performs five **fail-fast** validations before any bitmap
    /// mutation, so partial-mutation states are structurally
    /// impossible:
    ///
    /// 1. `extent.start` and `extent.end` are [`PAGE_SIZE`]-aligned —
    ///    returns [`PmmError::MisalignedAddress`] otherwise.
    /// 2. The bitmap covers the extent (`extent.frame_count() <= N * 8`)
    ///    — returns [`PmmError::OutOfRange`] otherwise (BSP picked too
    ///    small an `N` for its extent).
    /// 3. `reserved.len() <= R` — returns [`PmmError::TooManyReservedRanges`]
    ///    otherwise.
    /// 4. Each reserved range is page-aligned, fits inside
    ///    `[extent.start, extent.end)`, and is non-inverted
    ///    (`range.end >= range.start`) — returns
    ///    [`PmmError::MisalignedAddress`] or [`PmmError::OutOfRange`]
    ///    otherwise.
    /// 5. No two reserved ranges overlap (pairwise half-open check) —
    ///    returns [`PmmError::OverlappingReservedRanges`] otherwise.
    ///    Touching boundaries (`[a, b)` + `[b, c)`) are accepted.
    ///
    /// Per [ADR-0035 §Simulation §Step 0][adr-0035]. The overlap check
    /// (step 5) was added in PR #26 round-1 review to prevent bitmap-
    /// vs-counter drift when two ranges share frames.
    ///
    /// # Errors
    ///
    /// Per the five validations above. On any error the constructor
    /// returns without partial state; the caller's `[u8; N]` storage
    /// is unobservably affected (a fresh `[0u8; N]` is constructed
    /// inside the function).
    ///
    /// # Panics
    ///
    /// Does not panic. The bitmap-sizing relationship between `N`
    /// and the extent's frame count is checked against
    /// `extent.frame_count() <= N * 8` and reported as
    /// [`PmmError::OutOfRange`] (no kernel-static panic).
    ///
    /// [adr-0035]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0035-physical-memory-manager.md#simulation
    pub fn new(extent: PhysFrameRange, reserved: &[PhysFrameRange]) -> Result<Self, PmmError> {
        // Validation (i): extent page-aligned.
        if !extent.is_aligned() {
            return Err(PmmError::MisalignedAddress);
        }

        let total_frames = extent.frame_count();

        // The bitmap must cover the extent. `N * 8` bits should be
        // ≥ total_frames; if a BSP picks too small an N for its
        // extent that's an init-time programming error, surfaced
        // here as OutOfRange (rather than a silent buffer
        // overflow at the per-range bit-set step below).
        if total_frames > N.saturating_mul(8) {
            return Err(PmmError::OutOfRange);
        }

        // Validation (iii): reserved-list capacity. Done before the
        // per-range range-bounds check so a too-long list fails
        // fast without iterating.
        if reserved.len() > R {
            return Err(PmmError::TooManyReservedRanges);
        }

        // Validation (ii): every reserved range fits inside
        // `[extent.start, extent.end)` AND each range's bounds are
        // page-aligned (otherwise the per-range bit-set loop below
        // would compute non-integer frame indices).
        for range in reserved {
            if !range.is_aligned() {
                return Err(PmmError::MisalignedAddress);
            }
            if range.start.0 < extent.start.0 || range.end.0 > extent.end.0 {
                return Err(PmmError::OutOfRange);
            }
            if range.end.0 < range.start.0 {
                return Err(PmmError::OutOfRange);
            }
        }

        // Validation (iv): pairwise overlap check. Two half-open
        // ranges `[a, b)` and `[c, d)` overlap iff `a < d && c <
        // b`. O(R²) but R ≤ 8 in v1 → ≤ 28 comparisons; trivial cost
        // and prevents the bitmap-vs-counter drift PR #26 review-
        // round 1 flagged (overlapping ranges would each increment
        // `reserved_count` while sharing bitmap bits — `stats()` ends
        // up inconsistent with the bitmap, and `free_count` can
        // report 0 while `alloc_frame()` still finds free frames).
        for (i, range_a) in reserved.iter().enumerate() {
            for range_b in reserved.iter().skip(i.saturating_add(1)) {
                if range_a.start.0 < range_b.end.0 && range_b.start.0 < range_a.end.0 {
                    return Err(PmmError::OverlappingReservedRanges);
                }
            }
        }

        // All validations passed; construct + populate.
        let mut bitmap = [0u8; N];
        let mut reserved_ranges: [Option<PhysFrameRange>; R] = [None; R];

        let mut reserved_count: usize = 0;
        for (idx, range) in reserved.iter().enumerate() {
            // Mark every covered frame as Reserved.
            //
            // Saturating sub keeps `clippy::arithmetic_side_effects`
            // happy; validation (ii) above guarantees
            // `range.start >= extent.start` so the saturation never
            // truncates in well-formed input.
            let start_idx = range
                .start
                .0
                .saturating_sub(extent.start.0)
                .wrapping_div(PAGE_SIZE);
            let frames_in_range = range.frame_count();
            for off in 0..frames_in_range {
                let frame_idx = start_idx.saturating_add(off);
                set_bit(&mut bitmap, frame_idx);
            }
            reserved_count = reserved_count.saturating_add(frames_in_range);
            reserved_ranges[idx] = Some(*range);
        }

        // Hint = first frame *not* in any reserved range. Walks the
        // bitmap forward from index 0; finds the first 0 bit.
        let hint = first_zero_bit(&bitmap, total_frames).unwrap_or(total_frames);

        let free_count = total_frames.saturating_sub(reserved_count);

        Ok(Self {
            bitmap,
            extent,
            reserved_ranges,
            hint,
            free_count,
            reserved_count,
            allocated_count: 0,
            #[cfg(test)]
            alloc_failure_after: None,
        })
    }

    /// Return the managed extent.
    #[must_use]
    pub fn extent(&self) -> PhysFrameRange {
        self.extent
    }

    /// Return a diagnostic snapshot of the PMM's frame-state
    /// counters.
    ///
    /// `total_frames` is anchored against [`PhysFrameRange::frame_count`]
    /// on the managed extent (rather than the sum of the three
    /// cached state counters) so a counter-drift bug surfaces as a
    /// stats-vs-extent disagreement rather than silently
    /// re-establishing internal consistency. The `stats_parity_with_bitmap_bit_count`
    /// host test pins the relationship.
    #[must_use]
    pub fn stats(&self) -> PmmStats {
        PmmStats {
            total_frames: self.extent.frame_count(),
            reserved_frames: self.reserved_count,
            allocated_frames: self.allocated_count,
            free_frames: self.free_count,
        }
    }

    /// Allocate one [`PAGE_SIZE`]-frame from the managed extent.
    ///
    /// Returns `Some(frame)` with the frame zero-initialised and
    /// caller-owned, or `None` if no Free frame remains. Callers
    /// reaching `Mmu::map` propagate `None` as
    /// `tyrne_hal::MmuError::OutOfFrames` per the
    /// [`tyrne_hal::FrameProvider`] contract.
    ///
    /// Algorithm: forward-from-`hint` linear scan for the first 0
    /// bit; on hit, sets the bit, advances `hint`, decrements
    /// `free_count`, increments `allocated_count`, zero-fills the
    /// 4 KiB frame contents via `core::ptr::write_bytes`. Per
    /// [ADR-0035 §Simulation §Step 1][adr-0035].
    ///
    /// The forward-from-hint scan is sufficient under v1's
    /// single-core cooperative model — the unconditional
    /// `hint = min(hint, freed_idx)` rewind in `free_frame` keeps
    /// `hint <= lowest-free-index`, so any free frame is reachable
    /// on the forward pass. Per [ADR-0035 §Simulation §Step 3][adr-0035]'s
    /// forward-compat note, a wrap-then-scan-prefix step would land
    /// when SMP per-core caches arrive; not v1.
    ///
    /// [adr-0035]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0035-physical-memory-manager.md#simulation
    pub fn alloc_frame(&mut self) -> Option<PhysFrame> {
        // Test-only failure injection (see `alloc_failure_after`'s
        // doc-comment). Runs before the production body so a forced
        // failure leaves the bitmap and counters byte-stable. The
        // `#[cfg(test)]` gate keeps this branch out of production
        // builds entirely.
        #[cfg(test)]
        {
            if let Some(remaining) = self.alloc_failure_after.as_mut() {
                if *remaining == 0 {
                    return None;
                }
                *remaining = remaining.saturating_sub(1);
            }
        }

        let total_frames = self.extent.frame_count();

        // Forward scan from `hint`. v1 single-core cooperative
        // discipline keeps `hint <= lowest-free-index` (per the
        // free_frame rewind below); the wrap pass is forward-compat
        // scaffolding for SMP per-core-caches, which v1 doesn't
        // need but we leave the wrap in place to keep the
        // future-extension path one-line clean.
        let mut idx_opt: Option<usize> =
            (self.hint..total_frames).find(|&idx| !read_bit(&self.bitmap, idx));
        if idx_opt.is_none() && self.hint > 0 {
            // Wrap to start (forward-compat; structurally
            // unreachable in v1's single-core cooperative model).
            idx_opt = (0..self.hint).find(|&idx| !read_bit(&self.bitmap, idx));
        }
        let idx = idx_opt?;

        // Mark allocated.
        set_bit(&mut self.bitmap, idx);
        self.hint = idx.saturating_add(1);
        self.free_count = self.free_count.saturating_sub(1);
        self.allocated_count = self.allocated_count.saturating_add(1);

        // Compute the frame's PA. Validation (i) on `extent` at
        // `Pmm::new` time guarantees `extent.start` is page-aligned;
        // `idx * PAGE_SIZE` preserves alignment. The
        // `from_aligned` unwrap_or(unreachable!) pair is therefore
        // structurally provable.
        let pa_off = idx.saturating_mul(PAGE_SIZE);
        let pa_usize = self.extent.start.0.saturating_add(pa_off);
        let pa_ptr = pa_usize as *mut u8;

        // SAFETY:
        // **Why unsafe is needed.** The FrameProvider contract
        // ("Returned frames must be page-aligned and
        // zero-initialised") requires us to write zeros to a 4 KiB
        // region whose only handle is a freshly-minted PhysFrame
        // (a wrapped PhysAddr, not a Rust-owned slice). Safe Rust
        // has no way to express "write zeros to this PA range"
        // without first materialising a `&mut [u8; PAGE_SIZE]`
        // from the raw pointer — and that materialisation step is
        // itself `unsafe` (`core::slice::from_raw_parts_mut`).
        // The raw `core::ptr::write_bytes` form is the minimum-
        // surface expression of what we're doing.
        //
        // **Invariants upheld.**
        // (1) `pa_ptr` is page-aligned by construction (extent.start
        //     is page-aligned per Pmm::new validation (i); idx *
        //     PAGE_SIZE preserves alignment).
        // (2) the 4 KiB region [pa_ptr, pa_ptr + PAGE_SIZE) is
        //     exclusively owned by this PMM right now — the just-set
        //     bitmap bit is the proof; no other kernel subsystem can
        //     hold a PhysFrame for this index until alloc_frame
        //     returns ownership to the caller.
        // (3) the region is identity-mapped to a kernel-readable VA
        //     per ADR-0027's identity-only v1 layout (post-MMU
        //     activation in mmu_bootstrap, kernel sees PA == VA);
        //     the high-half migration (ADR-0033 placeholder) will
        //     introduce a `phys_to_virt` helper at this site.
        // (4) PAGE_SIZE = 4096 is well within isize::MAX on aarch64;
        //     `write_bytes` cannot overflow any intermediate
        //     arithmetic.
        // (5) v1 is single-core + cooperative; no peer reader can
        //     observe the partially-written frame. SMP extension
        //     keeps this invariant via the set_bit atomicity that
        //     precedes the write.
        //
        // **Why safer alternatives were rejected.**
        // - `core::slice::from_raw_parts_mut(pa_ptr, PAGE_SIZE)` +
        //   `slice.fill(0)`: the slice construction is itself
        //   unsafe; safe-looking syntax wrapping the same operation
        //   would obscure the audit point without removing it.
        // - Lazy zero-fill on first page-fault: v1 has no page-fault
        //   routing into the capability system; eager zero is
        //   simpler and ~sub-microsecond on Cortex-A72.
        // - Skip zero-fill: violates the FrameProvider contract and
        //   leaks previous frame contents — a B5+ userspace-
        //   isolation hazard.
        // - `volatile_set_memory`: no peer observer in single-core
        //   v1; volatile semantics would buy nothing.
        //
        // Audit: UNSAFE-2026-0026 (new entry — PMM frame-zeroing
        // is semantically distant from UNSAFE-2026-0001's PL011 MMIO
        // base blessing per ADR-0035 §Dependency chain step 5
        // adjudication). The audit-log entry carries the full
        // Rejected-alternatives discussion (5 alternatives walked);
        // the SAFETY block above summarises the four most-asked at
        // the call site.
        unsafe {
            core::ptr::write_bytes(pa_ptr, 0u8, PAGE_SIZE);
        }

        // Return the page-aligned PhysFrame. `from_aligned` is
        // provably-Some here: validation (i) on Pmm::new guarantees
        // `extent.start` is page-aligned, and `idx * PAGE_SIZE`
        // preserves that alignment. Returning the Option directly
        // (rather than unwrap / expect) keeps `clippy::unwrap_used`
        // happy without adding a panic path.
        //
        // **Unreachable-leak caveat.** Mutation of the bitmap, hint,
        // and counters above happens BEFORE this call. If a future
        // change ever weakens the alignment proof (e.g., a BSP whose
        // extent.start is not page-aligned and the validation is
        // bypassed), `from_aligned` could return `None` and this
        // function would return `None` to the caller while the
        // bitmap state has already moved — the frame would be
        // permanently leaked (bit set, no PhysFrame handed out). The
        // path is structurally unreachable in v1; a future
        // maintainer who alters Pmm::new's validation set must
        // either preserve the alignment proof or move the mutation
        // block below this call to keep the leak structurally
        // impossible.
        PhysFrame::from_aligned(PhysAddr(pa_usize))
    }

    /// Free a previously-allocated frame.
    ///
    /// Validates (in order) that the frame's PA falls within the
    /// managed `extent` (returns [`PmmError::OutOfRange`] if not),
    /// is *not* in any cached reserved range (returns
    /// [`PmmError::DoubleFree`] — defensive scan over the populated
    /// `Some(_)` slots; ADR-0035 §Simulation §Step 2 Critical row),
    /// and is currently Allocated (returns [`PmmError::DoubleFree`]
    /// if the bitmap bit is already 0). On success: clears the
    /// bit, rewinds `hint = min(hint, idx)`, increments
    /// `free_count`, decrements `allocated_count`.
    ///
    /// # Errors
    ///
    /// Per the validations above. Each error path leaves the bitmap
    /// state and counters byte-stable.
    pub fn free_frame(&mut self, frame: PhysFrame) -> Result<(), PmmError> {
        let pa = frame.addr();

        // Extent-bounds fail-fast (precedes index arithmetic).
        if !self.extent.contains(pa) {
            return Err(PmmError::OutOfRange);
        }

        // Compute the bitmap index. extent.contains(pa) above
        // guarantees pa.0 >= extent.start.0, so saturating_sub
        // never truncates a valid input.
        let idx =
            pa.0.saturating_sub(self.extent.start.0)
                .wrapping_div(PAGE_SIZE);

        // Defensive reserved-range scan (Critical row): iterate
        // only the populated Some(_) slots — `flatten()` over the
        // [Option<PhysFrameRange>; R] array yields O(populated
        // entries), not O(R).
        for range in self.reserved_ranges.iter().flatten() {
            if range.contains(pa) {
                return Err(PmmError::DoubleFree);
            }
        }

        // Bitmap-bit check (already-Free fail).
        if !read_bit(&self.bitmap, idx) {
            return Err(PmmError::DoubleFree);
        }

        // Clear bit; rewind hint; update counters.
        clear_bit(&mut self.bitmap, idx);
        if idx < self.hint {
            self.hint = idx;
        }
        self.free_count = self.free_count.saturating_add(1);
        self.allocated_count = self.allocated_count.saturating_sub(1);

        Ok(())
    }

    /// Returns `true` if any byte of `pa_range` falls inside a PA that
    /// [`alloc_frame`][Self::alloc_frame] could yield — i.e., inside
    /// the managed `extent` AND outside every cached reserved range.
    /// Returns `false` for empty ranges and ranges disjoint from the
    /// extent.
    ///
    /// # Purpose
    ///
    /// Used by callers that hold an external pointer and need to prove
    /// it cannot alias a future `alloc_frame()` return. The task
    /// loader (see
    /// [`obj::task_loader::load_image`][crate::obj::task_loader::load_image])
    /// uses this query to discharge
    /// [UNSAFE-2026-0027][unsafe-27]'s "source and destination do not
    /// overlap" invariant at runtime rather than via BSP memory-layout
    /// discipline (ADR-0027 + ADR-0035). The check treats
    /// `pa_range`'s endpoints as physical addresses — correct under
    /// v1's identity-mapped post-bootstrap kernel AS per
    /// [ADR-0027 §Decision outcome (a)][adr-0027]; a future high-half
    /// migration (ADR-0033 placeholder) introduces a `virt_to_phys`
    /// helper at the loader's call site.
    ///
    /// # Conservatism (over-approximation)
    ///
    /// The helper queries the *extent + reserved-range* set only — it
    /// does **not** consult the live bitmap to filter out frames that
    /// are currently `Allocated`. An `Allocated` frame cannot be
    /// returned by the very next `alloc_frame()` (the bitmap bit is
    /// set), but the helper reports it as a candidate yield anyway.
    /// This is *deliberate over-approximation*: an `Allocated` frame
    /// becomes reachable as a yield candidate again the moment its
    /// owner calls `free_frame` on it, so a staging region overlapping
    /// such a frame is at risk over its lifetime. The conservative
    /// "non-reserved frame ⇒ might be yielded" rule keeps the
    /// soundness argument independent of allocation-timing reasoning.
    ///
    /// The production BSP wiring uses `.rodata`-resident images that
    /// live entirely in PMM-reserved memory (kernel image range), so
    /// the conservatism does not trip a real caller. Callers that
    /// *intentionally* stage data in an `Allocated` PMM frame and
    /// pass that PA to a `could_yield_pa_overlapping`-using helper
    /// will see a false-positive rejection — they should either keep
    /// the data in `.rodata` (the supported pattern) or build a
    /// stricter helper that consults the bitmap.
    ///
    /// # Algorithm
    ///
    /// Clip `pa_range` to `extent`, then walk the covered frame
    /// indices linearly; return `true` on the first frame whose PA is
    /// not inside any populated `Some(_)` slot of `reserved_ranges`.
    /// Worst-case `O((pa_range.len() / PAGE_SIZE) × populated_reserved)`;
    /// for the loader's v1 placeholder (8-byte image, 1 frame of
    /// coverage) this is a single iteration over at most `R` slots
    /// (`R = 8` for `bsp-qemu-virt`).
    ///
    /// [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
    /// [unsafe-27]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md
    #[must_use]
    pub fn could_yield_pa_overlapping(&self, pa_range: core::ops::Range<usize>) -> bool {
        // Empty range cannot overlap anything.
        if pa_range.start >= pa_range.end {
            return false;
        }
        let extent_start = self.extent.start.0;
        let extent_end = self.extent.end.0;
        // Disjoint from extent → cannot overlap a yieldable frame.
        if pa_range.end <= extent_start || pa_range.start >= extent_end {
            return false;
        }
        // Clip to extent.
        let clipped_start = if pa_range.start > extent_start {
            pa_range.start
        } else {
            extent_start
        };
        let clipped_end = if pa_range.end < extent_end {
            pa_range.end
        } else {
            extent_end
        };
        // Frame-index bounds: any frame whose PA range
        // `[f, f + PAGE_SIZE)` overlaps `[clipped_start, clipped_end)`.
        // Equivalently: start_idx is the frame containing `clipped_start`;
        // end_idx is one past the frame containing `clipped_end - 1`.
        let start_idx = clipped_start
            .saturating_sub(extent_start)
            .wrapping_div(PAGE_SIZE);
        let end_idx = clipped_end
            .saturating_sub(extent_start)
            .saturating_add(PAGE_SIZE)
            .saturating_sub(1)
            .wrapping_div(PAGE_SIZE);
        // Walk frame PAs; return true on first non-reserved frame.
        for idx in start_idx..end_idx {
            let frame_pa = extent_start.saturating_add(idx.saturating_mul(PAGE_SIZE));
            let frame_addr = PhysAddr(frame_pa);
            let in_reserved = self
                .reserved_ranges
                .iter()
                .flatten()
                .any(|r| r.contains(frame_addr));
            if !in_reserved {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
impl<const N: usize, const R: usize> Pmm<N, R> {
    /// **Test-only.** Schedule [`alloc_frame`][Self::alloc_frame] to
    /// start returning `None` after the next `n` successful calls.
    /// Calling again replaces the prior schedule. Used by host tests
    /// in `obj::task_loader::tests` to deterministically drive the
    /// `OutOfFrames` rollback path; production code has no caller.
    pub(crate) fn force_alloc_failure_after(&mut self, n: usize) {
        self.alloc_failure_after = Some(n);
    }
}

/// Implements [`tyrne_hal::FrameProvider`] so the PMM can be passed
/// directly to [`tyrne_hal::Mmu::map`] as `&mut dyn FrameProvider`.
///
/// The `Mmu::map` contract returns `MmuError::OutOfFrames` when the
/// `FrameProvider` returns `None`. Per [ADR-0009][adr-0009] and
/// [ADR-0035 §Decision drivers][adr-0035], this is the canonical
/// surface the PMM layer satisfies.
///
/// [adr-0009]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0009-mmu-trait.md
/// [adr-0035]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0035-physical-memory-manager.md
impl<const N: usize, const R: usize> FrameProvider for Pmm<N, R> {
    fn alloc_frame(&mut self) -> Option<PhysFrame> {
        Pmm::alloc_frame(self)
    }
}

// ── Bitmap helpers (private) ──────────────────────────────────────────────────

/// Set bit `idx` in `bitmap`. Caller's responsibility to ensure
/// `idx < bitmap.len() * 8`.
fn set_bit(bitmap: &mut [u8], idx: usize) {
    let byte = idx / 8;
    let bit = idx % 8;
    bitmap[byte] |= 1 << bit;
}

/// Read bit `idx` in `bitmap`. Caller's responsibility to ensure
/// `idx < bitmap.len() * 8`.
fn read_bit(bitmap: &[u8], idx: usize) -> bool {
    let byte = idx / 8;
    let bit = idx % 8;
    (bitmap[byte] >> bit) & 1 == 1
}

/// Clear bit `idx` in `bitmap`. Caller's responsibility to ensure
/// `idx < bitmap.len() * 8`.
fn clear_bit(bitmap: &mut [u8], idx: usize) {
    let byte = idx / 8;
    let bit = idx % 8;
    bitmap[byte] &= !(1u8 << bit);
}

/// Return the first `0` bit in `bitmap` over the range
/// `[0, frame_count)`, or `None` if every bit is `1`.
fn first_zero_bit(bitmap: &[u8], frame_count: usize) -> Option<usize> {
    (0..frame_count).find(|&idx| !read_bit(bitmap, idx))
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests may use pragmas forbidden in production kernel code"
)]
mod tests {
    use super::{Pmm, PmmError};
    use crate::mm::PhysFrameRange;
    use tyrne_hal::{PhysAddr, PhysFrame};

    /// Test fixture: 4 frames (16 KiB), starting at `0x4000_0000`.
    /// Bitmap holds 4 bits → 1 byte (rounded up); `N = 2` is the
    /// smallest non-trivial PMM with headroom for the sanity check.
    /// `R = 4` for the reserved-range capacity.
    fn extent_4f() -> PhysFrameRange {
        PhysFrameRange::new(PhysAddr(0x4000_0000), PhysAddr(0x4000_4000))
    }

    #[test]
    fn new_marks_reserved_ranges_and_initialises_counters() {
        // Extent: 16 frames at 0x4000_0000..0x4001_0000 (64 KiB).
        let extent = PhysFrameRange::new(PhysAddr(0x4000_0000), PhysAddr(0x4001_0000));
        let reserved = [
            // Frame 0 (kernel-image stand-in): 0x4000_0000..0x4000_1000.
            PhysFrameRange::new(PhysAddr(0x4000_0000), PhysAddr(0x4000_1000)),
            // Frame 8 (boot-stack stand-in): 0x4000_8000..0x4000_9000.
            PhysFrameRange::new(PhysAddr(0x4000_8000), PhysAddr(0x4000_9000)),
        ];

        let pmm: Pmm<2, 4> = Pmm::new(extent, &reserved).expect("new must succeed");

        let stats = pmm.stats();
        assert_eq!(stats.total_frames, 16);
        assert_eq!(stats.reserved_frames, 2);
        assert_eq!(stats.allocated_frames, 0);
        assert_eq!(stats.free_frames, 14);

        // Hint must be the first non-reserved frame: frame 1
        // (0 is reserved by the first range; 1 is the first 0-bit).
        assert_eq!(pmm.hint, 1);
        // Cached reserved_ranges populated.
        assert_eq!(pmm.reserved_ranges[0], Some(reserved[0]));
        assert_eq!(pmm.reserved_ranges[1], Some(reserved[1]));
        assert_eq!(pmm.reserved_ranges[2], None);
        assert_eq!(pmm.reserved_ranges[3], None);
    }

    #[test]
    fn new_rejects_too_many_reserved_ranges() {
        let extent = PhysFrameRange::new(PhysAddr(0x4000_0000), PhysAddr(0x4001_0000));
        // 5 ranges with R = 4 → TooManyReservedRanges.
        let reserved: [PhysFrameRange; 5] = [
            PhysFrameRange::new(PhysAddr(0x4000_0000), PhysAddr(0x4000_1000)),
            PhysFrameRange::new(PhysAddr(0x4000_1000), PhysAddr(0x4000_2000)),
            PhysFrameRange::new(PhysAddr(0x4000_2000), PhysAddr(0x4000_3000)),
            PhysFrameRange::new(PhysAddr(0x4000_3000), PhysAddr(0x4000_4000)),
            PhysFrameRange::new(PhysAddr(0x4000_4000), PhysAddr(0x4000_5000)),
        ];

        let result: Result<Pmm<2, 4>, _> = Pmm::new(extent, &reserved);
        assert_eq!(result.err(), Some(PmmError::TooManyReservedRanges));
    }

    #[test]
    fn new_rejects_overlapping_reserved_ranges() {
        // PR #26 round-1 review: overlapping reserved ranges would
        // double-count `reserved_count` while sharing bitmap bits,
        // leaving stats() inconsistent with the bitmap.
        let extent = PhysFrameRange::new(PhysAddr(0x4000_0000), PhysAddr(0x4001_0000));

        // Two ranges that overlap on frame 2 (0x4000_2000).
        let overlapping = [
            PhysFrameRange::new(PhysAddr(0x4000_0000), PhysAddr(0x4000_3000)),
            PhysFrameRange::new(PhysAddr(0x4000_2000), PhysAddr(0x4000_5000)),
        ];
        let result: Result<Pmm<2, 4>, _> = Pmm::new(extent, &overlapping);
        assert_eq!(result.err(), Some(PmmError::OverlappingReservedRanges));

        // Duplicate ranges (a stricter overlap form).
        let duplicate = [
            PhysFrameRange::new(PhysAddr(0x4000_1000), PhysAddr(0x4000_2000)),
            PhysFrameRange::new(PhysAddr(0x4000_1000), PhysAddr(0x4000_2000)),
        ];
        let result: Result<Pmm<2, 4>, _> = Pmm::new(extent, &duplicate);
        assert_eq!(result.err(), Some(PmmError::OverlappingReservedRanges));

        // Touching-but-not-overlapping ranges (`[a, b)` + `[b, c)`)
        // must NOT be rejected — half-open ranges don't overlap at
        // the boundary.
        let touching = [
            PhysFrameRange::new(PhysAddr(0x4000_0000), PhysAddr(0x4000_2000)),
            PhysFrameRange::new(PhysAddr(0x4000_2000), PhysAddr(0x4000_4000)),
        ];
        let result: Result<Pmm<2, 4>, _> = Pmm::new(extent, &touching);
        assert!(
            result.is_ok(),
            "touching half-open ranges must NOT trigger overlap rejection"
        );
    }

    #[test]
    fn new_rejects_unaligned_extent() {
        // Unaligned start.
        let extent_bad_start = PhysFrameRange::new(PhysAddr(0x4000_0001), PhysAddr(0x4001_0000));
        let result: Result<Pmm<2, 4>, _> = Pmm::new(extent_bad_start, &[]);
        assert_eq!(result.err(), Some(PmmError::MisalignedAddress));

        // Unaligned end.
        let extent_bad_end = PhysFrameRange::new(PhysAddr(0x4000_0000), PhysAddr(0x4001_0001));
        let result: Result<Pmm<2, 4>, _> = Pmm::new(extent_bad_end, &[]);
        assert_eq!(result.err(), Some(PmmError::MisalignedAddress));
    }

    #[test]
    fn new_rejects_reserved_range_outside_extent() {
        let extent = PhysFrameRange::new(PhysAddr(0x4000_0000), PhysAddr(0x4001_0000));

        // Range entirely above extent.
        let reserved_above = [PhysFrameRange::new(
            PhysAddr(0x4002_0000),
            PhysAddr(0x4002_1000),
        )];
        let result: Result<Pmm<2, 4>, _> = Pmm::new(extent, &reserved_above);
        assert_eq!(result.err(), Some(PmmError::OutOfRange));

        // Range partially exceeding extent.end.
        let reserved_partial = [PhysFrameRange::new(
            PhysAddr(0x4000_F000),
            PhysAddr(0x4001_2000),
        )];
        let result: Result<Pmm<2, 4>, _> = Pmm::new(extent, &reserved_partial);
        assert_eq!(result.err(), Some(PmmError::OutOfRange));

        // Range below extent.start.
        let reserved_below = [PhysFrameRange::new(
            PhysAddr(0x3FFF_0000),
            PhysAddr(0x3FFF_1000),
        )];
        let result: Result<Pmm<2, 4>, _> = Pmm::new(extent, &reserved_below);
        assert_eq!(result.err(), Some(PmmError::OutOfRange));

        // Unaligned reserved-range bound (still within extent).
        let reserved_unaligned = [PhysFrameRange::new(
            PhysAddr(0x4000_0001),
            PhysAddr(0x4000_1000),
        )];
        let result: Result<Pmm<2, 4>, _> = Pmm::new(extent, &reserved_unaligned);
        assert_eq!(result.err(), Some(PmmError::MisalignedAddress));
    }

    #[test]
    fn extent_4f_fixture_sanity() {
        // Sanity-check the test fixture itself.
        let e = extent_4f();
        assert_eq!(e.frame_count(), 4);
        assert!(e.is_aligned());
        assert!(e.contains(PhysAddr(0x4000_0000)));
        assert!(e.contains(PhysAddr(0x4000_3FFF)));
        assert!(!e.contains(PhysAddr(0x4000_4000)));
    }

    // ── alloc_frame / free_frame / stats tests ─────────────────────────────────
    //
    // These tests use a host-allocated [u8; 64 KiB] backing buffer
    // and offset the PMM's "extent" to point at that buffer's
    // address. This way the frame-zeroing write_bytes lands in
    // host RAM the test harness owns — no UB even though the PMM
    // believes it's writing to the QEMU virt RAM range. The
    // bitmap math is unaffected because Pmm::new operates on the
    // extent's start/end values, not on the backing storage.

    use std::vec;
    use std::vec::Vec;

    /// Allocate a `Vec<u8>` aligned to `PAGE_SIZE` that we can use as
    /// the PMM's "extent" backing storage. Returns the raw pointer
    /// + the Vec (kept alive for the test).
    fn aligned_backing(frames: usize) -> (Vec<u8>, *mut u8) {
        let bytes = frames.checked_mul(4096).expect("test math overflow");
        let alloc = bytes.checked_add(4096).expect("test math overflow");
        let mut v: Vec<u8> = vec![0u8; alloc];
        let raw = v.as_mut_ptr();
        let aligned = ((raw as usize + 4095) & !4095) as *mut u8;
        (v, aligned)
    }

    fn pmm_over_backing(
        backing_ptr: *mut u8,
        frames: usize,
        reserved: &[(usize, usize)],
    ) -> Pmm<2, 4> {
        let base = backing_ptr as usize;
        let extent = PhysFrameRange::new(PhysAddr(base), PhysAddr(base + frames * 4096));
        let reserved_ranges: Vec<PhysFrameRange> = reserved
            .iter()
            .map(|&(start_off, end_off)| {
                PhysFrameRange::new(
                    PhysAddr(base + start_off * 4096),
                    PhysAddr(base + end_off * 4096),
                )
            })
            .collect();
        Pmm::new(extent, &reserved_ranges).expect("Pmm::new must succeed")
    }

    #[test]
    fn alloc_frame_returns_first_free_and_zeroes_payload() {
        let (_buf, ptr) = aligned_backing(16);
        // Pre-poison the backing with non-zero bytes so we can
        // assert the alloc actually zero-fills.
        // SAFETY: ptr points to the head of a 16-frame
        // PAGE_SIZE-aligned host-allocated Vec<u8> kept alive by
        // _buf for the test's duration; the write covers exactly
        // the Vec's payload range.
        unsafe {
            core::ptr::write_bytes(ptr, 0xA5u8, 16 * 4096);
        }
        // Reserve frame 0; alloc should return frame 1.
        let mut pmm = pmm_over_backing(ptr, 16, &[(0, 1)]);

        let frame = pmm.alloc_frame().expect("alloc must succeed");
        let expected_pa = PhysAddr(ptr as usize + 4096);
        assert_eq!(frame.addr(), expected_pa);

        // Verify the returned frame is zeroed.
        let returned_ptr = frame.as_usize() as *const u8;
        for off in 0..4096 {
            // SAFETY: returned_ptr is a PhysFrame the PMM just
            // returned from the same backing buffer; reading the
            // 4 KiB page is in-bounds for the host-allocated Vec.
            let byte = unsafe { *returned_ptr.add(off) };
            assert_eq!(byte, 0u8, "alloc_frame must zero-fill (off={off})");
        }

        // Counters updated.
        let stats = pmm.stats();
        assert_eq!(stats.allocated_frames, 1);
        assert_eq!(stats.free_frames, 14);
        assert_eq!(stats.reserved_frames, 1);
    }

    #[test]
    fn free_frame_clears_bit_and_rewinds_hint() {
        let (_buf, ptr) = aligned_backing(16);
        let mut pmm = pmm_over_backing(ptr, 16, &[]);

        let f1 = pmm.alloc_frame().expect("alloc 1");
        let f2 = pmm.alloc_frame().expect("alloc 2");
        let f3 = pmm.alloc_frame().expect("alloc 3");
        // hint is now at 3 (after f3 = idx 2).
        assert!(pmm.hint >= 3);

        // Free f1 (idx 0). Hint should rewind to 0.
        pmm.free_frame(f1).expect("free f1");
        assert_eq!(pmm.hint, 0);

        // Next alloc should return f1 again.
        let f1_again = pmm.alloc_frame().expect("alloc after free");
        assert_eq!(f1_again, f1);

        // Free f2 + f3 to clean up.
        pmm.free_frame(f2).expect("free f2");
        pmm.free_frame(f3).expect("free f3");

        let stats = pmm.stats();
        assert_eq!(stats.allocated_frames, 1);
        assert_eq!(stats.free_frames, 15);
    }

    #[test]
    fn free_frame_rejects_double_free_and_reserved() {
        let (_buf, ptr) = aligned_backing(16);
        let base = ptr as usize;

        // Reserve frame 0.
        let mut pmm = pmm_over_backing(ptr, 16, &[(0, 1)]);

        // Reserved frame: free should reject.
        let reserved_frame = PhysFrame::from_aligned(PhysAddr(base)).expect("aligned");
        assert_eq!(
            pmm.free_frame(reserved_frame),
            Err(PmmError::DoubleFree),
            "free of reserved frame must be DoubleFree"
        );

        // Already-free frame: free should reject.
        let already_free = PhysFrame::from_aligned(PhysAddr(base + 4096)).expect("aligned");
        assert_eq!(
            pmm.free_frame(already_free),
            Err(PmmError::DoubleFree),
            "free of never-allocated frame must be DoubleFree"
        );

        // Counters unchanged.
        let stats = pmm.stats();
        assert_eq!(stats.reserved_frames, 1);
        assert_eq!(stats.allocated_frames, 0);
        assert_eq!(stats.free_frames, 15);
    }

    #[test]
    fn alloc_frame_returns_none_when_exhausted() {
        let (_buf, ptr) = aligned_backing(4);
        let mut pmm = pmm_over_backing(ptr, 4, &[]);

        // Allocate every frame.
        let _f0 = pmm.alloc_frame().expect("alloc 0");
        let _f1 = pmm.alloc_frame().expect("alloc 1");
        let _f2 = pmm.alloc_frame().expect("alloc 2");
        let _f3 = pmm.alloc_frame().expect("alloc 3");

        // Next alloc must return None.
        assert_eq!(pmm.alloc_frame(), None);
        assert_eq!(pmm.stats().free_frames, 0);
        assert_eq!(pmm.stats().allocated_frames, 4);
    }

    #[test]
    fn alloc_frame_recovers_after_free_under_exhaustion() {
        let (_buf, ptr) = aligned_backing(4);
        let mut pmm = pmm_over_backing(ptr, 4, &[]);

        let f0 = pmm.alloc_frame().expect("alloc 0");
        let _f1 = pmm.alloc_frame().expect("alloc 1");
        let _f2 = pmm.alloc_frame().expect("alloc 2");
        let _f3 = pmm.alloc_frame().expect("alloc 3");
        assert_eq!(pmm.alloc_frame(), None);

        // Free f0; alloc should return it.
        pmm.free_frame(f0).expect("free f0");
        let f0_again = pmm.alloc_frame().expect("alloc after free");
        assert_eq!(f0_again, f0);
    }

    #[test]
    fn stats_parity_with_bitmap_bit_count() {
        let (_buf, ptr) = aligned_backing(16);
        let mut pmm = pmm_over_backing(ptr, 16, &[(0, 2)]);

        let _f0 = pmm.alloc_frame().expect("alloc 0");
        let _f1 = pmm.alloc_frame().expect("alloc 1");
        let f2 = pmm.alloc_frame().expect("alloc 2");
        pmm.free_frame(f2).expect("free f2");

        // Count set bits in bitmap (16 bits / 2 bytes).
        let mut set_bits: usize = 0;
        for byte in &pmm.bitmap {
            set_bits += byte.count_ones() as usize;
        }
        // 2 reserved + 2 still-allocated = 4 set bits.
        assert_eq!(set_bits, 4);

        let stats = pmm.stats();
        // Cached counters must agree with the bitmap.
        assert_eq!(
            stats.reserved_frames + stats.allocated_frames,
            set_bits,
            "cached counters must match bitmap bit-count"
        );
        assert_eq!(stats.total_frames, 16);
        assert_eq!(stats.free_frames, 16 - set_bits);
    }

    #[test]
    fn free_frame_reserved_check_iterates_only_populated_slots() {
        let (_buf, ptr) = aligned_backing(16);
        // Only 1 reserved range out of R=4 slots; remaining 3 slots
        // are None. The defensive scan must skip None slots
        // (treating them as "no reservation"), not as wildcards.
        let mut pmm = pmm_over_backing(ptr, 16, &[(0, 1)]);

        // Allocate a non-reserved frame.
        let f = pmm.alloc_frame().expect("alloc");
        // Free should succeed (the None slots in reserved_ranges
        // must NOT cause the defensive scan to reject this PA).
        assert_eq!(
            pmm.free_frame(f),
            Ok(()),
            "non-reserved frame must free OK even when R=4 has None slots"
        );
    }

    #[test]
    fn alloc_frame_implements_frame_provider() {
        // Exercise the trait method via &mut dyn FrameProvider so the
        // impl integration is pinned (catches accidental signature
        // drift between Pmm::alloc_frame and FrameProvider::alloc_frame).
        use tyrne_hal::FrameProvider;

        let (_buf, ptr) = aligned_backing(4);
        let mut pmm = pmm_over_backing(ptr, 4, &[]);

        let provider: &mut dyn FrameProvider = &mut pmm;
        let f0 = provider.alloc_frame().expect("alloc via dyn trait");
        let f1 = provider.alloc_frame().expect("alloc 2 via dyn trait");
        assert_ne!(f0, f1);

        // Three more allocs (total 4 = capacity) then None.
        let _f2 = provider.alloc_frame().expect("alloc 3");
        let _f3 = provider.alloc_frame().expect("alloc 4");
        assert_eq!(provider.alloc_frame(), None);
    }

    // ── `could_yield_pa_overlapping` conservatism regression ──────────────────

    #[test]
    fn could_yield_pa_overlapping_treats_allocated_frame_as_yieldable() {
        // PR #31 review-round 5 P2 regression: the helper queries
        // extent + reserved-ranges only — it deliberately does NOT
        // consult the bitmap to filter out currently-`Allocated`
        // frames. A staging region overlapping an Allocated frame is
        // therefore reported as a yield candidate (conservative
        // over-approximation), even though the very next
        // `alloc_frame()` cannot return that exact frame until it is
        // freed. The conservatism is load-bearing: the staged region
        // becomes a yield candidate the moment the owner calls
        // `free_frame`, so the soundness argument stays independent
        // of allocation timing.
        let (_buf, ptr) = aligned_backing(4);
        let base = ptr as usize;
        let mut pmm = pmm_over_backing(ptr, 4, &[]);

        // Allocate frame 0; the bitmap bit at index 0 is now set,
        // so `alloc_frame()` cannot return frame 0 again until it is
        // freed.
        let frame0 = pmm.alloc_frame().expect("alloc must succeed");
        assert_eq!(frame0.as_usize(), base);
        assert_eq!(pmm.stats().allocated_frames, 1);
        assert_eq!(pmm.stats().free_frames, 3);

        // The helper STILL reports frame 0 as a possible yield —
        // because it doesn't consult the bitmap. This is the
        // documented conservatism (over-approximation).
        let allocated_range = base..base.saturating_add(4096);
        assert!(
            pmm.could_yield_pa_overlapping(allocated_range),
            "helper must over-conservatively report an Allocated \
             frame as a yield candidate (regardless of the live \
             bitmap bit)"
        );

        // Reserved frames are excluded, so the conservatism does
        // NOT extend to reserved-range coverage. Build a second PMM
        // with frame 0 reserved (offset (0, 1) in frame-index terms)
        // and check the negative case.
        let (_buf2, ptr2) = aligned_backing(4);
        let base2 = ptr2 as usize;
        let pmm2 = pmm_over_backing(ptr2, 4, &[(0, 1)]);
        assert!(
            !pmm2.could_yield_pa_overlapping(base2..base2.saturating_add(4096)),
            "helper must exclude reserved-range frames"
        );
    }

    #[test]
    fn free_frame_rejects_pa_outside_extent() {
        let (_buf, ptr) = aligned_backing(16);
        let base = ptr as usize;
        let mut pmm = pmm_over_backing(ptr, 16, &[]);

        // PA below extent.start.
        let below = PhysFrame::from_aligned(PhysAddr(base.saturating_sub(4096))).expect("aligned");
        assert_eq!(pmm.free_frame(below), Err(PmmError::OutOfRange));

        // PA at/above extent.end.
        let above = PhysFrame::from_aligned(PhysAddr(base + 16 * 4096)).expect("aligned");
        assert_eq!(pmm.free_frame(above), Err(PmmError::OutOfRange));

        // Counters unchanged.
        let stats = pmm.stats();
        assert_eq!(stats.allocated_frames, 0);
        assert_eq!(stats.free_frames, 16);
    }
}
