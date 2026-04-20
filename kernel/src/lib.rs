//! # umbrix-kernel
//!
//! Architecture- and board-agnostic kernel core for Umbrix.
//!
//! This crate defines the capability system, scheduler, IPC primitives, memory
//! management, and interrupt dispatch. It depends on [`umbrix_hal`] for every
//! operation that touches hardware, and contains no architecture- or
//! board-specific code — see
//! [ADR-0006][adr-0006] and
//! [architectural principle P6][p6].
//!
//! Host-side unit tests wire in [`umbrix_test_hal`] as a `[dev-dependency]`.
//!
//! [adr-0006]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0006-workspace-layout.md
//! [p6]: https://github.com/cemililik/UmbrixOS/blob/main/docs/standards/architectural-principles.md#p6--hal-separation
//!
//! ## Status
//!
//! Scaffolding only. Subsystems (capabilities, scheduler, IPC, memory,
//! interrupt dispatch) land in subsequent commits per the architecture
//! documents in [`docs/architecture/`][arch-docs].
//!
//! [arch-docs]: https://github.com/cemililik/UmbrixOS/tree/main/docs/architecture

#![no_std]
// Kernel-specific stricter lints on top of the workspace set.
// See docs/standards/error-handling.md and docs/standards/unsafe-policy.md.
#![deny(clippy::panic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::todo)]
#![deny(clippy::arithmetic_side_effects)]
#![deny(clippy::float_arithmetic)]
