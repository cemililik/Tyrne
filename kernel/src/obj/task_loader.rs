//! Task loader — embedded raw-flat userspace image → [`LoadedImage`]
//! metadata.
//!
//! Per [ADR-0029][adr-0029] (raw-flat format choice) + [T-019][t-019]
//! (loader implementation). This module owns the public types
//! [`LoadedImage`] (success descriptor) and [`LoadError`] (failure
//! taxonomy) and the [`load_image`] function that consumes an embedded
//! raw-flat userspace blob and produces a populated address space
//! described by a `LoadedImage`.
//!
//! ## Pipeline (§Simulation rows of T-019)
//!
//! 1. Argument preflight ([`LoadError::InvalidImage`] /
//!    [`LoadError::InvalidStackSize`] /
//!    [`LoadError::MisalignedImageBaseVa`]).
//! 2. Cap preflight: lookup + [`CapKind::AddressSpace`] kind check
//!    ([`LoadError::InvalidParentCap`]). The DERIVE-rights check is
//!    *delegated* to [`cap_create_address_space`]'s step 2a and
//!    surfaces in step 4 as
//!    [`LoadError::AddressSpaceCreationFailed`] wrapping
//!    [`CapError::InsufficientRights`].
//! 3. Frame-budget preflight: `1 + image_pages + stack_pages +
//!    intermediate_frame_count(...) <= pmm.stats().free_frames`
//!    (see [`intermediate_frame_count`]) — returns
//!    [`LoadError::FrameBudgetExceeded`] on miss.
//! 4. Image-PA-overlap preflight: reject if the `image` slice's PA
//!    range overlaps a frame [`Pmm::alloc_frame`] could yield
//!    ([`LoadError::ImageOverlapsAllocatableMemory`]). Discharges
//!    UNSAFE-2026-0027's non-overlap invariant at runtime instead of
//!    relying on BSP-layout discipline.
//! 5. Create the AS via [`cap_create_address_space`]; on failure no
//!    state was committed (T-018's preflight discipline)
//!    ([`LoadError::AddressSpaceCreationFailed`]).
//! 6. Image-page loop: `pmm.alloc_frame` → `copy_nonoverlapping` byte
//!    copy → [`cap_map`] under `USER | EXECUTE` per page. Tail-zeroing
//!    on the partial last page is automatic via the PMM's zero-init
//!    contract.
//! 7. Stack-page loop: `pmm.alloc_frame` → [`cap_map`] under
//!    `USER | WRITE` per page.
//! 8. Construct [`LoadedImage`] and return.
//!
//! Steps 6+7 are fallible mid-loop; the rollback contract
//! ([`cap_unmap`] + [`Pmm::free_frame`] for the committed pages, plus
//! [`Pmm::free_frame`] for the failing iteration's leaf frame, plus
//! [`CapabilityTable::cap_drop`][crate::cap::CapabilityTable::cap_drop]
//! for the AS cap) is documented canonically in T-019 §Approach
//! §"Rollback contract (explicit)".
//!
//! ## Scope boundary (load-complete, not B5/B6-runnable)
//!
//! [`LoadedImage`] is intentionally **not** a
//! `CapHandle{CapObject::Task(...)}` — it describes a populated
//! address space but does not mint a runnable task. The current
//! [`Task`][super::Task] struct carries no PC/SP context register
//! file; the loader's new AS holds only image + stack mappings (no
//! kernel mappings, so an EL1 exception while the AS is active would
//! translation-fault on the vector fetch). The `task_create_from_image`
//! wrapper that turns a [`LoadedImage`] into a runnable task cap lands
//! with B5 (syscall ABI — placeholder slot ADR-0030; not yet authored)
//! and B6 (first userspace "hello") per
//! [phase-b §B4 §Revision-notes][phase-b-b4-rider].
//!
//! [adr-0029]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0029-initial-userspace-image-format.md
//! [t-019]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-b/T-019-task-loader.md
//! [phase-b-b4-rider]: https://github.com/cemililik/Tyrne/blob/main/docs/roadmap/phases/phase-b.md#milestone-b4--task-loader

use crate::cap::{CapError, CapHandle, CapKind, CapRights, CapabilityTable};
use crate::mm::{
    cap_create_address_space, cap_map, cap_unmap, AddressSpaceArena, AddressSpaceError, Pmm,
};
use tyrne_hal::{MappingFlags, Mmu, VirtAddr, PAGE_SIZE};

/// Compute the exact number of intermediate page-table frames
/// (L1 + L2 + L3 tables) `cap_map` will allocate for the loader's
/// image + stack mappings under `VMSAv8` 4-level translation.
///
/// Walks the `VMSAv8` 4 KiB granule index decomposition:
///
/// - **L3** (4 KiB page) table covers 2 MiB (`VA[20:12]` index, 9 bits).
/// - **L2** (2 MiB block) table covers 1 GiB (`VA[29:21]` index).
/// - **L1** (1 GiB block) table covers 512 GiB (`VA[38:30]` index).
/// - **L0** root covers the full 48-bit VA range — already allocated
///   by [`crate::mm::cap_create_address_space`], **not** counted here.
///
/// Image and stack are contiguous by construction (`stack_base =
/// image_base_va + image_pages * PAGE_SIZE`), so their combined VA
/// span is closed; the distinct-slot count at each level is exactly
/// `(last >> shift) - (first >> shift) + 1`.
///
/// The returned count is **exact** for a BSP that lazy-allocates
/// table descriptors only when the parent entry is invalid (v1's
/// [`bsp-qemu-virt/src/mmu.rs`](../../../../bsp-qemu-virt/src/mmu.rs)
/// `walk_or_alloc_table` behaviour); a BSP that pre-allocates more
/// aggressively would observe this count as a lower bound. v1 has
/// the single aarch64 BSP, so the count is exact in practice.
///
/// Returns 0 for an empty span (defensive; row 1 preflight rejects
/// zero-page requests before this helper is reached).
///
/// ## Why not a closed-form constant
///
/// Earlier T-019 commits used a hard-coded constant `6` (3 L0/L1/L2
/// intermediates per range × 2 contiguous ranges = 6). PR #31
/// review-round 3 (Finding 1) observed this is **wrong** for image
/// spans that cross more than one L2 slot — every additional 2 MiB
/// boundary the image crosses adds one L3 table. Example: an 8 MiB
/// image at VA `0x0080_0000` covers L2 slots 4–7 + the stack's slot
/// 8 = 5 distinct L2 slots, requiring 1 L1 + 1 L2 + 5 L3 = **7**
/// intermediates, not 6. The exact computation below tracks the true
/// count and satisfies the "safe upper bound" contract T-019
/// §Acceptance criteria promises.
///
/// ## `VMSAv8`-specificity
///
/// The `21`/`30`/`39` bit-shift constants encode `VMSAv8`'s 4 KiB
/// granule + 9-bit per-level index decomposition per
/// [ADR-0009 §Decision drivers][adr-0009] and
/// [ADR-0027 §Decision outcome (a)][adr-0027]. Future BSPs targeting
/// a different page-table format (e.g. RISC-V `Sv39`) would either
/// re-derive these constants per their format or surface an
/// `Mmu::intermediate_frames_for_span` HAL method. v1's single
/// aarch64 BSP keeps the constants inline.
///
/// [adr-0009]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0009-mmu-trait.md
/// [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
#[must_use]
pub fn intermediate_frame_count(
    image_base_va: VirtAddr,
    image_pages: usize,
    stack_pages: usize,
) -> usize {
    let total_pages = image_pages.saturating_add(stack_pages);
    if total_pages == 0 {
        return 0;
    }
    let span_bytes = total_pages.saturating_mul(PAGE_SIZE);
    let span_start = image_base_va.0;
    // Last addressed byte. `total_pages > 0` guarantees `span_bytes
    // >= PAGE_SIZE > 0`, so the `saturating_sub` does not underflow.
    let last_byte = span_start.saturating_add(span_bytes).saturating_sub(1);

    // L3 (4 KiB page) table: each covers 2 MiB → shift 21.
    let l3_count = (last_byte >> 21)
        .saturating_sub(span_start >> 21)
        .saturating_add(1);
    // L2 (2 MiB block) table: each covers 1 GiB → shift 30.
    let l2_count = (last_byte >> 30)
        .saturating_sub(span_start >> 30)
        .saturating_add(1);
    // L1 (1 GiB block) table: each covers 512 GiB → shift 39.
    let l1_count = (last_byte >> 39)
        .saturating_sub(span_start >> 39)
        .saturating_add(1);

    l1_count.saturating_add(l2_count).saturating_add(l3_count)
}

/// One-past-the-highest legal userspace virtual address.
///
/// Per [ADR-0027 §Decision outcome (a)][adr-0027], `TCR_EL1.T0SZ = 16`
/// commits the kernel to 48-bit `TTBR0_EL1` virtual addresses; the
/// architected userspace VA range is therefore `[0, USERSPACE_VA_LIMIT)`
/// with `USERSPACE_VA_LIMIT = 1 << 48 = 0x1_0000_0000_0000`. The
/// loader's mapped span `[image_base_va, stack_top_va)` (half-open)
/// must lie within this range — a span that crosses or exceeds it
/// could not be addressed by the hardware page-walker even after the
/// future ADR-0033 high-half migration opens (which moves *kernel*
/// VAs into `TTBR1_EL1`'s range, leaving the userspace `TTBR0_EL1`
/// bound unchanged).
///
/// The 0 bound on the low side is not enforced separately: a
/// `image_base_va = 0` is legal (page 0 mapped under `USER|EXECUTE`
/// is unusual but not architecturally forbidden); the loader does
/// not impose a stylistic "no-null-page" policy in v1.
///
/// [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
pub const USERSPACE_VA_LIMIT: usize = 1usize << 48;

/// Metadata describing a freshly populated address space produced by
/// [`load_image`].
///
/// `LoadedImage` is a *descriptor of loaded state*, not a runnable-task
/// capability — see the module-level §"Scope boundary" note. Returned
/// by-value (it is [`Copy`]); the caller owns the metadata after
/// `load_image` returns.
///
/// Fields are all `pub` per the public-struct-literal convention chosen
/// for T-019: callers construct a `LoadedImage` directly when they need
/// one for tests, and the loader's success path writes a literal at the
/// end of its sequence. There is no hidden invariant a constructor
/// would protect — every field is independently derived from the
/// loader's arguments + the cap-table / arena state.
///
/// # Field invariants
///
/// - `as_cap` is a freshly-minted leaf `CapHandle` wrapping
///   `CapObject::AddressSpace(AddressSpaceHandle)`, valid against the
///   `CapabilityTable` the loader was passed.
/// - `entry_va == image_base_va` (raw-flat: offset 0 of the embedded
///   blob is the userspace entry instruction).
/// - `stack_top_va` is **one-past-the-highest** mapped VA of the stack
///   region. The stack mapped range is `[stack_base, stack_top_va)`
///   half-open; `sp = stack_top_va` at task-creation initialisation
///   is correct because the first userspace push (e.g. `sp -= 16`)
///   lands inside the mapped range. Matches the AAPCS64 convention.
/// - `image_bytes == image.len()` from the loader's `image: &[u8]`
///   argument. May be smaller than `image_pages * PAGE_SIZE` because
///   tail-zeroing happens on the partial last page (the loader copies
///   only `image.len()` bytes; the remainder of the last page stays
///   zero per [UNSAFE-2026-0026][unsafe-26]'s PMM zero-init contract).
/// - `stack_bytes == stack_size_pages * PAGE_SIZE` (always a multiple
///   of `PAGE_SIZE`).
///
/// [unsafe-26]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct LoadedImage {
    /// Cap handle for the newly-minted address space. Backed by a
    /// `CapObject::AddressSpace(AddressSpaceHandle)` cap-table entry
    /// minted via
    /// [`cap_create_address_space`][crate::mm::cap_create_address_space]
    /// during `load_image` step 4.
    pub as_cap: CapHandle,

    /// Userspace entry-point VA — equals `image_base_va` (raw-flat
    /// format: offset 0 of the embedded blob is the entry instruction).
    pub entry_va: VirtAddr,

    /// One-past-the-highest mapped VA of the stack region (half-open
    /// `[stack_base, stack_top_va)` convention; see struct-level
    /// invariants).
    pub stack_top_va: VirtAddr,

    /// Byte-count of the image as loaded into the AS (may be smaller
    /// than `image_pages * PAGE_SIZE` due to tail-zeroing on the
    /// partial last page).
    pub image_bytes: usize,

    /// Stack region size in bytes (always a multiple of `PAGE_SIZE`).
    pub stack_bytes: usize,
}

