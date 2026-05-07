//! Generic bounded arena for kernel-object storage.
//!
//! `Arena<T, N>` is a fixed-size array of slots, each either empty or
//! holding a `T`. Slot identity is captured by a [`SlotId`] — a pair of
//! `(index, generation)` that survives slot reuse by matching both
//! parts; a stale id fails lookup even if the underlying slot has been
//! refilled.
//!
//! Shape and rationale: [ADR-0016][adr-0016]. The pattern mirrors the
//! [`CapabilityTable`][`crate::cap::CapabilityTable`] from A2 — one
//! audited arena shape, now generic, instantiated three times in the
//! per-kind kernel-object modules.
//!
//! [adr-0016]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0016-kernel-object-storage.md

/// Index into an [`Arena`]'s backing array.
type Index = u16;

/// Generation counter; bumped on every free to make stale ids detectable.
type Generation = u32;

/// Identifier of a slot within an arena.
///
/// A `SlotId` is valid as long as the slot it names still holds the
/// value that was allocated with that id. Once the slot is freed and
/// reused, the slot's generation advances and the old id fails lookup.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SlotId {
    index: Index,
    generation: Generation,
}

impl SlotId {
    /// Raw index component. Crate-internal; used by the IPC layer to index
    /// into parallel state arrays.
    #[must_use]
    pub(crate) const fn index(self) -> u16 {
        self.index
    }

    /// Raw generation component. Crate-internal; used by the IPC layer to
    /// detect stale waiter state after endpoint slot reuse.
    #[must_use]
    pub(crate) const fn generation(self) -> Generation {
        self.generation
    }

    /// Construct a `SlotId` from parts. Crate-internal; production code
    /// obtains `SlotId`s only from [`Arena::allocate`]. Exposed for
    /// unit-test scaffolding in sibling modules.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn from_parts(index: Index, generation: Generation) -> Self {
        Self { index, generation }
    }
}

/// One storage cell of an [`Arena`]. Either populated or participating
/// in the free list.
struct Slot<T> {
    entry: Option<T>,
    generation: Generation,
    next_free: Option<Index>,
}

/// Fixed-capacity, heap-free, generation-tagged arena.
///
/// `N` is the compile-time capacity. The arena never allocates; free
/// slots form an embedded linked list threaded through `next_free`.
pub struct Arena<T, const N: usize> {
    slots: [Slot<T>; N],
    free_head: Option<Index>,
}

impl<T, const N: usize> Default for Arena<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Arena<T, N> {
    /// Construct an empty arena with every slot threaded into the free
    /// list. The first allocation returns index `0`.
    ///
    /// Invariant: `N <= u16::MAX` so that every slot index fits in the
    /// [`Index`] type. The `const` assertion inside the body catches
    /// violation at compile time.
    #[must_use]
    pub fn new() -> Self {
        const {
            assert!(
                N <= Index::MAX as usize,
                "arena capacity exceeds Index::MAX"
            );
        }

        let slots: [Slot<T>; N] = core::array::from_fn(|i| {
            let next = i.wrapping_add(1);
            let next_free = if next < N {
                // Bounded by `N <= Index::MAX`; checked by the const
                // assertion above.
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "bounded by N <= Index::MAX"
                )]
                Some(next as Index)
            } else {
                None
            };
            Slot {
                entry: None,
                generation: 0,
                next_free,
            }
        });

        Self {
            slots,
            free_head: if N > 0 { Some(0) } else { None },
        }
    }

    /// Allocate a new slot, storing `value` in it. Returns a [`SlotId`]
    /// that refers to the allocation until it is freed.
    ///
    /// Returns `None` when every slot is in use.
    pub fn allocate(&mut self, value: T) -> Option<SlotId> {
        let head = self.free_head?;
        debug_assert!((head as usize) < N, "free_head out of bounds");
        let slot = self.slots.get_mut(head as usize)?;
        let next_free = slot.next_free;
        slot.entry = Some(value);
        slot.next_free = None;
        self.free_head = next_free;
        Some(SlotId {
            index: head,
            generation: slot.generation,
        })
    }

    /// Free the slot named by `id`, returning the stored value.
    ///
    /// Returns `None` if the id is stale (generation mismatch) or
    /// points at an already-free slot.
    pub fn free(&mut self, id: SlotId) -> Option<T> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        let value = slot.entry.take()?;
        slot.generation = slot.generation.wrapping_add(1);
        slot.next_free = self.free_head;
        self.free_head = Some(id.index);
        Some(value)
    }

    /// Return a reference to the value at `id`, or `None` if stale /
    /// freed.
    #[must_use]
    pub fn get(&self, id: SlotId) -> Option<&T> {
        let slot = self.slots.get(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.entry.as_ref()
    }

    /// Return a mutable reference to the value at `id`, or `None` if
    /// stale / freed.
    pub fn get_mut(&mut self, id: SlotId) -> Option<&mut T> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.entry.as_mut()
    }

    /// Return `true` when `id` still names a live slot.
    #[must_use]
    pub fn contains(&self, id: SlotId) -> bool {
        self.get(id).is_some()
    }
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests may use pragmas forbidden in production kernel code"
)]
mod tests {
    use super::Arena;

    #[test]
    fn allocate_and_get_round_trip() {
        let mut arena: Arena<u32, 4> = Arena::new();
        let id = arena.allocate(42).unwrap();
        assert_eq!(arena.get(id), Some(&42));
        assert!(arena.contains(id));
    }

    #[test]
    fn free_invalidates_id() {
        let mut arena: Arena<u32, 4> = Arena::new();
        let id = arena.allocate(7).unwrap();
        assert_eq!(arena.free(id), Some(7));
        assert_eq!(arena.get(id), None);
        assert!(!arena.contains(id));
    }

    #[test]
    fn free_then_allocate_bumps_generation() {
        let mut arena: Arena<u32, 4> = Arena::new();
        let first = arena.allocate(1).unwrap();
        arena.free(first).unwrap();
        let second = arena.allocate(2).unwrap();
        assert_eq!(first.index(), second.index(), "slot reuse expected");
        assert_ne!(first.generation(), second.generation());
        assert_eq!(arena.get(first), None, "stale id must fail");
        assert_eq!(arena.get(second), Some(&2));
    }

    #[test]
    fn exhaustion_returns_none() {
        let mut arena: Arena<u8, 2> = Arena::new();
        let _a = arena.allocate(1).unwrap();
        let _b = arena.allocate(2).unwrap();
        assert!(arena.allocate(3).is_none());
    }

    #[test]
    fn free_middle_then_allocate_reuses_that_slot() {
        let mut arena: Arena<u32, 3> = Arena::new();
        let a = arena.allocate(10).unwrap();
        let b = arena.allocate(20).unwrap();
        let c = arena.allocate(30).unwrap();
        arena.free(b).unwrap();
        let d = arena.allocate(99).unwrap();
        assert_eq!(d.index(), b.index(), "b's slot was reused");
        assert_eq!(arena.get(a), Some(&10));
        assert_eq!(arena.get(c), Some(&30));
        assert_eq!(arena.get(d), Some(&99));
        assert_eq!(arena.get(b), None);
    }

    #[test]
    fn get_mut_permits_mutation() {
        let mut arena: Arena<u32, 2> = Arena::new();
        let id = arena.allocate(1).unwrap();
        *arena.get_mut(id).unwrap() = 2;
        assert_eq!(arena.get(id), Some(&2));
    }

    #[test]
    fn empty_capacity_arena_has_no_free_slot() {
        let mut arena: Arena<u32, 0> = Arena::new();
        assert!(arena.allocate(0).is_none());
    }
}
