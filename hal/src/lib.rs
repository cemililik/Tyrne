//! # tyrne-hal
//!
//! Trait surface that decouples the Tyrne kernel core from any specific CPU,
//! board, or peripheral. Concrete implementations live in per-board Board
//! Support Package crates named `tyrne-bsp-*`.
//!
//! This crate defines **traits only**. It contains no logic, no implementations,
//! and no hardware addresses. See [`docs/architecture/hal.md`][hal-doc] for the
//! full responsibilities of each trait and [ADR-0006][adr-0006] for the
//! crate-boundary rationale.
//!
//! [hal-doc]: https://github.com/cemililik/TyrneOS/blob/main/docs/architecture/hal.md
//! [adr-0006]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0006-workspace-layout.md
//!
//! ## Status
//!
//! In progress. Traits are pinned down one at a time, each behind a dedicated
//! ADR. Accepted so far: [`Console`] (ADR-0007), [`Cpu`] (ADR-0008),
//! [`Mmu`] (ADR-0009), [`Timer`] (ADR-0010), [`IrqController`] (ADR-0011),
//! [`ContextSwitch`] (ADR-0020).
//! The remaining trait stub below is a placeholder whose method surface
//! will be pinned by its own ADR when a concrete caller needs it.

#![no_std]

mod console;
mod context_switch;
mod cpu;
mod irq_controller;
mod mmu;
mod timer;

pub use console::{Console, FmtWriter};
pub use context_switch::ContextSwitch;
pub use cpu::{CoreId, Cpu, IrqGuard, IrqState};
pub use irq_controller::{IrqController, IrqNumber};
pub use mmu::{
    FrameProvider, MappingFlags, Mmu, MmuError, PhysAddr, PhysFrame, VirtAddr, PAGE_SIZE,
};
pub use timer::Timer;

/// System `IOMMU` interaction, on platforms that have one.
///
/// Scopes a peripheral's `DMA` to the regions granted to its driver. On
/// platforms without an `IOMMU` (for example, Raspberry Pi 4), this trait is
/// absent from the BSP or implemented as a no-op per the BSP's explicit
/// design. See
/// [`docs/architecture/security-model.md`][sec-doc] for the trust-boundary
/// implications.
///
/// [sec-doc]: https://github.com/cemililik/TyrneOS/blob/main/docs/architecture/security-model.md
pub trait Iommu {}
