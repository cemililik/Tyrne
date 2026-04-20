//! Capability table with generation-tagged handles and an index-based
//! derivation tree.
//!
//! Shape and rationale: [ADR-0014][adr-0014]. This module implements the
//! data structure and the four public operations — `cap_copy`,
//! `cap_derive`, `cap_revoke`, `cap_drop` — plus `insert_root` for
//! bootstrapping. No `unsafe`, no heap.
//!
//! [adr-0014]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0014-capability-representation.md

use super::{CapError, CapObject, CapRights, Capability};

/// Maximum number of capabilities in a single table.
///
/// Per [ADR-0014][adr-0014]; revisit when a real use-case demands more.
/// For v1 this is a compile-time constant.
///
/// [adr-0014]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0014-capability-representation.md
pub const CAP_TABLE_CAPACITY: usize = 64;

/// Hard cap on derivation depth.
///
/// Prevents pathological chains from consuming the whole table through a
/// single derivation path. `cap_derive` returns
/// [`CapError::DerivationTooDeep`] when it would cross this limit.
pub const MAX_DERIVATION_DEPTH: usize = 16;

/// Index into the slot array.
type Index = u16;

/// Generation counter for a slot. Incremented on every free to make
/// stale handles detectable.
type Generation = u32;

/// Opaque handle into a [`CapabilityTable`].
///
/// Handles are stable for the lifetime of the capability they refer to
/// (equivalently: until the slot is freed). A handle whose slot has been
/// freed or reused fails lookup with [`CapError::InvalidHandle`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CapHandle {
    index: Index,
    generation: Generation,
}

impl CapHandle {
    /// Return the raw index component. Kernel-internal; used by tests.
    #[must_use]
    #[doc(hidden)]
    pub const fn index(self) -> u16 {
        self.index
    }

    /// Return the raw generation component. Kernel-internal; used by tests.
    #[must_use]
    #[doc(hidden)]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

/// A slot in the [`CapabilityTable`]. Either holds a populated
/// [`SlotEntry`] or participates in the free list.
struct Slot {
    entry: Option<SlotEntry>,
    generation: Generation,
    next_free: Option<Index>,
}

/// Populated slot contents.
struct SlotEntry {
    capability: Capability,
    parent: Option<Index>,
    first_child: Option<Index>,
    next_sibling: Option<Index>,
    depth: u8,
}

/// Per-task capability table.
///
/// Bounded capacity ([`CAP_TABLE_CAPACITY`]), `no_std`, heap-free. The
/// table holds every capability the owning task can reach; the
/// derivation tree is embedded inside it via slot indices.
pub struct CapabilityTable {
    slots: [Slot; CAP_TABLE_CAPACITY],
    free_head: Option<Index>,
}

impl Default for CapabilityTable {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityTable {
    /// Construct an empty table with every slot in the free list.
    #[must_use]
    pub fn new() -> Self {
        // Build the slot array with each slot's `next_free` pointing at
        // the subsequent index, so the free list covers all slots at
        // construction time.
        //
        // Invariant: `CAP_TABLE_CAPACITY` is a small `const` well below
        // `Index::MAX`, so every `index.wrapping_add(1)` fits in `Index`.
        const _: () = assert!(CAP_TABLE_CAPACITY <= Index::MAX as usize);
        let slots: [Slot; CAP_TABLE_CAPACITY] = core::array::from_fn(|i| {
            let next_idx = i.wrapping_add(1);
            let next_free = if next_idx < CAP_TABLE_CAPACITY {
                // Safe cast: `next_idx < CAP_TABLE_CAPACITY <= Index::MAX`.
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "bounded by CAP_TABLE_CAPACITY <= Index::MAX"
                )]
                Some(next_idx as Index)
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
            free_head: Some(0),
        }
    }

