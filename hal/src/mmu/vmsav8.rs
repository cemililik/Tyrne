//! Pure `VMSAv8` (aarch64 stage-1) page-table descriptor encoders.
//!
//! Host-testable `const fn` helpers consumed by every aarch64 BSP that
//! implements the [`Mmu`](super::Mmu) trait — currently
//! `bsp-qemu-virt`'s `QemuVirtMmu`, in the future `bsp-pi4`'s equivalent.
//! The shape mirrors the precedent set by [`crate::timer::ticks_to_ns`]
//! (host-testable arithmetic shared across concrete implementations).
//!
//! See [ADR-0027 §Decision outcome][adr-0027] for the layout decisions
//! these encoders implement and
//! [`docs/architecture/memory-management.md` §"Page-table entry encoding"][mm-doc]
//! for the field-by-field bit map this module reifies.
//!
//! Lands with [T-016](https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-b/T-016-mmu-activation.md).
//!
//! ## Scope
//!
//! - `block_descriptor` — L2 block descriptor (2 MiB blocks; v1 bootstrap)
//! - `page_descriptor` — L3 page descriptor (4 KiB pages; post-bootstrap
//!   per-page mappings via `Mmu::map`)
//! - `table_descriptor` — L0/L1/L2 table descriptor (pointer to next-level
//!   table)
//! - `flags_to_descriptor_bits` — translates [`MappingFlags`] to the
//!   `(attr_idx, ap, sh, pxn, uxn, ng)` tuple the descriptor encoders
//!   accept
//!
//! All helpers are pure: no state, no `unsafe`, no MMIO. The functions
//! that actually write descriptors to memory (and therefore inherit a
//! safety contract) live in the BSP's `Mmu::map` / `Mmu::unmap` impl
//! and are audited under UNSAFE-2026-0025.
//!
//! [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
//! [mm-doc]: https://github.com/cemililik/Tyrne/blob/main/docs/architecture/memory-management.md

use super::MappingFlags;

// ── MAIR attribute indices ─────────────────────────────────────────────────────
//
// Pinned by ADR-0027 §Decision outcome (a). Encoded values live in MAIR_EL1
// at boot; the descriptor's `AttrIndx` field selects which 8-bit attribute
// applies to a given mapping.

/// MAIR index for **device-nGnRnE** memory (encoding `0x00`).
///
/// Used for every `MappingFlags::DEVICE` mapping (GIC + UART MMIO).
pub const ATTR_IDX_DEVICE: u8 = 0;

/// MAIR index for **normal cached** memory (encoding `0xFF` — write-back,
/// write-allocate, inner+outer shareable).
///
/// Used for every non-device mapping (RAM, including the kernel image).
pub const ATTR_IDX_NORMAL: u8 = 1;

/// `MAIR_EL1` value v1 commits to: `Attr0 = device-nGnRnE (0x00)`,
/// `Attr1 = normal cached, write-back, write-allocate (0xFF)`,
/// `Attr2..7 = reserved (0x00)`.
///
/// Per [ADR-0027 §Decision outcome (a)][adr-0027] / [`memory-management.md`
/// §"`MAIR_EL1` attribute encoding"][mm-doc].
///
/// [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
/// [mm-doc]: https://github.com/cemililik/Tyrne/blob/main/docs/architecture/memory-management.md
pub const MAIR_EL1_VALUE: u64 = 0x0000_0000_0000_FF00;

// ── Shareability + access-permission encodings ─────────────────────────────────

/// Shareability: non-shareable (used for device mappings).
pub const SH_NON_SHAREABLE: u8 = 0b00;
/// Shareability: outer shareable.
pub const SH_OUTER_SHAREABLE: u8 = 0b10;
/// Shareability: inner shareable (used for normal RAM mappings).
pub const SH_INNER_SHAREABLE: u8 = 0b11;

/// Access Permissions field encoding kernel-only / user-accessible and
/// read-only / read-write per the `VMSAv8` `AP[2:1]` table:
///
/// - `0b00` = kernel R/W, no userspace
/// - `0b01` = kernel R/W + user R/W
/// - `0b10` = kernel R/O, no userspace
/// - `0b11` = kernel R/O + user R/O
pub const AP_KERNEL_RW: u8 = 0b00;
/// Access permissions: user R/W (and kernel R/W).
pub const AP_USER_RW: u8 = 0b01;
/// Access permissions: kernel R/O.
pub const AP_KERNEL_RO: u8 = 0b10;
/// Access permissions: user R/O (and kernel R/O).
pub const AP_USER_RO: u8 = 0b11;