/// Error taxonomy for [`load_image`].
///
/// Variants are split per the explicit rollback contract documented in
/// [T-019 §Approach §"Rollback contract (explicit)"][t-019-rollback].
/// Each variant's doc-comment names whether rollback is required and
/// what the v1 baseline leaks on the rollback path. The
/// [T-019 §"Rollback contract"][t-019-rollback] section remains the
/// canonical reference; this enum's doc-comments are summaries.
///
/// `#[non_exhaustive]` because future-state variants are foreseeable
/// — e.g. a per-section permission failure that lands with ADR-0034
/// (placeholder; B5+).
///
/// [t-019-rollback]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-b/T-019-task-loader.md#rollback-contract-explicit
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LoadError {
    /// `image.is_empty()`. Pre-PMM preflight; no state change.
    InvalidImage,

    /// `stack_size_pages == 0`. Pre-PMM preflight; no state change.
    InvalidStackSize,

    /// `image_base_va` is not `PAGE_SIZE`-aligned. Pre-PMM preflight
    /// rejects the request before any state change. Without this
    /// check, the misalignment would surface only at the first
    /// `cap_map` call inside the image-page loop — by which point
    /// `cap_create_address_space` has already allocated the root L0
    /// frame, which then leaks via the v1 baseline rollback. Per
    /// PR #31 review-round 4 P2: the leak is preventable by argument
    /// validation, so this variant takes precedence over the
    /// `MapFailed(MmuMapError(MisalignedAddress))` path the loader
    /// would otherwise return. The wrapped `VirtAddr` names the
    /// offending base for diagnostics.
    MisalignedImageBaseVa(VirtAddr),

    /// The requested VA span `[image_base_va, stack_top_va)` exceeds
    /// the architected userspace VA range
    /// `[0, USERSPACE_VA_LIMIT)`. Per
    /// [ADR-0027 §Decision outcome (a)][adr-0027], `TCR_EL1.T0SZ = 16`
    /// commits userspace VAs to 48 bits; an `image_base_va` plus
    /// `image_pages + stack_pages` worth of bytes that lands past
    /// `1 << 48` could not be addressed by the hardware page-walker.
    /// The variant fires either when `image_base_va` itself is past
    /// the limit, when the saturated-add of the span end exceeds it,
    /// or when the byte-count multiplication overflows `usize`
    /// (saturated to `usize::MAX`). Pre-PMM preflight; no state
    /// change.
    ///
    /// `end == VirtAddr(usize::MAX)` is the sentinel for the overflow
    /// path; non-sentinel `end` values name the offending
    /// saturated-add result for diagnostics.
    ///
    /// [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
    InvalidImageBaseVa {
        /// The caller-supplied `image_base_va`.
        base: VirtAddr,
        /// The computed one-past-the-highest VA the loader would map.
        /// Sentinel `VirtAddr(usize::MAX)` indicates the byte-count or
        /// VA-end arithmetic saturated.
        end: VirtAddr,
    },

    /// Parent AS cap lookup or `CapKind::AddressSpace` kind check
    /// failed. Wraps the underlying [`CapError`]. **DERIVE-rights
    /// enforcement is *not* in this variant** — the DERIVE check is
    /// delegated to `cap_create_address_space`'s step 2a and surfaces
    /// as [`AddressSpaceCreationFailed`][LoadError::AddressSpaceCreationFailed]
    /// wrapping `CapError::InsufficientRights`.
    InvalidParentCap(CapError),

    /// Frame-budget preflight: `1 + image_pages + stack_pages +
    /// intermediate_frame_count(...)` exceeds `pmm.stats().free_frames`.
    /// The leading `1` accounts for the root L0 frame
    /// `cap_create_address_space` will allocate;
    /// [`intermediate_frame_count`] returns the **exact** distinct
    /// L1/L2/L3 page-table-frame count for the requested VA span
    /// (per `VMSAv8` 4-level index decomposition — *not* a hard-coded
    /// constant, since the previous `INTERMEDIATE_FRAME_BUDGET = 6`
    /// undercounted for image spans crossing more than one 2 MiB L2
    /// slot per PR #31 review-round 3 Finding 1). Pre-PMM preflight;
    /// no state change.
    FrameBudgetExceeded {
        /// Frames the loader would commit (root + image + stack +
        /// exact intermediate count).
        needed: usize,
        /// Frames currently available per `pmm.stats().free_frames`.
        available: usize,
    },

    /// `cap_create_address_space` returned `Err`. Covers
    /// `CapError::InsufficientRights` if `parent_as_cap` lacks DERIVE,
    /// plus the T-018-guarded `CapsExhausted` / `DerivationTooDeep` /
    /// `ArenaFull` paths. No rollback needed at this layer (T-018's
    /// preflight ensures no committed state on failure).
    AddressSpaceCreationFailed(AddressSpaceError),

    /// The `image` byte slice's PA range overlaps a frame
    /// [`Pmm::alloc_frame`] could yield. If accepted, the loader's
    /// `core::ptr::copy_nonoverlapping` would alias source and
    /// destination — undefined behaviour per the Rust safety contract.
    /// The check is a runtime preflight on
    /// [UNSAFE-2026-0027][unsafe-27]'s "source and destination do not
    /// overlap" invariant, replacing the BSP-layout-documented form
    /// (ADR-0027 + ADR-0035) with a mechanically enforced rejection.
    /// Pre-PMM preflight; no state change.
    ///
    /// Practically unreachable under correct BSP wiring (`.rodata`-
    /// resident images are in PMM-reserved memory by ADR-0035), but
    /// retained as a defensive variant so a misconfigured BSP fails
    /// fast with a typed error instead of UB.
    ///
    /// [unsafe-27]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md
    ImageOverlapsAllocatableMemory,

    /// `pmm.alloc_frame()` returned `None` mid-image-or-stack-loop.
    /// Structurally unreachable post-[`FrameBudgetExceeded`][LoadError::FrameBudgetExceeded]
    /// preflight under v1's single-thread cooperative model; retained
    /// as a defensive variant for budget-calculation bugs and future-
    /// concurrency scenarios. **Rollback required**, per the
    /// [T-019 §"Rollback contract"][t-019-rollback] (leaf frames +
    /// `cap_unmap` undo + `cap_drop(loaded_as_cap)`).
    ///
    /// [t-019-rollback]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-b/T-019-task-loader.md#rollback-contract-explicit
    OutOfFrames,

    /// `cap_map` returned `Err` mid-loop. Wraps the underlying
    /// [`AddressSpaceError`] — typically
    /// `MmuMapError(MmuError::OutOfFrames)` if the intermediate-frame
    /// budget was underestimated, `MmuMapError(MmuError::AlreadyMapped)`
    /// on a VA-range collision, or `MmuMapError(MmuError::BlockMapped)`
    /// if the VA falls inside a block descriptor. **Rollback required**,
    /// same shape as [`OutOfFrames`][LoadError::OutOfFrames].
    MapFailed(AddressSpaceError),
}

