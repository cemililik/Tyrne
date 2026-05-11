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

use crate::cap::{CapError, CapHandle, CapKind, CapObject, CapRights, Capability, CapabilityTable};
use crate::obj::arena::{Arena, SlotId};
use tyrne_hal::{FrameProvider, MappingFlags, Mmu, MmuError, PhysFrame, VirtAddr};

/// Compile-time bound on the number of live `AddressSpace` kernel
/// objects. v1's QEMU virt BSP has the bootstrap AS + headroom for
/// T-018's two-AS isolation tests; later BSPs may grow this.
pub const ADDRESS_SPACE_ARENA_CAPACITY: usize = 8;

/// The canonical [`AddressSpaceHandle`] for the bootstrap address space.
///
/// The bootstrap AS lives in arena slot 0 (kernel-init allocates it
/// first per [ADR-0028 §Simulation row 0][adr-0028]). Code that needs
/// to name the bootstrap AS **before** the arena allocation runs uses
/// this constant — most notably the BSP-side `Task` constructors in
/// `kernel_entry` (commit 4 lands the field on `Task`; commit 5's BSP
/// wiring then ensures arena slot 0 holds the bootstrap AS that this
/// handle names).
///
/// Calling discipline: the BSP MUST allocate the bootstrap AS first
/// (before any other AS or any cap-table operation that consumes a
/// slot) so this handle's `(index=0, generation=0)` deterministically
/// matches the live arena slot.
///
/// [adr-0028]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0028-address-space-data-structure.md
pub const BOOTSTRAP_ADDRESS_SPACE_HANDLE: AddressSpaceHandle =
    AddressSpaceHandle::from_slot(SlotId::first_slot());

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

    /// Wrap a freshly-constructed [`Mmu::AddressSpace`] value (returned
    /// by [`Mmu::create_address_space`] from a zero-filled root frame)
    /// as a kernel-object.
    ///
    /// Used by [`cap_create_address_space`] after the root-frame
    /// allocation + BSP `create_address_space` call. Structurally
    /// identical to [`AddressSpace::wrap_bootstrap`] — both wrap an
    /// `M::AddressSpace` with kernel-side metadata — but the name
    /// documents the caller's intent: `wrap_bootstrap` for the
    /// already-live bootstrap topology (commit 5 BSP path);
    /// `from_mmu_address_space` for the post-`Mmu::create_address_space`
    /// path (commit 3 cap-gated wrapper).
    #[must_use]
    pub const fn from_mmu_address_space(inner: M::AddressSpace) -> Self {
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
    /// Crate-internal: the activation hook (T-018 commit 4) uses
    /// this to pass `&Mmu::AddressSpace` to [`Mmu::activate`] on
    /// the context-switch path. Outside code accesses an
    /// `AddressSpace<M>` only through the cap-gated surface,
    /// never through this accessor directly.
    #[must_use]
    #[allow(
        dead_code,
        reason = "T-018 commit 4 (activation hook in yield_now) is the first \
                  caller; landed for module-shape completeness so commit 4 \
                  adds only the scheduler-side hook, not the accessor surface"
    )]
    pub(crate) const fn inner(&self) -> &M::AddressSpace {
        &self.inner
    }

    /// Return a mutable reference to the BSP-specific inner value.
    ///
    /// Crate-internal: used by the cap-gated wrappers (T-018 commit 3)
    /// for [`Mmu::map`] / [`Mmu::unmap`] calls that need `&mut`.
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
/// `#[non_exhaustive]` so future variant additions remain
/// non-breaking. v1 surface (commits 1–3 of T-018):
///
/// - [`ArenaFull`] — arena exhausted (commit 1).
/// - [`StaleHandle`] — generation-tag mismatch (commit 1).
/// - [`OutOfFrames`] — PMM exhausted during root-frame alloc
///   (commit 3).
/// - [`CapError(CapError)`] — pass-through from cap-resolution
///   (commit 3).
/// - [`MmuMapError(MmuError)`] — pass-through from [`Mmu::map`]
///   (commit 3).
/// - [`MmuUnmapError(MmuError)`] — pass-through from [`Mmu::unmap`]
///   (commit 3).
///
/// The wrap variants preserve the underlying [`CapError`] /
/// [`MmuError`] taxonomy without flattening, so capability-side
/// observers see exactly the cap-side or HAL-side failure. The
/// cap-gated wrappers (`cap_create_address_space` / `cap_map` /
/// `cap_unmap`) expose a single unified return type — every
/// cap-resolution failure surfaces as `Err(CapError(_))`, every
/// map failure as `Err(MmuMapError(_))`, etc.
///
/// [`ArenaFull`]: AddressSpaceError::ArenaFull
/// [`StaleHandle`]: AddressSpaceError::StaleHandle
/// [`OutOfFrames`]: AddressSpaceError::OutOfFrames
/// [`CapError(CapError)`]: AddressSpaceError::CapError
/// [`MmuMapError(MmuError)`]: AddressSpaceError::MmuMapError
/// [`MmuUnmapError(MmuError)`]: AddressSpaceError::MmuUnmapError
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AddressSpaceError {
    /// The arena is full; no free slot for a new address space.
    /// Returned by [`create_address_space`] when every slot in the
    /// underlying [`AddressSpaceArena`] is in use.
    ArenaFull,
    /// The handle does not name a live slot — either never allocated,
    /// already freed, or stale after reuse. Returned by
    /// [`destroy_address_space`] and by the cap-gated `cap_map` /
    /// `cap_unmap` wrappers when the cap's handle has gone stale.
    StaleHandle,
    /// PMM exhausted: the underlying [`FrameProvider::alloc_frame`]
    /// returned `None`. Returned by [`cap_create_address_space`]
    /// when no physical frame is available for the new root
    /// translation table.
    OutOfFrames,
    /// Capability-resolution failure. Wraps the underlying
    /// [`CapError`] so the caller sees the exact cap-side variant
    /// (`InvalidHandle`, `WrongKind`, `CapsExhausted`, etc.) rather
    /// than a flattened "cap error" discriminator.
    CapError(CapError),
    /// [`Mmu::map`] failure. Wraps the underlying [`MmuError`]
    /// (`OutOfFrames` for intermediate-table allocation,
    /// `AlreadyMapped`, `MisalignedAddress`, `InvalidFlags`, etc.)
    /// without flattening.
    MmuMapError(MmuError),
    /// [`Mmu::unmap`] failure. Wraps the underlying [`MmuError`]
    /// (`NotMapped`, `MisalignedAddress`, etc.) without flattening.
    MmuUnmapError(MmuError),
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
/// Reserved for B4+ `cap_revoke(AddressSpaceCap)` — the full destroy
/// path walks the page-table tree, frees every L3 mapping via
/// [`Mmu::unmap`], then frees the L3/L2/L1/L0 frames back to PMM
/// before reaching this function. T-018 commit 3 uses this function
/// for one v1 caller — the rollback path in [`cap_create_address_space`]
/// when cap-table minting fails after a successful arena allocation.
///
/// # Errors
///
/// [`AddressSpaceError::StaleHandle`] when `handle` is stale or
/// already freed.
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

/// Activate the address space named by `handle` on the current CPU.
///
/// Looks up the [`AddressSpace<M>`] in `arena`, then invokes
/// [`Mmu::activate`] on its inner BSP-specific value. Used by the
/// scheduler activation hook (T-018 commit 4) — the BSP wraps a
/// call to this function as the closure passed to [`yield_now`] /
/// [`ipc_send_and_yield`] / [`ipc_recv_and_yield`] / [`start`] (T-018
/// commit 5).
///
/// **Stale-handle behaviour.** Returns silently if `handle` is stale
/// (the underlying [`Arena::get`] returns `None`). A stale handle on
/// the context-switch path indicates a kernel programming error
/// (scheduler's `task_address_space_handles` array points at a freed
/// arena slot), but panicking inside the activation hook would abort
/// the kernel from inside an `IrqGuard` scope which is worse than
/// running the next task on the previously-active AS. The next
/// `yield_now` may re-fire the activation if the handle slot becomes
/// live again.
///
/// Crate-level (`pub`) because the BSP's activation closure invokes
/// it; the kernel-side surface does not otherwise expose
/// [`Mmu::activate`] outside the cap-gated `cap_*` wrappers.
///
/// [yield_now]: crate::sched::yield_now
/// [ipc_send_and_yield]: crate::sched::ipc_send_and_yield
/// [ipc_recv_and_yield]: crate::sched::ipc_recv_and_yield
/// [start]: crate::sched::start
pub fn activate_address_space_handle<M: Mmu>(
    arena: &AddressSpaceArena<M>,
    handle: AddressSpaceHandle,
    mmu: &M,
) {
    if let Some(as_) = get_address_space(arena, handle) {
        mmu.activate(as_.inner());
    }
}

// ── Capability-gated wrappers (T-018 commit 3) ────────────────────────────────
//
// Per [ADR-0028 §Decision outcome][adr-0028]: every `Mmu::map` /
// `Mmu::unmap` / `Mmu::create_address_space` call site in the kernel
// goes through a `cap_*` wrapper that resolves a capability first.
// No ambient authority. The wrappers expose a unified
// [`AddressSpaceError`] return type that wraps the underlying
// [`CapError`] / [`MmuError`] taxonomies without flattening.
//
// [adr-0028]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0028-address-space-data-structure.md

/// Resolve a capability handle to an [`AddressSpaceHandle`].
///
/// Looks up the cap by handle, validates that its kind is
/// [`CapKind::AddressSpace`], and extracts the typed handle from
/// the [`CapObject::AddressSpace`] variant. Used internally by
/// [`cap_create_address_space`] (to validate parent-cap authority),
/// [`cap_map`], and [`cap_unmap`] (to resolve the target AS).
///
/// # Errors
///
/// - [`AddressSpaceError::CapError(CapError::InvalidHandle)`] when
///   the cap-table lookup fails (stale handle, freed slot).
/// - [`AddressSpaceError::CapError(CapError::WrongKind)`] when the
///   cap exists but its kind is not [`CapKind::AddressSpace`].
fn resolve_address_space_cap(
    table: &CapabilityTable,
    cap_handle: CapHandle,
) -> Result<AddressSpaceHandle, AddressSpaceError> {
    let cap = table
        .lookup(cap_handle)
        .map_err(AddressSpaceError::CapError)?;
    match cap.object() {
        CapObject::AddressSpace(h) => Ok(h),
        _ => Err(AddressSpaceError::CapError(CapError::WrongKind)),
    }
}

/// Capability-gated address-space creation.
///
/// Per [ADR-0028 §Simulation row 1][adr-0028]. The wrapper:
///
/// 1. Resolves `parent_cap_handle` via [`resolve_address_space_cap`];
///    in v1 the kernel-init holds an "ambient" AS cap on slot 0
///    (the bootstrap AS) that grants creation authority. Future
///    B4+ work introduces an `Untyped`-style frame ownership
///    discipline; v1 adopts the simplest "any AS cap grants AS
///    creation" shape.
/// 2. Validates `new_rights ⊆ parent_cap.rights` per the
///    no-widening discipline of [ADR-0014][adr-0014] (mirrors
///    [`CapabilityTable::cap_copy`]).
/// 3. Allocates a root frame via [`FrameProvider::alloc_frame`].
///    Returns [`OutOfFrames`] on PMM exhaustion; no other state
///    has been mutated at this point.
/// 4. Calls `unsafe { mmu.create_address_space(root) }` to
///    materialise the BSP-specific inner value. The unsafe rides
///    [UNSAFE-2026-0023]'s existing umbrella scope — `root` is
///    page-aligned (statically by [`PhysFrame`]) and zero-filled
///    ([UNSAFE-2026-0026], the PMM contract), satisfying
///    `create_address_space`'s precondition.
/// 5. Allocates an arena slot via [`create_address_space`].
///    On [`ArenaFull`] the PMM root frame is leaked (v1
///    limitation: [`FrameProvider`] has no `free_frame` method;
///    direct PMM access would force the wrapper to take
///    `&mut Pmm<N, R>` and lose BSP-agnosticism at the wrapper
///    surface). This path is structurally unreachable in v1's
///    sized-arena BSPs.
/// 6. Mints the cap via [`CapabilityTable::insert_root`]. On
///    cap-table failure, rolls back the arena slot via
///    [`destroy_address_space`]; the PMM frame still leaks.
///
/// # Errors
///
/// - [`OutOfFrames`] — PMM exhausted at step 3.
/// - [`ArenaFull`] — arena exhausted at step 5; PMM frame leaks.
/// - [`CapError(CapError)`] — cap lookup / kind validation /
///   widening check / table-mint failure (each preserves the
///   underlying [`CapError`] discriminator).
///
/// [adr-0014]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0014-capability-representation.md
/// [adr-0028]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0028-address-space-data-structure.md
/// [UNSAFE-2026-0023]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md
/// [UNSAFE-2026-0026]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md
/// [`OutOfFrames`]: AddressSpaceError::OutOfFrames
/// [`ArenaFull`]: AddressSpaceError::ArenaFull
/// [`CapError(CapError)`]: AddressSpaceError::CapError
#[allow(
    clippy::too_many_arguments,
    reason = "cap-gated wrappers thread the full kernel-state surface \
              (table + parent_cap + rights + mmu + pmm + arena) through \
              by reference per the no-ambient-authority discipline; \
              bundling into a struct would obscure the data-flow without \
              reducing argument count at the call site"
)]
pub fn cap_create_address_space<M: Mmu>(
    table: &mut CapabilityTable,
    parent_cap_handle: CapHandle,
    new_rights: CapRights,
    mmu: &M,
    pmm: &mut dyn FrameProvider,
    arena: &mut AddressSpaceArena<M>,
) -> Result<CapHandle, AddressSpaceError> {
    // Step 1: resolve parent-cap, validate kind. No state change.
    let parent_cap = table
        .lookup(parent_cap_handle)
        .map_err(AddressSpaceError::CapError)?;
    if parent_cap.kind() != CapKind::AddressSpace {
        return Err(AddressSpaceError::CapError(CapError::WrongKind));
    }

    // Step 2: no-widening check on rights.
    if !parent_cap.rights().contains(new_rights) {
        return Err(AddressSpaceError::CapError(CapError::WidenedRights));
    }

    // Step 3: PMM frame for the new root. Fail-fast before any
    // arena / cap-table mutation.
    let root = pmm.alloc_frame().ok_or(AddressSpaceError::OutOfFrames)?;

    // Step 4: materialise the BSP-specific inner value.
    //
    // SAFETY:
    // **Why unsafe is needed.** [`Mmu::create_address_space`] is
    // `unsafe fn` per ADR-0009 + UNSAFE-2026-0023's existing umbrella
    // — the trait method requires the caller to guarantee `root` is
    // page-aligned and zero-filled before the BSP writes a fresh L0
    // table descriptor set into it. The unsafety lives at the trait
    // surface, not in this wrapper.
    //
    // **Invariants upheld.** (1) Page-alignment: the [`PhysFrame`]
    // type encodes alignment statically; `pmm.alloc_frame()` returns
    // `Option<PhysFrame>` where the Some-case is page-aligned by
    // construction. (2) Zero-fill: PMM's `alloc_frame` zero-fills
    // the returned frame per UNSAFE-2026-0026's contract; the
    // 4 KiB region is all zeros when this wrapper observes it.
    //
    // **Why safer alternatives were rejected.** The unsafe is on
    // [`Mmu::create_address_space`]'s trait surface; the wrapper
    // can't replace it with a safe alternative without changing the
    // HAL trait (rejected per ADR-0028 §Decision outcome's HAL
    // stability driver). Audit: UNSAFE-2026-0023 (existing
    // umbrella scope; first runtime exerciser per T-018 lifts the
    // `Pending QEMU smoke verification` note when commit 6's smoke
    // fixture lands).
    let inner = unsafe { mmu.create_address_space(root) };

    // Step 5: arena slot.
    let handle = create_address_space(arena, AddressSpace::from_mmu_address_space(inner))?;

    // Step 6: mint cap. On failure, rollback the arena slot. The
    // PMM frame leaks (v1 limitation; documented above).
    let cap = Capability::new(new_rights, CapObject::AddressSpace(handle));
    match table.insert_root(cap) {
        Ok(cap_handle) => Ok(cap_handle),
        Err(e) => {
            // Best-effort rollback of the arena slot; ignore the
            // unlikely double-free path (the slot was just allocated
            // and we hold the only handle to it).
            let _ = destroy_address_space(arena, handle);
            Err(AddressSpaceError::CapError(e))
        }
    }
}

