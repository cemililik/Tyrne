//! BSP-side [`Mmu`] implementation for the QEMU `virt` aarch64 machine.
//!
//! Implements the [`tyrne_hal::Mmu`] trait surface against the `VMSAv8`
//! 4 KiB-granule, 48-bit virtual-address, 4-level translation regime per
//! [ADR-0027][adr-0027]. The pure descriptor-encoding helpers
//! ([`tyrne_hal::mmu::vmsav8`]) live in HAL — host-tested there;
//! the asm- and stateful-side of the impl lives here.
//!
//! ## Audit-log coordinates
//!
//! - [UNSAFE-2026-0023] — `activate`'s `MSR TTBR0_EL1 + ISB + TLBI VMALLE1`
//!   sequence (system-register writes).
//! - [UNSAFE-2026-0024] — `invalidate_tlb_address` / `invalidate_tlb_all`
//!   (TLB asm + barriers).
//! - UNSAFE-2026-0025 — per-call `Mmu::map` / `Mmu::unmap` page-table entry
//!   writes; lands with the body of those methods (Stage 4).
//!
//! [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
//! [UNSAFE-2026-0023]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md
//! [UNSAFE-2026-0024]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md

// `QemuVirtMmu` and its `Mmu` impl are the post-bootstrap address-
// space-management surface (per ADR-0027 §Decision outcome (c)). The
// v1 cooperative demo does not call `Mmu::map` / `Mmu::unmap`
// post-bootstrap — bootstrap covers everything v1 needs — so the
// impl + helpers are infrastructure for the first B3+ task that
// introduces a post-bootstrap mapping caller (e.g., a real PMM-driven
// kernel-stack remap or the future userspace bring-up). They are
// `pub` so future BSP / kernel callers can use them; the
// `dead_code` allow disappears when that first caller lands.
#![allow(
    dead_code,
    reason = "QemuVirtMmu post-bootstrap surface; first caller is a future B3+ task (no v1 demo caller per ADR-0027 §Out of scope)"
)]

use core::arch::asm;

use tyrne_hal::mmu::vmsav8::{
    flags_to_descriptor_bits, page_descriptor, table_descriptor, PAGE_OA_MASK_L3, TABLE_NLA_MASK,
};
use tyrne_hal::{
    FrameProvider, MapperFlush, MappingFlags, Mmu, MmuError, PhysAddr, PhysFrame, VirtAddr,
    PAGE_SIZE,
};

/// Translation-table layout constants for the `VMSAv8` 4 KiB-granule,
/// 48-bit-VA, 4-level scheme used in v1.
///
/// The VA is split as: `L0 idx (47:39) | L1 idx (38:30) | L2 idx (29:21) | L3 idx (20:12) | offset (11:0)`.
const VA_L0_SHIFT: u32 = 39;
const VA_L1_SHIFT: u32 = 30;
const VA_L2_SHIFT: u32 = 21;
const VA_L3_SHIFT: u32 = 12;
const VA_INDEX_MASK: u64 = 0x1FF;

/// Number of `u64` entries per 4 KiB translation table.
const ENTRIES_PER_TABLE: usize = PAGE_SIZE / 8;

const DESC_VALID_BIT: u64 = 1 << 0;
const DESC_TABLE_OR_PAGE_BIT: u64 = 1 << 1;

/// Address-space representation for [`QemuVirtMmu`].
///
/// Carries the root translation-table frame (the L0 table that
/// [`Mmu::activate`] writes into `TTBR0_EL1`). v1 has a single
/// kernel-half address space populated at boot by `mmu_bootstrap`;
/// post-MMU code that wants additional address spaces will allocate
/// a fresh root frame, build the layout, then call `activate`.
#[derive(Copy, Clone, Debug)]
pub struct QemuVirtAddressSpace {
    root: PhysFrame,
}

impl QemuVirtAddressSpace {
    /// Return the root translation-table frame.
    ///
    /// Mirrors [`Mmu::address_space_root`] but as an inherent method —
    /// useful for callers that hold a `&QemuVirtAddressSpace` directly
    /// (e.g., the bootstrap routine) without needing the `Mmu` instance.
    #[must_use]
    pub fn root(self) -> PhysFrame {
        self.root
    }

