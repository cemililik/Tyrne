//! `Endpoint` kernel object — v1 skeleton for synchronous-rendezvous IPC.
//!
//! Per [ADR-0016][adr-0016], v1 stores endpoints in a per-type
//! [`Arena`][super::arena::Arena] with a typed [`EndpointHandle`]. The
//! IPC wait/wake queues are reserved here as zero-sized placeholders;
//! Milestone A4 populates them with real waiter lists when `send` /
//! `recv` / `reply_recv` arrive.
//!
//! [adr-0016]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0016-kernel-object-storage.md

use super::arena::{Arena, SlotId};
use super::{ObjError, ENDPOINT_ARENA_CAPACITY};

/// The v1 `Endpoint` kernel object — an IPC rendezvous point.
///
/// The waiter queues are added in A4. v1 carries only an identifier so
/// tests can distinguish endpoints during creation / destruction flows.
#[derive(Debug)]
pub struct Endpoint {
    id: u32,
}

impl Endpoint {
    /// Construct an endpoint with the given identifier.
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self { id }
    }

    /// Return the endpoint's identifier.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.id
    }
}

/// Typed handle referring to an endpoint in an [`EndpointArena`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct EndpointHandle(SlotId);

impl EndpointHandle {
    pub(crate) const fn from_slot(slot: SlotId) -> Self {
        Self(slot)
    }

    pub(crate) const fn slot(self) -> SlotId {
        self.0
    }

    /// Construct a handle from raw parts for unit-test scaffolding in
    /// callers that need distinct endpoint references without allocating.
    #[cfg(test)]
    #[allow(dead_code, reason = "symmetric with TaskHandle::test_handle")]
    #[must_use]
    pub(crate) const fn test_handle(index: u16, generation: u32) -> Self {
        Self(SlotId::from_parts(index, generation))
    }
}

/// The concrete arena type for endpoints.
pub type EndpointArena = Arena<Endpoint, ENDPOINT_ARENA_CAPACITY>;

/// Allocate an endpoint in `arena`.
///
/// # Errors
///
/// [`ObjError::ArenaFull`] when every slot is in use.
pub fn create_endpoint(
    arena: &mut EndpointArena,
    endpoint: Endpoint,
) -> Result<EndpointHandle, ObjError> {
    arena
        .allocate(endpoint)
        .map(EndpointHandle::from_slot)
        .ok_or(ObjError::ArenaFull)
}

/// Free the endpoint at `handle`.
///
/// # Errors
///
/// [`ObjError::InvalidHandle`] when `handle` is stale or already freed.
pub fn destroy_endpoint(
    arena: &mut EndpointArena,
    handle: EndpointHandle,
) -> Result<Endpoint, ObjError> {
    arena.free(handle.slot()).ok_or(ObjError::InvalidHandle)
}

/// Return a reference to the endpoint at `handle`.
#[must_use]
pub fn get_endpoint(arena: &EndpointArena, handle: EndpointHandle) -> Option<&Endpoint> {
    arena.get(handle.slot())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests may use pragmas forbidden in production kernel code"
)]
mod tests {
    use super::{create_endpoint, destroy_endpoint, get_endpoint, Endpoint, EndpointArena};

    #[test]
    fn create_destroy_round_trip() {
        let mut arena = EndpointArena::default();
        let handle = create_endpoint(&mut arena, Endpoint::new(42)).unwrap();
        assert_eq!(get_endpoint(&arena, handle).map(Endpoint::id), Some(42));
        let removed = destroy_endpoint(&mut arena, handle).unwrap();
        assert_eq!(removed.id(), 42);
        assert!(get_endpoint(&arena, handle).is_none());
    }
}
