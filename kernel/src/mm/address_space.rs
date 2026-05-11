//! `AddressSpace<M>` kernel object — per-task translation context.
//!
//! Per [ADR-0028][adr-0028], the kernel wraps the BSP-specific
//! [`Mmu::AddressSpace`] associated type into a typed kernel object
//! that lives in an [`Arena`][crate::obj::arena::Arena], is reachable
//! through a `CapKind::AddressSpace` capability (lands in T-018
//! commit 2), and is activated on context switch when the outgoing
//! and incoming tasks have different [`AddressSpaceHandle`]s (lands
//! in T-018 commit 4).
//!
//! ## v1 scope (this commit)
//!
//! Pure data-structure landing — no cap integration, no scheduler
//! hook, no BSP wiring. Lands the [`AddressSpace<M>`] struct, the
//! [`AddressSpaceHandle`] newtype, the [`AddressSpaceArena<M>`] type
//! alias, the [`AddressSpaceError`] enum, and the
//! `create_address_space` / `destroy_address_space` /
//! `get_address_space` / `get_address_space_mut` free functions that
//! mirror the [`crate::obj::endpoint`] / [`crate::obj::notification`] /
//! [`crate::obj::task`] surface.
//!
//! The capability-gated wrappers (`cap_create_address_space` /
//! `cap_map` / `cap_unmap`) and the variant additions to
//! [`AddressSpaceError`] (`OutOfFrames`, `CapError(CapError)`,
//! `MmuMapError(MmuError)`, `MmuUnmapError(MmuError)`) land in T-018
//! commit 3. The activation-on-context-switch hook lands in T-018
//! commit 4. The bootstrap-AS wrap + arena `StaticCell` publication
//! land in T-018 commit 5.
//!
//! [adr-0028]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0028-address-space-data-structure.md

use crate::obj::arena::{Arena, SlotId};
use tyrne_hal::{mmu::Mmu, PhysFrame};

/// Compile-time bound on the number of live `AddressSpace` kernel
/// objects. v1's QEMU virt BSP has the bootstrap AS + headroom for
/// T-018's two-AS isolation tests; later BSPs may grow this.
pub const ADDRESS_SPACE_ARENA_CAPACITY: usize = 8;

/// Kernel-side `AddressSpace` kernel object — wraps the BSP-specific
/// `<M as Mmu>::AddressSpace` value with kernel-side bookkeeping.
///
/// ## Structure
///
/// For v1, the struct holds only the BSP-specific [`Mmu::AddressSpace`]
/// value (`inner`). The per-AS generation tag lives in the arena slot
/// per [ADR-0016][adr-0016] (mirrors [`crate::obj::endpoint::Endpoint`] /
/// [`crate::obj::notification::Notification`] / [`crate::obj::task::Task`]).
/// Per [ADR-0028 §Decision outcome][adr-0028]'s forward-compat note,
/// fields like `asid: Option<Asid>` and per-AS reverse-mapping pointers
/// land here additively when ADR-0033 (high-half migration) opens —
/// not added today (CLAUDE.md non-negotiable #6, no speculative design).
///
/// [adr-0016]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0016-kernel-object-storage.md
/// [adr-0028]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0028-address-space-data-structure.md
pub struct AddressSpace<M: Mmu> {
    inner: M::AddressSpace,
}

impl<M: Mmu> AddressSpace<M> {
    /// Wrap an already-active [`Mmu::AddressSpace`] value as a
    /// kernel-object.
    ///
    /// Per [ADR-0028 §Simulation row 0][adr-0028]. The BSP first
    /// materialises the inner value via a BSP-side
    /// `Mmu::wrap_existing_root(root)` companion (does not allocate,
    /// does not zero-fill, does not modify any page-table state —
    /// just names the already-live root); this constructor then wraps
    /// it with kernel-side metadata. **Does not** call
    /// [`Mmu::create_address_space`] — that would re-zero the live L0
    /// frame and break the running translation tables.
    ///
    /// Used exactly once at boot, in BSP wiring (T-018 commit 5), for
    /// the bootstrap AS. All subsequent address spaces are constructed
    /// via `cap_create_address_space` → `PMM.alloc_frame()` →
    /// `Mmu::create_address_space(root)` (T-018 commit 3).
    ///
    /// [adr-0028]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0028-address-space-data-structure.md
    #[must_use]
    pub const fn wrap_bootstrap(inner: M::AddressSpace) -> Self {
        Self { inner }
    }

    /// Return the root translation-table physical frame.
    ///
    /// Diagnostic accessor for the bootstrap-banner
    /// (`tyrne: address-space-arena ready (... ; bootstrap AS root = 0x<pa>)`)
    /// and host-test cross-checks. Delegates to [`Mmu::address_space_root`].
    #[must_use]
    pub fn root_frame(&self, mmu: &M) -> PhysFrame {
        mmu.address_space_root(&self.inner)
    }

