//! Memory-management subsystem.
//!
//! Top-level parent for the kernel-side memory-management modules.
//! Hosts the Physical Memory Manager (PMM) per [ADR-0035] and the
//! kernel-side `AddressSpace<M>` object per [ADR-0028].
//!
//! See [T-017] for the PMM arc, [T-018] for the `AddressSpace` arc, and
//! [`docs/architecture/memory-management.md`] for the synthesised
//! architecture chapter.
//!
//! [ADR-0028]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0028-address-space-data-structure.md
//! [ADR-0035]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0035-physical-memory-manager.md
//! [T-017]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-b/T-017-physical-memory-manager.md
//! [T-018]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-b/T-018-address-space-kernel-object.md

pub mod address_space;
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

pub use address_space::{
    cap_create_address_space, cap_map, cap_unmap, AddressSpace, AddressSpaceArena,
    AddressSpaceError, AddressSpaceHandle, ADDRESS_SPACE_ARENA_CAPACITY,
    BOOTSTRAP_ADDRESS_SPACE_HANDLE,
};
pub use pmm::{Pmm, PmmError, PmmStats};