// ── TCR_EL1 value v1 commits to ────────────────────────────────────────────────
//
// Per ADR-0027 §Decision outcome (a) / memory-management.md §"TCR_EL1
// configuration":
//   T0SZ      = 16        (48-bit TTBR0_EL1 VA)
//   EPD0      = 0         (TTBR0_EL1 walks enabled)
//   IRGN0     = 0b01      (page-table walk: inner write-back write-allocate)
//   ORGN0     = 0b01      (page-table walk: outer write-back write-allocate)
//   SH0       = 0b11      (inner shareable for TTBR0_EL1 walks)
//   TG0       = 0b00      (4 KiB granule for TTBR0_EL1)
//   T1SZ      = 16        (mirrors T0SZ for symmetry; inert while EPD1=1)
//   EPD1      = 1         (TTBR1_EL1 walks DISABLED in v1)
//   IRGN1/ORGN1/SH1 = 0b01/0b01/0b11   (mirror; inert while EPD1=1)
//   TG1       = 0b10      (4 KiB granule for TTBR1_EL1; correct for ADR-0033 future)
//   IPS       = 0b010     (40-bit IPA — Cortex-A72 + QEMU virt)
//   AS        = 0         (8-bit ASID; v1 uses ASID=0 globally)
//   A1        = 0         (ASID lives in TTBR0_EL1.ASID; not used in v1)
//
// Composing the value:
//   T0SZ           bits  5:0   = 16
//   EPD0           bit   7     = 0
//   IRGN0          bits  9:8   = 0b01
//   ORGN0          bits 11:10  = 0b01
//   SH0            bits 13:12  = 0b11
//   TG0            bits 15:14  = 0b00
//   T1SZ           bits 21:16  = 16
//   A1             bit  22     = 0
//   EPD1           bit  23     = 1
//   IRGN1          bits 25:24  = 0b01
//   ORGN1          bits 27:26  = 0b01
//   SH1            bits 29:28  = 0b11
//   TG1            bits 31:30  = 0b10
//   IPS            bits 34:32  = 0b010
//   AS             bit  36     = 0

/// `TCR_EL1` value v1 commits to. See the comment in this module's source
/// for the field-by-field decomposition.
///
/// Per [ADR-0027 §Decision outcome (a)][adr-0027].
///
/// [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
#[allow(
    clippy::unreadable_literal,
    reason = "system-register bit-pattern; field-by-field decomposition lives in the surrounding comment"
)]
pub const TCR_EL1_VALUE: u64 = {
    let t0sz: u64 = 16;
    let epd0: u64 = 0 << 7;
    let irgn0: u64 = 0b01 << 8;
    let orgn0: u64 = 0b01 << 10;
    let sh0: u64 = 0b11 << 12;
    let tg0: u64 = 0b00 << 14;
    let t1sz: u64 = 16 << 16;
    let a1: u64 = 0 << 22;
    let epd1: u64 = 1 << 23;
    let irgn1: u64 = 0b01 << 24;
    let orgn1: u64 = 0b01 << 26;
    let sh1: u64 = 0b11 << 28;
    let tg1: u64 = 0b10 << 30;
    let ips: u64 = 0b010 << 32;
    let as_field: u64 = 0 << 36;
    t0sz | epd0
        | irgn0
        | orgn0
        | sh0
        | tg0
        | t1sz
        | a1
        | epd1
        | irgn1
        | orgn1
        | sh1
        | tg1
        | ips
        | as_field
};

/// `SCTLR_EL1` bits we **set** when activating the MMU: `M` (bit 0,
/// MMU on), `C` (bit 2, D-cache enable), `I` (bit 12, I-cache enable).
///
/// Per [ADR-0027 §Decision outcome (a)][adr-0027]. Other `SCTLR_EL1`
/// bits are read-modify-written: the bootstrap reads the current value,
/// ORs in this mask, and writes back.
///
/// [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
pub const SCTLR_EL1_MMU_ENABLE_MASK: u64 = (1 << 0) | (1 << 2) | (1 << 12);