    /// Construct a `QemuVirtAddressSpace` naming an already-live root
    /// translation table.
    ///
    /// Companion to [`Mmu::create_address_space`] for the bootstrap
    /// case (per [ADR-0028 §Simulation row 0][adr-0028]). `mmu_bootstrap`
    /// activates the L0/L1/L2 frames via `MSR TTBR0_EL1` before any
    /// `AddressSpace<QemuVirtMmu>` value exists at the kernel layer;
    /// this constructor lets `kernel_entry` wrap the already-live root
    /// without going through the unsafe `create_address_space` trait
    /// method (whose contract requires a *zero-filled* root — true for
    /// post-PMM-alloc frames, false for the live bootstrap root).
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `root` is a valid, **currently-
    /// live** `VMSAv8` L0 translation table — i.e., a 4 KiB frame whose
    /// 512 × 8-byte entries are correctly-encoded `VMSAv8` table /
    /// block / page descriptors, with at least the kernel-half
    /// mappings populated. Subsequent operations on the resulting
    /// `QemuVirtAddressSpace` (e.g., [`Mmu::map`] / [`Mmu::unmap`])
    /// perform `volatile` reads + writes through this root's descriptor
    /// chain; passing an arbitrary `PhysFrame` would dereference
    /// garbage bytes as descriptors and produce undefined behaviour at
    /// the page-table walker level.
    ///
    /// This contract is **distinct** from [`Mmu::create_address_space`]'s:
    /// `create_address_space` requires the root to be *zero-filled*
    /// (and the kernel-half mappings to be populated by the caller
    /// post-construction); `from_existing_root` requires the root to
    /// be *already populated and live*. Both are caller-side
    /// preconditions the type system cannot enforce.
    ///
    /// v1's only caller is `bsp-qemu-virt/src/main.rs::kernel_entry`,
    /// which derives `root` from the `__boot_pt_l0` linker symbol —
    /// the L0 frame `mmu_bootstrap` populated and wrote into
    /// `TTBR0_EL1`. The bootstrap path is the only well-known
    /// already-live root in v1.
    ///
    /// [adr-0028]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0028-address-space-data-structure.md
    #[must_use]
    pub unsafe fn from_existing_root(root: PhysFrame) -> Self {
        Self { root }
    }
}

/// `Mmu` impl for QEMU `virt` (`VMSAv8` page-table format, 4 KiB granule,
/// 48-bit VA, 4-level translation, kernel in `TTBR0_EL1`).
///
/// Zero-sized: the per-`Mmu`-instance state is empty because every
/// method's effect is on a global system-register / TLB resource that
/// is per-core, not per-instance. v1 instantiates this once in
/// `kernel_entry`; future multi-CPU work may need per-core instances.
#[derive(Copy, Clone, Debug, Default)]
pub struct QemuVirtMmu;

impl QemuVirtMmu {
    /// Construct a `QemuVirtMmu`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Mmu for QemuVirtMmu {
    type AddressSpace = QemuVirtAddressSpace;

    unsafe fn create_address_space(&self, root: PhysFrame) -> QemuVirtAddressSpace {
        // No allocation; the safety contract of the trait method covers
        // exclusive ownership + zero-initialisation of `root`.
        QemuVirtAddressSpace { root }
    }

    fn address_space_root(&self, as_: &Self::AddressSpace) -> PhysFrame {
        as_.root
    }

