//! Kernel-object subsystem.
//!
//! Every capability points at a kernel object. This module owns the
//! three v1 object types ([`Task`], [`Endpoint`], [`Notification`]),
//! their typed handles ([`TaskHandle`], [`EndpointHandle`],
//! [`NotificationHandle`]), their per-type arenas, and the create /
//! destroy APIs that produce and consume them. It also hosts the
//! task-loader surface ([`task_loader::load_image`] +
//! [`LoadedImage`] + [`LoadError`]) — added in T-019 (B4) per
//! [ADR-0029][adr-0029] to compose `Pmm` / `AddressSpace` / cap-table
//! into a single state machine that turns an embedded raw-flat
//! userspace binary into a populated address space.
//!
//! The storage shape for the three object types is pinned in
//! [ADR-0016][adr-0016]: per-type fixed-size-block arenas,
//! generation-tagged typed handles, global ownership. Rationale is
//! unchanged from the capability table ([ADR-0014][adr-0014]);
//! [`Arena`][arena::Arena] is the audited pattern generalised and
//! instantiated three times.
//!
//! [adr-0014]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0014-capability-representation.md
//! [adr-0016]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0016-kernel-object-storage.md
//! [adr-0029]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0029-initial-userspace-image-format.md
//!
//! ## Public surface (v1)
//!
//! Four exported families, all under `crate::obj`:
//!
//! - **Endpoint** — [`Endpoint`], [`EndpointArena`], [`EndpointHandle`]
//!   (IPC endpoint kernel-object; T-002).
//! - **Notification** — [`Notification`], [`NotificationArena`],
//!   [`NotificationHandle`] (asynchronous notification; T-002).
//! - **Task** — [`Task`], [`TaskArena`], [`TaskHandle`] (per-task
//!   kernel-object; T-002).
//! - **Task loader** — [`task_loader::load_image`] +
//!   [`LoadedImage`] + [`LoadError`] (T-019 / ADR-0029; *loader half*
//!   of B4 — produces a `LoadedImage` descriptor, **not** a runnable
//!   `CapHandle{CapObject::Task(...)}`; runnability gates on
//!   B5/B6 per phase-b §B4 §Revision-notes).
//!
//! ## Status (v1, T-002 + T-019)
//!
//! - Three object kinds: [`Task`], [`Endpoint`], [`Notification`].
//!   `MemoryRegion` is deferred to a B5+ ADR.
//! - Typed handles prevent cross-kind confusion at compile time.
//! - Lifecycle is explicit destruction; a reachability check against a
//!   given set of capability tables is available through
//!   [`crate::cap::CapabilityTable::references_object`] but is *not*
//!   automatically performed by the destroy functions. Callers that
//!   need the check wire it in at their call site; a successor ADR will
//!   bundle it when the kernel owns a registry of tables.
//! - The arenas + create/destroy/get APIs in [`endpoint`], [`task`],
//!   [`notification`], and [`arena`] are 100% safe Rust. The
//!   [`task_loader`] module introduces **one** audited `unsafe` block
//!   for the `copy_nonoverlapping` byte-copy from the embedded image
//!   slice into a freshly-PMM-allocated frame — covered by
//!   [UNSAFE-2026-0027][unsafe-27]. No other `unsafe` lives in this
//!   subsystem.
//!
//! [unsafe-27]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md

pub mod arena;
pub mod endpoint;
pub mod notification;
pub mod task;
pub mod task_loader;

pub use endpoint::{Endpoint, EndpointArena, EndpointHandle};
pub use notification::{Notification, NotificationArena, NotificationHandle};
pub use task::{Task, TaskArena, TaskHandle};
pub use task_loader::{LoadError, LoadedImage};

/// Compile-time bound on the number of live `Task` kernel objects.
/// Conservatively small for v1; revisit when a real deployment asks
/// for more.
pub const TASK_ARENA_CAPACITY: usize = 16;

/// Compile-time bound on the number of live `Endpoint` kernel objects.
pub const ENDPOINT_ARENA_CAPACITY: usize = 16;

/// Compile-time bound on the number of live `Notification` kernel objects.
pub const NOTIFICATION_ARENA_CAPACITY: usize = 16;

/// Errors returned by kernel-object operations.
///
/// `#[non_exhaustive]` so that variants added as new kinds land are not
/// breaking changes to matches outside the crate.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ObjError {
    /// The arena of this kind is full; no free slot.
    ArenaFull,
    /// The handle does not name a live slot — either never allocated,
    /// already freed, or stale after reuse.
    InvalidHandle,
    /// Returned by callers that enforce the reachability invariant: at
    /// least one capability table still names the object. The `destroy_*`
    /// functions themselves do not walk tables; callers check via
    /// [`crate::cap::CapabilityTable::references_object`] and return this
    /// variant when any table still names the handle.
    StillReachable,
}
