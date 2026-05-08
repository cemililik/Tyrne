# T-016 — Activate MMU with identity-mapped kernel + `MapperFlush` token discipline

- **Phase:** B
- **Milestone:** B2 — MMU activation (kernel-half mapping)
- **Status:** Draft
- **Created:** 2026-05-08
- **Author:** @cemililik (+ Claude Opus 4.7 agent)
- **Dependencies:** [ADR-0027](../../../decisions/0027-kernel-virtual-memory-layout.md) — must be `Accepted` before code lands.
- **Informs:** Unblocks the B-phase MMU-touching work that depends on a live MMU surface (frame allocator / PMM, finer-grained kernel section permissions, eventually userspace mappings in B5+). Closes [ADR-0012 §Open questions "Boot-time MMU activation"](../../../decisions/0012-boot-flow-qemu-virt.md). Sets up the future high-half migration (ADR-0033 placeholder) without locking it in today.
- **ADRs required:** [ADR-0027](../../../decisions/0027-kernel-virtual-memory-layout.md). Touches [ADR-0009](../../../decisions/0009-mmu-trait.md) §Revision notes (additive `MapperFlush` return type). No supersession.

---

## User story

As the Tyrne kernel, I want the MMU active with identity-mapped kernel image and MMIO regions, MAIR-attribute-aware mappings, and a `MapperFlush` typed flush-token discipline at the [`Mmu`](../../../../hal/src/mmu.rs) trait surface, so that B3+ MMU-touching work can build on a live translation regime, MMIO accesses obey the device-attribute contract, and TLB-invalidation discipline is enforced at the type system instead of in reviewer attention.

## Context

[ADR-0027](../../../decisions/0027-kernel-virtual-memory-layout.md) settles the B2 layout decision: identity-only mapping, kernel in `TTBR0_EL1`, `TTBR1_EL1` reserved for a future high-half ADR (ADR-0033 placeholder), MAIR indices 0/1 for device-nGnRnE / normal-cached, four bootstrap page-table frames covering the QEMU virt memory map (RAM at `0x4000_0000..0x4800_0000`, GIC + UART at `0x0800_0000..0x0902_0000`). The ADR's §Decision outcome (c) extends the [ADR-0009](../../../decisions/0009-mmu-trait.md) `Mmu` trait so `map` / `unmap` return a typed `MapperFlush` token that the caller must `.flush(mmu)` or `.ignore()` — a `#[must_use]`-decorated newtype that converts "did you remember to flush?" from a reviewer-attention concern into a `unused_must_use` lint failure (denied workspace-wide).

T-016 is the implementation of those decisions per the ADR's §Dependency chain: a single bundled task covering HAL trait extension + BSP `QemuVirtMmu` implementation + linker-script `.boot_pt` reservation + boot-time `mmu_bootstrap` routine + audit-log entries + companion architecture-doc cross-references. The task's shape mirrors [T-012](T-012-exception-and-irq-infrastructure.md) (which bundled GIC + IVT + asm trampolines + timer-IRQ in one task); the implementation may land across multiple commits within the same task.

## Acceptance criteria

- [ ] **ADR-0027 Accepted** before code lands. Same-day Accept after careful re-read is permitted per [ADR-0025 §Revision notes](../../../decisions/0025-adr-governance-amendments.md); Propose commit is separate from the Accept commit per [`write-adr` skill §10](../../../../.claude/skills/write-adr/SKILL.md).

### HAL trait extension (`hal/src/mmu.rs`)

