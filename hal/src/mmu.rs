//! Memory management unit interaction.
//!
//! See [ADR-0009] for the v1 scope and the list of deferred capabilities.
//!
//! [ADR-0009]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0009-mmu-trait.md

use core::ops::{BitAnd, BitOr, BitOrAssign};

/// Page size used by the MMU.
///
/// Fixed at 4 KiB in v1. Huge-page support is deferred to a later ADR.
pub const PAGE_SIZE: usize = 4096;

/// A virtual address.
///
/// The underlying integer is exposed as a `pub` field so call sites can
/// perform the arithmetic they need; the newtype provides type-distinct
/// signatures at the [`Mmu`] surface.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct VirtAddr(pub usize);

/// A physical address.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PhysAddr(pub usize);

/// A [`PAGE_SIZE`]-aligned physical address.
///
/// `PhysFrame` is the unit of physical memory the MMU works with: root
/// translation tables, intermediate tables, and user pages are all
/// `PhysFrame`s. The type cannot be constructed from an unaligned address
/// without going through [`PhysFrame::from_aligned`], which enforces the
/// alignment invariant.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PhysFrame(PhysAddr);

impl PhysFrame {
    /// Construct a `PhysFrame` from a page-aligned physical address.
    ///
    /// Returns `None` if `addr` is not aligned to [`PAGE_SIZE`].
    #[must_use]
    pub const fn from_aligned(addr: PhysAddr) -> Option<Self> {
        if addr.0.is_multiple_of(PAGE_SIZE) {
            Some(Self(addr))
        } else {
            None
        }
    }

    /// Return the physical address at the base of this frame.
    #[must_use]
    pub const fn addr(self) -> PhysAddr {
        self.0
    }

    /// Return the frame's base address as a raw `usize`.
    #[must_use]
    pub const fn as_usize(self) -> usize {
        self.0 .0
    }
}

/// Access and attribute flags for a mapping installed via [`Mmu::map`].
///
/// v1 exposes five flags: [`Self::WRITE`], [`Self::EXECUTE`], [`Self::USER`],
/// [`Self::DEVICE`], [`Self::GLOBAL`]. Read permission is implicit (an
/// unreadable mapping is useless). Richer attributes (cache modes,
/// shareability domains, software-available bits) are deferred to a later
/// ADR.
///
/// `MappingFlags` is a hand-rolled bitfield rather than a `bitflags!` macro
/// to avoid taking an external dependency at this stage; that tradeoff is
/// revisited in ADR-0009's open questions.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct MappingFlags(u32);

impl MappingFlags {
    /// No flags set: kernel-only, read-only, normal-cached, non-global.
    pub const EMPTY: Self = Self(0);
    /// The mapping is writable.
    pub const WRITE: Self = Self(1 << 0);
    /// The mapping is executable.
    pub const EXECUTE: Self = Self(1 << 1);
    /// The mapping is accessible from unprivileged (user) mode.
    pub const USER: Self = Self(1 << 2);
    /// The mapping targets device memory rather than normal RAM.
    pub const DEVICE: Self = Self(1 << 3);
    /// The mapping is global (not scoped to the current ASID).
    pub const GLOBAL: Self = Self(1 << 4);

    /// Construct an empty flag set.
    #[must_use]
    pub const fn empty() -> Self {
        Self::EMPTY
    }

    /// Construct a flag set from raw bits.
    ///
    /// Callers should prefer combining the named constants; `from_raw`
    /// exists so BSP implementations can pass bits across ABI boundaries.
    #[must_use]
    pub const fn from_raw(bits: u32) -> Self {
        Self(bits)
    }

    /// Return the raw bit pattern.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Return `true` if every flag in `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Return the bitwise union of two flag sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Return the bitwise intersection of two flag sets.
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Return `self` with every flag in `other` cleared.
    #[must_use]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Return `true` if no flags are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for MappingFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl BitAnd for MappingFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        self.intersection(rhs)
    }
}

impl BitOrAssign for MappingFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}