    fn activate(&self, as_: &Self::AddressSpace) {
        // Build the TTBR0_EL1 value: ASID (bits 63:48) = 0 (v1 uses a
        // single global address space) + base address (bits 47:0). The
        // root frame's base PA is already 4 KiB-aligned by the
        // PhysFrame invariant, so no masking is required.
        let ttbr0 = as_.root.as_usize() as u64;

        // SAFETY: writing TTBR0_EL1 swaps the active page-table base.
        // The new base must be a valid root translation table; that
        // invariant is established by the trait's `create_address_space`
        // safety contract (root is a zero-initialised, exclusively-owned
        // PhysFrame populated with a valid VMSAv8 layout before this
        // call) and by the caller.
        //
        // Sequence: `MSR TTBR0_EL1` + `ISB` (translation regime now
        // staged but stale TLB entries may exist) + `DSB ISHST`
        // (ensure any prior page-table descriptor stores are globally
        // observable inner-shareable before the TLBI broadcast — see
        // 2026-05-09 review-round Finding 4 / ADR-0027 §"Why DSB ISH"
        // forward-compat) + `TLBI VMALLE1` + `DSB ISH` (drain
        // invalidate completion) + `ISB` (drain pipeline so the next
        // instruction-fetch goes through the freshly-installed
        // regime). `options(nostack)` only — `nomem` omitted so the
        // compiler treats this asm as a memory clobber and cannot
        // reorder prior page-table writes past it.
        // Audit: UNSAFE-2026-0023.
        unsafe {
            asm!(
                "msr ttbr0_el1, {0}",
                "isb",
                "dsb ishst",
                "tlbi vmalle1",
                "dsb ish",
                "isb",
                in(reg) ttbr0,
                options(nostack),
            );
        }
    }

    fn map(
        &self,
        as_: &mut Self::AddressSpace,
        va: VirtAddr,
        pa: PhysFrame,
        flags: MappingFlags,
        frames: &mut dyn FrameProvider,
    ) -> Result<MapperFlush, MmuError> {
        // VA must be 4 KiB-aligned (the trait method's contract).
        if !va.0.is_multiple_of(PAGE_SIZE) {
            return Err(MmuError::MisalignedAddress);
        }

        // Reject unrepresentable flag combinations up-front instead of
        // letting `flags_to_descriptor_bits` silently coerce them.
        // DEVICE mappings are unconditionally non-executable (PXN=1,
        // UXN=1) per ADR-0027 §Decision outcome (b) — the v1 MMIO
        // attack surface gains nothing from execute permissions, and
        // userspace MMIO is out of scope. A caller passing
        // DEVICE | EXECUTE has either a bug or a misunderstanding;
        // either way, the trait surface should reject the request
        // rather than silently drop EXECUTE. (2026-05-09 review-round
        // Finding 3.)
        if flags.contains(MappingFlags::DEVICE) && flags.contains(MappingFlags::EXECUTE) {
            return Err(MmuError::InvalidFlags);
        }

        // Walk L0 → L1 → L2, allocating intermediate tables when needed,
        // then write the L3 page descriptor for the (va, pa, flags)
        // tuple.
        // SAFETY: page-table-walk + descriptor write through the
        // address-space root. The root is a valid VMSAv8 root frame
        // by `Mmu::create_address_space`'s safety contract; `frames`
        // supplies fresh page-aligned, exclusively-owned frames per
        // the `FrameProvider` contract; `pa` is page-aligned by the
        // `PhysFrame` invariant. Writes are scoped to descriptor
        // slots indexed by the VA's L0/L1/L2/L3 bits — the index
        // bounds (`(va >> shift) & 0x1FF`) cannot exceed
        // `ENTRIES_PER_TABLE - 1` (= 511), so every `add(idx)` stays
        // within the 4 KiB frame. Ordering: the leaf descriptor is
        // written last after intermediate table descriptors are
        // installed, so a translation walk cannot race against a
        // half-built path. Per-VA TLB invalidate is the caller's
        // responsibility, enforced via the `MapperFlush` token this
        // method returns. options(nostack, nomem) is *not* applicable
        // here; the asm is `core::ptr::write_volatile` on raw
        // pointers, not inline asm. Audit: UNSAFE-2026-0025.
        unsafe {
            walk_and_install_leaf(as_.root, va, pa, flags, frames, /* unmap */ false)
        }
        .map(|_| MapperFlush::new(va))
    }

