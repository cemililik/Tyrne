# 0027 — Kernel virtual memory layout (B2 — identity-mapped MMU activation)

- **Status:** Proposed
- **Date:** 2026-05-08
- **Deciders:** @cemililik

## Context

Phase B2 activates the MMU. Until this milestone the kernel runs with translation off — every address is a physical address, the data and instruction caches operate in their post-reset (mostly disabled) state, and there is no way to differentiate device-MMIO accesses from normal-RAM accesses at the architectural level. The next milestones (B3 onwards) want a programmable address-space surface for capability-mediated grants, MMIO that obeys the device-attribute discipline, and (eventually, in B5) a per-task user half. None of that is reachable while `SCTLR_EL1.M = 0`.

The B2 milestone scope, per [phase-b.md §B2](../roadmap/phases/phase-b.md), is "MMU activation (kernel-half mapping)". The phase-plan note explicitly enumerates three sub-decisions ADR-0027 must settle:

1. **Identity vs. high-half split.** Where does the kernel image live in virtual address space the moment the MMU comes on? Identity-mapped at its physical load address, or relocated to a high-half base (ARM convention `0xFFFF_FFFF_8000_0000+`)?
2. **Memory-type attributes.** `MAIR_EL1` carries up to eight named attribute encodings; the kernel must commit to which indices represent which memory types — at minimum *normal cached* for RAM and *device-nGnRnE* for MMIO — so that page-table entries can encode the right `AttrIndx` value.
3. **TLB-invalidation discipline.** Every mapping mutation needs a matching TLB invalidate. Forgetting that step is a class-of-bug that produces stale-translation hazards which only surface under load. The HAL's [`Mmu`](../../hal/src/mmu.rs) trait currently returns `Result<(), MmuError>` from `map` / `unmap` — leaving the *did-the-caller-flush?* question to reviewer judgement. Should the trait surface make the responsibility unmissable?

The MMU-activation moment is itself a multi-step state-machine transition: the kernel runs at PA `0x4008_0000` with `SCTLR_EL1.M = 0`; we build page tables; we configure `MAIR_EL1`, `TCR_EL1`, `TTBR0_EL1`; we set `SCTLR_EL1.M = 1`; from the next instruction onwards the PC and every load go through the TLB. Getting any of those steps in the wrong order produces an instruction-fetch fault on the very next instruction — a class of hazard the [2026-05-06 B1 smoke regression](../analysis/reviews/business-reviews/2026-05-06-B1-smoke-regression.md) taught the project to walk through with a §Simulation table before Accept. ADR-0027 is the **first ADR drafted under [`write-adr` skill](../../.claude/skills/write-adr/SKILL.md) §Simulation discipline going forward** (ADR-0026's table was the empirical retro-source; ADR-0032's was the first application; this is the first *non-recovery-primitive* state machine to use the rule).

The decision is load-bearing for the next four ADRs the phase-b ledger reserves: ADR-0028 (address-space data structure) inherits the `AddressSpace` shape from this ADR's TTBR / page-table topology; ADR-0029 (initial userspace image format) inherits the kernel-vs-user VA boundary settled here; ADR-0030 (syscall ABI) inherits the page-fault / capability-grant story; ADR-0031 / future MMU follow-ups (ASID assignment, copy-on-write, huge pages) all build on the same layout.

