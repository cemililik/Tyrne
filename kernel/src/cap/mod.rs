//! Capability subsystem.
//!
//! Every privileged action in Tyrne requires the caller to hold a
//! capability that authorizes it. A capability is an unforgeable,
//! move-only kernel-held token, referenced from userspace (eventually)
//! and from the kernel's own code (now) through an opaque handle.
//!
//! The representation — index-based arena, generation-tagged handles,
//! explicit derivation tree, cascading revocation — is pinned in
//! [ADR-0014][adr-0014]. The architectural role of capabilities lives in
//! [`security-model.md`][sec] and [architectural principle P1][p1].
//!
//! [adr-0014]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0014-capability-representation.md
//! [adr-0016]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0016-kernel-object-storage.md
//! [sec]: https://github.com/cemililik/TyrneOS/blob/main/docs/architecture/security-model.md
//! [p1]: https://github.com/cemililik/TyrneOS/blob/main/docs/standards/architectural-principles.md#p1--no-ambient-authority
//!
//! ## Status (T-001 + T-002)
//!
//! - [`Capability`] is move-only (not `Copy`, not `Clone`).
//! - [`CapRights`] carries four v1 rights (`DUPLICATE`, `DERIVE`, `REVOKE`,
//!   `TRANSFER`); more rights land with their subsystems.
//! - [`CapObject`] is a typed enum that names a kernel object by its
//!   typed handle — [`super::obj::TaskHandle`] / [`super::obj::EndpointHandle`]
//!   / [`super::obj::NotificationHandle`] — following [ADR-0016][adr-0016].
//!   `MemoryRegion` arrives in Phase B.
//! - [`CapabilityTable`] implements
//!   [`cap_copy`][CapabilityTable::cap_copy],
//!   [`cap_derive`][CapabilityTable::cap_derive],
//!   [`cap_revoke`][CapabilityTable::cap_revoke], and
//!   [`cap_drop`][CapabilityTable::cap_drop] with zero `unsafe`.
//!
//! What v1 deliberately omits: IPC integration, multi-core safety,
//! persistent capabilities, badge schemes. Each has a named open question
//! in [ADR-0014][adr-0014] or a later ADR.

mod rights;
mod table;

pub use rights::CapRights;
pub use table::{CapHandle, CapabilityTable, CAP_TABLE_CAPACITY, MAX_DERIVATION_DEPTH};

use crate::obj::{EndpointHandle, NotificationHandle, TaskHandle};

/// Kinds of kernel object a capability can refer to.
///
/// The discriminator for a capability's [`CapObject`]; `CapObject`
/// carries the actual typed handle. `MemoryRegion` is reserved here but
/// has no `CapObject` variant until Phase B introduces the MMU.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CapKind {
    /// Refers to a task kernel object.
    Task,
    /// Refers to an IPC endpoint kernel object.
    Endpoint,
    /// Refers to an asynchronous notification kernel object.
    Notification,
    /// Refers to a physical memory region (Phase B).
    MemoryRegion,
}

/// Typed reference to a kernel object.
///
/// Each variant carries the [typed handle][crate::obj] of its kind, so
/// passing a `TaskHandle` where an `EndpointHandle` is expected is a
/// compile-time error. The discriminator matches [`CapKind`] one-to-one.
/// `MemoryRegion` is deferred to Phase B; a capability with that kind
/// cannot be constructed in v1.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CapObject {
    /// Capability naming a [`Task`][crate::obj::Task] kernel object.
    Task(TaskHandle),
    /// Capability naming an [`Endpoint`][crate::obj::Endpoint] kernel object.
    Endpoint(EndpointHandle),
    /// Capability naming a [`Notification`][crate::obj::Notification] kernel object.
    Notification(NotificationHandle),
}

impl CapObject {
    /// Return the [`CapKind`] discriminator matching this object.
    #[must_use]
    pub const fn kind(self) -> CapKind {
        match self {
            Self::Task(_) => CapKind::Task,
            Self::Endpoint(_) => CapKind::Endpoint,
            Self::Notification(_) => CapKind::Notification,
        }
    }
}

/// A capability.
///
/// Deliberately **not** `Copy` and **not** `Clone`. Duplication happens
/// only through [`CapabilityTable::cap_copy`], which requires the caller
/// to hold the [`CapRights::DUPLICATE`] authority on the source. The
/// Rust type system enforces the move-only discipline by construction.
///
/// `Debug` is derived so that test assertions can format capabilities;
/// the derived impl exposes typed handles but no other unforgeable bits.
#[derive(Debug)]
pub struct Capability {
    rights: CapRights,
    object: CapObject,
}

impl Capability {
    /// Construct a capability with the given rights over `object`. The
    /// [`CapKind`] is derived from the `object`'s variant, so
    /// kind-and-object cannot disagree by construction.
    #[must_use]
    pub const fn new(rights: CapRights, object: CapObject) -> Self {
        Self { rights, object }
    }

    /// Return the capability's kind, derived from its object variant.
    #[must_use]
    pub const fn kind(&self) -> CapKind {
        self.object.kind()
    }

    /// Return the capability's rights.
    #[must_use]
    pub const fn rights(&self) -> CapRights {
        self.rights
    }

    /// Return the capability's typed object reference.
    #[must_use]
    pub const fn object(&self) -> CapObject {
        self.object
    }
}

/// Errors returned by capability-table operations.
///
/// `#[non_exhaustive]` so that future additions (introduced by later
/// ADRs as new operations land) are not breaking changes.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CapError {
    /// The capability table is full; no free slot.
    CapsExhausted,
    /// The handle does not refer to a currently-allocated slot, either
    /// because the slot is free or because the handle's generation is
    /// stale (the slot was freed and reused or revoked).
    InvalidHandle,
    /// `cap_copy` or `cap_derive` was asked to grant rights the source
    /// capability does not itself hold.
    WidenedRights,
    /// The caller's rights on the source capability do not include the
    /// authority required for the operation (for example, `DUPLICATE` for
    /// `cap_copy`, `DERIVE` for `cap_derive`, `REVOKE` for `cap_revoke`).
    InsufficientRights,
    /// `cap_derive` would produce a capability whose depth exceeds
    /// [`MAX_DERIVATION_DEPTH`].
    DerivationTooDeep,
    /// `cap_drop` was called on a capability that still has descendants.
    /// The caller must `cap_revoke` the subtree first so orphaned
    /// children cannot outlive their parent.
    HasChildren,
}