// ── Descriptor field bit positions ─────────────────────────────────────────────

const DESC_VALID_BIT: u64 = 1 << 0;
/// Bit 1: discriminates table from block/page. The semantic *changes*
/// with level:
/// - L0/L1/L2 with bit1=1 → table descriptor (points at next level)
/// - L0/L1 with bit1=0 → block descriptor (huge page)
/// - L2 with bit1=0 → 2 MiB block descriptor
/// - L3 with bit1=1 → page descriptor (4 KiB)
/// - L3 with bit1=0 → reserved (translation fault)
const DESC_TABLE_OR_PAGE_BIT: u64 = 1 << 1;
const DESC_AF_BIT: u64 = 1 << 10;
const DESC_NG_BIT: u64 = 1 << 11;
const DESC_PXN_BIT: u64 = 1 << 53;
const DESC_UXN_BIT: u64 = 1 << 54;

// AttrIndx[2:0] occupies bits [4:2]; AP[2:1] occupies bits [7:6];
// SH[1:0] occupies bits [9:8]. NS bit 5 is left at 0 (Tyrne does not use
// TrustZone in v1).

/// Mask covering the 2 MiB-aligned output address range carried by an L2
/// block descriptor — bits `[47:21]` of the entry encode bits `[47:21]`
/// of the physical address.
pub const BLOCK_OA_MASK_L2: u64 = 0x0000_FFFF_FFE0_0000;

/// Mask covering the 4 KiB-aligned output address range carried by an L3
/// page descriptor — bits `[47:12]` of the entry encode bits `[47:12]`
/// of the physical address.
pub const PAGE_OA_MASK_L3: u64 = 0x0000_FFFF_FFFF_F000;

/// Mask covering the 4 KiB-aligned next-level-table address carried by
/// a table descriptor — same bit layout as `PAGE_OA_MASK_L3`.
pub const TABLE_NLA_MASK: u64 = 0x0000_FFFF_FFFF_F000;

// ── Translated descriptor-bits bundle ──────────────────────────────────────────

/// Decomposed descriptor fields produced by [`flags_to_descriptor_bits`].
///
/// One [`MappingFlags`] value translates to one `DescriptorBits`; the
/// caller passes the fields to the `VMSAv8` encoders
/// [`block_descriptor`] / [`page_descriptor`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DescriptorBits {
    /// `MAIR_EL1` index.
    pub attr_idx: u8,
    /// `AP[2:1]` access-permission encoding.
    pub ap: u8,
    /// `SH[1:0]` shareability encoding.
    pub sh: u8,
    /// `PXN` — privileged eXecute Never.
    pub pxn: bool,
    /// `UXN` — unprivileged eXecute Never.
    pub uxn: bool,
    /// `nG` — Not Global. v1 keeps every mapping global (`nG = 0`); the
    /// flag flips when per-task ASIDs land with ADR-0033.
    pub ng: bool,
}