/// Load an embedded raw-flat userspace image into a fresh address
/// space and return a [`LoadedImage`] descriptor.
///
/// See the module-level pipeline summary + [T-019][t-019] for the full
/// state-machine specification. The sequence of fallible steps is:
///
/// 1. Argument preflight ([`LoadError::InvalidImage`] /
///    [`LoadError::InvalidStackSize`] /
///    [`LoadError::MisalignedImageBaseVa`]).
/// 2. Cap preflight: lookup + [`CapKind::AddressSpace`] kind check
///    ([`LoadError::InvalidParentCap`]).
/// 3. VA-range preflight: the mapped span end
///    `image_base_va + (image_pages + stack_pages) * PAGE_SIZE` must
///    be `<= USERSPACE_VA_LIMIT` (= `1 << 48` per ADR-0027 §Decision
///    outcome (a)). Returns [`LoadError::InvalidImageBaseVa`] on
///    miss. Frame-budget preflight: `1 + image_pages + stack_pages +
///    intermediate_frame_count(...) <= pmm.stats().free_frames` (see
///    [`intermediate_frame_count`]). Returns
///    [`LoadError::FrameBudgetExceeded`] on miss. VA-range runs
///    first so an arithmetic-saturated request surfaces the
///    documented [`LoadError::InvalidImageBaseVa`] sentinel instead
///    of being masked by [`LoadError::FrameBudgetExceeded`].
/// 4. Image-PA-overlap preflight: reject if the `image` slice's PA
///    range overlaps a frame [`Pmm::alloc_frame`] could yield
///    ([`LoadError::ImageOverlapsAllocatableMemory`]). Discharges
///    [UNSAFE-2026-0027][unsafe-27]'s non-overlap invariant at runtime.
/// 5. [`cap_create_address_space`]: mint the AS cap
///    ([`LoadError::AddressSpaceCreationFailed`] — no rollback,
///    T-018's preflight guarantees no committed state on failure).
/// 6. Image-page loop under `USER | EXECUTE`
///    ([`LoadError::OutOfFrames`] / [`LoadError::MapFailed`] — rollback
///    discipline below).
/// 7. Stack-page loop under `USER | WRITE` (same).
/// 8. Construct and return [`LoadedImage`].
///
/// [unsafe-27]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md
///
/// # Arguments
///
/// - `image`: the embedded raw-flat blob. Offset 0 is the userspace
///   entry instruction (per [ADR-0029][adr-0029]).
/// - `pmm`: the kernel PMM. Direct concrete type — *not*
///   `&mut dyn FrameProvider` — because the rollback path needs
///   [`Pmm::free_frame`] which is not on the trait surface.
/// - `mmu`: the BSP MMU instance.
/// - `table`: the cap table the new AS cap will be minted into.
/// - `as_arena`: the address-space arena slot pool.
/// - `parent_as_cap`: cap authorising the mint via
///   [`cap_create_address_space`]. Must be `CapKind::AddressSpace`;
///   must hold `CapRights::DERIVE` (delegated check).
/// - `new_rights`: rights set the new AS cap will carry.
/// - `image_base_va`: VA at which the image's offset 0 lands. The
///   caller's userspace linker script dictates this. Must be
///   `PAGE_SIZE`-aligned — verified in row 1's argument preflight;
///   misaligned bases return [`LoadError::MisalignedImageBaseVa`]
///   before any state change.
/// - `stack_size_pages`: stack-region size in `PAGE_SIZE`-multiples;
///   minimum 1.
///
/// # Errors
///
/// Every variant of [`LoadError`]. See per-variant doc-comments for
/// when each fires and whether rollback runs.
///
/// # Rollback discipline
///
/// On any `Err` from step 6 or 7 the function unwinds *every*
/// committed mapping (via [`cap_unmap`] + [`Pmm::free_frame`] in
/// reverse order), frees the failing iteration's already-allocated
/// leaf frame, and drops the AS cap via
/// [`CapabilityTable::cap_drop`][crate::cap::CapabilityTable::cap_drop].
/// The v1 baseline leaks the root L0 frame + the intermediate
/// L1/L2/L3 frames `cap_map` allocated + the AS arena slot itself —
/// per [T-019 §"Rollback contract"][t-019-rollback]; full reclaim
/// arrives with the future `MemoryRegionCap` + per-AS destroy ADR
/// (B5+).
///
/// `cap_drop` (not `cap_revoke`) is used because the AS cap is a leaf
/// in the derivation tree by construction (the loader does not
/// derive children from it) and `cap_revoke(src)` would walk
/// `src`'s *descendants* while leaving `src` itself valid; it also
/// requires `CapRights::REVOKE` which `new_rights` may omit.
/// `cap_drop` `free_slot`s the leaf directly, is rights-agnostic, and
/// fails only with `HasChildren` (impossible here).
///
/// [adr-0029]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0029-initial-userspace-image-format.md
/// [t-019]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-b/T-019-task-loader.md
/// [t-019-rollback]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-b/T-019-task-loader.md#rollback-contract-explicit
#[allow(
    clippy::too_many_arguments,
    reason = "load_image threads the full kernel-state surface (pmm + mmu + \
              table + arena + parent cap + rights + VA + stack size) through \
              by reference per the no-ambient-authority discipline; bundling \
              would obscure the data-flow without reducing argument count at \
              the call site (same pattern as cap_create_address_space)"
)]
#[allow(
    clippy::too_many_lines,
    reason = "the function is a linear state-machine matching T-019 §Simulation \
              rows 1–7 one-for-one; splitting into helpers (e.g. one per row) \
              would obscure the row-to-code mapping that reviewers verify \
              against the §Simulation table"
)]
pub fn load_image<M: Mmu, const N: usize, const R: usize>(
    image: &[u8],
    pmm: &mut Pmm<N, R>,
    mmu: &M,
    table: &mut CapabilityTable,
    as_arena: &mut AddressSpaceArena<M>,
    parent_as_cap: CapHandle,
    new_rights: CapRights,
    image_base_va: VirtAddr,
    stack_size_pages: usize,
) -> Result<LoadedImage, LoadError> {
    // §Simulation row 1: argument preflight. No state change.
    if image.is_empty() {
        return Err(LoadError::InvalidImage);
    }
    if stack_size_pages == 0 {
        return Err(LoadError::InvalidStackSize);
    }
    // PR #31 review-round 4 P2: an unaligned `image_base_va` would
    // surface as `MmuError::MisalignedAddress` from the first cap_map
    // call inside the image-page loop, but only *after*
    // `cap_create_address_space` has allocated the root L0 frame
    // (which then leaks via the v1 baseline rollback). Catch it at
    // argument-preflight time to keep PMM byte-stable on rejection.
    if !image_base_va.0.is_multiple_of(PAGE_SIZE) {
        return Err(LoadError::MisalignedImageBaseVa(image_base_va));
    }

    // §Simulation row 2: cap preflight — lookup + kind check. The
    // DERIVE-rights check is delegated to cap_create_address_space
    // step 2a; surfaces as AddressSpaceCreationFailed below.
    let parent_cap = table
        .lookup(parent_as_cap)
        .map_err(LoadError::InvalidParentCap)?;
    if parent_cap.kind() != CapKind::AddressSpace {
        return Err(LoadError::InvalidParentCap(CapError::WrongKind));
    }

    // §Simulation row 3: VA-range preflight (first) + frame-budget
    // preflight (second). VA-range runs first so an arithmetic-
    // saturated request (e.g. `stack_size_pages = usize::MAX`)
    // surfaces the documented `InvalidImageBaseVa` sentinel instead
    // of being masked by the saturated-then-overshoot frame-budget
    // comparison — addresses PR #31 review-round 3 Finding 2.
    let image_pages = image.len().div_ceil(PAGE_SIZE);
    let stack_pages = stack_size_pages;

    // VA-range bound: the mapped span `[image_base_va, stack_top_va)`
    // (half-open) must lie within `[0, USERSPACE_VA_LIMIT)` per
    // ADR-0027 §Decision outcome (a). saturating_* primitives cap
    // overflow at usize::MAX which is far past USERSPACE_VA_LIMIT
    // (`1 << 48`), so the single `> USERSPACE_VA_LIMIT` check below
    // catches both "base too high" and "span overflowed". The end
    // value `usize::MAX` is the documented sentinel for the saturated
    // path; non-sentinel ends name the offending VA for diagnostics.
    let total_va_bytes = image_pages
        .saturating_add(stack_pages)
        .saturating_mul(PAGE_SIZE);
    let span_end = image_base_va.0.saturating_add(total_va_bytes);
    if span_end > USERSPACE_VA_LIMIT {
        return Err(LoadError::InvalidImageBaseVa {
            base: image_base_va,
            end: VirtAddr(span_end),
        });
    }

    // Frame-budget bound. The intermediate count is *exact* (not a
    // safe upper bound) per `intermediate_frame_count`'s docstring;
    // PR #31 review-round 3 Finding 1 replaced the previous
    // hard-coded `6` (which under-counted when the image span crossed
    // more than one 2 MiB L2 slot — every additional boundary adds
    // one L3 table).
    let intermediate_budget = intermediate_frame_count(image_base_va, image_pages, stack_pages);
    let needed = 1usize
        .saturating_add(image_pages)
        .saturating_add(stack_pages)
        .saturating_add(intermediate_budget);
    let available = pmm.stats().free_frames;
    if needed > available {
        return Err(LoadError::FrameBudgetExceeded { needed, available });
    }

    // §Simulation row 4: image-PA-overlap preflight. Discharges
    // UNSAFE-2026-0027 invariant "source and destination do not
    // overlap" at runtime — `image.as_ptr() as usize` is treated as a
    // PA under v1's identity-mapped post-bootstrap kernel AS (ADR-0027
    // §Decision outcome (a)). If any byte of the image's PA range
    // could be returned by `pmm.alloc_frame()`, `copy_nonoverlapping`
    // in the image-page loop below would alias source and destination
    // — undefined behaviour per Rust's `core::ptr::copy_nonoverlapping`
    // safety contract. The check is practically unreachable under
    // correct BSP wiring (`.rodata`-resident images live in PMM-
    // reserved memory by ADR-0035) but defensive against BSP
    // misconfiguration. Pre-state-change; no rollback needed.
    let image_pa_start = image.as_ptr() as usize;
    let image_pa_end = image_pa_start.saturating_add(image.len());
    if pmm.could_yield_pa_overlapping(image_pa_start..image_pa_end) {
        return Err(LoadError::ImageOverlapsAllocatableMemory);
    }

    // §Simulation row 5: mint the new AS cap. T-018's preflight
    // discipline guarantees no PMM / arena / cap-table state was
    // committed on failure → no rollback at this layer.
    let loaded_as_cap =
        cap_create_address_space(table, parent_as_cap, new_rights, mmu, pmm, as_arena)
            .map_err(LoadError::AddressSpaceCreationFailed)?;

    // Stack base = first VA above the image region.
    let stack_base_va = VirtAddr(
        image_base_va
            .0
            .saturating_add(image_pages.saturating_mul(PAGE_SIZE)),
    );

    // §Simulation row 6: image-page loop.
    let mut image_pages_mapped: usize = 0;
    for (i, chunk) in image.chunks(PAGE_SIZE).enumerate() {
        // Defensive: alloc_frame returning None here is structurally
        // unreachable post-budget preflight in v1's single-thread
        // cooperative model; the OutOfFrames variant is retained
        // (with rollback) for forward-concurrency scenarios.
        let Some(frame) = pmm.alloc_frame() else {
            rollback(
                table,
                pmm,
                mmu,
                as_arena,
                loaded_as_cap,
                image_base_va,
                stack_base_va,
                image_pages_mapped,
                0,
            );
            return Err(LoadError::OutOfFrames);
        };

        // Byte-copy from .rodata-resident image bytes into the freshly
        // PMM-allocated frame. Tail-zeroing on the partial last page
        // happens automatically: the chunk is at most PAGE_SIZE bytes;
        // bytes (chunk.len()..PAGE_SIZE) stay zero from the PMM's
        // zero-init contract (UNSAFE-2026-0026). Audit:
        // UNSAFE-2026-0027.
        //
        // SAFETY:
        // **Why unsafe is needed.** `core::ptr::copy_nonoverlapping`
        // requires raw pointers; the destination is a PA obtained from
        // `pmm.alloc_frame()` (a `PhysFrame` whose payload is not a
        // Rust-owned slice), so the materialisation step into a
        // writable pointer is itself the operation we're auditing.
        //
        // **Invariants upheld.** (1) `chunk.as_ptr()` is a valid pointer
        // to at least `chunk.len()` initialised bytes inside `image`'s
        // backing storage (slice invariant). (2) The destination
        // pointer is materialised via
        // [`crate::mm::phys_frame_kernel_ptr`], which centralises v1's
        // identity-mapping invariant per
        // [ADR-0027 §Decision outcome (a)][adr-0027] so the future
        // high-half migration ([ADR-0033 placeholder][adr-0027])
        // updates one helper instead of every call site. The returned
        // pointer is page-aligned (the `PhysFrame` type enforces
        // alignment via `from_aligned`) and points at 4 KiB of PMM-
        // owned, zero-initialised RAM exclusively owned by this stack
        // frame until `cap_map` moves it into the AS (per the PMM's
        // single-thread cooperative ownership discipline +
        // [UNSAFE-2026-0026]'s zero-fill contract). (3) `chunk.len()`
        // is at most `PAGE_SIZE`, so the write is in-bounds for the
        // destination frame. (4) Source and destination are
        // non-overlapping — runtime-enforced by the row-4 preflight
        // (`Pmm::could_yield_pa_overlapping(image_pa_range)`); the
        // BSP-layout discipline (`.rodata` ⊆ PMM-reserved range per
        // ADR-0027 + ADR-0035) remains the production reality.
        //
        // **Why safer alternatives were rejected.** Per
        // [UNSAFE-2026-0027][audit]: `write_volatile` would falsely
        // imply MMIO ordering for a plain RAM-to-RAM copy; a
        // `slice::from_raw_parts_mut(...).copy_from_slice(...)` form
        // would push the same unsafety into the slice construction
        // step; `Mmu::copy_into_frame`-style HAL relocation just moves
        // the audit point without removing it.
        //
        // [UNSAFE-2026-0026]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md
        // [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
        // [audit]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md
        unsafe {
            let src = chunk.as_ptr();
            let dst = crate::mm::phys_frame_kernel_ptr(frame);
            core::ptr::copy_nonoverlapping(src, dst, chunk.len());
        }

        // cap_map: install the mapping under USER|EXECUTE.
        let va = VirtAddr(image_base_va.0.saturating_add(i.saturating_mul(PAGE_SIZE)));
        if let Err(e) = cap_map(
            table,
            loaded_as_cap,
            mmu,
            pmm,
            as_arena,
            va,
            frame,
            MappingFlags::USER | MappingFlags::EXECUTE,
        ) {
            // Per [`tyrne_hal::Mmu::map`]'s failure-semantics contract
            // (clause 2: "`pa` is not consumed on Err"), the leaf
            // frame `frame` is still owned by this stack frame after
            // the failed `cap_map`. Free it directly so it doesn't
            // leak. Previously-mapped pages roll back via the helper
            // below. **If a future BSP `Mmu::map` impl violates clause
            // 2**, this `pmm.free_frame` would alias a still-mapped
            // frame — the trait contract is the load-bearing safety
            // argument here.
            let _ = pmm.free_frame(frame);
            rollback(
                table,
                pmm,
                mmu,
                as_arena,
                loaded_as_cap,
                image_base_va,
                stack_base_va,
                image_pages_mapped,
                0,
            );
            return Err(LoadError::MapFailed(e));
        }
        image_pages_mapped = image_pages_mapped.saturating_add(1);
    }

    // §Simulation row 7: stack-page loop. Same shape, USER|WRITE.
    let mut stack_pages_mapped: usize = 0;
    for i in 0..stack_pages {
        let Some(frame) = pmm.alloc_frame() else {
            rollback(
                table,
                pmm,
                mmu,
                as_arena,
                loaded_as_cap,
                image_base_va,
                stack_base_va,
                image_pages_mapped,
                stack_pages_mapped,
            );
            return Err(LoadError::OutOfFrames);
        };

        let va = VirtAddr(stack_base_va.0.saturating_add(i.saturating_mul(PAGE_SIZE)));
        if let Err(e) = cap_map(
            table,
            loaded_as_cap,
            mmu,
            pmm,
            as_arena,
            va,
            frame,
            MappingFlags::USER | MappingFlags::WRITE,
        ) {
            // See image-loop equivalent above: `pa` is not consumed
            // on Err per `tyrne_hal::Mmu::map`'s failure-semantics
            // contract (clause 2). Free the leaf frame; previous
            // mappings unwind via `rollback` below.
            let _ = pmm.free_frame(frame);
            rollback(
                table,
                pmm,
                mmu,
                as_arena,
                loaded_as_cap,
                image_base_va,
                stack_base_va,
                image_pages_mapped,
                stack_pages_mapped,
            );
            return Err(LoadError::MapFailed(e));
        }
        stack_pages_mapped = stack_pages_mapped.saturating_add(1);
    }

    // §Simulation row 8: construct + return. `stack_top_va` is
    // one-past-the-highest mapped address (half-open `[stack_base,
    // stack_top_va)` convention; matches AAPCS64 sp init).
    let stack_top_va = VirtAddr(
        stack_base_va
            .0
            .saturating_add(stack_pages.saturating_mul(PAGE_SIZE)),
    );
    Ok(LoadedImage {
        as_cap: loaded_as_cap,
        entry_va: image_base_va,
        stack_top_va,
        image_bytes: image.len(),
        stack_bytes: stack_pages.saturating_mul(PAGE_SIZE),
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "rollback mirrors load_image's argument surface (every kernel-state \
              handle the forward-direction touches must be reachable for \
              undo); bundling would obscure the symmetry"
)]
/// Roll back a partial `load_image` commit.
///
/// Reverses the committed mappings (stack pages first, then image
/// pages, reverse-order within each range) via [`cap_unmap`] + the
/// returned frame's [`Pmm::free_frame`], then drops the AS cap via
/// [`CapabilityTable::cap_drop`][crate::cap::CapabilityTable::cap_drop].
///
/// Errors during rollback are intentionally swallowed (the rollback
/// path runs only after a primary failure; surfacing a secondary
/// rollback error would mask the first one and provide no actionable
/// information). The leaks documented in T-019 §"Rollback contract"
/// (root L0, intermediate L1/L2/L3, AS arena slot) are unavoidable in
/// v1; full reclaim arrives with the future `MemoryRegionCap` +
/// per-AS destroy ADR.
fn rollback<M: Mmu, const N: usize, const R: usize>(
    table: &mut CapabilityTable,
    pmm: &mut Pmm<N, R>,
    mmu: &M,
    as_arena: &mut AddressSpaceArena<M>,
    loaded_as_cap: CapHandle,
    image_base_va: VirtAddr,
    stack_base_va: VirtAddr,
    image_pages_mapped: usize,
    stack_pages_mapped: usize,
) {
    // Unmap stack pages first (reverse install order).
    for i in (0..stack_pages_mapped).rev() {
        let va = VirtAddr(stack_base_va.0.saturating_add(i.saturating_mul(PAGE_SIZE)));
        if let Ok(frame) = cap_unmap(table, loaded_as_cap, mmu, as_arena, va) {
            let _ = pmm.free_frame(frame);
        }
    }
    // Then image pages (reverse install order).
    for i in (0..image_pages_mapped).rev() {
        let va = VirtAddr(image_base_va.0.saturating_add(i.saturating_mul(PAGE_SIZE)));
        if let Ok(frame) = cap_unmap(table, loaded_as_cap, mmu, as_arena, va) {
            let _ = pmm.free_frame(frame);
        }
    }
    // Cap-side cleanup. cap_drop (not cap_revoke) because the AS cap
    // is a leaf by construction (no descendants derived from it).
    let _ = table.cap_drop(loaded_as_cap);
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "tests may use pragmas forbidden in production kernel code"
)]
mod tests {
    use super::{intermediate_frame_count, load_image, rollback, LoadError, LoadedImage};
    use crate::cap::{CapError, CapObject, CapRights, Capability, CapabilityTable};
    use crate::mm::{AddressSpaceArena, AddressSpaceError, PhysFrameRange, Pmm};
    use crate::obj::EndpointHandle;
    use std::sync::Mutex;
    use std::vec;
    use std::vec::Vec;
    use tyrne_hal::{
        FrameProvider, MapperFlush, MappingFlags, Mmu, MmuError, PhysAddr, PhysFrame, VirtAddr,
        PAGE_SIZE,
    };
    use tyrne_test_hal::{FakeAddressSpace, FakeMmu};