Out of scope of ADR-0027 (deferred by reference, not relitigated): per-page flag updates ([ADR-0009 §Open questions](0009-mmu-trait.md#open-questions)), huge-page block mappings as a first-class trait surface (block descriptors at L2 are used during bootstrap only), multi-core TLB shootdown, ASID assignment, copy-on-write, and translation-walk queries.

## Decision drivers

- **Methodical pace.** The project's standing rule (CLAUDE.md non-negotiable #6) is "minimum required surface per milestone". B2 needs *MMU on with caching enabled and MMIO type-aware*; it does **not** need *userspace context-switch readiness*. Choices that defer userspace-shape complexity are preferred when they are reversible without code surgery.
- **Future-userspace compatibility without user code today.** B5 introduces userspace; that ADR (currently reserved as ADR-0030 for syscall ABI; the high-half migration would be a separate ADR slot) needs to swap `TTBR0_EL1` per task. The B2 layout must not paint the project into a corner where adding the user half later requires rewriting the kernel layout.
- **Bounded `unsafe` surface.** Page-table writes, system-register writes, and TLB invalidation all add to the audit-log. The choice of layout determines how many sites we audit; complex layouts (high-half + identity + transition + teardown) audit more sites than simple ones (identity-only).
- **Reproducible bootstrap.** Boot-time page tables must live in a predictable, statically-sized region (ADR-0009's "frame allocation is the kernel's responsibility" principle, applied at the bootstrap moment when there is no PMM yet). The layout decision determines how many bootstrap frames are needed.
- **Standard practice across reference kernels.** Linux, FreeBSD, NetBSD, seL4, and Hubris all enable the MMU early in boot; Linux/FreeBSD/NetBSD use a high-half kernel; seL4 uses a high-half kernel; Hubris (Cortex-M, no MMU) is not directly comparable. The "identity for now, high-half later" path is the *Linux 0.x → 1.x evolution*, also seen in NetBSD's evbarm port — well-trodden if we choose it.
- **Type-system safety for the mutation discipline.** Rust gives us the `#[must_use]` attribute and unique-lifetime-bound types; we can compile-fail callers who forget to flush after a mapping mutation, instead of relying on reviewer attention. The cost is one new type per HAL trait return; the win is "TLB-invalidation forgotten" becoming a class of bug the type system rejects.
- **Compatibility with [ADR-0009](0009-mmu-trait.md)'s `Mmu` trait.** v1 trait surface: `create_address_space` / `address_space_root` / `activate` / `map` / `unmap` / `invalidate_tlb_address` / `invalidate_tlb_all`. ADR-0027 cannot break this surface; it can extend the return types of `map` / `unmap` and add helper types, but cannot retract or rename existing methods.

## Considered options

1. **Option A — Identity-only mapping; no high-half migration in B2; `Mmu::map` / `unmap` keep their `Result<(), MmuError>` shape (no flush token).** Smallest possible B2 surface. MMU is enabled; kernel image and MMIO are mapped at their physical addresses; `TTBR1_EL1` left zero. TLB-invalidation discipline stays in code-comments and reviewer attention.
2. **Option B — Identity-only mapping; no high-half migration; `Mmu::map` / `unmap` return a typed `MapperFlush` token that the caller must `.flush(mmu)` or `.ignore()`.** Same layout as Option A but adds the type-system-enforced flush discipline.
3. **Option C — Identity-mapped during bootstrap, jump to kernel high-half (`0xFFFF_FFFF_8008_0000+`), tear down identity in `TTBR0_EL1`; `MapperFlush` token included.** The "Linux-on-aarch64" canonical shape, applied at B2.
4. **Option D — Identity-only mapping with the flush token; defer the high-half decision to a clearly-named *future ADR* (e.g., ADR-0033 "Kernel high-half migration"); commit to the layout shape that supports both today's identity model AND tomorrow's high-half migration without re-doing this ADR's body.** Scoped middle ground: settles B2 at identity, settles the *flush-token* discipline (HAL surface change is permanent regardless of which way we go on identity-vs-high-half), and explicitly forward-flags the future migration as a known-and-named follow-up rather than an unknown-unknown. **This is the chosen option (see Decision outcome below).**

## Decision outcome

Chosen option: **Option D — Identity-only mapping for B2, `MapperFlush` typed flush-token discipline, high-half migration deferred to a clearly-named future ADR (ADR-0033 placeholder; opens when B5 userspace work surfaces the per-task `TTBR0_EL1` swap requirement).**

The decision splits the three sub-questions of the §Context:

### (a) Layout — identity, with TTBR1 reserved

- **Translation regime.** 4 KiB granule, 48-bit virtual addresses, four-level translation (L0 → L1 → L2 → L3). Locked by [ADR-0009](0009-mmu-trait.md)'s `PAGE_SIZE = 4096` constant and the QEMU `virt` Cortex-A72 target's 40-bit physical-address support (more than enough for 128 MiB of v1 RAM; future Pi 4 has 4–8 GiB and stays inside the 40-bit IPS range).
- **`TTBR0_EL1` holds the bootstrap identity table.** Identity-maps two regions:
  - `0x4000_0000 .. 0x4800_0000` (128 MiB RAM) as **normal cached** memory.
  - `0x0800_0000 .. 0x0902_0000` (≈18 MiB covering the GIC distributor / GIC CPU interface / PL011 UART) as **device-nGnRnE** memory.
- **`TTBR1_EL1 = 0` (effectively disabled by `TCR_EL1.EPD1 = 1`).** Kernel runs at `TTBR0_EL1`-mapped identity addresses for the entire B2 milestone. The reservation is structural, not active.
- **`MAIR_EL1`.** Two attribute encodings:
  - Index 0: **device-nGnRnE** (`0x00`) — non-Gathering, non-Reordering, non-Early-write-acknowledgement; the strictest device-memory mode, appropriate for GIC + UART control registers.
  - Index 1: **normal cached, write-back, write-allocate, inner+outer shareable** (`0xFF`) — the mainline RAM attribute.
  - Indices 2–7 reserved (zero-initialised) for future ADR extension (e.g., index 2 = device-GRE for less-strict device regions; index 3 = normal-uncached for MMIO-DMA buffers).
- **`TCR_EL1`.** `T0SZ = 16` (48-bit VA), `TG0 = 0b00` (4 KiB granule), `IPS = 0b010` (40-bit IPA), `SH0 = 0b11` (inner shareable), `IRGN0 = ORGN0 = 0b01` (write-back write-allocate cacheable for page-table walks), `EPD0 = 0` (translations enabled), `EPD1 = 1` (`TTBR1_EL1` walks disabled in v1; flipped in the future high-half ADR).
- **`SCTLR_EL1`.** `M = 1` (MMU on), `C = 1` (D-cache enabled), `I = 1` (I-cache enabled). All other bits left at their reset / pre-existing values.
- **ASID.** Single global address space in v1; `TCR_EL1.AS = 0` (8-bit ASID field; not actively used). All bootstrap mappings are *global* (ARM `nG` bit clear → `MappingFlags::GLOBAL` semantics — the v1 `MappingFlags::GLOBAL` already exists per [ADR-0009](0009-mmu-trait.md), so no HAL surface change is needed here). Per-task ASID assignment lands with the future high-half ADR's `TTBR0_EL1`-swap discipline.

### (b) Memory-type discipline — MAIR + MappingFlags

- **`MappingFlags::DEVICE`** (already in [ADR-0009](0009-mmu-trait.md)) maps to MAIR index 0 (device-nGnRnE). The BSP's `Mmu::map` implementation is responsible for translating `flags.contains(DEVICE)` into the right `AttrIndx` value in the page-table entry.
- **Normal RAM** is the implicit default when `DEVICE` is not set: MAIR index 1.
- This is a one-bit discrimination today. When richer device modes (write-combining, non-cacheable RAM for DMA buffers) are needed, a future ADR introduces a `MemoryType` enum field on `MappingFlags` (or a new `mapping_type` parameter on `Mmu::map`) and adds the corresponding MAIR indices. Until then, the BSP's translation table is `DEVICE → 0, !DEVICE → 1`.

### (c) Mutation discipline — `MapperFlush` typed token

- **`Mmu::map` and `Mmu::unmap` change return type from `Result<(), MmuError>` to `Result<MapperFlush, MmuError>`** (and `Result<(MapperFlush, PhysFrame), MmuError>` for `unmap`, preserving the unmapped frame the current API returns).
- **`MapperFlush` is a `#[must_use]` newtype carrying a `VirtAddr`** that is consumed by either `flush(mmu: &impl Mmu)` (which executes `mmu.invalidate_tlb_address(va)` on the held address) or `ignore()` (a documented no-op for callers performing bulk operations who will issue a single `invalidate_tlb_all` afterwards). Forgetting to handle the token is a `unused_must_use` lint failure — the project's workspace lint config promotes this to a deny in the kernel crate.
- The surface is **additive** in the [ADR-0017](0017-ipc-primitive-set.md) sense: the existing `Mmu::activate` / `invalidate_tlb_address` / `invalidate_tlb_all` methods stay byte-stable; only `map` / `unmap` return-types grow. ADR-0009 §Revision notes records the additive change. No callers in v1 use these methods yet (B2 is the first consumer); the API breakage cost is zero callers today.

### Why Option D beats the alternatives

- **Beats Option A:** Option A skips the flush token, leaving TLB-invalidation discipline to reviewer attention. The 2026-05-06 B1 smoke regression's "what we learned" lesson (codified in the §Simulation rule) is *type-system-enforce the discipline where you can*; the token is the type-system-side enforcement of the same discipline at the MMU surface. Option A is fast-to-write but gives up a free correctness win.
- **Beats Option B:** Option B is *almost* this option — same layout, same flush token — but does not name the future high-half migration explicitly. Option D adds the named-future-ADR forward-flag so a B5 reader does not need to reverse-engineer "wait, where does kernel live in user-half VA?" by reading commit history. The cost is one paragraph of documentation (this section); the win is reader-affordance for the next year of the project.
- **Beats Option C:** Option C lands the high-half migration *now*, in B2. The implementation cost is significant (linker-script `AT > RAM` discipline, two-stage early-boot stub, identity teardown after jump-to-high-half, and more `unsafe` audit entries — minimum 4 new entries vs Option D's minimum 2). The benefit (no future ADR) is real but premature: B2's userspace surface is empty; B3 / B4 work does not need the high-half. Per CLAUDE.md non-negotiable #6 ("methodical, phased progress"), B5 is the natural moment to introduce high-half because that is when `TTBR0_EL1`-swap becomes load-bearing. Doing it in B2 pre-pays the cost without obtaining the benefit until B5.

### Simulation

The MMU-activation moment is the worst-case interaction. The table walks the kernel from "MMU off, running at PA `0x4008_0000`" to "MMU on, running at the same identity-mapped VA, with caching active" under the chosen Option D shape:

| Step | State pre | Action | State post | Switch target / observable effect |
|------|-----------|--------|------------|-----------------------------------|
| 0 | `SCTLR_EL1.M = 0`; PC at PA `0x4008_NNNN`; caches off; `TTBR0_EL1` undefined; `MAIR_EL1` undefined; `TCR_EL1` undefined | Reserved page-table frames at PAs `__boot_pt_l0` … `__boot_pt_l2_high` exist in `.boot_pt` (statically allocated, pre-zeroed by `_start`'s BSS loop because `.boot_pt` is bracketed by `__bss_start`/`__bss_end`); kernel begins `mmu_bootstrap` Rust function | unchanged | — |
| 1 | as Step 0; bootstrap page-table frames are zero | Populate L0[0] = table-pointing-at-L1 (with `Type=table, Valid=1`); L1[0] = table-pointing-at-L2_low (for MMIO range); L1[1] = table-pointing-at-L2_high (for RAM range); L2_low[64..72] = block-descriptors covering `0x0800_0000..0x0900_0000` with `AttrIndx=0` (device-nGnRnE), `AP=00` (kernel R/W), `SH=00` (non-shareable for device), `AF=1`, `nG=0`; L2_low[72] = block at `0x0900_0000` (UART) with same; L2_high[0..64] = block-descriptors covering `0x4000_0000..0x4800_0000` with `AttrIndx=1` (normal cached), `AP=00`, `SH=11` (inner shareable), `AF=1`, `nG=0` | All 4 bootstrap frames populated; PC still at PA; MMU still off | — |
| 2 | bootstrap frames populated; MMU off | `MSR MAIR_EL1, mair_value` (encoding device + normal); `MSR TCR_EL1, tcr_value` (`T0SZ=16, TG0=0, IPS=2, SH0=3, IRGN0/ORGN0=1, EPD0=0, EPD1=1`); `MSR TTBR0_EL1, &__boot_pt_l0`; `MSR TTBR1_EL1, 0`; `ISB` | system regs configured; MMU still off (SCTLR.M=0); ISB ensures the system-register writes are observed before any MMU enable | — |
| 3 | system regs configured; MMU off | TLB invalidate (`TLBI VMALLE1`) + `DSB ISH` (ensure invalidate completes) + `IC IALLU` (invalidate I-cache) + `DSB ISH` + `ISB`; then `MRS x0, SCTLR_EL1`; set bits `M`, `I`, `C` (and clear bits we explicitly want zero); `MSR SCTLR_EL1, x0`; `ISB` | `SCTLR_EL1.M = 1`; ISB drains the pipeline so the next instruction-fetch goes through the freshly-installed translation regime; PC still at PA `0x4008_NNNN` (which is identity-mapped, so the translation walks succeed and yield the same PA) | **Critical step:** any error here (typo'd page-table entry, wrong attribute index, off-by-one VA range, missing `AF` access flag, `EPD0` accidentally `1`) produces a Translation Fault on the very next instruction-fetch — caught by either a CPU exception (if the vectors are installed and the fault is a kernel-mode synchronous exception) or by QEMU `-d int,unimp,guest_errors` reporting a fault. The §Simulation table is itself the *list of things to triple-check before flipping the bit*. |
| 4 | MMU on; PC at identity-mapped PA; caches on; bootstrap mappings live | `mmu_bootstrap` returns to `kernel_entry`'s caller; rest of kernel proceeds with MMU active | unchanged | The Rust-side kernel from this point onwards observes (a) memory accesses to the RAM range have the cache attributes for normal-cached, write-back, write-allocate; (b) accesses to GIC + UART have device-nGnRnE semantics, no speculative read, no merging; (c) any subsequent `Mmu::map` / `unmap` returns a `MapperFlush` token the caller must explicitly discharge. |

The 5-step shape is identical to Linux's aarch64 boot's `__cpu_setup` → `__primary_switch` flow modulo the high-half jump (which Option D omits in v1). Simulation rows 2 and 3 are the two failure-class moments; row 4 documents the steady-state contract that B3+ MMU work inherits.

### Dependency chain

For this decision to be fully in effect:

```text
1. Extend [`hal::mmu`](../../hal/src/mmu.rs) with the `MapperFlush`
   typed flush token; change `Mmu::map` / `Mmu::unmap` return types
   to thread the token. Update the in-tree `test-hal` impl
   (currently `tyrne-test-hal::TestMmu`) to return tokens. — T-016
   (Draft, opens with this ADR)
2. Implement `QemuVirtMmu` in [`bsp-qemu-virt/src/mmu.rs`](../../bsp-qemu-virt/src/mmu.rs)
   covering `Mmu::create_address_space` / `address_space_root` /
   `activate` / `map` / `unmap` / `invalidate_tlb_*` for VMSAv8. — T-016
3. Reserve the four bootstrap page-table frames in `bsp-qemu-virt/linker.ld`
   as a new `.boot_pt` section (`PAGE_SIZE`-aligned, sized for L0 + L1
   + L2_low + L2_high; bracketed by `__boot_pt_start` / `__boot_pt_end`
   linker symbols; placed inside `.bss` so the existing BSS-zero loop
   pre-zeros them). — T-016
4. Implement `mmu_bootstrap` in `bsp-qemu-virt/src/main.rs` (or a
   dedicated `bsp-qemu-virt/src/mmu_bootstrap.rs` module): populate
   the four boot frames per the §Simulation §Step 1 layout, configure
   `MAIR_EL1` / `TCR_EL1` / `TTBR0_EL1` / `TTBR1_EL1`, perform the
   TLB + I-cache invalidate + barrier sequence of §Step 3, then flip
   `SCTLR_EL1.{M,I,C}`. Called once by `kernel_entry` between the
   timer banner and the GIC initialisation (so `gic.init()`'s MMIO
   writes go through the device-attribute mapping). — T-016
5. Add audit-log entries: UNSAFE-2026-0022 (page-table frame writes
   in `mmu_bootstrap`), UNSAFE-2026-0023 (`MAIR_EL1` / `TCR_EL1` /
   `TTBR0_EL1` / `TTBR1_EL1` / `SCTLR_EL1` writes), UNSAFE-2026-0024
   (`TLBI` / `IC IALLU` / `DSB` / `ISB` asm), UNSAFE-2026-0025
   (per-call `Mmu::map` / `unmap` page-table entry writes inside
   `QemuVirtMmu`). Per [unsafe-policy.md §3](../standards/unsafe-policy.md),
   each entry includes Operation / Invariants / Rejected alternatives. — T-016
6. Update [`docs/architecture/memory-management.md`](../architecture/memory-management.md)
   (new file landing in this PR) with the layout diagram, MAIR
   table, page-table topology, and `MapperFlush` discipline.
   Cross-linked from [ADR-0009](0009-mmu-trait.md), [ADR-0027](0027-kernel-virtual-memory-layout.md),
   and [ADR-0012](0012-boot-flow-qemu-virt.md) §Open questions
   (where the "Boot-time MMU activation" entry now resolves to this
   ADR). — T-016
7. Add host tests for the page-table descriptor encoding helpers
   (`block_descriptor(pa, attr_index, ap, sh, af, ng)` and friends);
   the asm-level transition is QEMU-smoke verified. Tests live in
   `bsp-qemu-virt/src/mmu.rs::tests` under `#[cfg(test)]`. — T-016
8. Update `current.md` headline + `phase-b.md` ADR ledger row.
   Closure trio is **not** required for T-016 in isolation; T-016
   `Done` flips on (cargo gates + miri + smoke unchanged); the B2
   *milestone* closure trio runs when the milestone closes (after
   any follow-on B2 tasks land). — T-016
```

The first task (T-016) covers steps 1 through 8. No further task is opened by this ADR. T-016 is a **single bundled task** — the same shape as T-012 (which bundled GIC + IVT + asm trampolines + timer-IRQ in one task). The implementation may land across multiple commits within T-016's scope; splitting into T-016a / T-016b is permitted if the scope grows past one reviewable task per [phase-b.md](../roadmap/phases/phase-b.md) precedent.

T-016's `Done` flip gates only on its own DoD (host-tests + miri + clippy + kernel-build + smoke-trace-byte-for-byte-unchanged-pre-MMU plus a *new* trace line "tyrne: mmu activated" or equivalent confirming the post-MMU steady state); it does not require a closure trio.

**ADR-0033 (high-half kernel migration) placeholder.** A future ADR will introduce the high-half kernel mapping when B5 userspace work surfaces the per-task `TTBR0_EL1` swap requirement. The placeholder slot is reserved (per [ADR-0025 §Rule 1](0025-adr-governance-amendments.md), no T-NNN is opened today because no implementation work depends on it before B5). When B5's first userspace-driven scheduling event arrives, ADR-0033 opens with a §Simulation table walking the high-half migration's own multi-step transition (build TTBR1_EL1 high-half tables → enable EPD1 → jump to high-half via absolute load → null TTBR0_EL1 / EPD0=1 → continue at high-half).

## Consequences

### Positive

- **MMU on with caching enabled.** B3+ kernel work benefits from D-cache + I-cache active, and from MAIR-attribute-aware MMIO accesses (no more relying on QEMU's MMU-off semantics being lenient about device-vs-RAM mixing).
- **Type-system-enforced TLB-invalidation discipline.** The `MapperFlush` token converts "did you remember to flush?" from a reviewer-attention concern into a `unused_must_use` lint failure. Pattern-of-record for future HAL traits where mutation requires a follow-up step.
- **Userspace-readiness inherited for free.** `TTBR0_EL1` already holds the kernel mapping; the future high-half ADR moves kernel to `TTBR1_EL1` and reuses `TTBR0_EL1` for per-task user mappings. The boundary is documented today.
- **Bounded `unsafe` surface.** Four new audit entries (UNSAFE-2026-0022 through 0025), all narrowly-scoped per [unsafe-policy.md §1](../standards/unsafe-policy.md). The same audit pattern T-012 used (one entry per concern) applies.
- **Bounded bootstrap frame budget.** Four 4 KiB frames (16 KiB total) for the entire boot-time mapping. Statically reserved in `.boot_pt`; no kernel allocator dependency for the bootstrap moment.
- **Smoke-trace continuity.** The kernel image at PA `0x4008_0000` continues to run at the same address pre- and post-MMU; no PC relocation, no linker-script `AT > RAM` discipline, no two-stage boot. The QEMU smoke trace adds at most one new line ("tyrne: mmu activated") and is otherwise byte-identical to the post-T-015 baseline. Clean regression-detection.

### Negative

- **The high-half migration is deferred, not skipped.** The future ADR-0033 will require linker-script changes, a brief identity-bootstrap-and-jump dance, and audit-log Amendments. Real cost; we accept it because the v1 useful work in B3 / B4 does not need the high-half today and the methodical-pace principle outranks the "do it once" gut instinct. *Mitigation:* ADR-0033 is named in this ADR's §Dependency chain explicitly, and the §Simulation table for that ADR will be drafted under the same `write-adr` §Simulation rule, so the migration's complexity gets the same scrutiny.
- **`MappingFlags::USER` is meaningful but unreachable in v1.** [ADR-0009](0009-mmu-trait.md) defines `USER` as one of the five flag bits. With `EPD1 = 1` and only `TTBR0_EL1` populated by kernel-only mappings, no user-permission entry exists in v1. The flag's translation to VMSAv8 `AP[1]` (unprivileged-access) bits is implemented in `QemuVirtMmu::map` (the BSP knows how to translate it) but never exercised. *Mitigation:* a host test in `bsp-qemu-virt/src/mmu.rs::tests` exercises the encoding for `USER`-bearing flags so the encoder is correct when B5 needs it.
- **Single MAIR attribute per memory class.** Index 0 is locked to device-nGnRnE; index 1 is locked to normal-cached-WB-WA-IS. Future memory types (write-combining, normal-uncached, device-GRE) require either re-allocating MAIR indices (back-compat hazard) or extending `MappingFlags` with a `MemoryType` discriminant. *Mitigation:* the unused MAIR indices 2..7 are reserved by this ADR for that purpose; future ADR adds the encoding without touching indices 0 / 1.
- **Bootstrap page-tables use 2 MiB block descriptors at L2.** The HAL trait promises 4 KiB granularity; the bootstrap takes a shortcut (block descriptors) for the boot-time identity mapping because subdividing 128 MiB of RAM into 32 768 4 KiB pages is wasteful and unnecessary. *Mitigation:* the shortcut is BSP-internal, not exposed via the trait; if any post-MMU code wants to remap a sub-2-MiB region inside an existing block, the BSP's `Mmu::map` implementation must split the block (a known follow-up; out of scope for T-016 since v1 has no caller exercising it).
- **Token discipline imposes a small ergonomic cost on every `map`/`unmap` caller.** Each call now ends with `flush.flush(mmu)` or `flush.ignore()`. *Mitigation:* the discipline is the win — the cost *is* the readability of mandatory flushes. Future helper macros (`map_and_flush!`) can sugar the common path if the noise becomes excessive.

### Neutral

- **No change to ADR-0017's IPC primitive set.** The MMU surface is internal infrastructure; user-observable IPC primitives (`send` / `recv` / `notify`) are untouched. ADR-0017 §Revision notes does not need a rider.
- **No change to `SchedError` / `IpcError` taxonomies.** MMU faults raise CPU exceptions handled by [T-012](../analysis/tasks/phase-b/T-012-exception-and-irq-infrastructure.md)'s vector table; they do not surface as scheduler / IPC errors in v1. A future ADR (preemption / fault-handling ABI) defines how MMU faults from userspace map to capability-system errors.
- **Bootstrap `mmu_bootstrap` runs once per boot.** It is **not** part of `Mmu` trait. The trait's `create_address_space` / `activate` are for *post-bootstrap* address-space management (dynamic mappings, B3+); bootstrap is BSP-internal.
- **No new ADR governance burden.** This ADR follows the [`write-adr` skill](../../.claude/skills/write-adr/SKILL.md) §Simulation discipline (codified in commit `77a578a`); the §Dependency chain section satisfies [ADR-0025 §Rule 1](0025-adr-governance-amendments.md) (every forward-reference is grounded in T-016 which opens with this ADR's Propose commit).

## Pros and cons of the options

### Option A — Identity-only, no flush token

- Pro: Smallest possible B2 surface; the fewest moving parts.
- Pro: Easy to review; no HAL surface change.
- Con: Skips the type-system-enforced flush discipline; "did you remember to flush?" stays in reviewer attention.
- Con: When B5 high-half migration lands, it has to add the flush token *then*, with all existing post-MMU callers needing their return-type-handling updated. Two waves of API churn instead of one.

### Option B — Identity-only, with flush token

- Pro: Everything Option A has, plus the flush discipline win.
- Con: Does not name the future high-half migration explicitly; a B5 reader reverse-engineers the future from commit history.
- Con: Marginally larger ADR scope (the flush-token discussion).

### Option C — Identity + high-half + identity teardown, with flush token

- Pro: One-shot B2 commitment; no future migration ADR.
- Pro: Standard "Linux on aarch64" shape; reference-kernel parity.
- Con: Implementation cost — linker-script `AT > RAM`, two-stage boot (early stub at low PA + main kernel at high VA), absolute-address jump after `SCTLR.M=1`, identity teardown after the jump. ~2× the asm and ~2× the audit-log entries of Option A/B/D.
- Con: Premature optimisation — B3/B4 do not need the high-half; B5 is the natural moment.
- Con: Pre-pays the cost without obtaining the benefit until B5.

### Option D — Identity-only with flush token, named-future high-half ADR (chosen)

- Pro: All of Option B's benefits.
- Pro: B5-readiness without B2 cost: the layout *supports* future high-half (TTBR1 reservation; MAIR reservation; ASID-zero global mappings), and the future ADR's slot is named.
- Pro: ADR-0033 placeholder gives a B5 reader a clear forward-pointer.
- Con: Adds one named-but-not-yet-opened ADR slot to the project's mental load. *Mitigation:* the slot is named, not allocated; per [ADR-0025 §Rule 1](0025-adr-governance-amendments.md), no T-NNN is opened today, and the ADR-0033 file does not exist until B5 surfaces the requirement (mirrors the ADR-0023 placeholder pattern, which has the file but explicitly Deferred status).

## References

- [ADR-0009 — `Mmu` HAL trait signature (v1)](0009-mmu-trait.md) — the trait this ADR extends with the `MapperFlush` token return type.
- [ADR-0012 — Boot flow and memory layout for `bsp-qemu-virt`](0012-boot-flow-qemu-virt.md) — §Open questions "Boot-time MMU activation" resolves here.
- [ADR-0024 — EL drop to EL1 policy](0024-el-drop-policy.md) — the kernel runs at EL1 when MMU activates; SCTLR / MAIR / TCR / TTBR0 are EL1 system registers.
- [ADR-0025 — ADR governance amendments](0025-adr-governance-amendments.md) — §Rule 1 (forward-reference contract) governs T-016's opening alongside this ADR's Propose commit.
- [ADR-0026 — Idle dispatch via separate fallback slot](0026-idle-dispatch-fallback.md) — §Simulation table is the empirical source of the §Simulation discipline this ADR applies forward.
- [ADR-0032 — Endpoint state rollback + `ipc_cancel_recv`](0032-endpoint-rollback-and-cancel-recv.md) — first ADR drafted under §Simulation; this is the second.
- [`docs/architecture/memory-management.md`](../architecture/memory-management.md) — landing in this PR; synthesises the layout in narrative + diagram form.
- [`docs/audits/unsafe-log.md`](../audits/unsafe-log.md) — UNSAFE-2026-0022 through 0025 land with T-016.
- [`docs/standards/unsafe-policy.md`](../standards/unsafe-policy.md) — the audit-discipline contract every new entry follows.
- ARM *Architecture Reference Manual* (ARMv8-A), ARM DDI 0487 — §D5.2 (VMSAv8 translation), §D5.3 (page-table entry formats), §D5.5 (memory attributes), §D7 (system registers `SCTLR_EL1`, `TCR_EL1`, `TTBRn_EL1`, `MAIR_EL1`).
- Linux aarch64 boot — `arch/arm64/kernel/head.S` + `arch/arm64/mm/proc.S` (`__cpu_setup`, `__primary_switch`) — prior art for the MMU-enable transition; the §Simulation rows 2 / 3 / 4 mirror Linux's `__primary_switch` shape modulo the high-half jump Option D defers.
- seL4 — `src/arch/arm/64/kernel/boot.c` (`init_freemem`, `init_kernel`) — capability-aware kernel that uses identity-mapped early boot before transitioning; Tyrne adopts the identity-only steady state for B2.
- `x86_64::structures::paging::MapperFlush` — Rust ecosystem prior art for the typed flush token; same shape adopted here for the aarch64 `Mmu` trait.
