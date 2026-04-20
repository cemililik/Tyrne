//! Capability subsystem.
//!
//! Every privileged action in Umbrix requires the caller to hold a
//! capability that authorizes it. A capability is an unforgeable,
//! move-only kernel-held token, referenced from userspace (eventually)
//! and from the kernel's own code (now) through an opaque handle.
//!
//! The representation — index-based arena, generation-tagged handles,
//! explicit derivation tree, cascading revocation — is pinned in
//! [ADR-0014][adr-0014]. The architectural role of capabilities lives in
//! [`security-model.md`][sec] and [architectural principle P1][p1].
//!
//! [adr-0014]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0014-capability-representation.md
//! [sec]: https://github.com/cemililik/UmbrixOS/blob/main/docs/architecture/security-model.md
//! [p1]: https://github.com/cemililik/UmbrixOS/blob/main/docs/standards/architectural-principles.md#p1--no-ambient-authority
//!
//! ## Status (v1, T-001)
//!
//! - [`Capability`] is move-only (not `Copy`, not `Clone`).
//! - [`CapRights`] carries four v1 rights (`DUPLICATE`, `DERIVE`, `REVOKE`,
//!   `TRANSFER`); more rights land with their subsystems.
//! - [`CapObject`] is a placeholder for the kernel object the capability
//!   points at; Milestone A3 replaces it with a typed reference.
//! - [`CapabilityTable`] implements
//!   [`cap_copy`][CapabilityTable::cap_copy],
//!   [`cap_derive`][CapabilityTable::cap_derive],
//!   [`cap_revoke`][CapabilityTable::cap_revoke], and
//!   [`cap_drop`][CapabilityTable::cap_drop] with `zero` `unsafe`.
//!
//! What v1 deliberately omits: IPC integration, multi-core safety,
//! persistent capabilities, badge schemes. Each has a named open question
//! in [ADR-0014][adr-0014] or a later ADR.

mod rights;
mod table;

pub use rights::CapRights;
pub use table::{CapHandle, CapabilityTable, CAP_TABLE_CAPACITY, MAX_DERIVATION_DEPTH};

/// Kinds of kernel object a capability can refer to.
///
/// The variants are placeholders in v1: they discriminate the capability
/// by *kind* but the actual kernel-object references are encoded as
/// opaque [`CapObject`] values until Milestone A3 introduces real
/// kernel-object types.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CapKind {
    /// Refers to a task kernel object.
    Task,
    /// Refers to an IPC endpoint kernel object.
    Endpoint,
    /// Refers to an asynchronous notification kernel object.
    Notification,
    /// Refers to a physical memory region.
    MemoryRegion,
}

/// Opaque reference to a kernel object.
///
/// In v1 this wraps a plain `u64` identifier whose interpretation is left
/// to the caller (kernel-internal). Milestone A3 replaces `CapObject` with
/// a typed reference; the outer API of the capability table does not
/// change. The wrapped value is kept private and read through
/// [`CapObject::raw`] so that every construction or inspection site goes
/// through an auditable function — a small safeguard for when the typed
/// replacement lands.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CapObject(u64);

impl CapObject {
    /// Construct a capability-object reference from its raw identifier.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Return the raw identifier this `CapObject` wraps.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
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
/// the derived impl does not expose any unforgeable bits because
/// [`CapObject`] is an opaque `u64` in v1.
#[derive(Debug)]
pub struct Capability {
    kind: CapKind,
    rights: CapRights,
    object: CapObject,
}

impl Capability {
    /// Construct a capability with the given kind, rights, and object.
    #[must_use]
    pub const fn new(kind: CapKind, rights: CapRights, object: CapObject) -> Self {
        Self {
            kind,
            rights,
            object,
        }
    }

    /// Return the capability's kind.
    #[must_use]
    pub const fn kind(&self) -> CapKind {
        self.kind
    }

    /// Return the capability's rights.
    #[must_use]
    pub const fn rights(&self) -> CapRights {
        self.rights
    }

    /// Return the capability's opaque object reference.
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