    fn unmap(
        &self,
        as_: &mut Self::AddressSpace,
        va: VirtAddr,
    ) -> Result<(MapperFlush, PhysFrame), MmuError> {
        if !va.0.is_multiple_of(PAGE_SIZE) {
            return Err(MmuError::MisalignedAddress);
        }
        // SAFETY: same argument as `map`, but on the unmap path: walk
        // to the L3 leaf, capture the frame the descriptor pointed at,
        // clear the descriptor (zero invalidates the entry per ARM ARM
        // §D5.3 — bit 0 = 0 = invalid). No intermediate-table
        // allocation; `frames` is unused on the unmap path. Per-VA
        // TLB invalidate via `MapperFlush`. Audit: UNSAFE-2026-0025.
        let pa = unsafe {
            walk_and_install_leaf(
                as_.root,
                va,
                /* pa: ignored on unmap */ PhysFrame::from_aligned(PhysAddr(0)).unwrap(),
                MappingFlags::empty(),
                &mut NullFrameProvider,
                /* unmap */ true,
            )
        }?;
        Ok((MapperFlush::new(va), pa))
    }

    fn invalidate_tlb_address(&self, va: VirtAddr) {
        // ARM ARM §D11.2.4: TLBI VAE1 takes a register operand whose
        // top 16 bits are the ASID and bottom 48 bits are the VA >>
        // PAGE_SHIFT (12). v1 uses ASID=0 globally, so the top 16
        // bits stay 0; the encoded operand is `(va >> 12) & 0x0000_FFFF_FFFF_FFFF`.
        let arg = ((va.0 as u64) >> 12) & 0x0000_FFFF_FFFF_FFFF;

        // SAFETY: TLBI VAE1 invalidates a per-VA TLB entry. The
        // operand encoding is per ARM ARM §D11.2.4. Sequence (per
        // ARM ARM canonical pattern + Linux `arch/arm64/include/asm/
        // tlbflush.h`):
        //   - DSB ISHST  ensures any preceding page-table descriptor
        //                store is globally observable inner-shareable
        //                BEFORE the TLBI broadcast. Without this, on
        //                SMP a peer core could receive the TLBI, walk
        //                the still-stale descriptor on a TLB miss,
        //                and re-cache it. (2026-05-09 review-round
        //                Finding 4 / ADR-0027 §"Why DSB ISH"
        //                forward-compat.)
        //   - TLBI VAE1  per-VA invalidate.
        //   - DSB ISH    ensures the invalidate completes within the
        //                inner-shareable domain.
        //   - ISB        drains the pipeline so a subsequent
        //                instruction-fetch sees the post-invalidate
        //                state.
        // `options(nostack)` only — `nomem` omitted so the compiler
        // treats this asm as a memory clobber against any prior
        // descriptor store.
        // Audit: UNSAFE-2026-0024.
        unsafe {
            asm!(
                "dsb ishst",
                "tlbi vae1, {0}",
                "dsb ish",
                "isb",
                in(reg) arg,
                options(nostack),
            );
        }
    }

    fn invalidate_tlb_all(&self) {
        // SAFETY: TLBI VMALLE1 invalidates every stage-1 EL1 TLB
        // entry on the current core (and within the inner-shareable
        // domain after the DSB ISH). DSB ISHST before TLBI ensures
        // prior descriptor stores are globally observable BEFORE the
        // broadcast; DSB ISH + ISB after sequence as above per ARM
        // ARM canonical pattern. (2026-05-09 review-round Finding 4.)
        // Audit: UNSAFE-2026-0024.
        unsafe {
            asm!(
                "dsb ishst",
                "tlbi vmalle1",
                "dsb ish",
                "isb",
                options(nostack),
            );
        }
    }
}

// ── Page-table walk implementation ─────────────────────────────────────────────

/// `FrameProvider` that always returns `None`. Used by `unmap`, which
/// walks an existing path and never allocates intermediate frames.
struct NullFrameProvider;

impl FrameProvider for NullFrameProvider {
    fn alloc_frame(&mut self) -> Option<PhysFrame> {
        None
    }
}

