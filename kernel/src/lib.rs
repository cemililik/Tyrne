//! # tyrne-kernel
//!
//! Architecture- and board-agnostic kernel core for Tyrne.
//!
//! This crate defines the capability system, scheduler, IPC primitives, memory
//! management, and interrupt dispatch. It depends on [`tyrne_hal`] for every
//! operation that touches hardware, and contains no architecture- or
//! board-specific code — see
//! [ADR-0006][adr-0006] and
//! [architectural principle P6][p6].
//!
//! Host-side unit tests wire in [`tyrne_test_hal`] as a `[dev-dependency]`.
//! `#![cfg_attr(not(test), no_std)]` disables `std` for production builds
//! while allowing the standard test harness in host-side `cargo test` runs.
//!
//! [adr-0006]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0006-workspace-layout.md
//! [p6]: https://github.com/cemililik/TyrneOS/blob/main/docs/standards/architectural-principles.md#p6--hal-separation
//!
//! ## Subsystems
//!
//! - [`obj`] — kernel-object subsystem (Phase A3 / [T-002]): per-type
//!   arenas holding the concrete entities that capabilities name.
//! - [`cap`] — capability subsystem (Phase A2 / [T-001]), the substrate every
//!   later subsystem refers through for authority.
//! - [`ipc`] — IPC subsystem (Phase A4 / [T-003]): `send` / `recv` / `notify`
//!   primitives over the A3 kernel objects, gated by capabilities.
//! - [`sched`] — cooperative scheduler (Phase A5 / [T-004]): bounded FIFO
//!   ready queue, per-task state, and IPC bridge.
//!
//! [T-001]: https://github.com/cemililik/TyrneOS/blob/main/docs/analysis/tasks/phase-a/T-001-capability-table-foundation.md
//! [T-002]: https://github.com/cemililik/TyrneOS/blob/main/docs/analysis/tasks/phase-a/T-002-kernel-object-storage.md
//! [T-003]: https://github.com/cemililik/TyrneOS/blob/main/docs/analysis/tasks/phase-a/T-003-ipc-primitives.md
//! [T-004]: https://github.com/cemililik/TyrneOS/blob/main/docs/analysis/tasks/phase-a/T-004-cooperative-scheduler.md

#![cfg_attr(not(test), no_std)]
// Kernel-specific stricter lints on top of the workspace set.
// See docs/standards/error-handling.md and docs/standards/unsafe-policy.md.
#![deny(clippy::panic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::todo)]
#![deny(clippy::arithmetic_side_effects)]
#![deny(clippy::float_arithmetic)]

pub mod cap;
pub mod ipc;
pub mod obj;
pub mod sched;