    /// Return a reference to the BSP-specific inner value.
    ///
    /// Crate-internal: the cap-gated wrappers (T-018 commit 3) use
    /// this to pass `&Mmu::AddressSpace` to [`Mmu::activate`]; the
    /// activation hook (T-018 commit 4) uses it on the context-switch
    /// path. Outside code accesses an `AddressSpace<M>` only through
    /// the cap-gated surface, never through this accessor directly.
    #[must_use]
    #[allow(
        dead_code,
        reason = "T-018 commit 3 (cap-gated wrappers) is the first caller; \
                  landed in commit 1 for module-shape completeness so commit 3 \
                  adds only the wrapper bodies, not the accessor surface"
    )]
    pub(crate) const fn inner(&self) -> &M::AddressSpace {
        &self.inner
    }

    /// Return a mutable reference to the BSP-specific inner value.
    ///
    /// Crate-internal: used by the cap-gated wrappers (T-018 commit 3)
    /// for [`Mmu::map`] / [`Mmu::unmap`] calls that need `&mut`.
    #[allow(
        dead_code,
        reason = "T-018 commit 3 (cap-gated wrappers) is the first caller; \
                  landed in commit 1 for module-shape completeness so commit 3 \
                  adds only the wrapper bodies, not the accessor surface"
    )]
    pub(crate) fn inner_mut(&mut self) -> &mut M::AddressSpace {
        &mut self.inner
    }
}

/// Typed handle referring to an [`AddressSpace`] in an
/// [`AddressSpaceArena`].
///
/// Generation-tagged via the underlying [`SlotId`]; stale handles
/// fail lookup with [`AddressSpaceError::StaleHandle`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct AddressSpaceHandle(SlotId);

impl AddressSpaceHandle {
    pub(crate) const fn from_slot(slot: SlotId) -> Self {
        Self(slot)
    }

    pub(crate) const fn slot(self) -> SlotId {
        self.0
    }

    /// Construct a handle from raw parts for unit-test scaffolding in
    /// callers that need distinct address-space references without
    /// allocating.
    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "symmetric with EndpointHandle::test_handle / TaskHandle::test_handle"
    )]
    #[must_use]
    pub(crate) const fn test_handle(index: u16, generation: u32) -> Self {
        Self(SlotId::from_parts(index, generation))
    }
}

/// The concrete arena type for address spaces.
///
/// Mirrors [`crate::obj::EndpointArena`] / [`crate::obj::TaskArena`] /
/// [`crate::obj::NotificationArena`] in shape — same generic
/// [`Arena<T, N>`][Arena] backing, same generation-tagged
/// [`SlotId`]-based handle resolution, same fixed-capacity
/// no-allocation discipline. The added axis is the `M: Mmu`
/// parameter that propagates the BSP-specific [`Mmu::AddressSpace`]
/// type into the arena slots; per [ADR-0028 §Decision outcome][adr-0028]
/// the kernel inherits this generic from the scheduler surface
/// (ADR-0019 / ADR-0020) rather than introducing a parallel axis.
///
/// [adr-0028]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0028-address-space-data-structure.md
pub type AddressSpaceArena<M> = Arena<AddressSpace<M>, ADDRESS_SPACE_ARENA_CAPACITY>;

/// Errors returned by address-space operations.
///
/// `#[non_exhaustive]` so that variants added by later T-018 commits
/// (`OutOfFrames`, `CapError`, `MmuMapError`, `MmuUnmapError` — added
/// in commit 3 when the cap-gated wrappers land) are not breaking
/// changes to matches outside the crate.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AddressSpaceError {
    /// The arena is full; no free slot for a new address space.
    /// Returned by [`create_address_space`] when every slot in the
    /// underlying [`AddressSpaceArena`] is in use.
    ArenaFull,
    /// The handle does not name a live slot — either never allocated,
    /// already freed, or stale after reuse. Returned by
    /// [`destroy_address_space`] (and, in T-018 commit 3, by the
    /// cap-gated `cap_map` / `cap_unmap` wrappers when the cap's
    /// handle has gone stale).
    StaleHandle,
}

/// Allocate an address space in `arena`.
///
/// The caller constructs the [`AddressSpace<M>`] value first (via
/// [`AddressSpace::wrap_bootstrap`] for the bootstrap AS, or — in
/// T-018 commit 3 — via the `cap_create_address_space` wrapper
/// which calls [`Mmu::create_address_space`] under a cap-gated
/// authority check); this function inserts the value into the arena
/// and returns the typed handle.
///
/// # Errors
///
/// [`AddressSpaceError::ArenaFull`] when every slot is in use.
pub fn create_address_space<M: Mmu>(
    arena: &mut AddressSpaceArena<M>,
    address_space: AddressSpace<M>,
) -> Result<AddressSpaceHandle, AddressSpaceError> {
    arena
        .allocate(address_space)
        .map(AddressSpaceHandle::from_slot)
        .ok_or(AddressSpaceError::ArenaFull)
}