/// Compute the per-level index for `va` at the level whose VA-bit
/// shift is `shift` (39 for L0, 30 for L1, 21 for L2, 12 for L3).
const fn va_index(va: VirtAddr, shift: u32) -> usize {
    (((va.0 as u64) >> shift) & VA_INDEX_MASK) as usize
}

/// Walk the L0 → L1 → L2 → L3 path for `va`, then either install a
/// page descriptor (when `unmap` is `false`) or clear the existing one
/// (when `unmap` is `true`). Returns the leaf physical frame on the
/// unmap path; returns `pa` on the map path (carried for symmetry but
/// the caller already has it).
///
/// # Safety
///
/// - `root` must be a valid `VMSAv8` root translation-table frame, in
///   the sense pinned by [`Mmu::create_address_space`]'s safety
///   contract (zero-initialised, exclusively-owned, with valid table
///   descriptors at every populated slot reached by this walk).
/// - The frames at every level reachable from `root` must be exclusively
///   owned by this address space for the duration of the call.
/// - `frames` must supply page-aligned frames the caller has the right
///   to plumb into `root`'s table chain.
///
/// Audit: UNSAFE-2026-0025.
#[allow(
    clippy::too_many_arguments,
    reason = "the function is BSP-internal; splitting into a builder pattern would obscure the page-table-walk shape"
)]
unsafe fn walk_and_install_leaf(
    root: PhysFrame,
    va: VirtAddr,
    pa: PhysFrame,
    flags: MappingFlags,
    frames: &mut dyn FrameProvider,
    unmap: bool,
) -> Result<PhysFrame, MmuError> {
    let l0_idx = va_index(va, VA_L0_SHIFT);
    let l1_idx = va_index(va, VA_L1_SHIFT);
    let l2_idx = va_index(va, VA_L2_SHIFT);
    let l3_idx = va_index(va, VA_L3_SHIFT);

    // SAFETY: `root` is a valid root table frame per the function's
    // safety contract; `l0_idx < ENTRIES_PER_TABLE` by `& 0x1FF`
    // construction. Audit: UNSAFE-2026-0025.
    let l1_table = unsafe { walk_or_alloc_table(root, l0_idx, frames, unmap)? };

    // SAFETY: `l1_table` is a valid table frame just installed (or
    // discovered) at L0[l0_idx]. Audit: UNSAFE-2026-0025.
    let l2_table = unsafe { walk_or_alloc_table(l1_table, l1_idx, frames, unmap)? };

    // SAFETY: `l2_table` is a valid table frame at L1[l1_idx].
    // Audit: UNSAFE-2026-0025.
    let l3_table = unsafe { walk_or_alloc_table(l2_table, l2_idx, frames, unmap)? };

    // L3 leaf write or clear.
    let l3_ptr = l3_table.as_usize() as *mut u64;

    // SAFETY: `l3_table` is a 4 KiB frame; `l3_idx < 512`; the offset
    // stays within the frame. Volatile access prevents the compiler
    // from reordering against the surrounding TLB invalidate (which
    // the `MapperFlush` token enforces at the call site). Audit:
    // UNSAFE-2026-0025.
    let leaf_slot = unsafe { l3_ptr.add(l3_idx) };

    if unmap {
        // SAFETY: `leaf_slot` is in-bounds; the read returns the
        // current descriptor. Audit: UNSAFE-2026-0025.
        let existing = unsafe { core::ptr::read_volatile(leaf_slot) };
        if (existing & DESC_VALID_BIT) == 0 {
            return Err(MmuError::NotMapped);
        }
        let leaf_pa = existing & PAGE_OA_MASK_L3;
        // SAFETY: clearing the descriptor invalidates it (bit 0 = 0);
        // a subsequent translation walk through this VA will fault.
        // The caller is responsible for the per-VA TLB invalidate
        // via the returned `MapperFlush`. Audit: UNSAFE-2026-0025.
        unsafe { core::ptr::write_volatile(leaf_slot, 0) };
        let leaf_frame = PhysFrame::from_aligned(PhysAddr(leaf_pa as usize))
            .ok_or(MmuError::MisalignedAddress)?;
        Ok(leaf_frame)
    } else {
        // SAFETY: `leaf_slot` is in-bounds; the read returns the
        // current descriptor. Audit: UNSAFE-2026-0025.
        let existing = unsafe { core::ptr::read_volatile(leaf_slot) };
        if (existing & DESC_VALID_BIT) != 0 {
            return Err(MmuError::AlreadyMapped);
        }
        let bits = flags_to_descriptor_bits(flags);
        let descriptor = page_descriptor(pa.as_usize() as u64, bits);
        // SAFETY: descriptor is the encoded L3 page entry per
        // `tyrne_hal::mmu::vmsav8::page_descriptor` (host-tested).
        // Audit: UNSAFE-2026-0025.
        unsafe { core::ptr::write_volatile(leaf_slot, descriptor) };
        Ok(pa)
    }
}