/// Translate a [`MappingFlags`] value to the descriptor field tuple the
/// `VMSAv8` encoders accept.
///
/// Per the [ADR-0027 §Decision outcome (b)][adr-0027] memory-type rule
/// (`DEVICE → AttrIdx 0; !DEVICE → AttrIdx 1`) and the AP / PXN / UXN
/// table in [`memory-management.md` §"Page-table entry encoding"][mm-doc].
///
/// Locked-shut-by-default: any combination of flags whose `EXECUTE`
/// or `USER` is unset produces `PXN = 1` / `UXN = 1` for the absent
/// dimension. The kernel never executes user pages (`PXN = 1` whenever
/// `USER` is set); userspace never executes kernel pages (`UXN = 1`
/// whenever `USER` is unset). DEVICE mappings are never executable
/// (`PXN = UXN = 1`) because the v1 attack surface gains nothing from
/// MMIO execute.
///
/// [adr-0027]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0027-kernel-virtual-memory-layout.md
/// [mm-doc]: https://github.com/cemililik/Tyrne/blob/main/docs/architecture/memory-management.md
#[must_use]
pub const fn flags_to_descriptor_bits(flags: MappingFlags) -> DescriptorBits {
    let device = flags.contains(MappingFlags::DEVICE);
    let write = flags.contains(MappingFlags::WRITE);
    let execute = flags.contains(MappingFlags::EXECUTE);
    let user = flags.contains(MappingFlags::USER);
    let global = flags.contains(MappingFlags::GLOBAL);

    let attr_idx = if device {
        ATTR_IDX_DEVICE
    } else {
        ATTR_IDX_NORMAL
    };

    // Access permissions — see ADR-0027 §Decision outcome (a) AP table.
    let ap = match (user, write) {
        (false, true) => AP_KERNEL_RW,
        (false, false) => AP_KERNEL_RO,
        (true, true) => AP_USER_RW,
        (true, false) => AP_USER_RO,
    };

    // Shareability — DEVICE mappings are non-shareable (the device is the
    // synchronisation domain); normal RAM is inner-shareable for
    // SMP-readiness per ADR-0027 §Simulation §"Why DSB ISH rather than
    // DSB NSH".
    let sh = if device {
        SH_NON_SHAREABLE
    } else {
        SH_INNER_SHAREABLE
    };

    // Execute-never: locked-shut-by-default.
    //
    // - DEVICE   : PXN=1, UXN=1   (MMIO is never executable)
    // - kernel-X : PXN=0, UXN=1   (kernel can execute, user cannot)
    // - user-X   : PXN=1, UXN=0   (user can execute, kernel cannot)
    // - non-X    : PXN=1, UXN=1   (no execute either side)
    let (pxn, uxn) = if device {
        (true, true)
    } else if execute && !user {
        (false, true)
    } else if execute && user {
        (true, false)
    } else {
        (true, true)
    };

    // nG (not global) is the inverse of MappingFlags::GLOBAL.
    let ng = !global;

    DescriptorBits {
        attr_idx,
        ap,
        sh,
        pxn,
        uxn,
        ng,
    }
}

// ── Descriptor encoders ────────────────────────────────────────────────────────

/// Encode an L2 block descriptor (2 MiB block at level 2 with 4 KiB
/// granule) per ARM ARM §D5.3.
///
/// `pa` must be 2 MiB-aligned (bottom 21 bits zero); behaviour for
/// unaligned inputs is to mask the address into the OA field, dropping
/// the low bits — callers are expected to validate alignment upstream
/// via [`crate::PhysFrame`] or equivalent.
#[must_use]
pub const fn block_descriptor(pa: u64, bits: DescriptorBits) -> u64 {
    DESC_VALID_BIT
        // bit 1 = 0 (block, not table)
        | ((bits.attr_idx as u64) & 0x7) << 2
        // bit 5 (NS) = 0
        | ((bits.ap as u64) & 0x3) << 6
        | ((bits.sh as u64) & 0x3) << 8
        | DESC_AF_BIT
        | (if bits.ng { DESC_NG_BIT } else { 0 })
        | (pa & BLOCK_OA_MASK_L2)
        | (if bits.pxn { DESC_PXN_BIT } else { 0 })
        | (if bits.uxn { DESC_UXN_BIT } else { 0 })
}

/// Encode an L3 page descriptor (4 KiB page) per ARM ARM §D5.3.
///
/// `pa` must be 4 KiB-aligned. The encoding is identical to
/// [`block_descriptor`] except for bit 1 (which is 1 for an L3 page,
/// 0 for an L2 block) and the OA mask (which uses bits `[47:12]`).
#[must_use]
pub const fn page_descriptor(pa: u64, bits: DescriptorBits) -> u64 {
    DESC_VALID_BIT
        | DESC_TABLE_OR_PAGE_BIT
        | ((bits.attr_idx as u64) & 0x7) << 2
        | ((bits.ap as u64) & 0x3) << 6
        | ((bits.sh as u64) & 0x3) << 8
        | DESC_AF_BIT
        | (if bits.ng { DESC_NG_BIT } else { 0 })
        | (pa & PAGE_OA_MASK_L3)
        | (if bits.pxn { DESC_PXN_BIT } else { 0 })
        | (if bits.uxn { DESC_UXN_BIT } else { 0 })
}

