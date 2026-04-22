# 0009 — `Mmu` HAL trait signature (v1)

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

The third HAL trait in Phase 4b, after [ADR-0007: Console](0007-console-trait.md) and [ADR-0008: Cpu](0008-cpu-trait.md). The `Mmu` trait is the largest HAL surface because the Memory Management Unit is where architectural detail is richest: on aarch64 alone, a page-table entry carries access permissions, memory-type attributes, shareability, execute-never flags, a non-global bit, ASID semantics, and a handful of software-available bits. A naive abstraction either leaks all of that (defeating the HAL) or hides so much that it cannot express what real systems need.

This ADR deliberately scopes to a **v1** useful enough to boot a single-core kernel with a static memory layout, a few MMIO mappings for the UART and interrupt controller, and nothing more. Advanced facilities (huge pages, per-page flag updates, multi-core TLB shootdown, full ASID management, memory-typing beyond normal-vs-device) are **explicitly out of scope** and catalogued under *Open questions*; each will get its own ADR when its need arrives.

The trait is also the first HAL trait where we accept **associated types** at the trait level, because the per-BSP `AddressSpace` structure genuinely differs between boards (aarch64 VMSAv8 vs. future RISC-V Sv39) and erasing that structure behind a type-erased handle buys little. See the Decision drivers.

## Decision drivers

- **Enough for Phase 4c.** The v1 surface must let the kernel bring up its own mappings (kernel code and stack, UART, GIC MMIO) and activate them. That is the bar. Richer functionality can come later.
- **Generic over the BSP's `AddressSpace`.** Each BSP defines its own concrete address-space structure; forcing a single shared representation would either leak VMSAv8 details to the kernel or require a type-erased `Box<dyn AddressSpace>` that brings heap allocation into the HAL. An associated type expresses the relationship exactly.
- **`Mmu` is called less pervasively than [`Cpu`](0008-cpu-trait.md) or [`Console`](0007-console-trait.md).** MMU operations are address-space lifecycle events, page-fault handling, and IPC memory-grant installation. They are not every-instruction hot paths. Generic `<M: Mmu>` parameterization for MMU-touching kernel code is acceptable; we do not need `&dyn Mmu` dispatch here.
- **Capability-aware.** The MMU surface must compose with the capability system (see [security-model.md](../architecture/security-model.md)). `MemoryRegionCap` grants become `Mmu::map` calls; revocation becomes `Mmu::unmap` + TLB invalidation.
- **Frame allocation is the kernel's responsibility.** The HAL must not call out to a global allocator for intermediate page-table frames; it must be handed the frames it needs. This keeps the HAL's resource model deterministic and auditable.
- **Explicit cache and TLB discipline.** Writes to page tables require architectural ordering before the MMU observes them. The trait hides this inside its methods but documents the fact.
- **Honest `v1` scope.** The ADR commits to what v1 can express and names what it cannot, so later ADRs can be authored against a known baseline rather than a mystery.

## Considered options

### Option A — fully associated-type trait with map/unmap/activate/invalidate + FrameProvider

```rust
pub trait Mmu: Send + Sync {
    type AddressSpace: Send;
    unsafe fn create_address_space(&self, root: PhysFrame) -> Self::AddressSpace;
    fn activate(&self, as_: &Self::AddressSpace);
    fn map(&self, as_: &mut Self::AddressSpace, va: VirtAddr, pa: PhysFrame,
           flags: MappingFlags, frames: &mut dyn FrameProvider) -> Result<(), MmuError>;
    fn unmap(&self, as_: &mut Self::AddressSpace, va: VirtAddr)
             -> Result<PhysFrame, MmuError>;
    fn invalidate_tlb_address(&self, va: VirtAddr);
    fn invalidate_tlb_all(&self);
}
```

### Option B — minimal trait (activate + TLB only), map/unmap in BSP-specific module

`Mmu` exposes only `activate` and `invalidate_tlb_*`. Page-table installation is a BSP-specific function the kernel calls directly (`bsp_qemu_virt::mmu::map(...)`), bypassing the HAL trait for mapping operations.

### Option C — type-erased `AddressSpace` handle

`AddressSpace` is a concrete struct defined in `tyrne-hal` carrying an opaque pointer and a vtable. All BSPs interpret the same struct. Mapping operations take `&mut AddressSpace`.

### Option D — split the MMU surface across two traits

`MmuCore` for activate / invalidate; `MmuMap` for create / map / unmap. Kernel code that only switches address spaces depends on `MmuCore` alone; mapping code depends on both. The two traits can evolve independently.

## Decision outcome

**Chosen: Option A — a single `Mmu` trait with an associated `AddressSpace` type and `map` / `unmap` / `activate` / `invalidate_tlb_*` methods, plus a separate `FrameProvider` trait for frame allocation callbacks.**

The `Mmu` trait in v1:

```rust
pub trait Mmu: Send + Sync {
    /// The address-space structure this MMU manages.
    ///
    /// BSPs define the concrete representation — for aarch64, this is the
    /// in-memory handle to a VMSAv8 translation regime; for future RISC-V
    /// BSPs, an Sv39/Sv48 equivalent.
    type AddressSpace: Send;

    /// Construct a new address space rooted at the given physical frame.
    ///
    /// # Safety
    /// `root` must be a page-sized physical frame that is exclusively
    /// owned by the caller and zero-initialized.
    unsafe fn create_address_space(&self, root: PhysFrame) -> Self::AddressSpace;

    /// Return the root translation-table frame of the given address space.
    fn address_space_root(&self, as_: &Self::AddressSpace) -> PhysFrame;

    /// Activate the given address space on the current core.
    fn activate(&self, as_: &Self::AddressSpace);

    /// Install a single-page mapping.
    ///
    /// # Errors
    /// Returns [`MmuError::AlreadyMapped`] if `va` already has a mapping.
    /// Returns [`MmuError::OutOfFrames`] if the allocator cannot provide
    /// a frame for an intermediate table.
    fn map(
        &self,
        as_: &mut Self::AddressSpace,
        va: VirtAddr,
        pa: PhysFrame,
        flags: MappingFlags,
        frames: &mut dyn FrameProvider,
    ) -> Result<(), MmuError>;

    /// Remove a single-page mapping and return the physical frame that
    /// had been mapped.
    ///
    /// # Errors
    /// Returns [`MmuError::NotMapped`] if `va` has no mapping.
    fn unmap(
        &self,
        as_: &mut Self::AddressSpace,
        va: VirtAddr,
    ) -> Result<PhysFrame, MmuError>;

    /// Invalidate any TLB entry covering `va` on the current core.
    fn invalidate_tlb_address(&self, va: VirtAddr);

    /// Invalidate every TLB entry on the current core.
    fn invalidate_tlb_all(&self);
}

pub trait FrameProvider {
    /// Allocate a zero-initialized physical frame.
    /// Returns None when the caller has no frames available.
    fn alloc_frame(&mut self) -> Option<PhysFrame>;
}
```

Supporting types, all in the same `hal::mmu` module:

```rust
pub const PAGE_SIZE: usize = 4096;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct VirtAddr(pub usize);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PhysAddr(pub usize);

/// A PAGE_SIZE-aligned physical address.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PhysFrame(PhysAddr);

/// Access and attribute bits for a mapping.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct MappingFlags(u32);

#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MmuError {
    AlreadyMapped,
    NotMapped,
    MisalignedAddress,
    OutOfFrames,
    InvalidFlags,
}
```

`MappingFlags` exposes five flag constants in v1: `WRITE`, `EXECUTE`, `USER`, `DEVICE`, `GLOBAL`. Read permission is implicit (unreadable mappings have no use); richer attributes (cache modes, shareability domains, non-cacheable vs. write-combining vs. device-nGnRnE, software bits) are deferred.