/// Error returned by [`Mmu`] operations.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MmuError {
    /// The target virtual address is already mapped in this address space.
    AlreadyMapped,
    /// The target virtual address is not mapped in this address space.
    NotMapped,
    /// The provided address is not aligned as the operation requires.
    MisalignedAddress,
    /// A frame could not be obtained from the supplied [`FrameProvider`].
    OutOfFrames,
    /// The requested [`MappingFlags`] are invalid for this operation.
    InvalidFlags,
}

/// Callback by which [`Mmu::map`] obtains frames for intermediate translation
/// tables when a mapping crosses an empty higher-level slot.
///
/// The kernel owns physical-frame allocation. The MMU never calls out to a
/// global allocator; it only pulls frames from the provider the caller
/// hands it.
pub trait FrameProvider {
    /// Allocate a zero-initialized [`PhysFrame`].
    ///
    /// Returns `None` if no frame is available. The MMU will propagate this
    /// as [`MmuError::OutOfFrames`].
    fn alloc_frame(&mut self) -> Option<PhysFrame>;
}

/// Memory management unit operations.
///
/// See [`docs/architecture/hal.md`] and [ADR-0009] for the v1 scope. In
/// particular: single page size (4 KiB), single-core TLB invalidation,
/// basic map / unmap / activate. Huge pages, per-page flag updates,
/// multi-core shootdown, translation-walk queries, and richer memory
/// typing are all future work.
///
/// `Mmu` uses an associated `AddressSpace` type because BSPs have genuinely
/// different in-memory representations (`VMSAv8` vs. future `Sv39`). Kernel
/// code that needs mapping operations is generic over `<M: Mmu>`; the
/// `activate` and `invalidate_tlb_*` methods can still be invoked through
/// `&dyn Mmu` via casting a concrete reference, but `map` / `unmap` require
/// the concrete type.
///
/// [ADR-0009]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0009-mmu-trait.md
pub trait Mmu: Send + Sync {
    /// Per-BSP address-space structure.
    type AddressSpace: Send;

    /// Construct a new address space rooted at the given physical frame.
    ///
    /// # Safety
    ///
    /// `root` must be a [`PAGE_SIZE`]-sized physical frame that is
    /// exclusively owned by the caller for the lifetime of the resulting
    /// address space, and zero-initialized.
    unsafe fn create_address_space(&self, root: PhysFrame) -> Self::AddressSpace;

    /// Return the root translation-table frame of the given address space.
    fn address_space_root(&self, as_: &Self::AddressSpace) -> PhysFrame;

    /// Activate the given address space on the current CPU core.
    fn activate(&self, as_: &Self::AddressSpace);

    /// Install a single-page mapping from `va` to `pa` with `flags`.
    ///
    /// If intermediate translation tables are needed, they are obtained
    /// from `frames`.
    ///
    /// # Errors
    ///
    /// - [`MmuError::AlreadyMapped`] if `va` already has a mapping.
    /// - [`MmuError::MisalignedAddress`] if `va` is not
    ///   [`PAGE_SIZE`]-aligned.
    /// - [`MmuError::OutOfFrames`] if an intermediate table needed a frame
    ///   and `frames` returned `None`.
    /// - [`MmuError::InvalidFlags`] if `flags` cannot be applied (for
    ///   example, user + kernel-only combinations).
    fn map(
        &self,
        as_: &mut Self::AddressSpace,
        va: VirtAddr,
        pa: PhysFrame,
        flags: MappingFlags,
        frames: &mut dyn FrameProvider,
    ) -> Result<(), MmuError>;

    /// Remove the mapping at `va` and return the physical frame it covered.
    ///
    /// # Errors
    ///
    /// Returns [`MmuError::NotMapped`] if `va` has no mapping, and
    /// [`MmuError::MisalignedAddress`] if `va` is not
    /// [`PAGE_SIZE`]-aligned.
    fn unmap(&self, as_: &mut Self::AddressSpace, va: VirtAddr) -> Result<PhysFrame, MmuError>;

    /// Invalidate any TLB entry covering `va` on the current core.
    fn invalidate_tlb_address(&self, va: VirtAddr);

    /// Invalidate every TLB entry on the current core.
    fn invalidate_tlb_all(&self);
}