    /// Insert a *root* capability into the table — one with no parent.
    ///
    /// Returns a fresh [`CapHandle`] whose generation matches the freshly
    /// allocated slot.
    ///
    /// # Errors
    ///
    /// Returns [`CapError::CapsExhausted`] when no free slot is
    /// available.
    pub fn insert_root(&mut self, cap: Capability) -> Result<CapHandle, CapError> {
        let index = self.pop_free().ok_or(CapError::CapsExhausted)?;
        // The newly-popped slot is guaranteed free; no descendants to
        // worry about.
        let generation = self.slots[index as usize].generation;
        self.slots[index as usize].entry = Some(SlotEntry {
            capability: cap,
            parent: None,
            first_child: None,
            next_sibling: None,
            depth: 0,
        });
        Ok(CapHandle { index, generation })
    }

    /// Install a peer capability with the same kind and object, narrower
    /// or equal rights, at the same position in the derivation tree as
    /// the source.
    ///
    /// # Errors
    ///
    /// - [`CapError::InvalidHandle`] if `src` is stale.
    /// - [`CapError::InsufficientRights`] if `src` lacks
    ///   [`CapRights::DUPLICATE`].
    /// - [`CapError::WidenedRights`] if `new_rights` is not a subset of
    ///   `src`'s rights.
    /// - [`CapError::CapsExhausted`] if the table is full.
    pub fn cap_copy(
        &mut self,
        src: CapHandle,
        new_rights: CapRights,
    ) -> Result<CapHandle, CapError> {
        // Snapshot the source. `kind` is no longer a separate field on
        // `Capability` (ADR-0016 put it on `CapObject`), so we read
        // rights / object / tree-link fields directly.
        let (rights, object, parent, depth) = {
            let entry = self.entry_of(src)?;
            (
                entry.capability.rights(),
                entry.capability.object(),
                entry.parent,
                entry.depth,
            )
        };

        if !rights.contains(CapRights::DUPLICATE) {
            return Err(CapError::InsufficientRights);
        }
        if !rights.contains(new_rights) {
            return Err(CapError::WidenedRights);
        }

        // Allocate a new slot.
        let new_index = self.pop_free().ok_or(CapError::CapsExhausted)?;
        let generation = self.slots[new_index as usize].generation;

        // Splice into the parent's child list (or leave as a root if the
        // source has no parent). Read the parent's current first_child
        // *before* writing the new entry.
        let former_first_child = match parent {
            Some(parent_idx) => match &self.slots[parent_idx as usize].entry {
                Some(parent_entry) => parent_entry.first_child,
                None => return Err(CapError::InvalidHandle),
            },
            None => None,
        };

        self.slots[new_index as usize].entry = Some(SlotEntry {
            capability: Capability::new(new_rights, object),
            parent,
            first_child: None,
            next_sibling: former_first_child,
            depth,
        });

        // Update the parent's first_child to point at us.
        if let Some(parent_idx) = parent {
            if let Some(parent_entry) = self.slots[parent_idx as usize].entry.as_mut() {
                parent_entry.first_child = Some(new_index);
            }
        }

        Ok(CapHandle {
            index: new_index,
            generation,
        })
    }