Option B was rejected because bypassing the HAL trait for mapping operations would bake BSP-specific module paths into the kernel — exactly the pattern [architectural principle P6](../standards/architectural-principles.md#p6--hal-separation) forbids. Option C (type-erased handle with a vtable in `tyrne-hal`) would either require heap allocation or a fixed maximum-size struct; both are worse than the associated-type approach for a trait used at lifecycle events. Option D (split traits) is attractive and may be the right shape later, but forces a trait-boundary decision now that is easier to make once we have more calling code; a single trait is the simpler starting point.

## Consequences

### Positive

- **Enough for Phase 4c.** Kernel bring-up can install the minimum set of mappings and activate them; UART access works; further ADRs extend without structural change.
- **Compiler-enforced separation.** Generic `<M: Mmu>` in kernel code keeps BSP-specific structures invisible above the trait.
- **Capability-compatible.** `MemoryRegionCap` grants call into `Mmu::map`; revocation calls `Mmu::unmap`. The capability surface and the MMU surface speak the same vocabulary (pages, frames, flags).
- **Deterministic resource model.** Frame allocation is explicit; the HAL never allocates on its own.
- **Clear upgrade path.** The open-question list below is the backlog of future ADRs; we have a runway rather than a moving target.

### Negative

- **Associated type reduces dyn usability.** `&dyn Mmu` is possible for activate / invalidate, but `map`/`unmap` require knowing the concrete `AddressSpace` type. Mitigation: kernel MMU-touching code is generic over `<M: Mmu>`. MMU operations are not hot-path dyn calls.
- **`MappingFlags` is a hand-rolled bitfield rather than a `bitflags!` macro.** Cleaner ergonomics of `bitflags` are forgone to avoid taking a dependency in Phase 4b. Re-evaluate when a second bitfield shows up in the HAL — at that point an `tyrne-hal-bits` module using `bitflags` may be justified.
- **Single page-size baked in.** `PAGE_SIZE = 4096`. Huge pages (2 MiB, 1 GiB) require a follow-on ADR and extension to `Mmu::map` (or a new `map_block`).
- **No per-page flag updates.** Changing the write permission on an existing mapping currently means `unmap` + `map`. A `change_flags` method will come; doing it right needs TLB semantics that are cleaner to pin down after we have the basic version running.
- **No multi-core TLB shootdown.** `invalidate_tlb_*` operates on the current core only. This is fine for single-core v1; multi-core demands a separate primitive and an IPI mechanism (future ADR).

### Neutral

- `VirtAddr` / `PhysAddr` / `PhysFrame` are newtypes over `usize` — transparent where it matters, type-distinct where it should be. Arithmetic operators are not implemented in v1; callers use `.0` when they need arithmetic.
- The `FrameProvider` trait is object-safe (`&mut dyn FrameProvider`). Kernel frame allocator crates implement it.

## Pros and cons of the options

### Option A — associated-type trait with full v1 surface (chosen)

- Pro: BSP-appropriate `AddressSpace` representation; no heap; no forced type erasure.
- Pro: all lifecycle operations on one trait; easy to find.
- Pro: explicit frame provider; deterministic.
- Con: associated type reduces dyn-dispatch flexibility; not usable uniformly through `&dyn Mmu`.

### Option B — minimal trait + BSP module for mapping

- Pro: dyn-friendly trait surface for the things it keeps.
- Con: kernel calls board-specific functions to create mappings → direct violation of P6.
- Con: every new BSP reimplements a module the kernel hard-codes the path to.

### Option C — type-erased AddressSpace handle

- Pro: single concrete type across BSPs, easy to store in kernel tables.
- Con: requires either heap allocation or a fixed-size buffer; both are worse than associated types.
- Con: vtable indirection on every map/unmap/activate call.

### Option D — split into `MmuCore` and `MmuMap`

- Pro: kernel code that only switches address spaces doesn't need the mapping trait.
- Pro: evolution can proceed independently.
- Con: more surface to track; more imports.
- Con: splitting is premature — we do not yet have the calling code that would benefit.

## Open questions

Each of these is a future ADR; the `Mmu` trait is expected to grow.

- **Per-page flag updates.** `change_flags(va, new_flags)` with TLB semantics (which invalidation granularity? visible to other cores when?).
- **Huge pages.** `map_block(va, pa, block_size, flags)` for 2 MiB / 1 GiB mappings. Requires either a new method or a parameterized `map` with a page-size argument.
- **Multi-core TLB shootdown.** Broadcasting `invalidate_tlb_*` to other cores. Needs inter-core notification (IPI) which is a cross-cutting concern.
- **Rich memory typing.** Write-combining, non-cacheable, device-nGnRE vs. device-nGnRnE vs. device-GRE. Requires extended `MappingFlags` or a new `MemoryType` enum field.
- **ASID management.** Currently entirely inside the BSP. A formal ADR if the kernel needs to influence ASID assignment (e.g., for context switch optimization).
- **Translation walk queries.** `lookup(va) -> Option<(PhysFrame, MappingFlags)>`. Useful for fault handlers and debugging; deferred until a concrete caller needs it.
- **Copy-on-write / shared mappings.** Higher-level capability feature; not an `Mmu` trait concern directly but will influence how `MappingFlags` is extended.
- **ACL on page-table structures themselves.** Should the kernel be able to mark page-table frames read-only to prevent corruption from errant kernel writes? Requires per-frame flags beyond what the current trait exposes.
- **Adopting the `bitflags` crate.** Once a second or third bitfield appears in the HAL (IrqController priority, mapping flags, etc.), the `bitflags` ergonomic win may outweigh the dependency cost. ADR-gated.

## References

- [ADR-0006: Workspace layout](0006-workspace-layout.md).
- [ADR-0007: Console HAL trait signature](0007-console-trait.md).
- [ADR-0008: Cpu HAL trait signature (v1, single-core scope)](0008-cpu-trait.md).
- [`docs/architecture/hal.md`](../architecture/hal.md) — Mmu's architectural role.
- [`docs/architecture/security-model.md`](../architecture/security-model.md) — capability-MMU interaction.
- [`docs/standards/architectural-principles.md`](../standards/architectural-principles.md) — P2, P6.
- ARM *Architecture Reference Manual*, ARMv8-A — Stage 1 translation, VMSAv8 entry layout, TLB invalidate-by-address (`TLBI VAE1`) and invalidate-all (`TLBI VMALLE1`) semantics.
- RISC-V Privileged Architecture specification — Sv39/Sv48 translation formats (future BSP reference).
- Hubris MMU abstractions (prior art): https://hubris.oxide.computer/
- seL4 page-table management (prior art): https://sel4.systems/
