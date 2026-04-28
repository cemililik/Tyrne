//! GIC v2 driver for QEMU virt aarch64.
//!
//! Implements [`tyrne_hal::IrqController`] (per [ADR-0011]) against
//! QEMU virt's GIC v2 distributor (`0x0800_0000`) and CPU interface
//! (`0x0801_0000`). Three audit-relevant facts:
//!
//! 1. **All MMIO uses `core::ptr::read_volatile` / `write_volatile`** —
//!    no `&mut` materialised; no compiler reordering of the access
//!    sequence.
//! 2. **The constructor stores bases only** — no MMIO. Initialisation
//!    is a separate [`QemuVirtGic::init`] call so the kernel can
//!    sequence "install vector table → init GIC → unmask DAIF" in
//!    that order.
//! 3. **Single-core v1.** The GIC's per-CPU registers (banked SGI/PPI,
//!    CPU interface) only exist for one CPU; multi-core would require
//!    extending this driver with per-CPU init.
//!
//! See [`docs/architecture/exceptions.md`] §"GIC v2 driver" for the
//! full design rationale; `UNSAFE-2026-0019` for the audit-log entry
//! covering this module's MMIO surface.
//!
//! [ADR-0011]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0011-irq-controller-trait.md
//! [`docs/architecture/exceptions.md`]: https://github.com/cemililik/TyrneOS/blob/main/docs/architecture/exceptions.md

use core::ptr::{read_volatile, write_volatile};

use tyrne_hal::{IrqController, IrqNumber};

/// QEMU virt's default `GICv2` distributor MMIO base.
///
/// Source: QEMU `hw/arm/virt.c` and the `virt` machine's device tree.
pub const QEMU_VIRT_GIC_DISTRIBUTOR_BASE: usize = 0x0800_0000;

/// QEMU virt's default `GICv2` CPU interface MMIO base.
pub const QEMU_VIRT_GIC_CPU_INTERFACE_BASE: usize = 0x0801_0000;

// ── Distributor register offsets (per ARM `GICv2` architecture spec, IHI 0048B) ─

const GICD_CTLR: usize = 0x000;
const GICD_TYPER: usize = 0x004;
const GICD_ISENABLER_BASE: usize = 0x100;
const GICD_ICENABLER_BASE: usize = 0x180;
const GICD_IPRIORITYR_BASE: usize = 0x400;
const GICD_ITARGETSR_BASE: usize = 0x800;

// ── CPU interface register offsets ────────────────────────────────────────────

const GICC_CTLR: usize = 0x000;
const GICC_PMR: usize = 0x004;
const GICC_IAR: usize = 0x00C;
const GICC_EOIR: usize = 0x010;

// ── Magic constants ────────────────────────────────────────────────────────────

/// `GICv2`'s spurious-interrupt INTID. Returned by `GICC_IAR` when
/// nothing real is pending; the trait contract folds this to `None`.
const GIC_SPURIOUS_INTID: u32 = 1023;

/// Bottom 10 bits of `GICC_IAR` are the INTID; upper bits are CPU ID.
const GICC_IAR_INTID_MASK: u32 = 0x3FF;

/// SPI numbering starts at IRQ 32 (IRQs 0–15 are SGIs, 16–31 are PPIs).
const FIRST_SPI: usize = 32;

/// Default mid-priority byte (one byte per IRQ in `GICD_IPRIORITYR`).
const DEFAULT_PRIORITY_BYTE: u32 = 0xA0;

/// "Target CPU 0" byte in `GICD_ITARGETSR`. Replicated four times to
/// fill a 32-bit word.
const TARGET_CPU0_BYTE: u32 = 0x01;

/// `GICv2` architectural maximum INTID. The architecture supports up to
/// 1020 real interrupts (IDs 0..=1019); 1020..=1023 are reserved
/// (1023 = the spurious-INTID sentinel returned by `GICC_IAR`). Any
/// `enable` / `disable` call with `irq.0 >= GIC_MAX_IRQ` is a kernel
/// bug — the offset math would index into reserved or out-of-window
/// MMIO. Per ARM IHI 0048B §4.3.2.
const GIC_MAX_IRQ: u32 = 1020;

// ─── QemuVirtGic ──────────────────────────────────────────────────────────────

/// QEMU virt `GICv2` controller.
///
/// Holds two MMIO base addresses; method bodies dispatch volatile
/// reads/writes against typed offsets. See module-level docs for the
/// audit context (`UNSAFE-2026-0019`).
pub struct QemuVirtGic {
    distributor_base: usize,
    cpu_interface_base: usize,
}

