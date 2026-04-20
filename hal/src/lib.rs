//! # umbrix-hal
//!
//! Trait surface that decouples the Umbrix kernel core from any specific CPU,
//! board, or peripheral. Concrete implementations live in per-board Board
//! Support Package crates named `umbrix-bsp-*`.
//!
//! This crate defines **traits only**. It contains no logic, no implementations,
//! and no hardware addresses. See [`docs/architecture/hal.md`][hal-doc] for the
//! full responsibilities of each trait and [ADR-0006][adr-0006] for the
//! crate-boundary rationale.
//!
//! [hal-doc]: https://github.com/cemililik/UmbrixOS/blob/main/docs/architecture/hal.md
//! [adr-0006]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0006-workspace-layout.md
//!
//! ## Status
//!
//! In progress. Traits are pinned down one at a time, each behind a dedicated
//! ADR. Accepted so far: [`Console`] (ADR-0007), [`Cpu`] (ADR-0008),
//! [`Mmu`] (ADR-0009), [`Timer`] (ADR-0010). The remaining trait stub below
//! is a placeholder whose method surface will be pinned by its own ADR at
//! Phase 4b implementation time.

#![no_std]

mod console;
mod cpu;
mod mmu;
mod timer;

pub use console::{Console, FmtWriter};
pub use cpu::{CoreId, Cpu, IrqGuard, IrqState};
pub use mmu::{
    FrameProvider, MappingFlags, Mmu, MmuError, PhysAddr, PhysFrame, VirtAddr, PAGE_SIZE,
};
pub use timer::Timer;

/// Interrupt controller dispatch and control.
///
/// Responsibilities: enable and disable specific `IRQ` lines, acknowledge
/// the current `IRQ` at entry, end-of-interrupt signalling, and optional
/// per-CPU routing.
///
/// Used by the kernel's minimal interrupt service routine. Drivers never see
/// this interface; they receive asynchronous notifications on their
/// `IrqCap`'s endpoint.
pub trait IrqController {}

/// System `IOMMU` interaction, on platforms that have one.
///
/// Scopes a peripheral's `DMA` to the regions granted to its driver. On
/// platforms without an `IOMMU` (for example, Raspberry Pi 4), this trait is
/// absent from the BSP or implemented as a no-op per the BSP's explicit
/// design. See
/// [`docs/architecture/security-model.md`][sec-doc] for the trust-boundary
/// implications.
///
/// [sec-doc]: https://github.com/cemililik/UmbrixOS/blob/main/docs/architecture/security-model.md
pub trait Iommu {}
