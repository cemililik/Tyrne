//! # umbrix-bsp-qemu-virt
//!
//! Board Support Package for QEMU's aarch64 `virt` machine — the primary
//! development target per [ADR-0004][adr-0004] and the BSP that every
//! Umbrix feature is first exercised against under CI.
//!
//! When complete, this crate will provide:
//!
//! - Reset vector and early-init (assembly stub plus Rust early-init).
//! - HAL implementations:
//!   [`umbrix_hal::Cpu`], [`umbrix_hal::Mmu`],
//!   [`umbrix_hal::IrqController`] (GICv3),
//!   [`umbrix_hal::Timer`] (ARM generic timer),
//!   [`umbrix_hal::Console`] (PL011 UART at `0x0900_0000`),
//!   [`umbrix_hal::Iommu`] (SMMUv3, used by CI).
//! - Board constants: RAM base at `0x4000_0000`, GICv3 distributor / redistributor
//!   layout, PL011 UART base.
//! - Linker script.
//! - The `kernel_main` entry point that wires [`umbrix_kernel`] into a bootable
//!   image.
//!
//! [adr-0004]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0004-target-platforms.md
//!
//! ## Status
//!
//! Scaffolding only. Content arrives in Phase 4b. This crate currently compiles
//! as a library so that the workspace builds; the `[[bin]]` target and the
//! reset vector are added when the first bootable kernel image is written.

#![no_std]
