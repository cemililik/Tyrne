//! PL011 UART implementation of [`tyrne_hal::Console`] for QEMU `virt`.
//!
//! See ADR-0007 (`Console` trait) and ADR-0012 (boot flow) for context.

use core::ptr::{read_volatile, write_volatile};

use tyrne_hal::Console;

/// PL011 UART driver, restricted to what [`Console`] needs.
///
/// Polling-only, TX-only, no interrupt handling, no baud-rate setup —
/// QEMU's `virt` PL011 is pre-initialized by the machine and accepts
/// bytes immediately. Real-hardware BSPs (Pi 4, Pi 5) will need an
/// initialization sequence; that is not in scope for this v1 BSP.
pub(crate) struct Pl011Uart {
    base: usize,
}

// UARTDR: data register (8-bit data in bits [7:0], receive status in high bits).
const UARTDR: usize = 0x00;
// UARTFR: flag register.
const UARTFR: usize = 0x18;
// UARTFR.TXFF: transmit FIFO full.
const UARTFR_TXFF: u32 = 1 << 5;

impl Pl011Uart {
    /// Construct a `Pl011Uart` from its MMIO base address.
    ///
    /// # Safety
    ///
    /// `base` must be the MMIO base address of a PL011-compatible UART
    /// that has been mapped for kernel access and is not concurrently
    /// owned by another subsystem. On QEMU `virt` this is
    /// `0x0900_0000`; on Pi 4 (when that BSP lands) it is the PL011
    /// secondary UART window.
    ///
    /// The caller must also ensure the UART has been initialized by
    /// firmware or an earlier kernel pass — this constructor performs
    /// no initialization.
    #[must_use]
    pub(crate) const unsafe fn new(base: usize) -> Self {
        Self { base }
    }
}

// SAFETY: PL011 MMIO is hardware-synchronized; the FIFO serializes
// writes. Send is therefore safe because the only state inside
// Pl011Uart is a base address and the hardware register window it
// names can be reached from any core.
// Audit: UNSAFE-2026-0003.
unsafe impl Send for Pl011Uart {}

// SAFETY: same reasoning as Send above — the hardware FIFO is the
// synchronization domain; concurrent writes from multiple cores may
// interleave at the byte level, which the Console contract
// (ADR-0007) accepts as best-effort behaviour.
// Audit: UNSAFE-2026-0004.
unsafe impl Sync for Pl011Uart {}

impl Console for Pl011Uart {
    fn write_bytes(&self, bytes: &[u8]) {
        for &byte in bytes {
            // SAFETY: UARTFR and UARTDR are PL011 MMIO registers at
            // fixed offsets within a window whose ownership is
            // established in Pl011Uart::new's safety contract. Reading
            // UARTFR has no side effects; writing UARTDR queues a byte
            // into the TX FIFO. Both accesses are volatile so the
            // compiler must not reorder them with surrounding memory
            // accesses.
            // Audit: UNSAFE-2026-0005.
            unsafe {
                let fr = (self.base + UARTFR) as *const u32;
                while read_volatile(fr) & UARTFR_TXFF != 0 {
                    core::hint::spin_loop();
                }
                let dr = (self.base + UARTDR) as *mut u32;
                write_volatile(dr, u32::from(byte));
            }
        }
    }
}
