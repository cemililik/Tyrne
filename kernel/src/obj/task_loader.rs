//! Task loader — embedded raw-flat userspace image → [`LoadedImage`]
//! metadata.
//!
//! Per [ADR-0029][adr-0029] (raw-flat format choice) + [T-019][t-019]
//! (loader implementation). This module owns the public types
//! [`LoadedImage`] (success descriptor) and [`LoadError`] (failure
//! taxonomy) the loader produces. The `load_image` function itself
//! lands incrementally across the T-019 commit chain: preflight chain
//! (commit 2), AS creation + image-page loop (commit 3), stack loop +
//! rollback machinery (commit 4), BSP wiring + smoke trace (commit 5).
//!
//! ## v1 scope (this commit)
//!
//! Type-level only — no `load_image` function yet, no `unsafe`, no
//! audit-log entries. Lands the [`LoadedImage`] struct + the
//! [`LoadError`] enum + the `obj` module's re-exports of both.
//! Compiles + Miri-clean without any kernel-reachable code path
//! depending on these types yet.
//!
//! ## Scope boundary (load-complete, not B5/B6-runnable)
//!
//! [`LoadedImage`] is intentionally **not** a
//! `CapHandle{CapObject::Task(...)}` — it describes a populated
//! address space but does not mint a runnable task. The current
//! [`Task`][super::Task] struct carries no PC/SP context register
//! file; the loader's new AS holds only image + stack mappings (no
//! kernel mappings, so an EL1 exception while the AS is active would
//! translation-fault on the vector fetch). The `task_create_from_image`
//! wrapper that turns a [`LoadedImage`] into a runnable task cap lands
//! with B5 (syscall ABI per [ADR-0030][adr-0030]) and B6 (first
//! userspace "hello") per [phase-b §B4 §Revision-notes][phase-b-b4-rider].
//!
//! [adr-0029]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0029-initial-userspace-image-format.md
//! [adr-0030]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0030-syscall-abi.md
//! [t-019]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-b/T-019-task-loader.md
//! [phase-b-b4-rider]: https://github.com/cemililik/Tyrne/blob/main/docs/roadmap/phases/phase-b.md#milestone-b4--task-loader

use crate::cap::{CapError, CapHandle};
use crate::mm::AddressSpaceError;
use tyrne_hal::VirtAddr;

/// Metadata describing a freshly populated address space produced by
/// the `load_image` function (lands in T-019 commit 2+).
///
/// `LoadedImage` is a *descriptor of loaded state*, not a runnable-task
/// capability — see the module-level §"Scope boundary" note. Returned
/// by-value (it is [`Copy`]); the caller owns the metadata after
/// `load_image` returns.
///
/// Fields are all `pub` per the public-struct-literal convention chosen
/// for T-019: callers construct a `LoadedImage` directly when they need
/// one for tests, and the loader's success path writes a literal at the
/// end of its sequence. There is no hidden invariant a constructor
/// would protect — every field is independently derived from the
/// loader's arguments + the cap-table / arena state.
///
/// # Field invariants
///
/// - `as_cap` is a freshly-minted leaf `CapHandle` wrapping
///   `CapObject::AddressSpace(AddressSpaceHandle)`, valid against the
///   `CapabilityTable` the loader was passed.
/// - `entry_va == image_base_va` (raw-flat: offset 0 of the embedded
///   blob is the userspace entry instruction).
/// - `stack_top_va` is **one-past-the-highest** mapped VA of the stack
///   region. The stack mapped range is `[stack_base, stack_top_va)`
///   half-open; `sp = stack_top_va` at task-creation initialisation
///   is correct because the first userspace push (e.g. `sp -= 16`)
///   lands inside the mapped range. Matches the AAPCS64 convention.
/// - `image_bytes == image.len()` from the loader's `image: &[u8]`
///   argument. May be smaller than `image_pages * PAGE_SIZE` because
///   tail-zeroing happens on the partial last page (the loader copies
///   only `image.len()` bytes; the remainder of the last page stays
///   zero per [UNSAFE-2026-0026][unsafe-26]'s PMM zero-init contract).
/// - `stack_bytes == stack_size_pages * PAGE_SIZE` (always a multiple
///   of `PAGE_SIZE`).
///
/// [unsafe-26]: https://github.com/cemililik/Tyrne/blob/main/docs/audits/unsafe-log.md
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct LoadedImage {
    /// Cap handle for the newly-minted address space. Backed by a
    /// `CapObject::AddressSpace(AddressSpaceHandle)` cap-table entry
    /// minted via
    /// [`cap_create_address_space`][crate::mm::cap_create_address_space]
    /// during `load_image` step 4.
    pub as_cap: CapHandle,

    /// Userspace entry-point VA — equals `image_base_va` (raw-flat
    /// format: offset 0 of the embedded blob is the entry instruction).
    pub entry_va: VirtAddr,

    /// One-past-the-highest mapped VA of the stack region (half-open
    /// `[stack_base, stack_top_va)` convention; see struct-level
    /// invariants).
    pub stack_top_va: VirtAddr,

    /// Byte-count of the image as loaded into the AS (may be smaller
    /// than `image_pages * PAGE_SIZE` due to tail-zeroing on the
    /// partial last page).
    pub image_bytes: usize,

    /// Stack region size in bytes (always a multiple of `PAGE_SIZE`).
    pub stack_bytes: usize,
}

