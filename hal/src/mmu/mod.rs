//! Memory management unit interaction.
//!
//! See [ADR-0009] for the v1 scope and the list of deferred capabilities.
//!
//! Pure descriptor-encoding arithmetic for the aarch64 `VMSAv8` page-table
//! format lives in the [`vmsav8`] submodule — host-testable const fn
//! helpers used by every aarch64 BSP that implements [`Mmu`]. See
//! [ADR-0009 §Revision notes][adr-0009-rev] for the additive-extension
//! record.
//!
//! [ADR-0009]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0009-mmu-trait.md
//! [adr-0009-rev]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0009-mmu-trait.md#revision-notes

pub mod vmsav8;

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
    /// The virtual address falls inside a large-block descriptor (e.g. a
    /// 2 MiB block at L1/L2 on `AArch64`). Page-granularity `map`/`unmap`
    /// inside a block requires block-splitting, which is deferred to B3+.
    /// Callers must not conflate this with [`AlreadyMapped`]: a
    /// [`BlockMapped`] region is large and may be only partially
    /// "mapped" at the requested 4 KiB granularity.
    ///
    /// [`AlreadyMapped`]: MmuError::AlreadyMapped
    /// [`BlockMapped`]: MmuError::BlockMapped
    BlockMapped,
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

/// Typed flush token returned by [`Mmu::map`] and [`Mmu::unmap`].
///
/// `MapperFlush` carries the just-mutated [`VirtAddr`] and is decorated
/// `#[must_use]` so a caller that drops it without explicitly handling it
/// triggers a `unused_must_use` lint failure (denied workspace-wide). Two
/// methods discharge the token:
///
/// - [`MapperFlush::flush`] executes [`Mmu::invalidate_tlb_address`] for
///   the held address. Use this at single-mutation call sites.
/// - [`MapperFlush::ignore`] is a documented no-op; use it after a bulk
///   sequence of mutations that will be followed by a single
///   [`Mmu::invalidate_tlb_all`].
///
/// Forgetting both is a compile-time error. Mirrors the
/// `x86_64::structures::paging::MapperFlush` Rust ecosystem precedent;
/// see [ADR-0027 §Decision outcome (c)][adr-0027] and
/// [`docs/architecture/memory-management.md` §"The MapperFlush flush-token
/// discipline"][mm-doc] for the full rationale.
///
/// The token does not bind the minting [`Mmu`] instance — `flush` accepts
/// any `Mmu` impl. v1 has a single `Mmu` instance so the absence of an
/// instance-identity check is harmless; future multi-CPU / multi-address-
/// space topologies may grow the shape (flagged in ADR-0027).
///
/// [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
/// [mm-doc]: https://github.com/cemililik/Tyrne/blob/main/docs/architecture/memory-management.md
#[must_use = "MapperFlush carries a TLB-invalidation responsibility — \
              call .flush(mmu) to invalidate the per-address TLB entry, \
              or .ignore() if a bulk invalidate_tlb_all() will follow"]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MapperFlush(VirtAddr);

impl MapperFlush {
    /// Mint a flush token for `va`.
    ///
    /// **Visibility note.** This constructor is `pub` (not `pub(crate)`)
    /// because BSP `Mmu` implementations (e.g.,
    /// `bsp-qemu-virt::QemuVirtMmu`) live in separate crates and must
    /// be able to mint tokens at every `Mmu::map` / `Mmu::unmap`
    /// return. The discipline that "kernel code never constructs
    /// tokens directly" is enforced **by convention**, not by
    /// visibility: kernel code receives tokens from `Mmu` calls and
    /// discharges them via [`Self::flush`] / [`Self::ignore`];
    /// constructing a `MapperFlush` directly outside an `Mmu`
    /// implementation is a code-smell that reviewers should reject.
    ///
    /// Soundness analysis: a misbehaving caller could mint extra
    /// tokens or never mint them, but cannot cause memory unsoundness
    /// — the token's only "power" is to call
    /// [`Mmu::invalidate_tlb_address`], which is a TLB hint, not a
    /// memory-safety operation. The 2026-05-09 PR #23 review-round
    /// Finding 5 + ADR-0027 §Decision outcome (c)'s "escape hatches
    /// are deliberate, documented, and rare" paragraph record this
    /// trade-off.
    pub const fn new(va: VirtAddr) -> Self {
        Self(va)
    }

    /// Return the virtual address this token covers.
    #[must_use]
    pub const fn virt_addr(self) -> VirtAddr {
        self.0
    }

    /// Discharge the token by invalidating the per-address TLB entry on
    /// `mmu`.
    ///
    /// Equivalent to `mmu.invalidate_tlb_address(self.virt_addr())`.
    pub fn flush<M: Mmu + ?Sized>(self, mmu: &M) {
        mmu.invalidate_tlb_address(self.0);
    }

    /// Discharge the token without issuing a per-address invalidate.
    ///
    /// Documented no-op for callers performing a bulk sequence of
    /// mutations followed by a single [`Mmu::invalidate_tlb_all`].
    /// The asymmetry between [`Self::flush`] and `ignore` is the
    /// discipline: forgetting both is a compile error; choosing `ignore`
    /// records the intent that a sweeping invalidate covers the work.
    #[inline(always)]
    #[allow(
        clippy::unused_self,
        reason = "consuming `self` is the entire point — it discharges the \
                  #[must_use] flush-token contract; converting to an \
                  associated function would defeat the lint discipline"
    )]
    pub fn ignore(self) {}
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
/// `map` and `unmap` return a typed [`MapperFlush`] token that the caller
/// must discharge via `.flush(mmu)` or `.ignore()` — see
/// [ADR-0027 §Decision outcome (c)][adr-0027] for the rationale and
/// [ADR-0009 §Revision notes][adr-0009-rev] for the additive-extension
/// record.
///
/// [ADR-0009]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0009-mmu-trait.md
/// [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
/// [adr-0009-rev]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0009-mmu-trait.md#revision-notes
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
    /// On success, returns a [`MapperFlush`] token that the caller must
    /// discharge via `.flush(self)` (executes [`Self::invalidate_tlb_address`]
    /// for `va`) or `.ignore()` (documented no-op for bulk operations
    /// followed by a single [`Self::invalidate_tlb_all`]).
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
    ) -> Result<MapperFlush, MmuError>;

    /// Remove the mapping at `va` and return the physical frame it covered
    /// paired with a [`MapperFlush`] token covering the just-removed
    /// virtual address.
    ///
    /// The caller must discharge the token via `.flush(self)` or
    /// `.ignore()` per the same discipline as [`Self::map`].
    ///
    /// # Errors
    ///
    /// Returns [`MmuError::NotMapped`] if `va` has no mapping, and
    /// [`MmuError::MisalignedAddress`] if `va` is not
    /// [`PAGE_SIZE`]-aligned.
    fn unmap(
        &self,
        as_: &mut Self::AddressSpace,
        va: VirtAddr,
    ) -> Result<(MapperFlush, PhysFrame), MmuError>;

    /// Invalidate any TLB entry covering `va` on the current core.
    fn invalidate_tlb_address(&self, va: VirtAddr);

    /// Invalidate every TLB entry on the current core.
    fn invalidate_tlb_all(&self);
}