/// Encode a table descriptor (L0/L1/L2) pointing at a next-level
/// translation table at physical address `next_level_pa`.
///
/// `next_level_pa` must be 4 KiB-aligned. Table descriptors have no
/// `AttrIdx` / `AP` / `SH` / `PXN` / `UXN` — those live in the leaf
/// descriptors (block at L1/L2 or page at L3). v1 leaves all four
/// `APTable` / `XNTable` / `PXNTable` / `NSTable` override bits clear
/// (they would further restrict the leaves below; v1 has no use for
/// the layered override).
#[must_use]
pub const fn table_descriptor(next_level_pa: u64) -> u64 {
    DESC_VALID_BIT | DESC_TABLE_OR_PAGE_BIT | (next_level_pa & TABLE_NLA_MASK)
}

#[cfg(test)]
mod tests {
    use super::{
        block_descriptor, flags_to_descriptor_bits, page_descriptor, table_descriptor,
        AP_KERNEL_RO, AP_KERNEL_RW, AP_USER_RO, AP_USER_RW, ATTR_IDX_DEVICE, ATTR_IDX_NORMAL,
        MAIR_EL1_VALUE, SCTLR_EL1_MMU_ENABLE_MASK, SH_INNER_SHAREABLE, SH_NON_SHAREABLE,
        TCR_EL1_VALUE,
    };
    use crate::MappingFlags;

    // ── Translation-table-format constants ────────────────────────────────────

    #[test]
    fn mair_value_attr0_device_attr1_normal_others_zero() {
        // Attr0 (bits 7:0) = 0x00 device-nGnRnE
        assert_eq!(MAIR_EL1_VALUE & 0xFF, 0x00);
        // Attr1 (bits 15:8) = 0xFF normal cached WB-WA
        assert_eq!((MAIR_EL1_VALUE >> 8) & 0xFF, 0xFF);
        // Attr2..7 reserved zero
        assert_eq!(MAIR_EL1_VALUE >> 16, 0);
    }

    #[test]
    fn tcr_value_carries_t0sz_16_and_ips_2_and_epd1_set() {
        // T0SZ at bits 5:0
        assert_eq!(TCR_EL1_VALUE & 0x3F, 16);
        // T1SZ at bits 21:16
        assert_eq!((TCR_EL1_VALUE >> 16) & 0x3F, 16);
        // IPS at bits 34:32 — 0b010 (40-bit)
        assert_eq!((TCR_EL1_VALUE >> 32) & 0x7, 0b010);
        // EPD1 set (bit 23) — TTBR1 walks disabled in v1
        assert_eq!((TCR_EL1_VALUE >> 23) & 0x1, 1);
        // EPD0 clear (bit 7) — TTBR0 walks enabled
        assert_eq!((TCR_EL1_VALUE >> 7) & 0x1, 0);
        // SH0 = 0b11 (inner shareable) at bits 13:12
        assert_eq!((TCR_EL1_VALUE >> 12) & 0x3, 0b11);
        // TG0 = 0b00 (4 KiB granule) at bits 15:14
        assert_eq!((TCR_EL1_VALUE >> 14) & 0x3, 0b00);
        // TG1 = 0b10 (4 KiB granule) at bits 31:30
        assert_eq!((TCR_EL1_VALUE >> 30) & 0x3, 0b10);
    }

    #[test]
    fn sctlr_mmu_enable_mask_sets_m_c_i_only() {
        assert_eq!(SCTLR_EL1_MMU_ENABLE_MASK & (1 << 0), 1 << 0); // M
        assert_eq!(SCTLR_EL1_MMU_ENABLE_MASK & (1 << 2), 1 << 2); // C
        assert_eq!(SCTLR_EL1_MMU_ENABLE_MASK & (1 << 12), 1 << 12); // I
                                                                    // No other bits set
        assert_eq!(
            SCTLR_EL1_MMU_ENABLE_MASK,
            (1 << 0) | (1 << 2) | (1 << 12),
            "only M, C, I should be in the OR mask"
        );
    }

    // ── flags_to_descriptor_bits ───────────────────────────────────────────────

