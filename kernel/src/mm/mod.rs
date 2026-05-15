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
    activate_address_space_handle, cap_create_address_space, cap_map, cap_unmap,
    create_address_space, get_address_space, AddressSpace, AddressSpaceArena, AddressSpaceError,
    AddressSpaceHandle, ADDRESS_SPACE_ARENA_CAPACITY, BOOTSTRAP_ADDRESS_SPACE_HANDLE,
};
pub use pmm::{Pmm, PmmError, PmmStats};

/// Return a kernel-writable raw pointer for `frame`'s base PA.
///
/// In v1 the kernel address space is identity-mapped over the entire
/// PMM-managed physical extent per
/// [ADR-0027 §Decision outcome (a)][adr-0027], so any
/// [`tyrne_hal::PhysFrame`] returned by [`Pmm::alloc_frame`] is
/// reachable at VA = PA from kernel code. This helper *centralises*
/// that assumption: every kernel-side caller that needs to read or
/// write a PMM-allocated frame's payload (e.g.
/// [`crate::obj::task_loader::load_image`]'s `copy_nonoverlapping`
/// byte-copy site under [UNSAFE-2026-0027]) routes through this
/// function so the future high-half migration
/// ([ADR-0033 placeholder][adr-0027]) can replace the body with a
/// real PA → kernel-VA translation in **one** place, leaving every
/// call site source-compatible.
///
/// The function itself is safe (the `as *mut u8` cast is infallible
/// Rust); only the *dereference* at the call site is `unsafe` and
/// requires the audit-log entry that names the call site's specific
/// ownership / aliasing discipline.
///
/// ## Forward-compat note
///
/// When [ADR-0033 placeholder][adr-0027] opens and the kernel moves
/// to a high-half virtual layout, this function's body grows to
/// `KERNEL_PHYS_BASE.checked_add(frame.as_usize()).expect(...)` or
/// similar; every call site keeps working without source changes.
/// The audit-log entries that cite "identity mapping post-MMU per
/// ADR-0027" (UNSAFE-2026-0026, UNSAFE-2026-0027) gain a "lifted via
/// ADR-0033 migration on date X" Amendment at the same commit. The
/// PMM's existing `core::ptr::write_bytes` site
/// ([`kernel/src/mm/pmm.rs`](pmm.rs)) is the second adopter — its
/// safety comment already names the future-migration plan; the
/// physical PMM site will route through this helper at the same
/// commit ADR-0033 lands (kept inline today to avoid churning the
/// audit-log entries that landed with T-017).
///
/// [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
/// [UNSAFE-2026-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md
#[must_use]
#[inline]
pub fn phys_frame_kernel_ptr(frame: tyrne_hal::PhysFrame) -> *mut u8 {
    frame.as_usize() as *mut u8
}
