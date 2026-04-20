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
//! `#![cfg_attr(not(test), no_std)]` disables `std` for production builds
//! while allowing the standard test harness in host-side `cargo test` runs.
//!
//! [adr-0006]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0006-workspace-layout.md
//! [p6]: https://github.com/cemililik/UmbrixOS/blob/main/docs/standards/architectural-principles.md#p6--hal-separation
//!
//! ## Subsystems
//!
//! - [`obj`] — kernel-object subsystem (Phase A3 / [T-002]): per-type
//!   arenas holding the concrete entities capabilities name.
//! - [`cap`] — capability subsystem (Phase A2 / [T-001]), the substrate every
//!   later subsystem refers through for authority.
//!
//! [T-001]: https://github.com/cemililik/UmbrixOS/blob/main/docs/analysis/tasks/phase-a/T-001-capability-table-foundation.md
//! [T-002]: https://github.com/cemililik/UmbrixOS/blob/main/docs/analysis/tasks/phase-a/T-002-kernel-object-storage.md

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
pub mod obj;

use umbrix_hal::Console;

/// Portable kernel entry, called by the BSP after early init.
///
/// In Phase 4c v0.0.1 this writes a greeting to the console and idles
/// the CPU in a `spin_loop`. Subsequent phases will bring up the
/// scheduler, IPC, capability system, and userspace init here before
/// reaching steady state.
///
/// # Never returns
///
/// This function is `-> !`. A return would be a kernel bug; the BSP's
/// reset stub halts defensively if it ever does.
pub fn run<C: Console>(console: &C) -> ! {
    console.write_bytes(b"umbrix: hello from kernel_main\n");

    loop {
        core::hint::spin_loop();
    }
}