/// Walk one level of the page-table tree: read the descriptor at
/// `parent_table[idx]`. If it points at a next-level table, return
/// that frame. Otherwise:
///   - if `unmap`: return `MmuError::NotMapped` (no point traversing
///     into an empty branch);
///   - else: allocate a fresh frame from `frames`, zero it, install a
///     table descriptor at `parent_table[idx]`, and return the new
///     frame.
///
/// # Safety
///
/// `parent_table` must be a valid table frame and `idx` must be less
/// than [`ENTRIES_PER_TABLE`]. Audit: UNSAFE-2026-0025.
unsafe fn walk_or_alloc_table(
    parent_table: PhysFrame,
    idx: usize,
    frames: &mut dyn FrameProvider,
    unmap: bool,
) -> Result<PhysFrame, MmuError> {
    debug_assert!(idx < ENTRIES_PER_TABLE);

    let parent_ptr = parent_table.as_usize() as *mut u64;
    // SAFETY: `parent_table` is a 4 KiB frame; `idx < 512`. Audit:
    // UNSAFE-2026-0025.
    let slot_ptr = unsafe { parent_ptr.add(idx) };
    // SAFETY: in-bounds read. Audit: UNSAFE-2026-0025.
    let existing = unsafe { core::ptr::read_volatile(slot_ptr) };

    if (existing & DESC_VALID_BIT) != 0 {
        // Existing entry at L0/L1/L2. Two shapes are possible:
        //   - Table descriptor (valid + table bit set) — walk into it.
        //   - Block descriptor (valid + table bit clear) — the v1
        //     bootstrap pre-mapped this region as a 2 MiB block (or
        //     larger at L0/L1). Splitting the block into 4 KiB pages
        //     is deferred to the first B3+ caller per T-016 §Out of
        //     scope, so both `map` and `unmap` return `AlreadyMapped`
        //     here. (An `unmap` against a block-mapped region is
        //     semantically asking "remove a sub-2-MiB page" inside a
        //     block — which requires the same block-split logic;
        //     a future `MmuError::BlockMapped` variant may
        //     disambiguate when block-split lands.)
        let is_table = (existing & DESC_TABLE_OR_PAGE_BIT) != 0;
        if !is_table {
            return Err(MmuError::AlreadyMapped);
        }
        let next_pa = existing & TABLE_NLA_MASK;
        return PhysFrame::from_aligned(PhysAddr(next_pa as usize))
            .ok_or(MmuError::MisalignedAddress);
    }

    // No existing entry.
    if unmap {
        return Err(MmuError::NotMapped);
    }

    // Allocate a fresh table.
    let new_table = frames.alloc_frame().ok_or(MmuError::OutOfFrames)?;

    // SAFETY: caller's `FrameProvider` contract guarantees the frame
    // is zero-initialised when we receive it; we install the table
    // descriptor pointing at it via the host-tested `table_descriptor`
    // encoder from `tyrne_hal::mmu::vmsav8`. Audit: UNSAFE-2026-0025.
    let descriptor = table_descriptor(new_table.as_usize() as u64);
    // SAFETY: in-bounds write. Audit: UNSAFE-2026-0025.
    unsafe { core::ptr::write_volatile(slot_ptr, descriptor) };

    Ok(new_table)
}