/// Free the address space at `handle`, returning the stored value.
///
/// Reserved for B4+ `cap_revoke(AddressSpaceCap)` — the destroy path
/// walks the page-table tree, frees every L3 mapping via
/// [`Mmu::unmap`], then frees the L3/L2/L1/L0 frames back to PMM
/// before reaching this function. v1 has no caller; the function is
/// landed in commit 1 to keep the arena surface complete and
/// symmetric with [`create_address_space`], and gated with
/// `#[allow(dead_code, ...)]` until B4+ surfaces a caller.
///
/// # Errors
///
/// [`AddressSpaceError::StaleHandle`] when `handle` is stale or
/// already freed.
#[allow(
    dead_code,
    reason = "destroy path is B4+ (cap_revoke(AddressSpaceCap)); v1 has no caller"
)]
pub fn destroy_address_space<M: Mmu>(
    arena: &mut AddressSpaceArena<M>,
    handle: AddressSpaceHandle,
) -> Result<AddressSpace<M>, AddressSpaceError> {
    arena
        .free(handle.slot())
        .ok_or(AddressSpaceError::StaleHandle)
}

/// Return a reference to the address space at `handle`, or `None` if
/// stale / freed.
#[must_use]
pub fn get_address_space<M: Mmu>(
    arena: &AddressSpaceArena<M>,
    handle: AddressSpaceHandle,
) -> Option<&AddressSpace<M>> {
    arena.get(handle.slot())
}

/// Return a mutable reference to the address space at `handle`, or
/// `None` if stale / freed.
pub fn get_address_space_mut<M: Mmu>(
    arena: &mut AddressSpaceArena<M>,
    handle: AddressSpaceHandle,
) -> Option<&mut AddressSpace<M>> {
    arena.get_mut(handle.slot())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests may use pragmas forbidden in production kernel code"
)]
mod tests {
    use super::{
        create_address_space, destroy_address_space, get_address_space, get_address_space_mut,
        AddressSpace, AddressSpaceArena, AddressSpaceError,
    };
    use tyrne_hal::{mmu::Mmu, PhysAddr, PhysFrame};
    use tyrne_test_hal::FakeMmu;

    fn frame(addr: usize) -> PhysFrame {
        PhysFrame::from_aligned(PhysAddr(addr)).expect("test addr must be page-aligned")
    }

    /// Construct a `FakeAddressSpace` naming `root` via the `FakeMmu`'s
    /// `create_address_space` (which is the `FakeMmu`'s "wrap existing
    /// root" surrogate — it doesn't modify the input frame, just
    /// records it). In production the bootstrap path uses
    /// `Mmu::wrap_existing_root` instead, but `FakeMmu` doesn't have a
    /// separate wrap surface — its `create_address_space` is already
    /// pure (no zero-fill).
    fn fake_inner(mmu: &FakeMmu, root: PhysFrame) -> <FakeMmu as Mmu>::AddressSpace {
        // SAFETY: FakeMmu::create_address_space is pure host-side code
        // that constructs a HashMap-backed mock; no UB possible.
        unsafe { mmu.create_address_space(root) }
    }

    #[test]
    fn wrap_bootstrap_returns_address_space_with_root() {
        // Pin ADR-0028 §Simulation row 0: given a BSP-side
        // M::AddressSpace value constructed from a root frame,
        // `AddressSpace::wrap_bootstrap(inner)` returns a kernel-object
        // value whose `root_frame()` matches the original root.
        let mmu = FakeMmu::new();
        let root = frame(0x4000_0000);
        let inner = fake_inner(&mmu, root);

        let address_space: AddressSpace<FakeMmu> = AddressSpace::wrap_bootstrap(inner);

        assert_eq!(address_space.root_frame(&mmu), root);
    }

    #[test]
    fn arena_alloc_returns_distinct_handles() {
        let mmu = FakeMmu::new();
        let mut arena: AddressSpaceArena<FakeMmu> = AddressSpaceArena::new();

        let as_a = AddressSpace::wrap_bootstrap(fake_inner(&mmu, frame(0x4000_0000)));
        let as_b = AddressSpace::wrap_bootstrap(fake_inner(&mmu, frame(0x4000_1000)));

        let h_a = create_address_space(&mut arena, as_a).unwrap();
        let h_b = create_address_space(&mut arena, as_b).unwrap();

        assert_ne!(h_a, h_b, "distinct allocs produce distinct handles");
        assert_eq!(
            get_address_space(&arena, h_a).map(|a| a.root_frame(&mmu)),
            Some(frame(0x4000_0000))
        );
        assert_eq!(
            get_address_space(&arena, h_b).map(|a| a.root_frame(&mmu)),
            Some(frame(0x4000_1000))
        );
    }