impl QemuVirtGic {
    /// Construct a `GICv2` driver handle from the two MMIO bases.
    ///
    /// # Safety
    ///
    /// `distributor_base` and `cpu_interface_base` must be the actual
    /// MMIO bases of a `GICv2` controller; the windows must be
    /// exclusively owned by this kernel (single-core v1 satisfies
    /// this trivially); the windows must remain mapped/identity for
    /// the kernel's lifetime.
    ///
    /// **Why `unsafe` is required:** the constructor itself does no
    /// MMIO and cannot fault. The `unsafe` is the contract that the
    /// stored bases are valid — every later trait-method call
    /// dereferences them via volatile MMIO and trusts the contract.
    /// **Invariants upheld:** the two `usize` fields are stored
    /// verbatim; no validation is attempted (none would help — any
    /// non-zero address looks plausible). **Rejected alternatives:**
    /// taking `&'static MmioWindow` types would force a runtime
    /// memory map, which v1 does not have; returning `Result` would
    /// move the validation burden to the caller without enabling any
    /// real check.
    ///
    /// Audit: UNSAFE-2026-0019.
    #[must_use]
    pub const unsafe fn new(distributor_base: usize, cpu_interface_base: usize) -> Self {
        Self {
            distributor_base,
            cpu_interface_base,
        }
    }

    /// Run the boot-time programming sequence: disable distributor,
    /// mask all SPIs, set per-IRQ priorities, route SPIs to CPU 0,
    /// enable distributor + CPU interface.
    ///
    /// After `init` returns, the GIC is ready to deliver enabled
    /// IRQs to the CPU. No IRQ is enabled yet — callers use
    /// [`Self::enable`] to enable specific lines.
    ///
    /// # Safety
    ///
    /// Caller must ensure no IRQ is currently being serviced on this
    /// CPU (typically because it has not yet unmasked `DAIF.I`).
    /// Typically called once from `kernel_entry` before any
    /// [`Self::enable`] call.
    ///
    /// **Why `unsafe` is required:** the body performs a long
    /// sequence of MMIO writes against the distributor and CPU
    /// interface. Any one of them could fault if the bases are
    /// wrong; the caller must have upheld [`Self::new`]'s contract.
    /// **Invariants upheld:** the sequence matches `GICv2` architecture
    /// spec §4 ("Distributor / CPU interface initialisation"):
    /// disable before reconfigure, mask all SPIs, priorities + targets
    /// before enable, PMR wide-open last. **Rejected alternatives:**
    /// folding init into `new` would prevent the kernel from
    /// sequencing "install vector table first, then init GIC"; that
    /// ordering matters because if `init` faulted, the vector table
    /// is what catches the fault visibly.
    ///
    /// Audit: UNSAFE-2026-0019.
    pub unsafe fn init(&self) {
        // Step 1: Disable distributor while reconfiguring.
        // SAFETY: distributor_base + 0 is within the GIC window per
        // new()'s contract. `GICD_CTLR` is W/R; writing 0 is the
        // safe initial state. Audit: UNSAFE-2026-0019.
        unsafe { self.write_distributor(GICD_CTLR, 0) };

        // Step 2: Read GICD_TYPER to learn the IT-line count.
        // The bottom 5 bits encode (count / 32 - 1); actual line
        // count = (bits[4:0] + 1) * 32.
        // SAFETY: GICD_TYPER at offset 0x004 is RO and within the
        // window. Audit: UNSAFE-2026-0019.
        let typer = unsafe { self.read_distributor(GICD_TYPER) };
        let it_lines_field = (typer & 0x1F) as usize;
        let irq_count = (it_lines_field.saturating_add(1)).saturating_mul(32);

        // Step 3: Disable every SPI (IRQ 32..irq_count). Each
        // ICENABLER<n> covers 32 IRQs starting from IRQ (32n);
        // writing all-ones disables every line in that range.
        // n=0 covers SGI/PPI (banked) — leave alone; iterate n=1..
        let mut n: usize = 1;
        while n.saturating_mul(32) < irq_count {
            // SAFETY: GICD_ICENABLER<n> at offset 0x180 + 4*n is
            // within the window for n in [1, irq_count/32). Each
            // one-bit in the value clears the corresponding enable
            // bit. Audit: UNSAFE-2026-0019.
            unsafe {
                self.write_distributor(
                    GICD_ICENABLER_BASE.saturating_add(n.saturating_mul(4)),
                    0xFFFF_FFFF,
                );
            }
            n = n.saturating_add(1);
        }

        // Step 4: Set all SPI priorities to mid-priority. IPRIORITYR
        // is byte-addressable; writing a 32-bit word covers four
        // consecutive IRQs. Replicate DEFAULT_PRIORITY_BYTE four
        // times. Iterate over SPI byte offsets (32..irq_count) in
        // 4-byte strides.
        let priority_word = DEFAULT_PRIORITY_BYTE
            | (DEFAULT_PRIORITY_BYTE << 8)
            | (DEFAULT_PRIORITY_BYTE << 16)
            | (DEFAULT_PRIORITY_BYTE << 24);
        let mut byte_idx: usize = FIRST_SPI;
        while byte_idx < irq_count {
            // SAFETY: GICD_IPRIORITYR + byte_idx is within the
            // window for SPI bytes (>= 32, < irq_count). Writing a
            // 32-bit word at a 4-byte-aligned offset covers four
            // contiguous IRQs. Audit: UNSAFE-2026-0019.
            unsafe {
                self.write_distributor(
                    GICD_IPRIORITYR_BASE.saturating_add(byte_idx),
                    priority_word,
                );
            }
            byte_idx = byte_idx.saturating_add(4);
        }

        // Step 5: Route every SPI to CPU 0 (single-core v1).
        // ITARGETSR is byte-addressable like IPRIORITYR; writing a
        // 32-bit word covers four IRQs.
        let target_word = TARGET_CPU0_BYTE
            | (TARGET_CPU0_BYTE << 8)
            | (TARGET_CPU0_BYTE << 16)
            | (TARGET_CPU0_BYTE << 24);
        let mut byte_idx: usize = FIRST_SPI;
        while byte_idx < irq_count {
            // SAFETY: GICD_ITARGETSR + byte_idx is within the window
            // for SPI bytes (SGI/PPI ITARGETSR are RO and banked, so
            // we deliberately skip n < FIRST_SPI). Writing target
            // CPU 0 to all four bytes routes the SPI to core 0 only.
            // Audit: UNSAFE-2026-0019.
            unsafe {
                self.write_distributor(GICD_ITARGETSR_BASE.saturating_add(byte_idx), target_word);
            }
            byte_idx = byte_idx.saturating_add(4);
        }

        // Step 6: Enable distributor.
        // SAFETY: GICD_CTLR write of 1 enables Group 0 forwarding;
        // the distributor will now propagate enabled IRQs to the CPU
        // interface. Audit: UNSAFE-2026-0019.
        unsafe { self.write_distributor(GICD_CTLR, 1) };

        // Step 7: CPU interface — priority mask wide-open + enable.
        // GICC_PMR = 0xFF allows every priority through; GICC_CTLR
        // = 1 enables the CPU interface.
        // SAFETY: cpu_interface_base + offsets are within the CPU
        // interface window per new()'s contract. Audit: UNSAFE-2026-0019.
        unsafe {
            self.write_cpu_interface(GICC_PMR, 0xFF);
            self.write_cpu_interface(GICC_CTLR, 1);
        }
    }