    /// Install a child capability — one whose parent is `src`. Typically
    /// used when narrowing the capability's *scope*; v1 passes scope
    /// narrowing through the opaque `new_object` since kernel objects do
    /// not yet carry typed scope.
    ///
    /// # Errors
    ///
    /// - [`CapError::InvalidHandle`] if `src` is stale.
    /// - [`CapError::InsufficientRights`] if `src` lacks
    ///   [`CapRights::DERIVE`].
    /// - [`CapError::WidenedRights`] if `new_rights` is not a subset of
    ///   `src`'s rights.
    /// - [`CapError::DerivationTooDeep`] if the new child's depth would
    ///   exceed [`MAX_DERIVATION_DEPTH`].
    /// - [`CapError::CapsExhausted`] if the table is full.
    pub fn cap_derive(
        &mut self,
        src: CapHandle,
        new_rights: CapRights,
        new_object: super::CapObject,
    ) -> Result<CapHandle, CapError> {
        // `entry_of` already validates the handle and resolves to a live
        // slot; its input handle gives us the index we need for the new
        // entry's `parent` link without a second `resolve_handle` call.
        // `kind` moved onto `CapObject` in ADR-0016, so we no longer
        // snapshot it here — the new entry's kind is carried by
        // `new_object`'s variant.
        let (rights, parent_index, parent_depth) = {
            let entry = self.entry_of(src)?;
            (entry.capability.rights(), src.index, entry.depth)
        };

        if !rights.contains(CapRights::DERIVE) {
            return Err(CapError::InsufficientRights);
        }
        if !rights.contains(new_rights) {
            return Err(CapError::WidenedRights);
        }

        // Enforce the depth cap.
        let new_depth_usize = (parent_depth as usize).saturating_add(1);
        if new_depth_usize > MAX_DERIVATION_DEPTH {
            return Err(CapError::DerivationTooDeep);
        }
        // `new_depth_usize` fits in `u8` because MAX_DERIVATION_DEPTH ≤ u8::MAX.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "bounded by MAX_DERIVATION_DEPTH"
        )]
        let new_depth = new_depth_usize as u8;

        let new_index = self.pop_free().ok_or(CapError::CapsExhausted)?;
        let generation = self.slots[new_index as usize].generation;

        // Read the parent's current first_child (cap_derive always has a
        // concrete parent: `src` itself).
        let former_first_child = match &self.slots[parent_index as usize].entry {
            Some(parent_entry) => parent_entry.first_child,
            None => return Err(CapError::InvalidHandle),
        };

        self.slots[new_index as usize].entry = Some(SlotEntry {
            capability: Capability::new(new_rights, new_object),
            parent: Some(parent_index),
            first_child: None,
            next_sibling: former_first_child,
            depth: new_depth,
        });

        if let Some(parent_entry) = self.slots[parent_index as usize].entry.as_mut() {
            parent_entry.first_child = Some(new_index);
        }

        Ok(CapHandle {
            index: new_index,
            generation,
        })
    }

    /// Invalidate every descendant of `src`. `src` itself remains valid.
    ///
    /// # Errors
    ///
    /// - [`CapError::InvalidHandle`] if `src` is stale.
    /// - [`CapError::InsufficientRights`] if `src` lacks
    ///   [`CapRights::REVOKE`].
    pub fn cap_revoke(&mut self, src: CapHandle) -> Result<(), CapError> {
        let src_index = self.resolve_handle(src)?;

        // Rights check.
        let rights = match &self.slots[src_index as usize].entry {
            Some(entry) => entry.capability.rights(),
            None => return Err(CapError::InvalidHandle),
        };
        if !rights.contains(CapRights::REVOKE) {
            return Err(CapError::InsufficientRights);
        }

        // Collect descendants via BFS on a fixed-size scratch array.
        //
        // Invariant: the derivation tree is rooted at a slot in this
        // table and can contain at most `CAP_TABLE_CAPACITY - 1`
        // descendants (the root itself is excluded from the queue). If
        // either `desc_len` bound check below fires at runtime, it means
        // the tree has a cycle or a duplicate node — an internal bug in
        // the table's bookkeeping, not a user-visible capacity issue.
        // The `debug_assert!` surfaces that bug in tests; in release
        // builds the extra entries are simply skipped so a bug cannot be
        // escalated to a revocation that silently fails to free memory.
        let mut descendants = [0 as Index; CAP_TABLE_CAPACITY];
        let mut desc_len: usize = 0;

        // Seed the queue with the direct children of `src`.
        let mut child = match &self.slots[src_index as usize].entry {
            Some(entry) => entry.first_child,
            None => return Err(CapError::InvalidHandle),
        };
        while let Some(c) = child {
            debug_assert!(
                desc_len < CAP_TABLE_CAPACITY,
                "derivation tree contains a cycle or duplicate node"
            );
            if desc_len >= CAP_TABLE_CAPACITY {
                break;
            }
            descendants[desc_len] = c;
            desc_len = desc_len.saturating_add(1);
            child = match &self.slots[c as usize].entry {
                Some(entry) => entry.next_sibling,
                None => None,
            };
        }

        // Expand: for each queued descendant, add its children.
        let mut scan: usize = 0;
        while scan < desc_len {
            let idx = descendants[scan];
            let mut gc = match &self.slots[idx as usize].entry {
                Some(entry) => entry.first_child,
                None => None,
            };
            while let Some(g) = gc {
                debug_assert!(
                    desc_len < CAP_TABLE_CAPACITY,
                    "derivation tree contains a cycle or duplicate node"
                );
                if desc_len >= CAP_TABLE_CAPACITY {
                    break;
                }
                descendants[desc_len] = g;
                desc_len = desc_len.saturating_add(1);
                gc = match &self.slots[g as usize].entry {
                    Some(entry) => entry.next_sibling,
                    None => None,
                };
            }
            scan = scan.saturating_add(1);
        }

        // Free every collected descendant.
        for &idx in &descendants[..desc_len] {
            self.free_slot(idx);
        }

        // Clear src's first_child list now that every descendant is gone.
        if let Some(entry) = self.slots[src_index as usize].entry.as_mut() {
            entry.first_child = None;
        }

        Ok(())
    }

