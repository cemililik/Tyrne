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

use tyrne_hal::PAGE_SIZE;

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
#[allow(
    dead_code,
    reason = "transient until commit 2 of T-017 lands `alloc_frame` / `free_frame` / `stats` \
              which read these fields; commit 1 establishes the struct shape + `Pmm::new` \
              constructor + counter init only"
)]
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
}

impl<const N: usize, const R: usize> Pmm<N, R> {
    /// Construct a PMM over `extent` with the given reserved ranges.
    ///
    /// Performs three **fail-fast** validations before any bitmap
    /// mutation, so partial-mutation states are structurally
    /// impossible:
    ///
    /// 1. `extent.start` and `extent.end` are [`PAGE_SIZE`]-aligned —
    ///    returns [`PmmError::MisalignedAddress`] otherwise.
    /// 2. Every reserved range fits inside `[extent.start, extent.end)`
    ///    — returns [`PmmError::OutOfRange`] otherwise.
    /// 3. `reserved.len() <= R` — returns [`PmmError::TooManyReservedRanges`]
    ///    otherwise.
    ///
    /// Per [ADR-0035 §Simulation §Step 0][adr-0035].
    ///
    /// # Errors
    ///
    /// Per the three validations above. On any error the constructor
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
    pub fn new(
        extent: PhysFrameRange,
        reserved: &[PhysFrameRange],
    ) -> Result<Self, PmmError> {
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
        })
    }

    /// Return the managed extent.
    #[must_use]
    pub fn extent(&self) -> PhysFrameRange {
        self.extent
    }

    /// Return a diagnostic snapshot of the PMM's frame-state
    /// counters.
    #[must_use]
    pub fn stats(&self) -> PmmStats {
        PmmStats {
            total_frames: self
                .free_count
                .saturating_add(self.reserved_count)
                .saturating_add(self.allocated_count),
            reserved_frames: self.reserved_count,
            allocated_frames: self.allocated_count,
            free_frames: self.free_count,
        }
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
    use tyrne_hal::PhysAddr;

    /// Test fixture: 16 frames (16 KiB), starting at `0x4000_0000`.
    /// Bitmap holds 16 bits → 2 bytes; we use `N = 2` to test the
    /// smallest non-trivial PMM. `R = 4` for the reserved-range
    /// capacity.
    fn extent_16f() -> PhysFrameRange {
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
    fn new_rejects_unaligned_extent() {
        // Unaligned start.
        let extent_bad_start = PhysFrameRange::new(
            PhysAddr(0x4000_0001),
            PhysAddr(0x4001_0000),
        );
        let result: Result<Pmm<2, 4>, _> = Pmm::new(extent_bad_start, &[]);
        assert_eq!(result.err(), Some(PmmError::MisalignedAddress));

        // Unaligned end.
        let extent_bad_end = PhysFrameRange::new(
            PhysAddr(0x4000_0000),
            PhysAddr(0x4001_0001),
        );
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
    fn extent_16f_fixture_sanity() {
        // Sanity-check the test fixture itself.
        let e = extent_16f();
        assert_eq!(e.frame_count(), 4);
        assert!(e.is_aligned());
        assert!(e.contains(PhysAddr(0x4000_0000)));
        assert!(e.contains(PhysAddr(0x4000_3FFF)));
        assert!(!e.contains(PhysAddr(0x4000_4000)));
    }
}
