//! `Notification` kernel object — v1 skeleton for asynchronous signals.
//!
//! Per [ADR-0016][adr-0016], v1 stores notifications in a per-type
//! [`Arena`][super::arena::Arena] with a typed [`NotificationHandle`].
//! The v1 state is the single saturating 64-bit word that Milestone A4's
//! `notify` / `wait` operations will OR bits into and read back; the
//! waiter list arrives in A4.
//!
//! [adr-0016]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0016-kernel-object-storage.md

use super::arena::{Arena, SlotId};
use super::{ObjError, NOTIFICATION_ARENA_CAPACITY};

/// The v1 `Notification` kernel object — a 64-bit saturating signal word.
#[derive(Debug)]
pub struct Notification {
    word: u64,
}

impl Notification {
    /// Construct a notification with the initial word (typically `0`).
    #[must_use]
    pub const fn new(word: u64) -> Self {
        Self { word }
    }

    /// Current word.
    #[must_use]
    pub const fn word(&self) -> u64 {
        self.word
    }

    /// Bit-wise OR `bits` into the word — "saturating" in the sense that
    /// once a bit is set, a later `set` against the same bit is a no-op.
    pub fn set(&mut self, bits: u64) {
        self.word |= bits;
    }

    /// Clear every set bit, returning the bits that were set before the
    /// clear. This is the "consume" half of the wait/notify pair.
    pub fn consume(&mut self) -> u64 {
        let current = self.word;
        self.word = 0;
        current
    }
}

/// Typed handle referring to a notification in a [`NotificationArena`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct NotificationHandle(SlotId);

impl NotificationHandle {
    pub(crate) const fn from_slot(slot: SlotId) -> Self {
        Self(slot)
    }

    pub(crate) const fn slot(self) -> SlotId {
        self.0
    }

    /// Construct a handle from raw parts for unit-test scaffolding in
    /// callers that need distinct notification references without
    /// allocating.
    #[cfg(test)]
    #[allow(dead_code, reason = "symmetric with TaskHandle::test_handle")]
    #[must_use]
    pub(crate) const fn test_handle(index: u16, generation: u32) -> Self {
        Self(SlotId::from_parts(index, generation))
    }
}

/// The concrete arena type for notifications.
pub type NotificationArena = Arena<Notification, NOTIFICATION_ARENA_CAPACITY>;

/// Allocate a notification in `arena`.
///
/// # Errors
///
/// [`ObjError::ArenaFull`] when every slot is in use.
pub fn create_notification(
    arena: &mut NotificationArena,
    notification: Notification,
) -> Result<NotificationHandle, ObjError> {
    arena
        .allocate(notification)
        .map(NotificationHandle::from_slot)
        .ok_or(ObjError::ArenaFull)
}

/// Free the notification at `handle`.
///
/// # Errors
///
/// [`ObjError::InvalidHandle`] when `handle` is stale or already freed.
pub fn destroy_notification(
    arena: &mut NotificationArena,
    handle: NotificationHandle,
) -> Result<Notification, ObjError> {
    arena.free(handle.slot()).ok_or(ObjError::InvalidHandle)
}

/// Return a reference to the notification at `handle`.
#[must_use]
pub fn get_notification(
    arena: &NotificationArena,
    handle: NotificationHandle,
) -> Option<&Notification> {
    arena.get(handle.slot())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests may use pragmas forbidden in production kernel code"
)]
mod tests {
    use super::{
        create_notification, destroy_notification, get_notification, Notification,
        NotificationArena,
    };

    #[test]
    fn set_and_consume_round_trip() {
        let mut arena = NotificationArena::default();
        let handle = create_notification(&mut arena, Notification::new(0)).unwrap();

        let note = arena.get_mut(handle.slot()).unwrap();
        note.set(0b0001);
        note.set(0b0100);
        assert_eq!(
            get_notification(&arena, handle).map(Notification::word),
            Some(0b0101)
        );

        let note = arena.get_mut(handle.slot()).unwrap();
        assert_eq!(note.consume(), 0b0101);
        assert_eq!(note.consume(), 0);
    }

    #[test]
    fn destroy_invalidates_handle() {
        let mut arena = NotificationArena::default();
        let handle = create_notification(&mut arena, Notification::new(0)).unwrap();
        destroy_notification(&mut arena, handle).unwrap();
        assert!(get_notification(&arena, handle).is_none());
        // Reallocating reuses the same slot with a bumped generation; the
        // original handle must still fail lookup (generation mismatch).
        let _new_handle = create_notification(&mut arena, Notification::new(1)).unwrap();
        assert!(
            get_notification(&arena, handle).is_none(),
            "stale handle must fail after slot reuse"
        );
    }
}