    /// Release the capability at `handle` from this table. Peers and
    /// siblings are unaffected; only the slot itself is freed.
    ///
    /// The caller must first revoke any descendants with `cap_revoke`:
    /// dropping an interior node would orphan its children and violate
    /// the derivation-tree invariant from [ADR-0014][adr-0014]. The
    /// conservative choice of refusing to drop interior nodes keeps the
    /// contract auditable and leaves cascade semantics to `cap_revoke`.
    ///
    /// [adr-0014]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0014-capability-representation.md
    ///
    /// # Errors
    ///
    /// - [`CapError::InvalidHandle`] if `handle` is stale.
    /// - [`CapError::HasChildren`] if the capability still has at least
    ///   one derived descendant in this table.
    pub fn cap_drop(&mut self, handle: CapHandle) -> Result<(), CapError> {
        let index = self.resolve_handle(handle)?;

        let has_children = match &self.slots[index as usize].entry {
            Some(entry) => entry.first_child.is_some(),
            None => return Err(CapError::InvalidHandle),
        };
        if has_children {
            return Err(CapError::HasChildren);
        }

        self.unlink_from_siblings(index)?;
        self.free_slot(index);
        Ok(())
    }

    /// Return the capability at `handle`, if the handle is still valid.
    ///
    /// # Errors
    ///
    /// Returns [`CapError::InvalidHandle`] if `handle` is stale.
    pub fn lookup(&self, handle: CapHandle) -> Result<&Capability, CapError> {
        let entry = self.entry_of(handle)?;
        Ok(&entry.capability)
    }

    /// Return `true` if any live capability in this table names the
    /// given kernel object. Used by the [`crate::obj`] destroy paths
    /// to implement the reachability check described in
    /// [ADR-0016][adr-0016] — callers pass their watcher tables and
    /// refuse destruction if any of them reports a reference.
    ///
    /// The check is linear in [`CAP_TABLE_CAPACITY`]; acceptable at
    /// Phase A's scale.
    ///
    /// [adr-0016]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0016-kernel-object-storage.md
    #[must_use]
    pub fn references_object(&self, target: CapObject) -> bool {
        self.slots
            .iter()
            .filter_map(|s| s.entry.as_ref())
            .any(|entry| entry.capability.object() == target)
    }

    // ----- internals -----