    // ── Private MMIO helpers ──────────────────────────────────────────────

    /// Volatile read of a distributor register at byte offset `offset`.
    ///
    /// # Safety
    ///
    /// `offset` must address a 32-bit-aligned register within the
    /// distributor window. Caller upholds [`Self::new`]'s base
    /// validity. Audit: UNSAFE-2026-0019.
    unsafe fn read_distributor(&self, offset: usize) -> u32 {
        let addr = self.distributor_base.saturating_add(offset) as *const u32;
        // SAFETY: `addr` is within the distributor window per the
        // contract above. Volatile read suppresses compiler reordering
        // and elision. Audit: UNSAFE-2026-0019.
        unsafe { read_volatile(addr) }
    }

    /// Volatile write of a distributor register at byte offset `offset`.
    ///
    /// # Safety
    ///
    /// Same as [`Self::read_distributor`]. Audit: UNSAFE-2026-0019.
    unsafe fn write_distributor(&self, offset: usize, value: u32) {
        let addr = self.distributor_base.saturating_add(offset) as *mut u32;
        // SAFETY: `addr` is within the distributor window per the
        // contract above. Volatile write suppresses compiler
        // reordering and elision. Audit: UNSAFE-2026-0019.
        unsafe { write_volatile(addr, value) };
    }

    /// Volatile read of a CPU interface register at byte offset `offset`.
    ///
    /// # Safety
    ///
    /// `offset` must address a 32-bit-aligned register within the CPU
    /// interface window. Caller upholds [`Self::new`]'s base validity.
    /// Audit: UNSAFE-2026-0019.
    unsafe fn read_cpu_interface(&self, offset: usize) -> u32 {
        let addr = self.cpu_interface_base.saturating_add(offset) as *const u32;
        // SAFETY: `addr` is within the CPU interface window per the
        // contract above. Audit: UNSAFE-2026-0019.
        unsafe { read_volatile(addr) }
    }

