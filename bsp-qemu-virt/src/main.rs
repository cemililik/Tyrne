//! # umbrix-bsp-qemu-virt
//!
//! Board Support Package for QEMU's aarch64 `virt` machine — the primary
//! development target per [ADR-0004][adr-0004] and the BSP that every
//! Umbrix feature is first exercised against.
//!
//! This crate is the bootable binary: it provides the reset vector
//! (`_start`, assembled from `boot.s` via [`core::arch::global_asm!`]),
//! the Rust entry `kernel_entry`, a panic handler, and the hardware
//! implementations of the HAL traits (currently only
//! [`umbrix_hal::Console`] via [`console::Pl011Uart`]; the remaining
//! trait implementations follow in later phases).
//!
//! The boot flow is documented in [`docs/architecture/boot.md`][boot-doc]
//! and the memory-layout decisions in [ADR-0012][adr-0012].
//!
//! [adr-0004]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0004-target-platforms.md
//! [adr-0012]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0012-boot-flow-qemu-virt.md
//! [boot-doc]: https://github.com/cemililik/UmbrixOS/blob/main/docs/architecture/boot.md

#![no_std]
#![no_main]

use core::arch::global_asm;
use core::fmt::Write;
use core::panic::PanicInfo;

use umbrix_hal::{Console, FmtWriter};

mod console;

use console::Pl011Uart;

/// MMIO base of the QEMU `virt` machine's PL011 UART.
///
/// Hardcoded per [ADR-0012][adr-0012]; each BSP carries its own
/// peripheral addresses. QEMU `virt` has exposed this address across
/// all versions the project targets.
///
/// [adr-0012]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0012-boot-flow-qemu-virt.md
const PL011_UART_BASE: usize = 0x0900_0000;

// Reset entry (`_start`). See `boot.s` and `docs/architecture/boot.md`.
global_asm!(include_str!("boot.s"));

/// First Rust entry after the assembly stub.
///
/// Constructs the BSP's concrete HAL implementations and hands the
/// portable kernel its console. This function never returns; the boot
/// stub halts defensively if it somehow does.
#[unsafe(no_mangle)]
pub extern "C" fn kernel_entry() -> ! {
    // SAFETY: 0x0900_0000 is the well-known QEMU virt PL011 UART MMIO
    // base, exclusively owned by this kernel in v1 (single-core, no
    // concurrent drivers). Alignment and addressability of the register
    // window are guaranteed by the machine.
    // Audit: UNSAFE-2026-0001.
    let console = unsafe { Pl011Uart::new(PL011_UART_BASE) };

    umbrix_kernel::run(&console)
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // SAFETY: constructing a fresh Pl011Uart in the panic path is
    // best-effort diagnostic output. If the original instance is still
    // reachable in some caller, writes may interleave at the FIFO —
    // acceptable per the Console contract (ADR-0007). The UART MMIO
    // window itself is the same one kernel_entry uses.
    // Audit: UNSAFE-2026-0002.
    let console = unsafe { Pl011Uart::new(PL011_UART_BASE) };

    console.write_bytes(b"\n!! umbrix panic !!\n");
    let mut w = FmtWriter(&console);
    // The Result is infallible for FmtWriter (ADR-0007) but the clippy
    // lint `write_literal` wants the outcome handled explicitly.
    let _ = writeln!(w, "{info}");

    loop {
        core::hint::spin_loop();
    }
}