    /// Validate a handle and return the underlying slot index.
    fn resolve_handle(&self, handle: CapHandle) -> Result<Index, CapError> {
        let slot = self
            .slots
            .get(handle.index as usize)
            .ok_or(CapError::InvalidHandle)?;
        if slot.generation != handle.generation {
            return Err(CapError::InvalidHandle);
        }
        if slot.entry.is_none() {
            return Err(CapError::InvalidHandle);
        }
        Ok(handle.index)
    }

    /// Return a reference to the slot entry at `handle`, validating the
    /// handle in the process.
    fn entry_of(&self, handle: CapHandle) -> Result<&SlotEntry, CapError> {
        let index = self.resolve_handle(handle)?;
        match &self.slots[index as usize].entry {
            Some(entry) => Ok(entry),
            None => Err(CapError::InvalidHandle),
        }
    }

    /// Pop and return the next free slot index, or `None` if the table is full.
    fn pop_free(&mut self) -> Option<Index> {
        let head = self.free_head?;
        self.free_head = self.slots[head as usize].next_free;
        self.slots[head as usize].next_free = None;
        Some(head)
    }

    /// Free the slot at `index`: clear the entry, bump the generation,
    /// prepend to the free list.
    fn free_slot(&mut self, index: Index) {
        let old_free_head = self.free_head;
        self.free_head = Some(index);

        let Some(slot) = self.slots.get_mut(index as usize) else {
            return;
        };
        slot.entry = None;
        slot.generation = slot.generation.wrapping_add(1);
        slot.next_free = old_free_head;
    }

    /// Remove the slot at `index` from its parent's child list.
    /// Used by `cap_drop` before freeing.
    fn unlink_from_siblings(&mut self, index: Index) -> Result<(), CapError> {
        let (parent, next_sibling) = match &self.slots[index as usize].entry {
            Some(entry) => (entry.parent, entry.next_sibling),
            None => return Err(CapError::InvalidHandle),
        };

        let Some(parent_idx) = parent else {
            // Root capability — nothing to unlink.
            return Ok(());
        };

        // Walk the parent's child list to find us and remove.
        let mut cursor = match &self.slots[parent_idx as usize].entry {
            Some(entry) => entry.first_child,
            None => return Err(CapError::InvalidHandle),
        };

        // Case 1: we are the head of the list.
        if cursor == Some(index) {
            if let Some(parent_entry) = self.slots[parent_idx as usize].entry.as_mut() {
                parent_entry.first_child = next_sibling;
            }
            return Ok(());
        }

        // Case 2: walk the sibling chain.
        while let Some(c) = cursor {
            let c_next = match &self.slots[c as usize].entry {
                Some(entry) => entry.next_sibling,
                None => None,
            };
            if c_next == Some(index) {
                if let Some(c_entry) = self.slots[c as usize].entry.as_mut() {
                    c_entry.next_sibling = next_sibling;
                }
                return Ok(());
            }
            cursor = c_next;
        }

        // Not found — either the slot was never linked or the parent's
        // child list is inconsistent. The latter is an internal bug.
        Err(CapError::InvalidHandle)
    }
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests may use pragmas forbidden in production kernel code"
)]
mod tests {
    use super::{CapHandle, CapabilityTable, CAP_TABLE_CAPACITY, MAX_DERIVATION_DEPTH};
    use crate::cap::{CapError, CapKind, CapObject, CapRights, Capability};
    use crate::obj::TaskHandle;

    /// All v1 rights set.
    fn all_rights() -> CapRights {
        CapRights::DUPLICATE | CapRights::DERIVE | CapRights::REVOKE | CapRights::TRANSFER
    }

    /// Build a task-kind `CapObject` with a synthesized handle — for
    /// tests that need distinct capability values without allocating
    /// through a real arena.
    fn task_object(tag: u16) -> CapObject {
        CapObject::Task(TaskHandle::test_handle(tag, 0))
    }

    fn root_cap() -> Capability {
        Capability::new(all_rights(), task_object(0xAA))
    }