    #[test]
    fn empty_flags_kernel_ro_normal_no_execute_global_inverted() {
        let bits = flags_to_descriptor_bits(MappingFlags::empty());
        assert_eq!(bits.attr_idx, ATTR_IDX_NORMAL);
        assert_eq!(bits.ap, AP_KERNEL_RO);
        assert_eq!(bits.sh, SH_INNER_SHAREABLE);
        assert!(bits.pxn, "non-X mapping must have PXN=1");
        assert!(bits.uxn, "non-X mapping must have UXN=1");
        assert!(bits.ng, "GLOBAL not set → nG=1");
    }

    #[test]
    fn write_alone_yields_kernel_rw_no_execute() {
        let bits = flags_to_descriptor_bits(MappingFlags::WRITE);
        assert_eq!(bits.ap, AP_KERNEL_RW);
        assert!(bits.pxn);
        assert!(bits.uxn);
    }

    #[test]
    fn write_plus_execute_yields_kernel_rwx_uxn_pxn_zero() {
        let bits = flags_to_descriptor_bits(MappingFlags::WRITE | MappingFlags::EXECUTE);
        assert_eq!(bits.ap, AP_KERNEL_RW);
        assert!(!bits.pxn, "kernel-X must have PXN=0");
        assert!(bits.uxn, "kernel-X mapping must keep UXN=1");
    }

    #[test]
    fn user_write_yields_user_rw_no_execute() {
        let bits = flags_to_descriptor_bits(MappingFlags::WRITE | MappingFlags::USER);
        assert_eq!(bits.ap, AP_USER_RW);
        assert!(bits.pxn, "non-X user mapping must have PXN=1");
        assert!(bits.uxn, "non-X user mapping must have UXN=1");
    }

    #[test]
    fn user_execute_yields_user_ro_pxn_one_uxn_zero() {
        let bits = flags_to_descriptor_bits(MappingFlags::USER | MappingFlags::EXECUTE);
        assert_eq!(bits.ap, AP_USER_RO);
        assert!(bits.pxn, "user-X kernel-PXN must be 1");
        assert!(!bits.uxn, "user-X mapping must have UXN=0");
    }

    #[test]
    fn device_flag_picks_device_attr_index() {
        let bits = flags_to_descriptor_bits(MappingFlags::DEVICE | MappingFlags::WRITE);
        assert_eq!(bits.attr_idx, ATTR_IDX_DEVICE);
        assert_eq!(bits.sh, SH_NON_SHAREABLE);
        assert!(bits.pxn);
        assert!(bits.uxn);
    }

    #[test]
    fn global_flag_clears_ng_bit() {
        let bits = flags_to_descriptor_bits(MappingFlags::WRITE | MappingFlags::GLOBAL);
        assert!(!bits.ng, "GLOBAL set → nG=0 (entry is global across ASIDs)");
    }

    // ── block_descriptor ───────────────────────────────────────────────────────

    #[test]
    fn block_descriptor_v1_kernel_ram_block_encoding() {
        // 2 MiB block at PA 0x4020_0000 (an aligned address inside the
        // v1 RAM range), kernel R/W/X, normal cached, inner shareable, AF=1, global.
        let bits = flags_to_descriptor_bits(
            MappingFlags::WRITE | MappingFlags::EXECUTE | MappingFlags::GLOBAL,
        );
        let entry = block_descriptor(0x4020_0000, bits);

        // Bit 0 (V) = 1
        assert_eq!(entry & 0x1, 1, "V");
        // Bit 1 (block-vs-table at L2) = 0
        assert_eq!((entry >> 1) & 0x1, 0, "block bit");
        // AttrIdx = 1 (normal)
        assert_eq!((entry >> 2) & 0x7, u64::from(ATTR_IDX_NORMAL), "AttrIdx");
        // AP = 0b00 (kernel R/W)
        assert_eq!((entry >> 6) & 0x3, u64::from(AP_KERNEL_RW), "AP");
        // SH = 0b11 (inner shareable)
        assert_eq!((entry >> 8) & 0x3, u64::from(SH_INNER_SHAREABLE), "SH");
        // AF = 1
        assert_eq!((entry >> 10) & 0x1, 1, "AF");
        // nG = 0 (GLOBAL set → not-not-global)
        assert_eq!((entry >> 11) & 0x1, 0, "nG");
        // OA bits [47:21] = PA bits [47:21]
        assert_eq!(entry & 0x0000_FFFF_FFE0_0000, 0x4020_0000, "OA");
        // PXN = 0 (kernel-X)
        assert_eq!((entry >> 53) & 0x1, 0, "PXN");
        // UXN = 1 (locked-shut for user)
        assert_eq!((entry >> 54) & 0x1, 1, "UXN");
    }

