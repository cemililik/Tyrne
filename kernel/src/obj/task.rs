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
use crate::mm::AddressSpaceHandle;

/// The v1 `Task` kernel object.
///
/// Carries the task's identifier and its [`AddressSpaceHandle`] (per
/// [ADR-0028][adr-0028]; T-018 commit 4) — the scheduler reads the
/// AS handle on every yield to decide whether to invoke
/// [`Mmu::activate`][tyrne_hal::Mmu::activate] before the
/// architectural context switch.
///
/// [adr-0028]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0028-address-space-data-structure.md
#[derive(Debug)]
pub struct Task {
    id: u32,
    address_space_handle: AddressSpaceHandle,
}

impl Task {
    /// Construct a task with the given identifier and address-space handle.
    ///
    /// In v1 every task is created against the bootstrap address space
    /// — callers pass [`crate::mm::BOOTSTRAP_ADDRESS_SPACE_HANDLE`].
    /// B5+ userspace tasks will pass per-task AS handles obtained
    /// from [`crate::mm::cap_create_address_space`].
    #[must_use]
    pub const fn new(id: u32, address_space_handle: AddressSpaceHandle) -> Self {
        Self {
            id,
            address_space_handle,
        }
    }

    /// Return the task's identifier.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.id
    }

    /// Return the [`AddressSpaceHandle`] this task runs against.
    ///
    /// Used by the scheduler's activation hook in
    /// [`yield_now`][crate::sched::yield_now] (and the IPC bridge's
    /// context-switch paths) to compare against the outgoing task's
    /// AS and decide whether [`Mmu::activate`][tyrne_hal::Mmu::activate]
    /// must fire.
    #[must_use]
    pub const fn address_space_handle(&self) -> AddressSpaceHandle {
        self.address_space_handle
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
    clippy::expect_used,
    clippy::panic,
    reason = "tests may use pragmas forbidden in production kernel code"
)]
mod tests {
    use super::{create_task, destroy_task, get_task, Task, TaskArena};
    use crate::mm::BOOTSTRAP_ADDRESS_SPACE_HANDLE;
    use crate::obj::{ObjError, TASK_ARENA_CAPACITY};

    #[test]
    fn create_then_get_round_trip() {
        let mut arena = TaskArena::default();
        let handle = create_task(&mut arena, Task::new(7, BOOTSTRAP_ADDRESS_SPACE_HANDLE)).unwrap();
        assert_eq!(get_task(&arena, handle).map(Task::id), Some(7));
    }

    #[test]
    fn destroy_invalidates_handle() {
        let mut arena = TaskArena::default();
        let handle = create_task(&mut arena, Task::new(1, BOOTSTRAP_ADDRESS_SPACE_HANDLE)).unwrap();
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
            create_task(
                &mut arena,
                Task::new(i as u32, BOOTSTRAP_ADDRESS_SPACE_HANDLE),
            )
            .unwrap();
        }
        assert_eq!(
            create_task(&mut arena, Task::new(99, BOOTSTRAP_ADDRESS_SPACE_HANDLE)).unwrap_err(),
            ObjError::ArenaFull
        );
    }

    #[test]
    fn address_space_handle_round_trips() {
        let task = Task::new(42, BOOTSTRAP_ADDRESS_SPACE_HANDLE);
        assert_eq!(task.address_space_handle(), BOOTSTRAP_ADDRESS_SPACE_HANDLE);
    }
}