    #[test]
    fn new_table_can_accept_one_root() {
        let mut t = CapabilityTable::new();
        let h = t.insert_root(root_cap()).unwrap();
        let cap = t.lookup(h).unwrap();
        assert_eq!(cap.kind(), CapKind::Task);
        assert_eq!(cap.rights(), all_rights());
        assert_eq!(cap.object(), task_object(0xAA));
    }

    #[test]
    fn drop_invalidates_handle() {
        let mut t = CapabilityTable::new();
        let h = t.insert_root(root_cap()).unwrap();
        t.cap_drop(h).unwrap();
        assert_eq!(t.lookup(h).unwrap_err(), CapError::InvalidHandle);
    }

    #[test]
    fn drop_twice_returns_invalid_handle() {
        let mut t = CapabilityTable::new();
        let h = t.insert_root(root_cap()).unwrap();
        t.cap_drop(h).unwrap();
        assert_eq!(t.cap_drop(h).unwrap_err(), CapError::InvalidHandle);
    }

    #[test]
    fn freed_slot_is_reused_with_bumped_generation() {
        let mut t = CapabilityTable::new();
        let h1 = t.insert_root(root_cap()).unwrap();
        t.cap_drop(h1).unwrap();
        let h2 = t.insert_root(root_cap()).unwrap();
        assert_eq!(h1.index(), h2.index(), "slot should be reused");
        assert_ne!(
            h1.generation(),
            h2.generation(),
            "generation must differ after reuse"
        );
        assert_eq!(t.lookup(h1).unwrap_err(), CapError::InvalidHandle);
        assert!(t.lookup(h2).is_ok());
    }

    #[test]
    fn cap_copy_with_same_rights_succeeds() {
        let mut t = CapabilityTable::new();
        let src = t.insert_root(root_cap()).unwrap();
        let dup = t.cap_copy(src, all_rights()).unwrap();
        assert_eq!(t.lookup(dup).unwrap().rights(), all_rights());
        assert!(t.lookup(src).is_ok(), "original is unaffected");
    }

    #[test]
    fn cap_copy_narrows_rights() {
        let mut t = CapabilityTable::new();
        let src = t.insert_root(root_cap()).unwrap();
        let narrower = CapRights::DUPLICATE;
        let dup = t.cap_copy(src, narrower).unwrap();
        assert_eq!(t.lookup(dup).unwrap().rights(), narrower);
    }

    #[test]
    fn cap_copy_rejects_widened_rights() {
        let mut t = CapabilityTable::new();
        let narrow = Capability::new(CapRights::DUPLICATE, task_object(0));
        let src = t.insert_root(narrow).unwrap();
        let wider = CapRights::DUPLICATE | CapRights::REVOKE;
        assert_eq!(t.cap_copy(src, wider).unwrap_err(), CapError::WidenedRights);
    }

    #[test]
    fn cap_copy_without_duplicate_right_fails() {
        let mut t = CapabilityTable::new();
        let no_dup = Capability::new(CapRights::DERIVE, task_object(0));
        let src = t.insert_root(no_dup).unwrap();
        assert_eq!(
            t.cap_copy(src, CapRights::EMPTY).unwrap_err(),
            CapError::InsufficientRights
        );
    }

    #[test]
    fn cap_derive_creates_child_with_narrower_rights() {
        let mut t = CapabilityTable::new();
        let src = t.insert_root(root_cap()).unwrap();
        let child_rights = CapRights::DUPLICATE;
        let child = t.cap_derive(src, child_rights, task_object(0xBB)).unwrap();
        let child_cap = t.lookup(child).unwrap();
        assert_eq!(child_cap.rights(), child_rights);
        assert_eq!(child_cap.object(), task_object(0xBB));
    }