/// Capability-gated mapping installation.
///
/// Per [ADR-0028 §Simulation row 2][adr-0028]. The wrapper resolves
/// the cap, gets a `&mut AddressSpace<M>` from the arena, calls
/// [`Mmu::map`] with the underlying BSP value, and discharges the
/// returned [`MapperFlush`][tyrne_hal::MapperFlush] token via
/// `flush(mmu)` which invokes [`Mmu::invalidate_tlb_address`] per
/// ADR-0027's flush-token discipline.
///
/// First post-bootstrap runtime exerciser of [UNSAFE-2026-0025]
/// (the `QemuVirtMmu::map` page-table-walker descriptor writes);
/// T-018 commit 6's audit-log Amendment lifts its `Pending QEMU
/// smoke verification` note.
///
/// # Errors
///
/// - [`CapError(_)`] — cap lookup / kind validation.
/// - [`StaleHandle`] — the cap's [`AddressSpaceHandle`] no longer
///   names a live arena slot (generation mismatch after slot reuse).
/// - [`MmuMapError(MmuError)`] — pass-through from `Mmu::map`
///   (`OutOfFrames` for intermediate-table allocs, `AlreadyMapped`,
///   `MisalignedAddress`, `InvalidFlags`, ...).
///
/// [adr-0028]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0028-address-space-data-structure.md
/// [UNSAFE-2026-0025]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md
/// [`CapError(_)`]: AddressSpaceError::CapError
/// [`StaleHandle`]: AddressSpaceError::StaleHandle
/// [`MmuMapError(MmuError)`]: AddressSpaceError::MmuMapError
#[allow(
    clippy::too_many_arguments,
    reason = "cap-gated wrappers thread the full kernel-state surface \
              (table + cap + mmu + pmm + arena + va + pa + flags) through \
              by reference per the no-ambient-authority discipline; \
              bundling into a struct would obscure the data-flow without \
              reducing argument count at the call site"
)]
pub fn cap_map<M: Mmu>(
    table: &CapabilityTable,
    cap_handle: CapHandle,
    mmu: &M,
    pmm: &mut dyn FrameProvider,
    arena: &mut AddressSpaceArena<M>,
    va: VirtAddr,
    pa: PhysFrame,
    flags: MappingFlags,
) -> Result<(), AddressSpaceError> {
    let handle = resolve_address_space_cap(table, cap_handle)?;
    let address_space =
        get_address_space_mut(arena, handle).ok_or(AddressSpaceError::StaleHandle)?;
    let token = mmu
        .map(address_space.inner_mut(), va, pa, flags, pmm)
        .map_err(AddressSpaceError::MmuMapError)?;
    token.flush(mmu);
    Ok(())
}

