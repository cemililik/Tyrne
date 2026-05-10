//! Memory-management subsystem.
//!
//! Top-level parent for the kernel-side memory-management modules.
//! Currently hosts the Physical Memory Manager (PMM) per [ADR-0035];
//! a future B3 commit will add the address-space data-structure
//! module per [ADR-0028 placeholder].
//!
//! See [T-017] for the implementation arc and [`docs/architecture/memory-management.md`]
//! for the synthesised architecture chapter.
//!
//! [ADR-0035]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0035-physical-memory-manager.md
//! [ADR-0028 placeholder]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
//! [T-017]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-b/T-017-physical-memory-manager.md

pub mod pmm;

use tyrne_hal::{PhysAddr, PAGE_SIZE};

/// A half-open physical-frame range: `[start, end)`.
///
/// Used by the Physical Memory Manager (PMM) to describe both the
/// total managed physical-RAM extent and the kernel-reserved regions
/// (kernel image / `.boot_pt` / boot stack) handed to [`Pmm::new`].
///
/// The range carries raw [`PhysAddr`] values rather than [`tyrne_hal::PhysFrame`]
/// so it can describe multi-page regions in one entry without
/// frame-by-frame enumeration. `Pmm::new` validates page-alignment of
/// the bounds before mutating any bitmap state per [ADR-0035 §Simulation
/// §Step 0][adr-0035].
///
/// `start <= end` is a soft invariant — `Pmm::new` treats an
/// inverted range as zero-length (no frames covered) rather than
/// panicking; the validation layer at the BSP is the canonical
/// source for "well-formed range".
///
/// [adr-0035]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0035-physical-memory-manager.md#simulation
/// [`Pmm::new`]: pmm::Pmm::new
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PhysFrameRange {
    /// Inclusive start of the range. Must be [`PAGE_SIZE`]-aligned for
    /// `Pmm::new` to accept it.
    pub start: PhysAddr,
    /// Exclusive end of the range. Must be [`PAGE_SIZE`]-aligned for
    /// `Pmm::new` to accept it.
    pub end: PhysAddr,
}

impl PhysFrameRange {
    /// Construct a range from raw bounds.
    #[must_use]
    pub const fn new(start: PhysAddr, end: PhysAddr) -> Self {
        Self { start, end }
    }

    /// Return `true` if both bounds are [`PAGE_SIZE`]-aligned.
    #[must_use]
    pub const fn is_aligned(self) -> bool {
        self.start.0.is_multiple_of(PAGE_SIZE) && self.end.0.is_multiple_of(PAGE_SIZE)
    }

    /// Return the half-open range's length in bytes (or 0 if `end <
    /// start`).
    #[must_use]
    pub const fn len_bytes(self) -> usize {
        // Saturating sub keeps `clippy::arithmetic_side_effects`
        // happy and treats inverted ranges as zero-length per the
        // soft-invariant note above.
        self.end.0.saturating_sub(self.start.0)
    }

    /// Return the number of [`PAGE_SIZE`]-frames the range covers.
    /// Assumes both bounds are page-aligned (caller's responsibility).
    #[must_use]
    pub const fn frame_count(self) -> usize {
        // `len_bytes()` is bounded by `usize::MAX`; integer division
        // by the non-zero `PAGE_SIZE` is total. No side effects to
        // trigger `arithmetic_side_effects`.
        self.len_bytes().wrapping_div(PAGE_SIZE)
    }

    /// Return `true` if `pa` falls in `[start, end)`.
    #[must_use]
    pub const fn contains(self, pa: PhysAddr) -> bool {
        pa.0 >= self.start.0 && pa.0 < self.end.0
    }
}

pub use pmm::{Pmm, PmmError, PmmStats};