/// Error taxonomy for the `load_image` function (lands in T-019
/// commit 2+).
///
/// Variants are split per the explicit rollback contract documented in
/// [T-019 §Approach §"Rollback contract (explicit)"][t-019-rollback].
/// Each variant's doc-comment names whether rollback is required and
/// what the v1 baseline leaks on the rollback path. The
/// [T-019 §"Rollback contract"][t-019-rollback] section remains the
/// canonical reference; this enum's doc-comments are summaries.
///
/// `#[non_exhaustive]` because future-state variants are foreseeable
/// — e.g. an `InvalidImageBaseVa` that fires when the caller-supplied
/// `image_base_va` falls outside the userspace VA range, lands with
/// the per-task VA-range ADR in B5+.
///
/// [t-019-rollback]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-b/T-019-task-loader.md#rollback-contract-explicit
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LoadError {
    /// `image.is_empty()`. Pre-PMM preflight; no state change.
    InvalidImage,

    /// `stack_size_pages == 0`. Pre-PMM preflight; no state change.
    InvalidStackSize,

    /// Parent AS cap lookup or `CapKind::AddressSpace` kind check
    /// failed. Wraps the underlying [`CapError`]. **DERIVE-rights
    /// enforcement is *not* in this variant** — the DERIVE check is
    /// delegated to `cap_create_address_space`'s step 2a and surfaces
    /// as [`AddressSpaceCreationFailed`][LoadError::AddressSpaceCreationFailed]
    /// wrapping `CapError::InsufficientRights`.
    InvalidParentCap(CapError),

    /// Frame-budget preflight: `1 + image_pages + stack_pages +
    /// intermediate_budget` exceeds `pmm.stats().free_frames`. The
    /// leading `1` accounts for the root L0 frame
    /// `cap_create_address_space` will allocate; `intermediate_budget
    /// = 6` is the safe upper bound for v1's fresh-AS scenario (up to
    /// 3 intermediate frames per contiguous VA range × 2 ranges for
    /// image + stack). Pre-PMM preflight; no state change.
    FrameBudgetExceeded {
        /// Frames the loader would commit (root + image + stack +
        /// intermediate upper bound).
        needed: usize,
        /// Frames currently available per `pmm.stats().free_frames`.
        available: usize,
    },

    /// `cap_create_address_space` returned `Err`. Covers
    /// `CapError::InsufficientRights` if `parent_as_cap` lacks DERIVE,
    /// plus the T-018-guarded `CapsExhausted` / `DerivationTooDeep` /
    /// `ArenaFull` paths. No rollback needed at this layer (T-018's
    /// preflight ensures no committed state on failure).
    AddressSpaceCreationFailed(AddressSpaceError),

    /// `pmm.alloc_frame()` returned `None` mid-image-or-stack-loop.
    /// Structurally unreachable post-[`FrameBudgetExceeded`][LoadError::FrameBudgetExceeded]
    /// preflight under v1's single-thread cooperative model; retained
    /// as a defensive variant for budget-calculation bugs and future-
    /// concurrency scenarios. **Rollback required**, per the
    /// [T-019 §"Rollback contract"][t-019-rollback] (leaf frames +
    /// `cap_unmap` undo + `cap_drop(loaded_as_cap)`).
    ///
    /// [t-019-rollback]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-b/T-019-task-loader.md#rollback-contract-explicit
    OutOfFrames,

    /// `cap_map` returned `Err` mid-loop. Wraps the underlying
    /// [`AddressSpaceError`] — typically
    /// `MmuMapError(MmuError::OutOfFrames)` if the intermediate-frame
    /// budget was underestimated, `MmuMapError(MmuError::AlreadyMapped)`
    /// on a VA-range collision, or `MmuMapError(MmuError::BlockMapped)`
    /// if the VA falls inside a block descriptor. **Rollback required**,
    /// same shape as [`OutOfFrames`][LoadError::OutOfFrames].
    MapFailed(AddressSpaceError),
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests may use pragmas forbidden in production kernel code"
)]
mod tests {
    use super::{LoadError, LoadedImage};
    use crate::cap::{CapError, CapObject, CapRights, Capability, CapabilityTable};
    use crate::mm::AddressSpaceError;
    use crate::obj::EndpointHandle;
    use tyrne_hal::VirtAddr;

    /// Mint a real `CapHandle` into a fresh `CapabilityTable` and
    /// return it. The underlying cap's object is an `Endpoint` (a
    /// real `EndpointHandle::test_handle`) — irrelevant for these
    /// tests; the point is to construct a valid `CapHandle` value
    /// so a `LoadedImage` literal can be assembled.
    fn fresh_cap_handle() -> crate::cap::CapHandle {
        let mut table = CapabilityTable::new();
        let cap = Capability::new(
            CapRights::empty(),
            CapObject::Endpoint(EndpointHandle::test_handle(0, 0)),
        );
        table.insert_root(cap).unwrap()
    }