    /// Volatile write of a CPU interface register at byte offset `offset`.
    ///
    /// # Safety
    ///
    /// Same as [`Self::read_cpu_interface`]. Audit: UNSAFE-2026-0019.
    unsafe fn write_cpu_interface(&self, offset: usize, value: u32) {
        let addr = self.cpu_interface_base.saturating_add(offset) as *mut u32;
        // SAFETY: `addr` is within the CPU interface window per the
        // contract above. Audit: UNSAFE-2026-0019.
        unsafe { write_volatile(addr, value) };
    }
}

// SAFETY: `QemuVirtGic` holds only two `usize` MMIO bases; no interior
// mutability beyond the volatile MMIO accesses themselves, which target
// per-CPU registers banked by the GIC architecture (single-core v1
// trivially satisfies the no-aliasing rule). A future SMP world needs
// per-CPU init but the trait surface stays the same. Audit: UNSAFE-2026-0019.
unsafe impl Send for QemuVirtGic {}
// SAFETY: same reasoning as `Send`.
unsafe impl Sync for QemuVirtGic {}

impl IrqController for QemuVirtGic {
    fn enable(&self, irq: IrqNumber) {
        assert!(
            irq.0 < GIC_MAX_IRQ,
            "QemuVirtGic::enable: irq.0 = {} exceeds GICv2 architectural max {}",
            irq.0,
            GIC_MAX_IRQ,
        );
        let n = irq.0 as usize;
        let reg_offset = GICD_ISENABLER_BASE.saturating_add((n / 32).saturating_mul(4));
        #[allow(
            clippy::cast_possible_truncation,
            reason = "n % 32 is in 0..32; fits trivially in u32"
        )]
        let bit = 1u32 << ((n % 32) as u32);
        // SAFETY: `irq.0 < GIC_MAX_IRQ` (1020) is enforced by the
        // assertion above; therefore `n / 32 < 32` and
        // `reg_offset = 0x100 + 4 * (n / 32)` lies in `[0x100, 0x180)`
        // — well within the distributor window. Writing a 1 bit to
        // ISENABLER is a per-bit *set* operation (writing 0 to other
        // bits has no effect), so the call is idempotent against
        // already-enabled lines. Rejected alternative: read-modify-
        // write — unnecessary because the semantics of ISENABLER are
        // write-only-bit-set. Audit: UNSAFE-2026-0019.
        unsafe { self.write_distributor(reg_offset, bit) };
    }

    fn disable(&self, irq: IrqNumber) {
        assert!(
            irq.0 < GIC_MAX_IRQ,
            "QemuVirtGic::disable: irq.0 = {} exceeds GICv2 architectural max {}",
            irq.0,
            GIC_MAX_IRQ,
        );
        let n = irq.0 as usize;
        let reg_offset = GICD_ICENABLER_BASE.saturating_add((n / 32).saturating_mul(4));
        #[allow(
            clippy::cast_possible_truncation,
            reason = "n % 32 is in 0..32; fits trivially in u32"
        )]
        let bit = 1u32 << ((n % 32) as u32);
        // SAFETY: range invariant established by the assertion above,
        // same as `enable`. `reg_offset` lies in `[0x180, 0x200)` —
        // within the distributor window. Writing a 1 bit to
        // ICENABLER is a per-bit *clear* operation; idempotent
        // against already-disabled lines. Audit: UNSAFE-2026-0019.
        unsafe { self.write_distributor(reg_offset, bit) };
    }

    fn acknowledge(&self) -> Option<IrqNumber> {
        // SAFETY: `GICC_IAR` at offset 0x00C is RO and within the CPU
        // interface window. Reading IAR has the side-effect of
        // marking the top-pending IRQ active, which is intentional
        // per the trait contract. Audit: UNSAFE-2026-0019.
        let raw = unsafe { self.read_cpu_interface(GICC_IAR) };
        let intid = raw & GICC_IAR_INTID_MASK;
        if intid == GIC_SPURIOUS_INTID {
            None
        } else {
            Some(IrqNumber(intid))
        }
    }

    fn end_of_interrupt(&self, irq: IrqNumber) {
        // SAFETY: `GICC_EOIR` at offset 0x010 is WO and within the
        // CPU interface window. Writing the active IRQ ID signals
        // end-of-interrupt; on `GICv2` this performs both Priority
        // Drop and Deactivation in one write. Audit: UNSAFE-2026-0019.
        unsafe { self.write_cpu_interface(GICC_EOIR, irq.0) };
    }
}