- [ ] **`MapperFlush` newtype added** at the same module level as the [`Mmu`](../../../../hal/src/mmu.rs) trait. Carries a `VirtAddr`. `#[must_use = "MapperFlush carries a TLB-invalidation responsibility …"]`. Two methods: `flush<M: Mmu<…>>(self, mmu: &M)` (executes `mmu.invalidate_tlb_address(self.va)`) and `ignore(self)` (documented no-op for bulk-operation callers who issue a single `invalidate_tlb_all` afterwards). Constructor is `pub(crate)` so only HAL impls (e.g., `QemuVirtMmu`) can mint tokens.
- [ ] **`Mmu::map` return type** changed from `Result<(), MmuError>` to `Result<MapperFlush, MmuError>`.
- [ ] **`Mmu::unmap` return type** changed from `Result<PhysFrame, MmuError>` to `Result<(MapperFlush, PhysFrame), MmuError>` — preserves the unmapped frame the current API returns and adds the token.
- [ ] **`tyrne-test-hal::TestMmu`** updated to return tokens. Existing test-HAL consumers (host tests for kernel-frame logic) updated to `.flush()` / `.ignore()` per the discipline.
- [ ] **ADR-0009 §Revision notes rider** records the additive return-type change. Wording: "the `Mmu` trait's `map` / `unmap` now return a typed `MapperFlush` token that callers must `.flush()` or `.ignore()`; this is an additive surface change that does not extend the user-observable IPC surface and does not supersede ADR-0009's v1 commitments." Mirrors the [ADR-0017 §Revision rider for `ipc_cancel_recv`](../../../decisions/0017-ipc-primitive-set.md) precedent.

### BSP MMU implementation (`bsp-qemu-virt/src/mmu.rs`)

- [ ] **New file `bsp-qemu-virt/src/mmu.rs`** implementing `QemuVirtMmu` for the [`Mmu`](../../../../hal/src/mmu.rs) trait surface. Concrete `AddressSpace` type holds the root `PhysFrame`. Methods:
  - `unsafe fn create_address_space(&self, root: PhysFrame) -> Self::AddressSpace` — wraps the frame; no allocation.
  - `fn address_space_root(&self, as_: &Self::AddressSpace) -> PhysFrame` — accessor.
  - `fn activate(&self, as_: &Self::AddressSpace)` — `MSR TTBR0_EL1, root; ISB; TLBI VMALLE1; DSB ISH; ISB`. (v1 only writes `TTBR0_EL1`; the future high-half ADR-0033 will introduce `TTBR1_EL1` activation.)
  - `fn map(...) -> Result<MapperFlush, MmuError>` — walks the L0/L1/L2/L3 hierarchy, allocating intermediate tables from `frames: &mut dyn FrameProvider` when a higher-level slot is empty. Returns `MapperFlush::new(va)` on success.
  - `fn unmap(...) -> Result<(MapperFlush, PhysFrame), MmuError>` — walks to the leaf, clears the entry, returns the previously-mapped frame paired with `MapperFlush::new(va)`.
  - `fn invalidate_tlb_address(&self, va: VirtAddr)` — `TLBI VAE1, x; DSB ISH; ISB`.
  - `fn invalidate_tlb_all(&self)` — `TLBI VMALLE1; DSB ISH; ISB`.
- [ ] **Page-table descriptor encoding helpers** as `const fn` (where possible) for host-testable encoding logic. At minimum: `block_descriptor(pa, attr_index, ap, sh, af, ng, pxn, uxn) -> u64`, `table_descriptor(pa) -> u64`, `page_descriptor(pa, attr_index, ap, sh, af, ng, pxn, uxn) -> u64`. Per the encoding table in [`docs/architecture/memory-management.md`](../../../architecture/memory-management.md) §"Page-table entry encoding".
- [ ] **`MappingFlags → AttrIndx + AP + UXN + PXN` translation** consolidated in one helper function (`flags_to_descriptor_bits` or similar). One input → one output; testable in isolation.

### Boot-time MMU activation (`bsp-qemu-virt/src/mmu_bootstrap.rs`)