    // ── Pmm-over-backing helper (mirrors kernel/src/mm/pmm.rs::tests) ─────────
    //
    // Allocates a host-side `Vec<u8>` aligned to PAGE_SIZE, constructs a Pmm
    // whose extent points at the Vec's payload, and returns both — the Vec
    // kept alive by the caller, the Pmm wrapped over its physical-storage
    // illusion. Real byte-copy writes via `copy_nonoverlapping` land in the
    // backing Vec; no UB even though the Pmm believes it's writing to a
    // physical-RAM extent.

    /// Bitmap byte count covering 256 frames (1 MiB of host backing).
    /// Sized large enough for every test scenario in this module.
    const TEST_PMM_N: usize = 32;
    /// Reserved-range cache capacity. Tests never reserve any range; 4
    /// matches the BSP's per-BSP scheme and gives forward-compat room.
    const TEST_PMM_R: usize = 4;

    /// Type alias for the concrete Pmm used in this module's tests.
    type TestPmm = Pmm<TEST_PMM_N, TEST_PMM_R>;

    fn aligned_backing(frames: usize) -> (Vec<u8>, *mut u8) {
        let bytes = frames.checked_mul(PAGE_SIZE).expect("test math");
        let alloc = bytes.checked_add(PAGE_SIZE).expect("test math");
        let mut v: Vec<u8> = vec![0u8; alloc];
        let raw = v.as_mut_ptr();
        let aligned = ((raw as usize + (PAGE_SIZE - 1)) & !(PAGE_SIZE - 1)) as *mut u8;
        (v, aligned)
    }

    fn pmm_over_backing(backing_ptr: *mut u8, frames: usize) -> TestPmm {
        let base = backing_ptr as usize;
        let extent = PhysFrameRange::new(PhysAddr(base), PhysAddr(base + frames * PAGE_SIZE));
        Pmm::new(extent, &[]).expect("test Pmm::new must succeed")
    }

    fn frame(addr: usize) -> PhysFrame {
        PhysFrame::from_aligned(PhysAddr(addr)).expect("test addr must be page-aligned")
    }

    /// Set up: a `CapabilityTable` holding an AS authority cap +
    /// arena pre-populated with a bootstrap AS. Returns
    /// `(table, parent_cap, mmu, arena, pmm, _backing)`. The
    /// `_backing` Vec must outlive the returned Pmm.
    fn fixture(
        frames: usize,
    ) -> (
        CapabilityTable,
        crate::cap::CapHandle,
        FakeMmu,
        AddressSpaceArena<FakeMmu>,
        TestPmm,
        Vec<u8>,
    ) {
        let mmu = FakeMmu::new();
        let mut arena: AddressSpaceArena<FakeMmu> = AddressSpaceArena::new();
        let mut table = CapabilityTable::new();

        // Bootstrap AS slot wraps a fake inner; not exercised at runtime
        // — only used to mint the AS-kind cap the loader's preflight
        // accepts and that cap_create_address_space derives from.
        // SAFETY: FakeMmu::create_address_space is pure host code with
        // no UB; the input frame is page-aligned by construction.
        let bootstrap_inner = unsafe { mmu.create_address_space(frame(0x4000_0000)) };
        let bootstrap_as = crate::mm::AddressSpace::wrap_bootstrap(bootstrap_inner);
        let bootstrap_handle = crate::mm::create_address_space(&mut arena, bootstrap_as).unwrap();

        let parent_cap = Capability::new(
            CapRights::DUPLICATE | CapRights::DERIVE | CapRights::REVOKE | CapRights::TRANSFER,
            CapObject::AddressSpace(bootstrap_handle),
        );
        let parent_cap_handle = table.insert_root(parent_cap).unwrap();

        let (backing, ptr) = aligned_backing(frames);
        let pmm = pmm_over_backing(ptr, frames);

        (table, parent_cap_handle, mmu, arena, pmm, backing)
    }

    // ── §Simulation row 1 — argument preflight ────────────────────────────────

    #[test]
    fn rejects_empty_image() {
        // Pin §Simulation row 1: an empty image fails before any
        // state change. PMM, table, and arena must be byte-stable
        // after the rejection.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(16);
        let pmm_before = pmm.stats().free_frames;
        let table_before = table.is_full();

        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[], // empty image
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            VirtAddr(0x0080_0000),
            4,
        );

