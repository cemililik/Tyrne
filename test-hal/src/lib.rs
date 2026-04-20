//! # umbrix-test-hal
//!
//! Deterministic fake implementations of [`umbrix_hal`] traits for host-side
//! unit tests of [`umbrix_kernel`]. Not intended to run on any real hardware
//! and not linked into any kernel image; this crate exists so that kernel
//! logic can be exercised on a developer's laptop via `cargo test -p
//! umbrix-kernel`.
//!
//! See [ADR-0006][adr-0006] for the crate-layout rationale and
//! [`docs/standards/testing.md`][testing-doc] for the test discipline this
//! supports.
//!
//! [adr-0006]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0006-workspace-layout.md
//! [testing-doc]: https://github.com/cemililik/UmbrixOS/blob/main/docs/standards/testing.md
//!
//! ## Status
//!
//! In progress. Fakes are added alongside the HAL traits they mirror, each
//! after its trait's ADR is accepted. Available so far: [`FakeConsole`]
//! (ADR-0007), [`FakeCpu`] (ADR-0008). Additional fakes land as their traits
//! are pinned down.

mod console;
mod cpu;

pub use console::FakeConsole;
pub use cpu::FakeCpu;