- [ ] **New file `bsp-qemu-virt/src/mmu_bootstrap.rs`** containing `unsafe fn mmu_bootstrap()` — the once-per-boot Rust function called by `kernel_entry` immediately after the `cpu.now_ns()` boot-snapshot and **before any MMIO-touching step** (both the timer banner and the GIC initialisation move to *after* `mmu_bootstrap()` so their UART / GIC writes go through the device-attribute mapping; see §Design notes for the full `kernel_entry` order). Implements the [ADR-0027 §Simulation](../../../decisions/0027-kernel-virtual-memory-layout.md#simulation) sequence:
  - Step 1: populate L0[0], L1[0], L1[1], L2_low[64..73], L2_high[0..64] with the v1 layout entries.
  - Step 2: configure `MAIR_EL1` (device + normal indices), `TCR_EL1` (T0SZ=16, IPS=2, EPD1=1), `TTBR0_EL1 = &__boot_pt_l0`, `TTBR1_EL1 = 0`. Followed by `ISB`.
  - Step 3: `TLBI VMALLE1; DSB ISH; IC IALLU; DSB ISH; ISB; SCTLR_EL1.{M,I,C} = 1; ISB`.
- [ ] **`extern "C" { static __boot_pt_l0: u64; static __boot_pt_l1: u64; static __boot_pt_l2_low: u64; static __boot_pt_l2_high: u64; }`** — linker symbols expose the bootstrap frames to the Rust bootstrap routine. Each `static` is `repr(align(4096))` via the linker script's section alignment; the symbols' *addresses* (not values) are what the routine uses.
- [ ] **`kernel_entry` wired**: call `mmu_bootstrap()` immediately after [`bsp-qemu-virt/src/main.rs:621`](../../../../bsp-qemu-virt/src/main.rs#L621) (the `boot_ns = cpu.now_ns()` snapshot) and **before** any MMIO-touching step (both the timer banner and the GIC initialisation must move to *after* `mmu_bootstrap()` because both write MMIO — UART for the banner, GIC distributor / CPU interface for `gic.init()` — and v1 cannot tolerate either MMIO write happening before the device-attribute mapping is live). The full post-fix `kernel_entry` order is `cpu.now_ns()` snapshot → `mmu_bootstrap()` → "tyrne: mmu activated" print → GIC init → timer banner → demo. Add the `tyrne: mmu activated` marker line printed by `kernel_entry` immediately after the bootstrap returns, so the QEMU smoke trace gains a single new line documenting the activation.

### Linker script (`bsp-qemu-virt/linker.ld`)

- [ ] **`.boot_pt` section reservation**: 4 × 4 KiB frames, page-aligned, bracketed by `__boot_pt_start` / `__boot_pt_end` symbols. Individual frames named `__boot_pt_l0`, `__boot_pt_l1`, `__boot_pt_l2_low`, `__boot_pt_l2_high` so the Rust bootstrap routine can reference them by linker symbol. Placed **inside** the existing `.bss` range so the BSS-zero loop in [`boot.s`](../../../../bsp-qemu-virt/src/boot.s) pre-zeros all four frames before `mmu_bootstrap` populates them.
- [ ] **No relocation of kernel image** — `.text` / `.rodata` / `.data` / `.bss` stay at their current load address (`0x4008_0000+`). v1 is identity-mapped; the linker script's `MEMORY` and `SECTIONS` blocks gain only the `.boot_pt` reservation, not a load-vs-link split.

### Audit-log entries (`docs/audits/unsafe-log.md`)

- [ ] **UNSAFE-2026-0022** — page-table frame writes in `mmu_bootstrap`. Operation: writing block + table descriptors directly to the four bootstrap frames. Invariants: frames are page-aligned, exclusively-owned by `mmu_bootstrap` for the duration of the call, pre-zeroed by the BSS loop. Rejected alternatives: dynamic frame allocation (no PMM yet), in-Rust struct + `core::ptr::write` (more `unsafe` not less).
- [ ] **UNSAFE-2026-0023** — system-register writes in `mmu_bootstrap`. Operation: `MSR MAIR_EL1`, `MSR TCR_EL1`, `MSR TTBR0_EL1`, `MSR TTBR1_EL1`, `MSR SCTLR_EL1`. Invariants: MMU is off when these run; they configure the regime that activates on the SCTLR.M=1 write. Rejected alternatives: piecemeal writes via separate functions (loses the must-run-in-order discipline).
- [ ] **UNSAFE-2026-0024** — TLB / I-cache invalidate asm + barriers. Operation: `TLBI VMALLE1` / `TLBI VAE1` / `IC IALLU` / `DSB ISH` / `ISB`. Invariants: barrier ordering (the `DSB ISH` after `TLBI` ensures the invalidate completes before subsequent translations; the `ISB` ensures pipeline drain before MMU enable). Rejected alternatives: relying on implementation-defined out-of-order behaviour (correctness hazard).
- [ ] **UNSAFE-2026-0025** — per-call `Mmu::map` / `unmap` page-table entry writes inside `QemuVirtMmu`. Operation: writing block / table / page descriptors to existing tables when `Mmu::map` / `unmap` is called post-bootstrap. Invariants: target frames are valid page-table frames in the active address space; per-page TLB invalidate is the caller's responsibility (enforced via the `MapperFlush` token).
- [ ] **All four entries follow** [unsafe-policy.md §3](../../../standards/unsafe-policy.md) Operation / Invariants / Rejected-alternatives shape; reviewer is named (`@cemililik (+ Claude Opus 4.7 agent)`).

### Documentation

- [ ] **`docs/architecture/memory-management.md`** — already drafted in this PR alongside ADR-0027; T-016 verifies that the layout diagrams + MAIR table + page-table-entry encoding table + activation sequence diagram match the implemented code byte-for-byte.
- [ ] **[`docs/architecture/hal.md`](../../../architecture/hal.md) §Mmu** — update to mention the `MapperFlush` token discipline (one-paragraph addition).
- [ ] **[`docs/decisions/0009-mmu-trait.md`](../../../decisions/0009-mmu-trait.md) §Revision notes** — additive rider per the ADR-0017 precedent (above).
- [ ] **[`docs/architecture/boot.md`](../../../architecture/boot.md)** — extend the boot-sequence diagram with the new `mmu_bootstrap` step; cross-link to memory-management.md.

### Verification gates

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo host-clippy` clean (`-D warnings`).
- [ ] `cargo kernel-clippy` clean.
- [ ] `cargo host-test` passes — expected ~165–170 (current 159 + ~10 new for descriptor encoders + flush-token semantics).
- [ ] `cargo +nightly miri test` passes on the same set.
- [ ] `cargo kernel-build` clean.
- [ ] **QEMU smoke unchanged plus one new line, with timer banner repositioned.** The post-T-016 trace adds `tyrne: mmu activated` immediately after the `tyrne: hello from kernel_main` line and **before** the timer banner (because the timer banner moves to after `mmu_bootstrap()` so its UART writes go through the device-attribute mapping). The post-fix trace order is: `tyrne: hello from kernel_main` → `tyrne: mmu activated` → `tyrne: timer ready (...)` → `tyrne: starting cooperative scheduler` → IPC demo lines → `tyrne: all tasks complete` → `tyrne: boot-to-end elapsed = ... ns`. Every line *other than* the new `mmu activated` insertion is byte-for-byte identical to the post-T-015 baseline. The boot-to-end timing increases by the cost of `mmu_bootstrap` (estimated < 100 µs); the [P10 wall-clock harness](../../../analysis/reviews/code-reviews/2026-05-07-pr-12-to-17-multi-axis-review/track-d-perf.md) (landing in PR #21) is the canonical band.
- [ ] **`-d int,unimp,guest_errors` empty** — no Translation Faults, Permission Faults, or unimplemented-instruction warnings during MMU activation. If any fault fires, **stop and diagnose** before merging; per ADR-0027 §Simulation §Step 3, faults at this transition are the load-bearing failure mode.

## Out of scope

- **High-half kernel migration** (TTBR1 active; identity teardown). Deferred to a future ADR (ADR-0033 placeholder) when B5 surfaces the per-task `TTBR0_EL1` swap requirement.
- **Physical-frame allocator (PMM).** v1 uses static reservation (`.boot_pt`); a real PMM is a separate B-phase task. The `Mmu::map` API accepts `&mut dyn FrameProvider` so the trait is PMM-ready; the BSP just doesn't have a PMM caller yet.
- **Sub-2-MiB `Mmu::map` calls that need to split a block descriptor.** v1 has no caller exercising this; the BSP's `Mmu::map` returns `MmuError::AlreadyMapped` if the target VA already has a block-descriptor mapping, and a follow-on B-phase task introduces the block-split logic when first needed.
- **Per-section permissions on the kernel image** (`.text` RX, `.rodata` R, `.bss/.data` RW). v1 maps the entire 128 MiB RAM range as kernel R/W/X via 2 MiB blocks; finer-grained kernel-image permissions await a follow-on task that re-maps the kernel-image region into 4 KiB pages with section-specific flags.
- **Multi-core TLB shootdown.** v1 is single-core; [ADR-0009 §Open questions](../../../decisions/0009-mmu-trait.md#open-questions) tracks multi-core TLB invalidation as a future ADR.
- **Page-fault routing into the capability system.** v1 vector table panics on synchronous EL1 exceptions; routing faults through to the capability surface awaits a future ADR (likely paired with the syscall ABI in ADR-0030).
- **ASID assignment per task.** v1 uses ASID=0 globally (`nG=0` in every page-table entry). Per-task ASIDs land with ADR-0033 (high-half migration) when `TTBR0_EL1` swap becomes per-task.
- **`MappingFlags::USER` exercise.** Encoded correctly by `QemuVirtMmu::map` (host-tested) but unused in v1 (no userspace yet). Future B5 work activates the path.
- **Translation-walk queries (`Mmu::lookup`)** — not in [ADR-0009](../../../decisions/0009-mmu-trait.md)'s v1 surface; not added by T-016.

## Approach

The implementation lands in roughly six independently-bisectable commits. Each commit ends green (`cargo host-test`, `kernel-clippy`); QEMU smoke is verified at the *end* of the chain (after step 6) because intermediate commits leave the kernel image with the bootstrap routine wired but not yet enabled.

1. **HAL `MapperFlush` token** — `hal/src/mmu.rs` gains the `MapperFlush` newtype; `Mmu::map` / `unmap` return-type signatures change. `tyrne-test-hal::TestMmu` updated to mint tokens. ADR-0009 §Revision notes rider lands. No BSP code yet; the kernel does not call `Mmu::map` post-bootstrap in v1, so the rest of the workspace continues to compile. Host tests added: `mapper_flush_must_use_lints_unused_token`, `mapper_flush_flush_invokes_invalidate_tlb_address`, `mapper_flush_ignore_is_documented_noop`.
2. **`bsp-qemu-virt/src/mmu.rs` — descriptor encoding helpers** — pure functions (`block_descriptor` / `table_descriptor` / `page_descriptor` / `flags_to_descriptor_bits`). Host-testable. No `unsafe`. Tests pin every `MappingFlags` permutation against the encoding table in [`docs/architecture/memory-management.md`](../../../architecture/memory-management.md) §"Page-table entry encoding".
3. **`bsp-qemu-virt/src/mmu.rs` — `QemuVirtMmu` impl skeleton** — `create_address_space` / `address_space_root` / `activate` / `invalidate_tlb_address` / `invalidate_tlb_all` (the methods that don't need `FrameProvider`). `map` and `unmap` are stubbed with `unimplemented!()` in this commit; the next commit fills them in. UNSAFE-2026-0023 (system-register writes in `activate`) and UNSAFE-2026-0024 (TLB asm) audit entries land here.
4. **`bsp-qemu-virt/src/mmu.rs` — `Mmu::map` / `unmap` body** — page-table walk + descriptor encoding + frame allocation from `FrameProvider` + `MapperFlush::new(va)` return. UNSAFE-2026-0025 (per-call descriptor writes) audit entry lands here. Tests: `map_into_empty_address_space_routes_through_frame_provider`, `map_returns_already_mapped_when_va_already_present`, `unmap_returns_frame_and_flush_token`.
5. **`bsp-qemu-virt/linker.ld` + `mmu_bootstrap`** — `.boot_pt` reservation; `__boot_pt_*` linker symbols; `bsp-qemu-virt/src/mmu_bootstrap.rs` containing the boot-time activation routine. UNSAFE-2026-0022 (bootstrap page-table writes) audit entry lands here. The routine is not yet called from `kernel_entry` — that's step 6. Tests verify the descriptor-construction logic (host-testable) but not the asm (QEMU-smoke verified).
6. **`kernel_entry` wiring + smoke verification** — call `mmu_bootstrap()` in `kernel_entry` at the right place; print `tyrne: mmu activated`. Run QEMU smoke; verify `tyrne: all tasks complete` still appears, the new `mmu activated` line is present, and `-d int,unimp,guest_errors` is empty. This commit is where the kernel actually *uses* the MMU.

## Definition of done

- [ ] All Acceptance criteria checked.
- [ ] All six commits are independently bisectable (each ends with green host-tests + clippy + kernel-build).
- [ ] QEMU smoke trace matches the post-T-015 baseline plus one new `tyrne: mmu activated` line.
- [ ] `cargo +nightly miri test` is green.
- [ ] No new `unsafe` block lacks a `// SAFETY:` comment naming the audit-log entry per [unsafe-policy.md §1](../../../standards/unsafe-policy.md).
- [ ] [`docs/audits/unsafe-log.md`](../../../audits/unsafe-log.md) gains UNSAFE-2026-0022 / 0023 / 0024 / 0025 with full Operation / Invariants / Rejected-alternatives shape.
- [ ] [`docs/decisions/0009-mmu-trait.md`](../../../decisions/0009-mmu-trait.md) §Revision notes records the additive `MapperFlush` rider.
- [ ] [`docs/architecture/memory-management.md`](../../../architecture/memory-management.md) is byte-stable from the ADR-0027 PR (this task verifies it matches the code; does not rewrite it).
- [ ] [`docs/architecture/hal.md`](../../../architecture/hal.md) §Mmu mentions the `MapperFlush` discipline.
- [ ] [`docs/architecture/boot.md`](../../../architecture/boot.md) records the `mmu_bootstrap` step.
- [ ] [`docs/roadmap/current.md`](../../../roadmap/current.md) updated: T-016 → Done; ADR-0027 → Accepted; B2 status → "MMU activated; B3+ work proceeds".
- [ ] [`docs/roadmap/phases/phase-b.md`](../../../roadmap/phases/phase-b.md) ADR ledger row for ADR-0027 → Accepted; T-016 marked Done; B2 status updated.
- [ ] Commit messages follow [`commit-style.md`](../../../standards/commit-style.md) with `Refs: ADR-0027, T-016` trailers and `Audit: UNSAFE-2026-NNNN` trailers where applicable.
- [ ] PR description includes the Simulation table from [ADR-0027 §Decision outcome / §Simulation](../../../decisions/0027-kernel-virtual-memory-layout.md#simulation) and the post-MMU smoke trace verbatim.

## Design notes

- **Why one bundled task instead of T-016 + T-017 + T-018?** Mirrors [T-012](T-012-exception-and-irq-infrastructure.md) which bundled GIC + IVT + asm trampolines + timer-IRQ in one task. The split would be artificial: HAL trait extension, BSP implementation, and bootstrap activation are tightly coupled at the type-system level (the trait change is a return-type change; the BSP impl honours it; the bootstrap consumes it). A single task with six bisectable commits is cleaner than three tasks each with two commits.
- **Why bootstrap routine in Rust, not in `boot.s`?** The page-table descriptor encoding logic benefits from `const fn` helpers, host-testable in isolation. Doing it in asm would require duplicating the encoding logic across the asm side and any future Rust-side `Mmu::map` impl, with the asm copy un-testable. The transition step (`SCTLR.M=1`) is asm-bare-metal-instructions only; that lives in `mmu_bootstrap` as `core::arch::asm!` blocks but is small (~30 lines).
- **Why `MapperFlush::flush(self, mmu: &impl Mmu)` instead of carrying a `&Mmu` reference in the token?** Carrying the reference would require a lifetime parameter on `MapperFlush`, complicating the return-type chain through `?`-cascades. Mirrors `x86_64::structures::paging::MapperFlush` (Rust ecosystem prior art for the same problem); the discipline is "the caller knows which mapper they used and passes it explicitly". One additional method call at every flush site; readability win > line-saving.
- **Why `.ignore()` as a separate method instead of just relying on `Drop`?** `Drop` would either run silently (defeats `#[must_use]`) or panic (gives up the lint-time error in favour of a runtime error). `.ignore()` is a documented intent that the type system can see and the reviewer can verify. The asymmetry between `flush` and `ignore` is the discipline.
- **Why identity-only and not high-half?** Per [ADR-0027 §Decision outcome / Why Option D beats the alternatives](../../../decisions/0027-kernel-virtual-memory-layout.md). Methodical pace: B5 is the natural moment for high-half because that's when `TTBR0_EL1` swap becomes per-task. Doing it in B2 pre-pays the cost without obtaining the benefit until B5.
- **Why 2 MiB blocks at L2 instead of 4 KiB pages?** Per [`docs/architecture/memory-management.md`](../../../architecture/memory-management.md) §"v1 layout" / "Why 2 MiB blocks instead of 4 KiB pages". 32 768 L3 entries (256 KiB of bootstrap tables) for 128 MiB of identity is disproportionate; 64 L2 block descriptors do the same job in 4 KiB.
- **Why `mmu_bootstrap` runs before any MMIO-touching step?** Both the GIC distributor / CPU interface writes (`gic.init()`) and the timer banner's UART writes are MMIO-attribute-sensitive: without device-nGnRnE attributes, speculative reads can produce phantom interrupts and merging on UART writes can re-order TX. The MMU must be on with the device attributes encoded before *either* MMIO write happens. **The post-fix `kernel_entry` order** is `cpu.now_ns()` snapshot → `mmu_bootstrap()` → "tyrne: mmu activated" print → GIC init → timer banner → demo. Both the timer banner and `gic.init()` move to after `mmu_bootstrap()` from their pre-T-016 positions; the boot-to-end timing baseline still includes MMU activation cost (`now_ns` is sampled before the bootstrap), so post-T-016 boot-to-end is comparable to the pre-T-016 baseline modulo the (~< 100 µs) bootstrap addition.

## References

- [ADR-0027 — Kernel virtual memory layout (B2 — identity-mapped MMU activation)](../../../decisions/0027-kernel-virtual-memory-layout.md) — the load-bearing design document.
- [ADR-0009 — `Mmu` HAL trait signature (v1)](../../../decisions/0009-mmu-trait.md) — the trait this task extends with `MapperFlush`.
- [ADR-0012 — Boot flow and memory layout for `bsp-qemu-virt`](../../../decisions/0012-boot-flow-qemu-virt.md) — the static image layout this task inherits and §Open questions "Boot-time MMU activation" closes here.
- [ADR-0024 — EL drop to EL1 policy](../../../decisions/0024-el-drop-policy.md) — kernel runs at EL1 when MMU activates.
- [ADR-0025 — ADR governance amendments](../../../decisions/0025-adr-governance-amendments.md) — §Rule 1 governs T-016 opening alongside ADR-0027 Propose.
- [`docs/architecture/memory-management.md`](../../../architecture/memory-management.md) — companion architecture chapter; lands in the same PR as ADR-0027.
- [`docs/audits/unsafe-log.md`](../../../audits/unsafe-log.md) — UNSAFE-2026-0022 through 0025 land with this task.
- [`docs/standards/unsafe-policy.md`](../../../standards/unsafe-policy.md) — audit-discipline contract every new entry follows.
- [`hal/src/mmu.rs`](../../../../hal/src/mmu.rs) — the trait this task extends.
- [`bsp-qemu-virt/src/mmu.rs`](../../../../bsp-qemu-virt/src/mmu.rs) — new file; lands with this task.
- [`bsp-qemu-virt/src/mmu_bootstrap.rs`](../../../../bsp-qemu-virt/src/mmu_bootstrap.rs) — new file; lands with this task.
- [`bsp-qemu-virt/linker.ld`](../../../../bsp-qemu-virt/linker.ld) — extended with `.boot_pt` reservation in this task.
- [T-012 — Exception infrastructure and interrupt delivery](T-012-exception-and-irq-infrastructure.md) — task-shape precedent for bundling multiple-concern work in one task.
- ARM *Architecture Reference Manual* (ARMv8-A), ARM DDI 0487 — §D5 (VMSAv8 translation), §D7 (system registers).

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-05-08 | @cemililik (+ Claude Opus 4.7 agent) | Opened with status `Draft`, paired with ADR-0027 (`Proposed`) per [ADR-0025 §Rule 1](../../../decisions/0025-adr-governance-amendments.md) (forward-reference contract) — ADR-0027's *Dependency chain* requires a real T-NNN file for the implementation step; this task is that file. Will move to `In Progress` only after ADR-0027 is `Accepted`. |