    #[test]
    fn block_descriptor_v1_device_block_encoding() {
        let bits = flags_to_descriptor_bits(
            MappingFlags::DEVICE | MappingFlags::WRITE | MappingFlags::GLOBAL,
        );
        // GIC distributor at 0x0800_0000 — first device 2 MiB block in v1.
        let entry = block_descriptor(0x0800_0000, bits);
        assert_eq!(entry & 0x1, 1, "V");
        assert_eq!((entry >> 2) & 0x7, u64::from(ATTR_IDX_DEVICE));
        assert_eq!((entry >> 8) & 0x3, u64::from(SH_NON_SHAREABLE));
        assert_eq!(entry & 0x0000_FFFF_FFE0_0000, 0x0800_0000);
        assert_eq!((entry >> 53) & 0x1, 1, "device PXN");
        assert_eq!((entry >> 54) & 0x1, 1, "device UXN");
    }

    #[test]
    fn block_descriptor_drops_low_bits_for_unaligned_pa() {
        // Caller is expected to pass aligned PAs; the encoder masks off
        // the low 21 bits regardless. This is the "garbage in, garbage
        // out" boundary contract — alignment validation lives upstream.
        let bits = flags_to_descriptor_bits(
            MappingFlags::WRITE | MappingFlags::EXECUTE | MappingFlags::GLOBAL,
        );
        let entry = block_descriptor(0x4020_1234, bits);
        assert_eq!(entry & 0x0000_FFFF_FFE0_0000, 0x4020_0000);
    }

    // ── page_descriptor ────────────────────────────────────────────────────────

    #[test]
    fn page_descriptor_v1_kernel_rw_page_encoding() {
        let bits = flags_to_descriptor_bits(MappingFlags::WRITE | MappingFlags::GLOBAL);
        let entry = page_descriptor(0x4040_1000, bits);
        assert_eq!(entry & 0x1, 1, "V");
        // L3 page descriptor has bit 1 = 1 (distinguishes from L3 reserved)
        assert_eq!((entry >> 1) & 0x1, 1, "page bit");
        assert_eq!((entry >> 2) & 0x7, u64::from(ATTR_IDX_NORMAL));
        assert_eq!((entry >> 6) & 0x3, u64::from(AP_KERNEL_RW));
        assert_eq!((entry >> 10) & 0x1, 1, "AF");
        assert_eq!((entry >> 11) & 0x1, 0, "nG (GLOBAL was set)");
        // OA [47:12] = PA [47:12]
        assert_eq!(entry & 0x0000_FFFF_FFFF_F000, 0x4040_1000);
    }

    #[test]
    fn page_descriptor_drops_low_bits_for_unaligned_pa() {
        let bits = flags_to_descriptor_bits(MappingFlags::WRITE | MappingFlags::GLOBAL);
        let entry = page_descriptor(0x4040_1ABC, bits);
        assert_eq!(entry & 0x0000_FFFF_FFFF_F000, 0x4040_1000);
    }

    // ── table_descriptor ───────────────────────────────────────────────────────

    #[test]
    fn table_descriptor_carries_valid_and_table_bits_and_address() {
        let entry = table_descriptor(0x4007_F000);
        assert_eq!(entry & 0x1, 1, "V");
        assert_eq!((entry >> 1) & 0x1, 1, "table bit");
        // No AP/AttrIdx/SH on table descriptors — bits [11:2] should be zero
        assert_eq!((entry >> 2) & 0x3FF, 0);
        // Next-level address [47:12]
        assert_eq!(entry & 0x0000_FFFF_FFFF_F000, 0x4007_F000);
        // No PXN / UXN on table descriptors
        assert_eq!((entry >> 53) & 0x3, 0);
    }

    #[test]
    fn table_descriptor_drops_low_bits_for_unaligned_address() {
        let entry = table_descriptor(0x4007_FABC);
        assert_eq!(entry & 0x0000_FFFF_FFFF_F000, 0x4007_F000);
    }
}