    #[test]
    fn cap_derive_without_derive_right_fails() {
        let mut t = CapabilityTable::new();
        let no_derive = Capability::new(CapRights::DUPLICATE, task_object(0));
        let src = t.insert_root(no_derive).unwrap();
        assert_eq!(
            t.cap_derive(src, CapRights::EMPTY, task_object(0))
                .unwrap_err(),
            CapError::InsufficientRights
        );
    }

    #[test]
    fn cap_derive_rejects_widened_rights() {
        let mut t = CapabilityTable::new();
        let narrow = Capability::new(CapRights::DERIVE, task_object(0));
        let src = t.insert_root(narrow).unwrap();
        let wider = CapRights::DERIVE | CapRights::REVOKE;
        assert_eq!(
            t.cap_derive(src, wider, task_object(0)).unwrap_err(),
            CapError::WidenedRights
        );
    }

    #[test]
    fn cap_derive_enforces_depth_cap() {
        let mut t = CapabilityTable::new();
        let mut current = t.insert_root(root_cap()).unwrap();
        // Build MAX_DERIVATION_DEPTH-deep chain (each child gets DERIVE so we
        // can go again); the next derive should fail.
        for _ in 0..MAX_DERIVATION_DEPTH {
            current = t.cap_derive(current, all_rights(), task_object(0)).unwrap();
        }
        assert_eq!(
            t.cap_derive(current, all_rights(), task_object(0))
                .unwrap_err(),
            CapError::DerivationTooDeep
        );
    }

    #[test]
    fn cap_revoke_removes_only_descendants() {
        let mut t = CapabilityTable::new();
        let src = t.insert_root(root_cap()).unwrap();
        let child = t.cap_derive(src, all_rights(), task_object(1)).unwrap();

        t.cap_revoke(src).unwrap();

        assert!(t.lookup(src).is_ok(), "src is preserved");
        assert_eq!(
            t.lookup(child).unwrap_err(),
            CapError::InvalidHandle,
            "child is revoked"
        );
    }

    #[test]
    fn cap_revoke_cascades_depth_three() {
        let mut t = CapabilityTable::new();
        let root = t.insert_root(root_cap()).unwrap();
        let child = t.cap_derive(root, all_rights(), task_object(1)).unwrap();
        let grand = t.cap_derive(child, all_rights(), task_object(2)).unwrap();
        let great = t.cap_derive(grand, all_rights(), task_object(3)).unwrap();

        t.cap_revoke(root).unwrap();

        assert!(t.lookup(root).is_ok());
        for h in [child, grand, great] {
            assert_eq!(t.lookup(h).unwrap_err(), CapError::InvalidHandle);
        }
    }

    #[test]
    fn references_object_sees_live_caps_only() {
        // `references_object` supports the ADR-0016 reachability check.
        // It returns true iff some live capability names the target.
        let mut t = CapabilityTable::new();
        let target = task_object(0xCC);
        let other = task_object(0xDD);
        assert!(
            !t.references_object(target),
            "empty table references nothing"
        );

        let h = t
            .insert_root(Capability::new(all_rights(), target))
            .unwrap();
        assert!(t.references_object(target));
        assert!(!t.references_object(other));

        // Cap-drop removes the reference.
        t.cap_drop(h).unwrap();
        assert!(!t.references_object(target));
    }

    #[test]
    fn cap_revoke_without_revoke_right_fails() {
        let mut t = CapabilityTable::new();
        let no_revoke = Capability::new(CapRights::DUPLICATE | CapRights::DERIVE, task_object(0));
        let src = t.insert_root(no_revoke).unwrap();
        assert_eq!(t.cap_revoke(src).unwrap_err(), CapError::InsufficientRights);
    }

    #[test]
    fn cap_revoke_on_leaf_is_a_noop() {
        let mut t = CapabilityTable::new();
        let leaf = t.insert_root(root_cap()).unwrap();
        assert!(t.cap_revoke(leaf).is_ok());
        assert!(t.lookup(leaf).is_ok());
    }