/// Capability-gated mapping removal.
///
/// Per [ADR-0028 §Simulation row 2][adr-0028]. Mirrors [`cap_map`]
/// inversely: resolve the cap, `&mut AddressSpace<M>` from the
/// arena, call [`Mmu::unmap`], discharge the flush token, return
/// the orphaned [`PhysFrame`] for caller-side handling (typically
/// PMM `free_frame` once `cap_revoke(MemoryRegionCap)` lands in
/// B4+; v1's T-018 tests just verify the return value matches the
/// originally-mapped PA).
///
/// The intermediate L1/L2/L3 frames that become orphaned when the
/// last L3 page in a subtree is unmapped are deferred to the per-AS
/// destroy path (B4+); T-018 wires the per-page unmap discipline
/// only.
///
/// # Errors
///
/// - [`CapError(_)`] — cap lookup / kind validation.
/// - [`StaleHandle`] — the cap's [`AddressSpaceHandle`] no longer
///   names a live arena slot.
/// - [`MmuUnmapError(MmuError)`] — pass-through from `Mmu::unmap`
///   (`NotMapped`, `MisalignedAddress`, ...).
///
/// [adr-0028]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0028-address-space-data-structure.md
/// [`CapError(_)`]: AddressSpaceError::CapError
/// [`StaleHandle`]: AddressSpaceError::StaleHandle
/// [`MmuUnmapError(MmuError)`]: AddressSpaceError::MmuUnmapError
pub fn cap_unmap<M: Mmu>(
    table: &CapabilityTable,
    cap_handle: CapHandle,
    mmu: &M,
    arena: &mut AddressSpaceArena<M>,
    va: VirtAddr,
) -> Result<PhysFrame, AddressSpaceError> {
    let handle = resolve_address_space_cap(table, cap_handle)?;
    let address_space =
        get_address_space_mut(arena, handle).ok_or(AddressSpaceError::StaleHandle)?;
    let (token, pa) = mmu
        .unmap(address_space.inner_mut(), va)
        .map_err(AddressSpaceError::MmuUnmapError)?;
    token.flush(mmu);
    Ok(pa)
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
        cap_create_address_space, cap_map, cap_unmap, create_address_space, destroy_address_space,
        get_address_space, get_address_space_mut, resolve_address_space_cap, AddressSpace,
        AddressSpaceArena, AddressSpaceError,
    };
    use crate::cap::{CapError, CapObject, CapRights, Capability, CapabilityTable};
    use tyrne_hal::{mmu::Mmu, MappingFlags, MmuError, PhysAddr, PhysFrame, VirtAddr};
    use tyrne_test_hal::{FakeMmu, VecFrameProvider};

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

    // ── Cap-gated wrapper tests (commit 3) ────────────────────────────────────

    /// Set up: a `CapabilityTable` holding the bootstrap AS cap.
    /// Returns `(table, bootstrap_cap_handle, bootstrap_as_handle, mmu, arena)`.
    /// Used by every cap-wrapper test to avoid repeating the boilerplate.
    fn bootstrap_setup() -> (
        CapabilityTable,
        crate::cap::CapHandle,
        super::AddressSpaceHandle,
        FakeMmu,
        AddressSpaceArena<FakeMmu>,
    ) {
        let mmu = FakeMmu::new();
        let mut arena: AddressSpaceArena<FakeMmu> = AddressSpaceArena::new();
        let mut table = CapabilityTable::new();

        // Bootstrap AS: wrap the already-active inner + alloc arena slot.
        let bootstrap_inner = fake_inner(&mmu, frame(0x4000_0000));
        let bootstrap_as = AddressSpace::wrap_bootstrap(bootstrap_inner);
        let as_handle = create_address_space(&mut arena, bootstrap_as).unwrap();

        // Mint the bootstrap AS cap. All rights (the kernel-init holds
        // full authority over the bootstrap AS).
        let bootstrap_cap = Capability::new(
            CapRights::DUPLICATE | CapRights::DERIVE | CapRights::REVOKE | CapRights::TRANSFER,
            CapObject::AddressSpace(as_handle),
        );
        let cap_handle = table.insert_root(bootstrap_cap).unwrap();

        (table, cap_handle, as_handle, mmu, arena)
    }

    #[test]
    fn resolve_address_space_cap_returns_handle_on_correct_kind() {
        let (table, cap_handle, as_handle, _mmu, _arena) = bootstrap_setup();
        let resolved = resolve_address_space_cap(&table, cap_handle).unwrap();
        assert_eq!(resolved, as_handle);
    }

    #[test]
    fn resolve_address_space_cap_returns_wrong_kind_on_endpoint_cap() {
        // Mint a non-AS cap (Endpoint) and try to resolve it as an AS cap.
        let mut table = CapabilityTable::new();
        let ep_handle = crate::obj::EndpointHandle::test_handle(0, 0);
        let ep_cap = Capability::new(CapRights::empty(), CapObject::Endpoint(ep_handle));
        let cap_handle = table.insert_root(ep_cap).unwrap();

        let result = resolve_address_space_cap(&table, cap_handle);
        assert!(matches!(
            result,
            Err(AddressSpaceError::CapError(CapError::WrongKind))
        ));
    }

    #[test]
    fn cap_create_address_space_consumes_one_pmm_frame_and_mints_cap() {
        let (mut table, parent_cap, _bootstrap_as, mmu, mut arena) = bootstrap_setup();
        // PMM has exactly two frames available; we expect cap_create to
        // consume one of them.
        let mut pmm = VecFrameProvider::new(vec![frame(0x5000_0000), frame(0x5000_1000)]);
        let pmm_before = pmm.remaining();

        let new_cap = cap_create_address_space(
            &mut table,
            parent_cap,
            CapRights::empty(),
            &mmu,
            &mut pmm,
            &mut arena,
        )
        .expect("cap_create with healthy PMM + arena + table must succeed");

        // Exactly one frame consumed.
        assert_eq!(pmm.remaining(), pmm_before - 1);

        // The new cap resolves to a fresh AS handle (distinct from
        // bootstrap).
        let new_as_handle = resolve_address_space_cap(&table, new_cap).unwrap();
        assert!(get_address_space(&arena, new_as_handle).is_some());
    }

    #[test]
    fn cap_create_address_space_returns_out_of_frames_on_pmm_exhaustion() {
        let (mut table, parent_cap, _, mmu, mut arena) = bootstrap_setup();
        let mut pmm = VecFrameProvider::new(vec![]); // PMM pre-drained

        let result = cap_create_address_space(
            &mut table,
            parent_cap,
            CapRights::empty(),
            &mmu,
            &mut pmm,
            &mut arena,
        );

        assert!(matches!(result, Err(AddressSpaceError::OutOfFrames)));
        // PMM still empty; arena and table unchanged (fail-fast before
        // any mutation).
        assert_eq!(pmm.remaining(), 0);
    }

    #[test]
    fn cap_create_address_space_rejects_wrong_parent_kind() {
        // Parent cap is an Endpoint cap, not an AS cap.
        let mmu = FakeMmu::new();
        let mut arena: AddressSpaceArena<FakeMmu> = AddressSpaceArena::new();
        let mut table = CapabilityTable::new();
        let ep_handle = crate::obj::EndpointHandle::test_handle(0, 0);
        let ep_cap = Capability::new(
            CapRights::DUPLICATE | CapRights::DERIVE,
            CapObject::Endpoint(ep_handle),
        );
        let ep_cap_handle = table.insert_root(ep_cap).unwrap();
        let mut pmm = VecFrameProvider::new(vec![frame(0x5000_0000)]);

        let result = cap_create_address_space(
            &mut table,
            ep_cap_handle,
            CapRights::empty(),
            &mmu,
            &mut pmm,
            &mut arena,
        );

        assert!(matches!(
            result,
            Err(AddressSpaceError::CapError(CapError::WrongKind))
        ));
        // PMM untouched (validation rejects before alloc).
        assert_eq!(pmm.remaining(), 1);
    }

    #[test]
    fn cap_create_address_space_rejects_widened_rights() {
        // Parent has empty rights; child cannot request DUPLICATE.
        let mmu = FakeMmu::new();
        let mut arena: AddressSpaceArena<FakeMmu> = AddressSpaceArena::new();
        let mut table = CapabilityTable::new();
        let bootstrap_inner = fake_inner(&mmu, frame(0x4000_0000));
        let bootstrap_as = AddressSpace::wrap_bootstrap(bootstrap_inner);
        let as_handle = create_address_space(&mut arena, bootstrap_as).unwrap();
        let narrow_cap = Capability::new(
            CapRights::empty(), // parent has no rights to grant
            CapObject::AddressSpace(as_handle),
        );
        let narrow_cap_handle = table.insert_root(narrow_cap).unwrap();
        let mut pmm = VecFrameProvider::new(vec![frame(0x5000_0000)]);

        let result = cap_create_address_space(
            &mut table,
            narrow_cap_handle,
            CapRights::DUPLICATE, // trying to widen
            &mmu,
            &mut pmm,
            &mut arena,
        );

        assert!(matches!(
            result,
            Err(AddressSpaceError::CapError(CapError::WidenedRights))
        ));
        assert_eq!(pmm.remaining(), 1);
    }

    #[test]
    fn cap_map_installs_mapping_and_flushes_tlb() {
        let (table, bootstrap_cap, _bootstrap_as, mmu, mut arena) = bootstrap_setup();
        let mut pmm = VecFrameProvider::new(vec![frame(0x6000_0000)]);
        let va = VirtAddr(0x0001_0000);
        let pa = frame(0x7000_0000);

        cap_map(
            &table,
            bootstrap_cap,
            &mmu,
            &mut pmm,
            &mut arena,
            va,
            pa,
            MappingFlags::WRITE,
        )
        .expect("cap_map on bootstrap AS with healthy inputs must succeed");

        // The FakeMmu records flush calls via `tlb_address_invalidations`.
        // The flush token's `flush(mmu)` invokes `invalidate_tlb_address(va)`.
        assert_eq!(mmu.tlb_address_invalidations(), vec![va]);
    }

    #[test]
    fn cap_map_wraps_mmu_error_passthrough() {
        let (table, bootstrap_cap, _, mmu, mut arena) = bootstrap_setup();
        let mut pmm = VecFrameProvider::new(vec![]);
        // FakeMmu::map returns MisalignedAddress for non-page-aligned VAs.
        let bad_va = VirtAddr(0x0001_0001);
        let pa = frame(0x7000_0000);

        let result = cap_map(
            &table,
            bootstrap_cap,
            &mmu,
            &mut pmm,
            &mut arena,
            bad_va,
            pa,
            MappingFlags::WRITE,
        );

        assert!(matches!(
            result,
            Err(AddressSpaceError::MmuMapError(MmuError::MisalignedAddress))
        ));
    }

    #[test]
    fn cap_map_rejects_wrong_kind() {
        // Use an Endpoint cap with cap_map; expect WrongKind.
        let mmu = FakeMmu::new();
        let mut arena: AddressSpaceArena<FakeMmu> = AddressSpaceArena::new();
        let mut table = CapabilityTable::new();
        let ep_handle = crate::obj::EndpointHandle::test_handle(0, 0);
        let ep_cap = Capability::new(CapRights::empty(), CapObject::Endpoint(ep_handle));
        let ep_cap_handle = table.insert_root(ep_cap).unwrap();
        let mut pmm = VecFrameProvider::new(vec![]);

        let result = cap_map(
            &table,
            ep_cap_handle,
            &mmu,
            &mut pmm,
            &mut arena,
            VirtAddr(0x0001_0000),
            frame(0x7000_0000),
            MappingFlags::WRITE,
        );

        assert!(matches!(
            result,
            Err(AddressSpaceError::CapError(CapError::WrongKind))
        ));
    }

    #[test]
    fn cap_unmap_returns_unmapped_frame() {
        let (table, bootstrap_cap, _, mmu, mut arena) = bootstrap_setup();
        let mut pmm = VecFrameProvider::new(vec![]);
        let va = VirtAddr(0x0001_0000);
        let pa = frame(0x7000_0000);

        // Install a mapping first.
        cap_map(
            &table,
            bootstrap_cap,
            &mmu,
            &mut pmm,
            &mut arena,
            va,
            pa,
            MappingFlags::WRITE,
        )
        .unwrap();

        // Now unmap it; the returned PhysFrame must match the original PA.
        let unmapped = cap_unmap(&table, bootstrap_cap, &mmu, &mut arena, va).unwrap();
        assert_eq!(unmapped, pa);

        // The flush token from unmap was discharged, so we should see
        // two flush calls now (one from map, one from unmap).
        assert_eq!(mmu.tlb_address_invalidations(), vec![va, va]);
    }

    #[test]
    fn cap_unmap_wraps_mmu_error_passthrough() {
        let (table, bootstrap_cap, _, mmu, mut arena) = bootstrap_setup();
        let va = VirtAddr(0x0001_0000);

        // Unmap something that was never mapped — FakeMmu returns NotMapped.
        let result = cap_unmap(&table, bootstrap_cap, &mmu, &mut arena, va);
        assert!(matches!(
            result,
            Err(AddressSpaceError::MmuUnmapError(MmuError::NotMapped))
        ));
    }
}
