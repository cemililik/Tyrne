//! `Task` kernel object — v1 skeleton.
//!
//! A `Task` is the kernel's representation of a scheduled execution
//! context. Per [ADR-0016][adr-0016], v1 stores tasks in a per-type
//! [`Arena`][super::arena::Arena] with a typed [`TaskHandle`]; scheduler
//! state and the context-save frame arrive in Milestone A5 as layered
//! additions.
//!
//! [adr-0016]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0016-kernel-object-storage.md

use super::arena::{Arena, SlotId};
use super::{ObjError, TASK_ARENA_CAPACITY};

/// The v1 `Task` kernel object.
///
/// Minimal fields — enough for the capability-to-object wiring T-002
/// delivers. Phase A5 adds scheduler state; Phase B adds address-space
/// ownership.
#[derive(Debug)]
pub struct Task {
    id: u32,
}

impl Task {
    /// Construct a task with the given identifier.
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self { id }
    }

    /// Return the task's identifier.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.id
    }
}

/// Typed handle referring to a task in a [`TaskArena`].
///
/// `TaskHandle` is intentionally not convertible to or from other kinds'
/// handles: the type system prevents e.g. passing a `TaskHandle` where
/// an `EndpointHandle` is expected.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TaskHandle(SlotId);

impl TaskHandle {
    pub(crate) const fn from_slot(slot: SlotId) -> Self {
        Self(slot)
    }

    pub(crate) const fn slot(self) -> SlotId {
        self.0
    }

    /// Construct a handle from raw `(index, generation)` for tests that
    /// need to compose capabilities without allocating through a real
    /// arena. Production code obtains handles via [`create_task`].
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn test_handle(index: u16, generation: u32) -> Self {
        Self(SlotId::from_parts(index, generation))
    }
}

/// The concrete arena type for tasks. Capacity is [`TASK_ARENA_CAPACITY`].
pub type TaskArena = Arena<Task, TASK_ARENA_CAPACITY>;

/// Allocate a task in `arena`, returning a [`TaskHandle`] that names it.
///
/// # Errors
///
/// [`ObjError::ArenaFull`] when every slot is in use.
pub fn create_task(arena: &mut TaskArena, task: Task) -> Result<TaskHandle, ObjError> {
    arena
        .allocate(task)
        .map(TaskHandle::from_slot)
        .ok_or(ObjError::ArenaFull)
}

/// Free the task at `handle`, returning the stored value.
///
/// v1 does not itself walk capability tables to enforce reachability;
/// callers that hold references to live tables should check via
/// [`CapabilityTable::references_object`][crate::cap::CapabilityTable::references_object]
/// first and pass [`ObjError::StillReachable`] back to their own caller
/// if any table still names this handle. A successor ADR will bundle
/// the check into this function once the kernel owns a registry of
/// tables.
///
/// # Errors
///
/// [`ObjError::InvalidHandle`] when `handle` is stale or already freed.
pub fn destroy_task(arena: &mut TaskArena, handle: TaskHandle) -> Result<Task, ObjError> {
    arena.free(handle.slot()).ok_or(ObjError::InvalidHandle)
}

/// Return a reference to the task at `handle`, or `None` if the handle
/// is stale.
#[must_use]
pub fn get_task(arena: &TaskArena, handle: TaskHandle) -> Option<&Task> {
    arena.get(handle.slot())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests may use pragmas forbidden in production kernel code"
)]
mod tests {
    use super::{create_task, destroy_task, get_task, Task, TaskArena};
    use crate::obj::{ObjError, TASK_ARENA_CAPACITY};

    #[test]
    fn create_then_get_round_trip() {
        let mut arena = TaskArena::default();
        let handle = create_task(&mut arena, Task::new(7)).unwrap();
        assert_eq!(get_task(&arena, handle).map(Task::id), Some(7));
    }

    #[test]
    fn destroy_invalidates_handle() {
        let mut arena = TaskArena::default();
        let handle = create_task(&mut arena, Task::new(1)).unwrap();
        let removed = destroy_task(&mut arena, handle).unwrap();
        assert_eq!(removed.id(), 1);
        assert!(get_task(&arena, handle).is_none());
        assert_eq!(
            destroy_task(&mut arena, handle).unwrap_err(),
            ObjError::InvalidHandle
        );
    }

    #[test]
    fn arena_exhaustion_returns_arena_full() {
        let mut arena = TaskArena::default();
        for i in 0..TASK_ARENA_CAPACITY {
            // `i` fits in u32 because TASK_ARENA_CAPACITY is small.
            #[allow(
                clippy::cast_possible_truncation,
                reason = "bounded by TASK_ARENA_CAPACITY"
            )]
            create_task(&mut arena, Task::new(i as u32)).unwrap();
        }
        assert_eq!(
            create_task(&mut arena, Task::new(99)).unwrap_err(),
            ObjError::ArenaFull
        );
    }
}