    #[test]
    fn cap_revoke_on_stale_handle_fails() {
        let mut t = CapabilityTable::new();
        let h = t.insert_root(root_cap()).unwrap();
        t.cap_drop(h).unwrap();
        assert_eq!(t.cap_revoke(h).unwrap_err(), CapError::InvalidHandle);
    }

    #[test]
    fn copy_of_a_child_shares_parent() {
        let mut t = CapabilityTable::new();
        let root = t.insert_root(root_cap()).unwrap();
        let child = t.cap_derive(root, all_rights(), task_object(1)).unwrap();
        let peer = t.cap_copy(child, all_rights()).unwrap();

        // Revoking `root` must invalidate both `child` and `peer` — they
        // share the same parent.
        t.cap_revoke(root).unwrap();
        assert_eq!(t.lookup(child).unwrap_err(), CapError::InvalidHandle);
        assert_eq!(t.lookup(peer).unwrap_err(), CapError::InvalidHandle);
    }

    #[test]
    fn drop_peer_does_not_affect_other_peer() {
        let mut t = CapabilityTable::new();
        let root = t.insert_root(root_cap()).unwrap();
        let peer_a = t.cap_copy(root, all_rights()).unwrap();
        let peer_b = t.cap_copy(root, all_rights()).unwrap();
        t.cap_drop(peer_a).unwrap();
        assert_eq!(t.lookup(peer_a).unwrap_err(), CapError::InvalidHandle);
        assert!(t.lookup(peer_b).is_ok(), "peer B survives peer A drop");
        assert!(t.lookup(root).is_ok(), "root survives peer drop");
    }

    #[test]
    fn table_exhaustion_returns_caps_exhausted() {
        let mut t = CapabilityTable::new();
        let mut handles: [Option<CapHandle>; CAP_TABLE_CAPACITY] = [None; CAP_TABLE_CAPACITY];
        for h in &mut handles {
            *h = Some(t.insert_root(root_cap()).unwrap());
        }
        assert_eq!(
            t.insert_root(root_cap()).unwrap_err(),
            CapError::CapsExhausted
        );
        // Free one, then reinserting should succeed.
        t.cap_drop(handles[0].unwrap()).unwrap();
        assert!(t.insert_root(root_cap()).is_ok());
    }

    #[test]
    fn cap_drop_on_interior_node_returns_has_children() {
        // Root has at least one derived child — dropping the root must
        // refuse rather than orphan the child.
        let mut t = CapabilityTable::new();
        let parent = t.insert_root(root_cap()).unwrap();
        let _child = t.cap_derive(parent, all_rights(), task_object(1)).unwrap();

        assert_eq!(
            t.cap_drop(parent).unwrap_err(),
            CapError::HasChildren,
            "dropping an interior node must refuse to orphan descendants"
        );

        // Revoking first, then dropping, works.
        t.cap_revoke(parent).unwrap();
        assert!(t.cap_drop(parent).is_ok());
    }

    #[test]
    fn drop_middle_sibling_preserves_list_integrity() {
        // Build three peers under a root and drop the middle one; the
        // outer two must remain reachable.
        let mut t = CapabilityTable::new();
        let root = t.insert_root(root_cap()).unwrap();
        let a = t.cap_derive(root, all_rights(), task_object(1)).unwrap();
        let b = t.cap_derive(root, all_rights(), task_object(2)).unwrap();
        let c = t.cap_derive(root, all_rights(), task_object(3)).unwrap();
        t.cap_drop(b).unwrap();
        assert!(t.lookup(a).is_ok());
        assert!(t.lookup(c).is_ok());
        assert_eq!(t.lookup(b).unwrap_err(), CapError::InvalidHandle);
        // Revoking root must still reach both remaining children.
        t.cap_revoke(root).unwrap();
        assert_eq!(t.lookup(a).unwrap_err(), CapError::InvalidHandle);
        assert_eq!(t.lookup(c).unwrap_err(), CapError::InvalidHandle);
    }
}