    #[test]
    fn loaded_image_struct_literal_round_trips_through_copy_and_eq() {
        // Pin the public-struct-literal convention: a caller can
        // construct a `LoadedImage` by writing the 5 fields directly,
        // round-trip it through `Copy` semantics, and compare it for
        // equality. This is the contract the loader's success path
        // depends on (it writes a literal at the end of its sequence).
        let cap = fresh_cap_handle();
        let img = LoadedImage {
            as_cap: cap,
            entry_va: VirtAddr(0x0080_0000),
            stack_top_va: VirtAddr(0x0090_0000),
            image_bytes: 4096,
            stack_bytes: 65_536,
        };

        // Copy semantics — `img` is still usable after the let-copy.
        let copy = img;
        assert_eq!(img, copy);

        // Field-by-field shape check.
        assert_eq!(img.as_cap, cap);
        assert_eq!(img.entry_va, VirtAddr(0x0080_0000));
        assert_eq!(img.stack_top_va, VirtAddr(0x0090_0000));
        assert_eq!(img.image_bytes, 4096);
        assert_eq!(img.stack_bytes, 65_536);
    }

    #[test]
    fn loaded_image_distinguishes_different_field_values() {
        // Two `LoadedImage`s with different fields are NOT equal.
        // Pins the derived `PartialEq` honesty (no accidental
        // wildcards in the struct definition).
        let cap = fresh_cap_handle();
        let a = LoadedImage {
            as_cap: cap,
            entry_va: VirtAddr(0x0080_0000),
            stack_top_va: VirtAddr(0x0090_0000),
            image_bytes: 4096,
            stack_bytes: 65_536,
        };
        let b = LoadedImage {
            stack_bytes: 131_072, // ← differs
            ..a
        };
        assert_ne!(a, b);
    }

    #[test]
    fn load_error_variants_pattern_match_exhaustively() {
        // Pin the 7-variant taxonomy by pattern-matching each one.
        // `#[non_exhaustive]` only forces external (out-of-crate)
        // consumers to add a wildcard arm; within-crate matches are
        // still subject to compiler exhaustiveness checking. Adding
        // a new variant in a future commit therefore breaks this test
        // at compile time — the strongest possible signal that the
        // taxonomy changed.
        let cases = [
            LoadError::InvalidImage,
            LoadError::InvalidStackSize,
            LoadError::InvalidParentCap(CapError::InvalidHandle),
            LoadError::FrameBudgetExceeded {
                needed: 100,
                available: 50,
            },
            LoadError::AddressSpaceCreationFailed(AddressSpaceError::OutOfFrames),
            LoadError::OutOfFrames,
            LoadError::MapFailed(AddressSpaceError::StaleHandle),
        ];

        for err in cases {
            match err {
                LoadError::InvalidImage
                | LoadError::InvalidStackSize
                | LoadError::InvalidParentCap(_)
                | LoadError::FrameBudgetExceeded { .. }
                | LoadError::AddressSpaceCreationFailed(_)
                | LoadError::OutOfFrames
                | LoadError::MapFailed(_) => { /* exhaustive within-crate */ }
            }
        }
    }

    #[test]
    fn load_error_variants_are_distinct() {
        // Pin `PartialEq` honesty across the variant set: two
        // distinct variants must not compare equal.
        assert_ne!(LoadError::InvalidImage, LoadError::InvalidStackSize);
        assert_ne!(LoadError::InvalidImage, LoadError::OutOfFrames);
        assert_ne!(LoadError::InvalidStackSize, LoadError::OutOfFrames);
        assert_ne!(
            LoadError::InvalidParentCap(CapError::InvalidHandle),
            LoadError::InvalidParentCap(CapError::WrongKind),
        );
        assert_ne!(
            LoadError::FrameBudgetExceeded {
                needed: 100,
                available: 50,
            },
            LoadError::FrameBudgetExceeded {
                needed: 200,
                available: 50,
            },
        );
        assert_ne!(
            LoadError::AddressSpaceCreationFailed(AddressSpaceError::OutOfFrames),
            LoadError::AddressSpaceCreationFailed(AddressSpaceError::StaleHandle),
        );
        assert_ne!(
            LoadError::MapFailed(AddressSpaceError::StaleHandle),
            LoadError::MapFailed(AddressSpaceError::OutOfFrames),
        );
    }

    #[test]
    fn load_error_frame_budget_exceeded_fields_round_trip() {
        // Pin the `FrameBudgetExceeded { needed, available }` struct-
        // variant shape: callers can destructure to read both fields.
        let err = LoadError::FrameBudgetExceeded {
            needed: 1024,
            available: 8,
        };
        match err {
            LoadError::FrameBudgetExceeded { needed, available } => {
                assert_eq!(needed, 1024);
                assert_eq!(available, 8);
            }
            other => panic!("expected FrameBudgetExceeded, got {other:?}"),
        }
    }
}