        assert_eq!(result, Err(LoadError::InvalidImage));
        assert_eq!(pmm.stats().free_frames, pmm_before);
        assert_eq!(table.is_full(), table_before);
    }

    #[test]
    fn rejects_zero_stack() {
        // Pin §Simulation row 1: stack_size_pages == 0 fails before
        // any state change.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(16);
        let pmm_before = pmm.stats().free_frames;

        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0x01, 0x02, 0x03],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            VirtAddr(0x0080_0000),
            0, // zero stack
        );

        assert_eq!(result, Err(LoadError::InvalidStackSize));
        assert_eq!(pmm.stats().free_frames, pmm_before);
    }

    // ── §Simulation row 2 — cap preflight ──────────────────────────────────────

    #[test]
    fn rejects_invalid_parent_cap_lookup() {
        // Pin §Simulation row 2: a stale handle (slot was freed)
        // fails with InvalidParentCap(InvalidHandle) before any state
        // change. Cross-table handle confusion is *not* tested here
        // because CapHandle is a (index, generation) pair with no
        // per-table marker — handles from different tables may
        // accidentally collide; the contract is "stale within the
        // same table".
        let (mut table, _real_cap, mmu, mut arena, mut pmm, _b) = fixture(16);
        let pmm_before = pmm.stats().free_frames;

        // Mint a throwaway leaf cap in the SAME table and drop it.
        // After drop, the handle's generation no longer matches the
        // freed slot's generation → lookup returns InvalidHandle.
        let throwaway = Capability::new(
            CapRights::empty(),
            CapObject::Endpoint(EndpointHandle::test_handle(0, 0)),
        );
        let stale = table.insert_root(throwaway).unwrap();
        table.cap_drop(stale).unwrap();

        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAA, 0xBB],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            stale,
            CapRights::empty(),
            VirtAddr(0x0080_0000),
            2,
        );

        assert!(
            matches!(
                result,
                Err(LoadError::InvalidParentCap(CapError::InvalidHandle))
            ),
            "expected InvalidParentCap(InvalidHandle), got {result:?}"
        );
        assert_eq!(pmm.stats().free_frames, pmm_before);
    }

    #[test]
    fn rejects_invalid_parent_cap_wrong_kind() {
        // Pin §Simulation row 2: a non-AS cap fails with
        // InvalidParentCap(WrongKind) before any state change.
        let (_t, _c, mmu, mut arena, mut pmm, _b) = fixture(16);
        let pmm_before = pmm.stats().free_frames;

        // Replace the AS parent cap with an Endpoint cap — wrong kind.
        let mut table = CapabilityTable::new();
        let ep_cap = Capability::new(
            CapRights::DERIVE,
            CapObject::Endpoint(EndpointHandle::test_handle(0, 0)),
        );
        let ep_cap_handle = table.insert_root(ep_cap).unwrap();

        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAA, 0xBB],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            ep_cap_handle,
            CapRights::empty(),
            VirtAddr(0x0080_0000),
            2,
        );

        assert!(
            matches!(
                result,
                Err(LoadError::InvalidParentCap(CapError::WrongKind))
            ),
            "expected InvalidParentCap(WrongKind), got {result:?}"
        );
        assert_eq!(pmm.stats().free_frames, pmm_before);
    }

    // ── §Simulation row 3 — frame-budget preflight ────────────────────────────

    #[test]
    fn rejects_image_base_va_past_userspace_va_limit() {
        // Pin §Simulation row 3 VA-range preflight: an `image_base_va`
        // past USERSPACE_VA_LIMIT (1 << 48 per ADR-0027 §Decision
        // outcome (a)) fails before any state change. PMM untouched.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(16);
        let pmm_before = pmm.stats().free_frames;

        let bad_base = VirtAddr(super::USERSPACE_VA_LIMIT);
        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAAu8; PAGE_SIZE],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            bad_base,
            1,
        );

        match result {
            Err(LoadError::InvalidImageBaseVa { base, end }) => {
                assert_eq!(base, bad_base);
                // span end = USERSPACE_VA_LIMIT + 2 * PAGE_SIZE.
                assert_eq!(end, VirtAddr(super::USERSPACE_VA_LIMIT + 2 * PAGE_SIZE));
            }
            other => panic!("expected InvalidImageBaseVa, got {other:?}"),
        }
        assert_eq!(pmm.stats().free_frames, pmm_before);
    }

    #[test]
    fn rejects_image_base_va_saturating_overflow() {
        // Pin §Simulation row 3 VA-range preflight: a near-usize::MAX
        // `image_base_va` whose span-end saturates is rejected with
        // `end == VirtAddr(usize::MAX)` as the documented sentinel.
        // PMM untouched. Base is page-aligned so the row-1 alignment
        // preflight (added in PR #31 review-round 4 P2) does not
        // intercept the overflow path.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(16);
        let pmm_before = pmm.stats().free_frames;

        // `usize::MAX & !(PAGE_SIZE - 1)` = `0xFFFF_FFFF_FFFF_F000` —
        // page-aligned, but `base + 2 * PAGE_SIZE` saturates past
        // `usize::MAX`.
        let bad_base = VirtAddr(usize::MAX & !(PAGE_SIZE - 1));
        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAAu8; PAGE_SIZE],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            bad_base,
            1,
        );

        match result {
            Err(LoadError::InvalidImageBaseVa { base, end }) => {
                assert_eq!(base, bad_base);
                assert_eq!(end, VirtAddr(usize::MAX), "saturating sentinel");
            }
            other => panic!("expected InvalidImageBaseVa, got {other:?}"),
        }
        assert_eq!(pmm.stats().free_frames, pmm_before);
    }

    #[test]
    fn accepts_image_base_va_exactly_at_userspace_va_limit_minus_span() {
        // Pin the half-open `[base, end)` boundary: `end ==
        // USERSPACE_VA_LIMIT` is **allowed** because the limit itself
        // is one-past-the-highest legal VA. A request whose span ends
        // exactly at the limit lands at the last legal frame.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(32);
        let span_pages = 1 + 1; // 1 image + 1 stack
        let edge_base = VirtAddr(super::USERSPACE_VA_LIMIT - span_pages * PAGE_SIZE);

        let loaded = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAAu8; PAGE_SIZE],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            edge_base,
            1,
        )
        .expect("span end == USERSPACE_VA_LIMIT must be accepted");

        assert_eq!(loaded.entry_va, edge_base);
        assert_eq!(loaded.stack_top_va, VirtAddr(super::USERSPACE_VA_LIMIT));
    }

    // ── §Simulation row 3 — intermediate-frame budget helper ─────────────────

    #[test]
    fn intermediate_frame_count_minimal_single_l2_slot() {
        // 1-byte image + 1-page stack at VA 8 MiB. Image VA range
        // [0x80_0000, 0x80_1000); stack VA range [0x80_1000, 0x80_2000).
        // Both fall inside L2 slot 4 (covers [0x80_0000, 0xA0_0000)).
        // Distinct slots: 1 L3 + 1 L2 + 1 L1 = 3.
        assert_eq!(
            intermediate_frame_count(VirtAddr(0x0080_0000), 1, 1),
            3,
            "minimal single-L2-slot case must yield 3 intermediates"
        );
    }

    #[test]
    fn intermediate_frame_count_8mib_image_one_stack_page_crosses_five_l2() {
        // Regression for PR #31 review-round 3 Finding 1: 8 MiB image
        // at 0x0080_0000 covers L2 slots 4..=7 (4 distinct); one stack
        // page at 0x0100_0000 (= 16 MiB) lands in L2 slot 8.
        // Distinct: 5 L3 + 1 L2 + 1 L1 = 7 (NOT the old hard-coded 6).
        let image_pages = (8 * 1024 * 1024) / PAGE_SIZE; // 2048
        assert_eq!(
            intermediate_frame_count(VirtAddr(0x0080_0000), image_pages, 1),
            7,
            "8 MiB image crossing 5 L2 slots must yield 7 intermediates"
        );
    }

    #[test]
    fn intermediate_frame_count_l1_boundary_crossing() {
        // Span crossing the 1 GiB L1 boundary: base at 1 GiB - 4 KiB,
        // 1 page image + 1 page stack. L2 slots: 511 (within L1 slot 0)
        // and 512 (within L1 slot 1) = 2 distinct L3 tables. L1 slots:
        // 0 and 1 = 2 distinct L2 tables. L0 slot: 0 = 1 L1 table.
        // Total: 2 L3 + 2 L2 + 1 L1 = 5.
        assert_eq!(
            intermediate_frame_count(VirtAddr(0x3FFF_F000), 1, 1),
            5,
            "L1 boundary crossing must yield 2 L3 + 2 L2 + 1 L1 = 5"
        );
    }

    #[test]
    fn intermediate_frame_count_zero_span_defensive() {
        // Defensive zero-span: row 1 preflight rejects zero-page
        // requests before this helper is reached, but the helper
        // itself must not panic on `image_pages = stack_pages = 0`.
        assert_eq!(intermediate_frame_count(VirtAddr(0), 0, 0), 0);
        assert_eq!(intermediate_frame_count(VirtAddr(0x0080_0000), 0, 0), 0);
    }

    #[test]
    fn intermediate_frame_count_saturated_total_pages() {
        // `total_pages.saturating_mul(PAGE_SIZE)` saturating at
        // usize::MAX must not cause the helper to overflow or panic;
        // the returned count is bounded by the L1/L2/L3 max-index
        // arithmetic regardless.
        let _ = intermediate_frame_count(VirtAddr(0), usize::MAX, 1);
        let _ = intermediate_frame_count(VirtAddr(0), 1, usize::MAX);
    }

    #[test]
    fn va_range_preflight_runs_before_frame_budget() {
        // Regression for PR #31 review-round 3 Finding 2: a saturated
        // request (`stack_size_pages = usize::MAX`) must surface as
        // `InvalidImageBaseVa` (the documented sentinel), NOT as
        // `FrameBudgetExceeded` (which previously fired first because
        // `needed.saturating_add(usize::MAX) > available` triggered
        // before the VA-range check ran).
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(16);
        let pmm_before = pmm.stats().free_frames;

        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAAu8],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            VirtAddr(0x0080_0000),
            usize::MAX,
        );

        match result {
            Err(LoadError::InvalidImageBaseVa { base, end }) => {
                assert_eq!(base, VirtAddr(0x0080_0000));
                assert_eq!(end, VirtAddr(usize::MAX), "saturating sentinel");
            }
            other => panic!("expected InvalidImageBaseVa with saturated end, got {other:?}"),
        }
        assert_eq!(pmm.stats().free_frames, pmm_before);
    }

    #[test]
    fn rejects_when_pmm_budget_exceeded() {
        // Pin §Simulation row 3: requested budget exceeding
        // pmm.stats().free_frames returns FrameBudgetExceeded with
        // accurate `needed` / `available` fields, no state change.
        // 4 frames available; ask for an 8-frame image + 8-page stack
        // at VA 0x0080_0000 (image+stack span = 9 pages = within 1
        // L2 slot) → intermediates = 1 L1 + 1 L2 + 1 L3 = 3,
        // needed = 1 + 8 + 8 + 3 = 20, available = 4 ⇒ reject.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(4);
        let pmm_before = pmm.stats().free_frames;

        // Image bytes: 8 pages worth (32 KiB).
        let image: Vec<u8> = vec![0xCDu8; 8 * PAGE_SIZE];
        let image_base = VirtAddr(0x0080_0000);
        let stack_pages = 8usize;
        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &image,
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            image_base,
            stack_pages,
        );

        match result {
            Err(LoadError::FrameBudgetExceeded { needed, available }) => {
                let expected_intermediates = intermediate_frame_count(image_base, 8, stack_pages);
                assert_eq!(needed, 1 + 8 + stack_pages + expected_intermediates);
                assert_eq!(available, pmm_before);
            }
            other => panic!("expected FrameBudgetExceeded, got {other:?}"),
        }
        assert_eq!(pmm.stats().free_frames, pmm_before);
    }

    #[test]
    fn frame_budget_includes_root_plus_intermediates() {
        // Pin the budget formula: a budget that's exactly one frame
        // short reports `needed` accounting for both the leading `1`
        // (root L0) and the exact intermediate-frame count for the
        // requested VA span.
        let image_base = VirtAddr(0x0080_0000);
        let image_pages = 2usize;
        let stack_pages = 1usize;
        let intermediates = intermediate_frame_count(image_base, image_pages, stack_pages);
        let frames_available = 1 + image_pages + stack_pages + intermediates - 1; // off-by-one short
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(frames_available);

        let image: Vec<u8> = vec![0u8; image_pages * PAGE_SIZE];
        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &image,
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            image_base,
            stack_pages,
        );

        match result {
            Err(LoadError::FrameBudgetExceeded { needed, available }) => {
                assert_eq!(needed, 1 + image_pages + stack_pages + intermediates);
                assert_eq!(available, frames_available);
                // Confirm `needed` includes both halves of the formula.
                assert!(
                    needed > image_pages + stack_pages,
                    "needed must exceed bare image+stack count"
                );
                assert!(needed >= intermediates, "needed must include intermediates");
            }
            other => panic!("expected FrameBudgetExceeded, got {other:?}"),
        }
    }

    // ── §Simulation row 4 — image-PA-overlap preflight ────────────────────────

    #[test]
    fn rejects_when_image_overlaps_allocatable_memory() {
        // Pin §Simulation row 4: an image slice whose PA range overlaps
        // a frame `pmm.alloc_frame()` could yield is rejected up front,
        // before any state change. The check discharges
        // UNSAFE-2026-0027's "source and destination do not overlap"
        // invariant at runtime; without it, a misconfigured BSP could
        // hand the loader an image that aliases a future allocation,
        // causing `copy_nonoverlapping` UB.
        //
        // Construction: a Pmm over a host-backed extent with no
        // reserved ranges (every frame is allocatable), and an image
        // slice whose `as_ptr()` falls inside the extent. The fixture's
        // `pmm_over_backing` returns exactly this shape — backing
        // bytes live in a host `Vec<u8>` reachable at the same address
        // the PMM treats as PA.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, backing) = fixture(16);
        let pmm_before = pmm.stats().free_frames;

        // The `backing` Vec's payload is the same memory the PMM's
        // extent claims to manage. Take a slice from inside the
        // backing — `image.as_ptr()` falls into the PMM's allocatable
        // region.
        let backing_ptr = backing.as_ptr();
        let aligned = ((backing_ptr as usize + (PAGE_SIZE - 1)) & !(PAGE_SIZE - 1)) as *const u8;
        // SAFETY: `aligned` is a page-aligned offset into the same
        // host allocation as `backing`; reading 8 bytes is well within
        // the backing's `(frames + 1) * PAGE_SIZE` size.
        let image: &[u8] = unsafe { core::slice::from_raw_parts(aligned, 8) };

        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            image,
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            VirtAddr(0x0080_0000),
            1,
        );

        assert_eq!(result, Err(LoadError::ImageOverlapsAllocatableMemory));
        // No state change: PMM byte-stable, table free-list unchanged.
        assert_eq!(pmm.stats().free_frames, pmm_before);
    }

    /// `.rodata`-resident image used by `accepts_image_disjoint_from_pmm_extent`.
    /// Lives in the test binary's read-only data segment and is therefore
    /// **structurally disjoint** from any heap allocation the host
    /// allocator might choose for the fixture's PMM backing — no chance
    /// of overlap, no host-allocator-dependent skip path. Mirrors the
    /// production BSP wiring shape (`static USERSPACE_IMAGE: &[u8] =
    /// ...` in `bsp-qemu-virt/src/main.rs`).
    static DISJOINT_RODATA_IMAGE: [u8; 8] = [0xAAu8; 8];

    #[test]
    fn accepts_image_disjoint_from_pmm_extent() {
        // Negative companion to the overlap test: an image whose PA
        // range is disjoint from the PMM extent (i.e., not a candidate
        // to alias a future `alloc_frame`) passes the preflight
        // cleanly. Confirms the preflight is precise (does not
        // false-positive on `.rodata`-resident images, which is the
        // production BSP wiring shape).
        //
        // PR #31 review-round 4 P3: previously this test used a
        // heap-allocated `Vec` for `image` and silently `return`-ed
        // (passing without asserting) when the host allocator happened
        // to place the Vec inside the PMM-extent host-backing region.
        // Switched to a `.rodata`-resident `static` so disjointness is
        // structurally guaranteed and any future drift surfaces as a
        // hard assertion failure rather than a silent skip.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(16);

        let image: &[u8] = &DISJOINT_RODATA_IMAGE;
        let image_pa = image.as_ptr() as usize;
        let extent_start = pmm.extent().start.0;
        let extent_end = pmm.extent().end.0;
        // Premise check — fail loudly if `.rodata` ever lands inside
        // the fixture's heap-backed PMM extent (would indicate a
        // toolchain or fixture-construction regression, not a
        // legitimate test pass).
        assert!(
            image_pa < extent_start || image_pa >= extent_end,
            "test premise broken: `.rodata` image PA {image_pa:#x} \
             is inside PMM extent [{extent_start:#x}, {extent_end:#x})"
        );

        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            image,
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            VirtAddr(0x0080_0000),
            1,
        );
        assert!(
            result.is_ok(),
            "disjoint image must not trigger overlap preflight; got {result:?}"
        );
    }

    // ── §Simulation row 5 — cap_create_address_space delegation ───────────────

    #[test]
    fn missing_derive_surfaces_via_address_space_creation_failed() {
        // Pin §Simulation row 2 ↔ row 5 split: DERIVE-rights enforcement
        // is delegated to cap_create_address_space (step 2a) and
        // surfaces as AddressSpaceCreationFailed(CapError::InsufficientRights),
        // NOT as InvalidParentCap.
        let mmu = FakeMmu::new();
        let mut arena: AddressSpaceArena<FakeMmu> = AddressSpaceArena::new();
        let mut table = CapabilityTable::new();
        // SAFETY: FakeMmu::create_address_space is pure host code.
        let bootstrap_inner = unsafe { mmu.create_address_space(frame(0x4000_0000)) };
        let bootstrap_handle = crate::mm::create_address_space(
            &mut arena,
            crate::mm::AddressSpace::wrap_bootstrap(bootstrap_inner),
        )
        .unwrap();

        // Parent cap is the right kind but lacks DERIVE.
        let no_derive_cap = Capability::new(
            CapRights::empty(),
            CapObject::AddressSpace(bootstrap_handle),
        );
        let cap_handle = table.insert_root(no_derive_cap).unwrap();

        let (backing, ptr) = aligned_backing(16);
        let mut pmm = pmm_over_backing(ptr, 16);

        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAAu8; 2 * PAGE_SIZE],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            cap_handle,
            CapRights::empty(),
            VirtAddr(0x0080_0000),
            1,
        );

        assert!(
            matches!(
                result,
                Err(LoadError::AddressSpaceCreationFailed(
                    AddressSpaceError::CapError(CapError::InsufficientRights)
                ))
            ),
            "expected AddressSpaceCreationFailed(InsufficientRights), got {result:?}"
        );
        // PMM untouched — cap_create_address_space's preflight rejects
        // pre-alloc when DERIVE is missing.
        assert_eq!(pmm.stats().free_frames, 16);
        drop(backing); // explicit lifetime extension
    }

    // ── Happy path: §Simulation rows 6 / 7 / 8 ────────────────────────────────

    #[test]
    fn returns_loaded_image_with_correct_metadata() {
        // Pin §Simulation row 8: the LoadedImage struct returned by
        // the happy path carries the values the §Simulation table
        // promises.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(32);
        let pmm_before = pmm.stats().free_frames;

        let image: Vec<u8> = vec![0xDEu8; 3 * PAGE_SIZE + 100]; // 4 pages worth (partial last)
        let image_base = VirtAddr(0x0080_0000);

        let loaded = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &image,
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            image_base,
            2, // stack pages
        )
        .expect("load_image must succeed on healthy fixture");

        // entry_va == image_base_va.
        assert_eq!(loaded.entry_va, image_base);
        // image_bytes == image.len() (not ceil_div * PAGE_SIZE).
        assert_eq!(loaded.image_bytes, 3 * PAGE_SIZE + 100);
        // stack_bytes == stack_pages * PAGE_SIZE.
        assert_eq!(loaded.stack_bytes, 2 * PAGE_SIZE);
        // stack_top_va = image_base + image_pages*PAGE_SIZE + stack_pages*PAGE_SIZE.
        // image_pages = ceil_div(3*PAGE_SIZE + 100, PAGE_SIZE) = 4.
        assert_eq!(
            loaded.stack_top_va,
            VirtAddr(0x0080_0000 + 4 * PAGE_SIZE + 2 * PAGE_SIZE)
        );

        // PMM consumed: 1 (root) + 4 (image) + 2 (stack) = 7 frames.
        // FakeMmu doesn't pull intermediate frames, so no +6 here.
        assert_eq!(pmm.stats().free_frames, pmm_before - 7);
    }

    #[test]
    fn stack_top_va_is_one_past_highest_mapped() {
        // Pin the half-open `[stack_base, stack_top_va)` convention:
        // stack_top_va is the first VA NOT mapped by the stack region.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(32);
        let image_base = VirtAddr(0x0080_0000);
        let loaded = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAAu8; PAGE_SIZE],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            image_base,
            3,
        )
        .unwrap();

        // 1 image page + 3 stack pages. Stack base = 0x0080_1000.
        // Highest mapped stack VA = 0x0080_3000 (third page).
        // stack_top_va = 0x0080_4000 (one past highest mapped).
        let stack_base = 0x0080_0000 + PAGE_SIZE;
        assert_eq!(
            loaded.stack_top_va,
            VirtAddr(stack_base + 3 * PAGE_SIZE),
            "stack_top_va must be one past highest mapped"
        );
    }

    #[test]
    fn maps_image_pages_with_user_execute_flags() {
        // Pin §Simulation row 6 mapping-flag choice: every image-page
        // mapping carries USER | EXECUTE (no WRITE).
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(32);
        let image_base = VirtAddr(0x0080_0000);

        let loaded = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAAu8; 2 * PAGE_SIZE],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            image_base,
            1,
        )
        .unwrap();

        // Inspect the new AS via the arena. The new AS lives in slot 1
        // (slot 0 is bootstrap); inspecting it requires resolving the
        // new cap.
        let new_as_handle = resolve_new_as(&table, loaded.as_cap);
        let as_ref = crate::mm::get_address_space(&arena, new_as_handle).unwrap();
        let inner: &FakeAddressSpace = inner_of(as_ref);
        for i in 0..2 {
            let va = VirtAddr(image_base.0 + i * PAGE_SIZE);
            let (_pa, flags) = inner.lookup(va).expect("image page must be mapped");
            assert!(flags.contains(MappingFlags::USER));
            assert!(flags.contains(MappingFlags::EXECUTE));
            assert!(!flags.contains(MappingFlags::WRITE));
        }
    }

    #[test]
    fn maps_stack_with_user_write_flags() {
        // Pin §Simulation row 7 mapping-flag choice: every stack-page
        // mapping carries USER | WRITE (no EXECUTE).
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(32);
        let image_base = VirtAddr(0x0080_0000);

        let loaded = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAAu8; PAGE_SIZE],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            image_base,
            2,
        )
        .unwrap();

        let new_as_handle = resolve_new_as(&table, loaded.as_cap);
        let as_ref = crate::mm::get_address_space(&arena, new_as_handle).unwrap();
        let inner: &FakeAddressSpace = inner_of(as_ref);
        let stack_base = image_base.0 + PAGE_SIZE; // 1 image page
        for i in 0..2 {
            let va = VirtAddr(stack_base + i * PAGE_SIZE);
            let (_pa, flags) = inner.lookup(va).expect("stack page must be mapped");
            assert!(flags.contains(MappingFlags::USER));
            assert!(flags.contains(MappingFlags::WRITE));
            assert!(!flags.contains(MappingFlags::EXECUTE));
        }
    }

    #[test]
    fn tail_zeroing_on_partial_last_page() {
        // Pin §Simulation row 6 tail-zeroing: image bytes are copied
        // into the leaf frame; bytes beyond image.len() % PAGE_SIZE
        // stay zero from the PMM's zero-init contract.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(32);
        let image_base = VirtAddr(0x0080_0000);

        // 100 bytes of pattern — last page is 100 bytes payload + 3996
        // zero-fill bytes.
        let image: Vec<u8> = (0u8..100u8).map(|i| 0xC0u8 ^ i).collect();

        let loaded = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &image,
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            image_base,
            1,
        )
        .unwrap();
        assert_eq!(loaded.image_bytes, 100);

        // Resolve the leaf PA for VA = image_base via the new AS and
        // read the backing host RAM directly.
        let new_as_handle = resolve_new_as(&table, loaded.as_cap);
        let as_ref = crate::mm::get_address_space(&arena, new_as_handle).unwrap();
        let inner = inner_of(as_ref);
        let (pa, _flags) = inner.lookup(image_base).expect("page mapped");

        // SAFETY: the leaf PA points into a host Vec<u8> kept alive
        // by `_b` (4 KiB of host-allocated, page-aligned memory). The
        // PMM's zero-init contract ensures bytes 100..4096 are zero;
        // the loader's copy_nonoverlapping wrote bytes 0..100.
        let payload_ptr = pa.as_usize() as *const u8;
        for off in 0u8..100u8 {
            // SAFETY: payload_ptr points to the head of a 4 KiB host
            // Vec<u8> kept alive by the fixture's `_b`; reading the
            // first 100 bytes is in-bounds.
            let actual = unsafe { *payload_ptr.add(off as usize) };
            let expected = 0xC0u8 ^ off;
            assert_eq!(actual, expected, "image byte at off={off} drifted");
        }
        for off in 100..PAGE_SIZE {
            // SAFETY: payload_ptr points to a 4 KiB host Vec<u8> kept
            // alive by `_b`; reading bytes 100..PAGE_SIZE is in-bounds.
            let actual = unsafe { *payload_ptr.add(off) };
            assert_eq!(actual, 0u8, "tail byte at off={off} must be zero");
        }
    }

    // ── Rollback: §Simulation row 6 / 7 failure paths ─────────────────────────

    /// `Mmu` decorator that fails the (1 + n)-th `map` call with
    /// `MmuError::AlreadyMapped`. Delegates everything else to
    /// `FakeMmu`. Used to drive mid-loop `cap_map` failures
    /// deterministically.
    struct FailingMapMmu {
        inner: FakeMmu,
        map_count: Mutex<usize>,
        fail_at: usize,
    }

    impl FailingMapMmu {
        fn new(fail_at: usize) -> Self {
            Self {
                inner: FakeMmu::new(),
                map_count: Mutex::new(0),
                fail_at,
            }
        }
    }

    impl Mmu for FailingMapMmu {
        type AddressSpace = <FakeMmu as Mmu>::AddressSpace;

        unsafe fn create_address_space(&self, root: PhysFrame) -> Self::AddressSpace {
            // SAFETY: delegating to FakeMmu's pure host-side impl.
            unsafe { self.inner.create_address_space(root) }
        }

        fn address_space_root(&self, as_: &Self::AddressSpace) -> PhysFrame {
            self.inner.address_space_root(as_)
        }

        fn activate(&self, as_: &Self::AddressSpace) {
            self.inner.activate(as_);
        }

        fn map(
            &self,
            as_: &mut Self::AddressSpace,
            va: VirtAddr,
            pa: PhysFrame,
            flags: MappingFlags,
            frames: &mut dyn FrameProvider,
        ) -> Result<MapperFlush, MmuError> {
            let n = {
                let mut guard = self.map_count.lock().unwrap();
                let cur = *guard;
                *guard = cur + 1;
                cur
            };
            if n >= self.fail_at {
                return Err(MmuError::AlreadyMapped);
            }
            self.inner.map(as_, va, pa, flags, frames)
        }

        fn unmap(
            &self,
            as_: &mut Self::AddressSpace,
            va: VirtAddr,
        ) -> Result<(MapperFlush, PhysFrame), MmuError> {
            self.inner.unmap(as_, va)
        }

        fn invalidate_tlb_address(&self, va: VirtAddr) {
            self.inner.invalidate_tlb_address(va);
        }

        fn invalidate_tlb_all(&self) {
            self.inner.invalidate_tlb_all();
        }
    }

    fn fixture_with_failing_mmu(
        frames: usize,
        fail_at: usize,
    ) -> (
        CapabilityTable,
        crate::cap::CapHandle,
        FailingMapMmu,
        AddressSpaceArena<FailingMapMmu>,
        TestPmm,
        Vec<u8>,
    ) {
        let mmu = FailingMapMmu::new(fail_at);
        let mut arena: AddressSpaceArena<FailingMapMmu> = AddressSpaceArena::new();
        let mut table = CapabilityTable::new();

        // SAFETY: FailingMapMmu's create_address_space delegates to
        // FakeMmu's pure host code.
        let bootstrap_inner = unsafe { mmu.create_address_space(frame(0x4000_0000)) };
        let bootstrap_handle = crate::mm::create_address_space(
            &mut arena,
            crate::mm::AddressSpace::wrap_bootstrap(bootstrap_inner),
        )
        .unwrap();

        let parent_cap = Capability::new(
            CapRights::DUPLICATE | CapRights::DERIVE | CapRights::REVOKE | CapRights::TRANSFER,
            CapObject::AddressSpace(bootstrap_handle),
        );
        let parent_cap_handle = table.insert_root(parent_cap).unwrap();

        let (backing, ptr) = aligned_backing(frames);
        let pmm = pmm_over_backing(ptr, frames);

        (table, parent_cap_handle, mmu, arena, pmm, backing)
    }

    #[test]
    fn rolls_back_on_cap_map_failure_mid_image_loop() {
        // Pin §Simulation row 6 rollback: a cap_map failure mid-image-
        // loop unwinds every committed mapping (via cap_unmap +
        // pmm.free_frame) AND frees the failing iteration's leaf frame
        // AND drops the AS cap. The v1 leaks are documented in
        // T-019 §"Rollback contract" (root L0 + AS arena slot + future
        // intermediate frames if FakeMmu had pulled any).
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture_with_failing_mmu(32, 2);

        let pmm_before = pmm.stats().free_frames;
        let image_base = VirtAddr(0x0080_0000);

        // 4-page image; cap_map fails on the 3rd call (fail_at = 2; 0-indexed
        // n >= fail_at).
        let result = load_image::<FailingMapMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAAu8; 4 * PAGE_SIZE],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            image_base,
            2,
        );

        assert!(
            matches!(
                result,
                Err(LoadError::MapFailed(AddressSpaceError::MmuMapError(
                    MmuError::AlreadyMapped
                )))
            ),
            "expected MapFailed(AlreadyMapped), got {result:?}"
        );

        // Rollback accounting:
        //   - 2 leaf frames committed via cap_map (idx 0, 1) → freed via
        //     cap_unmap + pmm.free_frame.
        //   - 1 leaf frame alloc'd for the failing iteration → freed
        //     directly via pmm.free_frame.
        //   - 1 root L0 frame for the new AS → LEAKED in v1.
        // Net: pmm.free_frames == pmm_before - 1 (the leaked root).
        assert_eq!(
            pmm.stats().free_frames,
            pmm_before - 1,
            "rollback must free all leaf frames; only the root L0 leaks in v1"
        );

        // The AS cap dropping itself is exercised by the dedicated
        // `rollback_helper_zero_pages_only_drops_cap` test below; the
        // load_image error path does not return the partial cap handle,
        // so PMM accounting is the load-bearing signal here.
    }

    #[test]
    fn rolls_back_on_cap_map_failure_mid_stack_loop() {
        // Pin §Simulation row 7 rollback: a cap_map failure mid-stack-
        // loop unwinds BOTH the image-loop's committed mappings AND
        // the stack-loop's mappings, then drops the AS cap.
        // fail_at = 3 means: first 3 map calls succeed (3 image pages),
        // 4th call (1st stack page) fails.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture_with_failing_mmu(32, 3);

        let pmm_before = pmm.stats().free_frames;
        let image_base = VirtAddr(0x0080_0000);

        // 3-page image; cap_map fails on the 4th call (first stack page).
        let result = load_image::<FailingMapMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAAu8; 3 * PAGE_SIZE],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            image_base,
            2,
        );

        assert!(
            matches!(
                result,
                Err(LoadError::MapFailed(AddressSpaceError::MmuMapError(
                    MmuError::AlreadyMapped
                )))
            ),
            "expected MapFailed(AlreadyMapped), got {result:?}"
        );

        // Rollback accounting:
        //   - 3 image-loop leaf frames committed → freed via cap_unmap.
        //   - 1 stack-loop leaf frame alloc'd, cap_map failed → freed
        //     directly via pmm.free_frame.
        //   - 1 root L0 frame → LEAKED.
        assert_eq!(
            pmm.stats().free_frames,
            pmm_before - 1,
            "rollback must free all leaf frames; only the root L0 leaks in v1"
        );
    }

    #[test]
    fn rolls_back_on_pmm_exhausted_mid_image_loop() {
        // Pin §Simulation row 6 rollback (OutOfFrames branch): a direct
        // `pmm.alloc_frame()` failure mid-image-loop unwinds every
        // committed mapping AND drops the AS cap. Distinguished from
        // the `MapFailed` rollback by the absence of a leaf-frame-to-
        // free-directly: alloc itself returned None, so no leaf was
        // ever in hand for the failing iteration.
        //
        // The OutOfFrames branch is structurally unreachable in v1
        // under the frame-budget preflight + single-thread cooperative
        // model (`load_image`'s rustdoc + LoadError::OutOfFrames
        // doc-comment both note this). Test-only failure injection via
        // `Pmm::force_alloc_failure_after` drives the path
        // deterministically so the defensive rollback is exercised by
        // live code rather than only by static reading.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(32);
        let pmm_before = pmm.stats().free_frames;

        // Schedule the 3rd alloc_frame call to fail:
        //   alloc #1: cap_create_address_space's root L0 — succeeds.
        //   alloc #2: image-page idx 0 leaf — succeeds; cap_map then
        //             commits the mapping.
        //   alloc #3: image-page idx 1 leaf — returns None → OutOfFrames.
        pmm.force_alloc_failure_after(2);

        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAAu8; 4 * PAGE_SIZE], // 4 image pages — failure mid-loop
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            VirtAddr(0x0080_0000),
            2,
        );

        assert_eq!(result, Err(LoadError::OutOfFrames));

        // Rollback accounting:
        //   - 1 leaf frame committed via cap_map (image idx 0) → freed
        //     via cap_unmap + pmm.free_frame.
        //   - 0 leaf frames to free directly (alloc itself failed; the
        //     OutOfFrames branch has no leaf in hand for the failing
        //     iteration, unlike the MapFailed branch).
        //   - 1 root L0 frame for the new AS → LEAKED per v1 baseline.
        // Net: pmm.free_frames == pmm_before - 1.
        assert_eq!(
            pmm.stats().free_frames,
            pmm_before - 1,
            "OutOfFrames rollback must free the one committed image leaf; only the root L0 leaks"
        );
    }

    #[test]
    fn rejects_misaligned_image_base_va_with_pmm_byte_stable() {
        // Pin PR #31 review-round 4 P2: a misaligned `image_base_va`
        // is rejected by the row-1 argument preflight, **before** any
        // `cap_create_address_space` call. PMM must be byte-stable on
        // return (no root L0 frame leaked); cap table free-list also
        // unchanged.
        //
        // Pre-fix, the misalignment surfaced as
        // `MapFailed(MmuMapError(MisalignedAddress))` from the first
        // cap_map call inside the image-page loop — by which point
        // `cap_create_address_space` had already allocated the root
        // L0 frame, leaking it via the v1 baseline rollback. The new
        // preflight prevents that leak entirely.
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(32);
        let pmm_before = pmm.stats().free_frames;
        let bad_base = VirtAddr(0x0080_0001); // off-by-one byte

        let result = load_image::<FakeMmu, TEST_PMM_N, TEST_PMM_R>(
            &[0xAAu8; 2 * PAGE_SIZE],
            &mut pmm,
            &mmu,
            &mut table,
            &mut arena,
            parent_cap,
            CapRights::empty(),
            bad_base,
            2,
        );

        assert_eq!(
            result,
            Err(LoadError::MisalignedImageBaseVa(bad_base)),
            "misaligned base must surface as MisalignedImageBaseVa, \
             not MapFailed(MisalignedAddress)"
        );
        // No PMM mutation: the preflight catches the misalignment
        // before any `pmm.alloc_frame` (including the root L0 alloc
        // inside `cap_create_address_space`).
        assert_eq!(
            pmm.stats().free_frames,
            pmm_before,
            "PMM must be byte-stable on misaligned-base rejection"
        );
    }

    // ── Variant-shape regression (carried forward from commit 1) ─────────────

    #[test]
    fn loaded_image_struct_literal_round_trips_through_copy_and_eq() {
        // Pin the public-struct-literal convention: callers can
        // construct a LoadedImage by writing the 5 fields directly,
        // round-trip it through Copy semantics, and compare for equality.
        let cap = fresh_cap_handle();
        let img = LoadedImage {
            as_cap: cap,
            entry_va: VirtAddr(0x0080_0000),
            stack_top_va: VirtAddr(0x0090_0000),
            image_bytes: 4096,
            stack_bytes: 65_536,
        };
        let copy = img;
        assert_eq!(img, copy);
        assert_eq!(img.as_cap, cap);
        assert_eq!(img.entry_va, VirtAddr(0x0080_0000));
        assert_eq!(img.stack_top_va, VirtAddr(0x0090_0000));
        assert_eq!(img.image_bytes, 4096);
        assert_eq!(img.stack_bytes, 65_536);
    }

    #[test]
    fn loaded_image_distinguishes_different_field_values() {
        let cap = fresh_cap_handle();
        let a = LoadedImage {
            as_cap: cap,
            entry_va: VirtAddr(0x0080_0000),
            stack_top_va: VirtAddr(0x0090_0000),
            image_bytes: 4096,
            stack_bytes: 65_536,
        };
        let b = LoadedImage {
            stack_bytes: 131_072,
            ..a
        };
        assert_ne!(a, b);
    }

    #[test]
    fn load_error_variants_pattern_match_exhaustively() {
        // Pin the 10-variant taxonomy. `#[non_exhaustive]` only forces
        // external (out-of-crate) consumers to add a wildcard arm;
        // within-crate exhaustiveness still fires. Adding a variant
        // breaks this test at compile time.
        let cases = [
            LoadError::InvalidImage,
            LoadError::InvalidStackSize,
            LoadError::MisalignedImageBaseVa(VirtAddr(0x0080_0001)),
            LoadError::InvalidImageBaseVa {
                base: VirtAddr(0xFFFF_FFFF_FFFF_0000),
                end: VirtAddr(usize::MAX),
            },
            LoadError::InvalidParentCap(CapError::InvalidHandle),
            LoadError::FrameBudgetExceeded {
                needed: 100,
                available: 50,
            },
            LoadError::ImageOverlapsAllocatableMemory,
            LoadError::AddressSpaceCreationFailed(AddressSpaceError::OutOfFrames),
            LoadError::OutOfFrames,
            LoadError::MapFailed(AddressSpaceError::StaleHandle),
        ];
        for err in cases {
            match err {
                LoadError::InvalidImage
                | LoadError::InvalidStackSize
                | LoadError::MisalignedImageBaseVa(_)
                | LoadError::InvalidImageBaseVa { .. }
                | LoadError::InvalidParentCap(_)
                | LoadError::FrameBudgetExceeded { .. }
                | LoadError::ImageOverlapsAllocatableMemory
                | LoadError::AddressSpaceCreationFailed(_)
                | LoadError::OutOfFrames
                | LoadError::MapFailed(_) => { /* exhaustive within-crate */ }
            }
        }
    }

    #[test]
    fn load_error_variants_are_distinct() {
        assert_ne!(LoadError::InvalidImage, LoadError::InvalidStackSize);
        assert_ne!(LoadError::InvalidImage, LoadError::OutOfFrames);
        assert_ne!(LoadError::InvalidStackSize, LoadError::OutOfFrames);
        assert_ne!(
            LoadError::ImageOverlapsAllocatableMemory,
            LoadError::OutOfFrames,
        );
        assert_ne!(
            LoadError::ImageOverlapsAllocatableMemory,
            LoadError::InvalidImage,
        );
        assert_ne!(
            LoadError::MisalignedImageBaseVa(VirtAddr(0x0080_0001)),
            LoadError::MisalignedImageBaseVa(VirtAddr(0x0080_0002)),
        );
        // Different variants both wrapping a VirtAddr stay distinct.
        assert_ne!(
            LoadError::MisalignedImageBaseVa(VirtAddr(0x0080_0001)),
            LoadError::InvalidImageBaseVa {
                base: VirtAddr(0x0080_0001),
                end: VirtAddr(usize::MAX),
            },
        );
        assert_ne!(
            LoadError::InvalidParentCap(CapError::InvalidHandle),
            LoadError::InvalidParentCap(CapError::WrongKind),
        );
        assert_ne!(
            LoadError::FrameBudgetExceeded {
                needed: 100,
                available: 50,
            },
            LoadError::FrameBudgetExceeded {
                needed: 200,
                available: 50,
            },
        );
        assert_ne!(
            LoadError::AddressSpaceCreationFailed(AddressSpaceError::OutOfFrames),
            LoadError::AddressSpaceCreationFailed(AddressSpaceError::StaleHandle),
        );
        assert_ne!(
            LoadError::MapFailed(AddressSpaceError::StaleHandle),
            LoadError::MapFailed(AddressSpaceError::OutOfFrames),
        );
    }

    #[test]
    fn load_error_frame_budget_exceeded_fields_round_trip() {
        let err = LoadError::FrameBudgetExceeded {
            needed: 1024,
            available: 8,
        };
        match err {
            LoadError::FrameBudgetExceeded { needed, available } => {
                assert_eq!(needed, 1024);
                assert_eq!(available, 8);
            }
            other => panic!("expected FrameBudgetExceeded, got {other:?}"),
        }
    }

    #[test]
    fn rollback_helper_zero_pages_only_drops_cap() {
        // Pin the rollback helper's behaviour when nothing was mapped:
        // only the cap_drop fires; no spurious cap_unmap calls (which
        // would all return Err(NotMapped) anyway).
        let (mut table, parent_cap, mmu, mut arena, mut pmm, _b) = fixture(32);

        // Mint an AS cap via cap_create_address_space directly so we
        // have something for rollback to drop.
        let new_cap = crate::mm::cap_create_address_space(
            &mut table,
            parent_cap,
            CapRights::empty(),
            &mmu,
            &mut pmm,
            &mut arena,
        )
        .unwrap();
        // Confirm the cap resolves before rollback.
        assert!(table.lookup(new_cap).is_ok());

        rollback(
            &mut table,
            &mut pmm,
            &mmu,
            &mut arena,
            new_cap,
            VirtAddr(0x0080_0000),
            VirtAddr(0x0090_0000),
            0,
            0,
        );

        // After rollback, the cap is dropped.
        assert!(matches!(
            table.lookup(new_cap),
            Err(CapError::InvalidHandle)
        ));
    }

    // ── Test helpers ──────────────────────────────────────────────────────────

    /// Mint a real `CapHandle` into a fresh `CapabilityTable` and
    /// return it. The underlying cap's object is an `Endpoint`
    /// (irrelevant; the point is to construct a valid `CapHandle`
    /// value so a `LoadedImage` literal can be assembled).
    fn fresh_cap_handle() -> crate::cap::CapHandle {
        let mut table = CapabilityTable::new();
        let cap = Capability::new(
            CapRights::empty(),
            CapObject::Endpoint(EndpointHandle::test_handle(0, 0)),
        );
        table.insert_root(cap).unwrap()
    }

    /// Resolve a `CapHandle` to the inner `AddressSpaceHandle`. Panics
    /// on shape mismatch.
    fn resolve_new_as(
        table: &CapabilityTable,
        cap_handle: crate::cap::CapHandle,
    ) -> crate::mm::AddressSpaceHandle {
        match table.lookup(cap_handle).unwrap().object() {
            CapObject::AddressSpace(h) => h,
            other => panic!("expected AddressSpace cap, got {other:?}"),
        }
    }

    /// Borrow the BSP-specific inner from an `AddressSpace<FakeMmu>`.
    /// `AddressSpace::inner` is `pub(crate)` and tests live in the
    /// same crate, so we call it directly.
    fn inner_of<M: Mmu>(as_: &crate::mm::AddressSpace<M>) -> &M::AddressSpace {
        as_.inner()
    }
}
