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
//! Scaffolding only. The traits below are placeholders whose method surfaces
//! will be pinned down — each in its own ADR — during Phase 4b implementation.

#![no_std]

/// Privileged CPU state and control.
///
/// Implementations are architecture-specific (aarch64 for the initial BSPs;
/// RISC-V planned). Responsibilities: core identification, CPU-level interrupt
/// masking, wait-for-interrupt, context-switch primitives, memory barriers that
/// Rust's atomics do not cover, and secondary-core start via PSCI or an
/// architecturally equivalent mechanism.
///
/// Final method signatures to be settled in a dedicated ADR at Phase 4b
/// implementation time.
pub trait Cpu {}

/// Memory management unit interaction.
///
/// Responsibilities: translation-table activation, entry installation and
/// removal, TLB invalidation (per-ASID, global, per-address), and the cache
/// maintenance sequences the architecture requires between page-table writes
/// and MMU reads.
///
/// Memory allocation for page tables is not the HAL's job — the kernel owns
/// a physical-frame allocator and hands the HAL frames to fill in.
pub trait Mmu {}

/// Interrupt controller dispatch and control.
///
/// Responsibilities: enable and disable specific IRQ lines, acknowledge the
/// current IRQ at entry, end-of-interrupt signalling, and optional per-CPU
/// routing.
///
/// Used by the kernel's minimal interrupt service routine. Drivers never see
/// this interface; they receive asynchronous notifications on their `IrqCap`'s
/// endpoint.
pub trait IrqController {}

/// Monotonic time and deadline arming.
///
/// Responsibilities: report nanoseconds since boot (monotonic, never goes
/// backwards across suspend), arm a one-shot deadline that arrives as an IRQ,
/// and cancel a deadline.
pub trait Timer {}

/// Byte-sink console for early boot and panic diagnostics.
///
/// Writes are synchronous. No formatting, no levels, no buffering — those
/// responsibilities belong to the userspace log service. This trait exists so
/// that diagnostic output is possible before userspace is up and during
/// panic-time when nothing else can be trusted.
pub trait Console {}

/// System IOMMU interaction, on platforms that have one.
///
/// Scopes a peripheral's DMA to the regions granted to its driver. On
/// platforms without an IOMMU (for example, Raspberry Pi 4), this trait is
/// absent from the BSP or implemented as a no-op per the BSP's explicit
/// design. See
/// [`docs/architecture/security-model.md`][sec-doc] for the trust-boundary
/// implications.
///
/// [sec-doc]: https://github.com/cemililik/UmbrixOS/blob/main/docs/architecture/security-model.md
pub trait Iommu {}