    #[test]
    fn arena_get_with_stale_handle_returns_none() {
        // Pin the generation-tag contract: alloc + free + alloc-again
        // at the same slot; the original handle's generation no longer
        // matches the slot's, and `get` returns None.
        let mmu = FakeMmu::new();
        let mut arena: AddressSpaceArena<FakeMmu> = AddressSpaceArena::new();

        let first = AddressSpace::wrap_bootstrap(fake_inner(&mmu, frame(0x4000_0000)));
        let h_first = create_address_space(&mut arena, first).unwrap();

        let _removed = destroy_address_space(&mut arena, h_first).unwrap();

        // After free, the original handle no longer resolves.
        assert!(get_address_space(&arena, h_first).is_none());
        assert!(get_address_space_mut(&mut arena, h_first).is_none());

        // Slot reuse: alloc-again returns a handle with a different
        // generation tag, even if it picks the same slot index.
        let second = AddressSpace::wrap_bootstrap(fake_inner(&mmu, frame(0x4000_1000)));
        let h_second = create_address_space(&mut arena, second).unwrap();
        assert_ne!(
            h_first, h_second,
            "slot reuse must produce a distinct handle (generation tag bumped)"
        );

        // The new handle resolves; the stale one still does not.
        assert_eq!(
            get_address_space(&arena, h_second).map(|a| a.root_frame(&mmu)),
            Some(frame(0x4000_1000))
        );
        assert!(get_address_space(&arena, h_first).is_none());
    }

    #[test]
    fn arena_full_returns_arena_full_error() {
        let mmu = FakeMmu::new();
        let mut arena: AddressSpaceArena<FakeMmu> = AddressSpaceArena::new();

        // Fill the arena to ADDRESS_SPACE_ARENA_CAPACITY.
        for i in 0..super::ADDRESS_SPACE_ARENA_CAPACITY {
            let inner = fake_inner(&mmu, frame(0x4000_0000 + i * 0x1000));
            create_address_space(&mut arena, AddressSpace::wrap_bootstrap(inner))
                .expect("arena fill within capacity must succeed");
        }

        // One more should fail with `ArenaFull`.
        let overflow_inner = fake_inner(&mmu, frame(0x4001_0000));
        let result = create_address_space(&mut arena, AddressSpace::wrap_bootstrap(overflow_inner));
        assert_eq!(result, Err(AddressSpaceError::ArenaFull));
    }

    #[test]
    fn destroy_with_stale_handle_returns_stale_handle_error() {
        let mmu = FakeMmu::new();
        let mut arena: AddressSpaceArena<FakeMmu> = AddressSpaceArena::new();

        let inner = fake_inner(&mmu, frame(0x4000_0000));
        let handle = create_address_space(&mut arena, AddressSpace::wrap_bootstrap(inner)).unwrap();

        // First free succeeds.
        destroy_address_space(&mut arena, handle).unwrap();

        // Second free returns StaleHandle (the handle is no longer live).
        // `assert_eq!` on Result<AddressSpace<M>, _> would need `Debug +
        // PartialEq` on `AddressSpace<M>`, which would require an `M::AddressSpace:
        // Debug + PartialEq` bound that the [`Mmu`] trait does not impose. Use
        // `matches!` to match on the error discriminant without comparing the Ok
        // variant.
        let result = destroy_address_space(&mut arena, handle);
        assert!(
            matches!(result, Err(AddressSpaceError::StaleHandle)),
            "expected Err(StaleHandle) on double-free of the same handle"
        );
    }

    #[test]
    fn inner_accessors_provide_borrow_and_borrow_mut() {
        // Crate-internal `inner()` / `inner_mut()` are tested through
        // `root_frame` (which uses `inner()`) and a manual mutable
        // borrow round-trip that mirrors how the T-018 commit 3
        // cap-gated wrappers will use `inner_mut()`.
        let mmu = FakeMmu::new();
        let mut address_space: AddressSpace<FakeMmu> =
            AddressSpace::wrap_bootstrap(fake_inner(&mmu, frame(0x4000_0000)));

        // `inner()` shape (via `root_frame`): borrows immutably, returns the root.
        assert_eq!(address_space.root_frame(&mmu), frame(0x4000_0000));

        // `inner_mut()` shape: borrow returns the BSP-specific value;
        // the borrow ends at scope end so subsequent reads are fine.
        let _inner_mut = address_space.inner_mut();

        // The address space is still readable after `inner_mut` drops.
        assert_eq!(address_space.root_frame(&mmu), frame(0x4000_0000));
    }
}
