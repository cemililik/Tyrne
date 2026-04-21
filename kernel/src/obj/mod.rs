//! Kernel-object subsystem.
//!
//! Every capability points at a kernel object. This module owns the
//! object types ([`Task`], [`Endpoint`], [`Notification`]), their typed
//! handles ([`TaskHandle`], [`EndpointHandle`], [`NotificationHandle`]),
//! their per-type arenas, and the create / destroy APIs that produce
//! and consume them.
//!
//! The storage shape is pinned in [ADR-0016][adr-0016]: per-type
//! fixed-size-block arenas, generation-tagged typed handles, global
//! ownership, zero `unsafe`. Rationale is unchanged from the capability
//! table ([ADR-0014][adr-0014]); [`Arena`] is the audited pattern
//! generalised and instantiated three times.
//!
//! [adr-0014]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0014-capability-representation.md
//! [adr-0016]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0016-kernel-object-storage.md
//!
//! ## Status (v1, T-002)
//!
//! - Three kinds: [`Task`], [`Endpoint`], [`Notification`]. `MemoryRegion`
//!   is deferred to Phase B.
//! - Typed handles prevent cross-kind confusion at compile time.
//! - Lifecycle is explicit destruction; a reachability check against a
//!   given set of capability tables is available through
//!   [`crate::cap::CapabilityTable::references_object`] but is *not*
//!   automatically performed by the destroy functions. Callers that
//!   need the check wire it in at their call site; a successor ADR will
//!   bundle it when the kernel owns a registry of tables.
//! - All v1 kernel-object code is safe Rust.

pub mod arena;
pub mod endpoint;
pub mod notification;
pub mod task;

pub use endpoint::{Endpoint, EndpointArena, EndpointHandle};
pub use notification::{Notification, NotificationArena, NotificationHandle};
pub use task::{Task, TaskArena, TaskHandle};

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
