# Track D — Performance & optimization (holistic audit)

- **Agent run by:** Claude Opus 4.7 general-purpose agent, 2026-05-06
- **Scope:** Full-tree optimization audit; hotspots, layout, asm, const-eval. Static analysis only — no benchmarks run, no source modified, no proposals implemented.
- **HEAD reviewed:** `214052d`

## Baselines (recap)

The prior performance artefacts establish what is on record at HEAD:

- **A6 baseline (2026-04-21,** [`A6-baseline.md`](../../performance-optimization-reviews/2026-04-21-A6-baseline.md)**)** — v0.0.1 numbers: stripped flat binary 80 KiB; `.text` 13.6 KiB; `.rodata` 1.9 KiB; `.bss` 17.5 KiB; context-switch static instruction count 27; IPC round-trip estimate ~162 instructions; no wall-clock measurement (timer not initialised in Phase A). Verdict: numbers plausible for a v0.0.1 cooperative kernel, no optimisation triggered.
- **B1 closure (2026-04-28,** [`B1-closure.md`](../../performance-optimization-reviews/2026-04-28-B1-closure.md)**)** — v0.0.2-track re-baseline after T-006/T-007/T-008/T-009/T-011/T-012/T-013: `.text` 21.4 KiB (+57 %), `.rodata` 2.7 KiB (+42 %), `.bss` 21.7 KiB (+24 %; +4 KiB is the T-007 idle stack alone); flat binary 24.1 KiB. IPC round-trip estimate revised to ~166–177 instructions (T-006 raw-pointer bridge adds 0–9 % upper bound; release-profile inlining likely absorbs most of it). New IRQ delivery cost (T-012): ~51 instructions per ack-and-ignore (30 trampoline save+restore + 1 `bl` + 20 handler body + ~25 cycles microcode). GIC `init` ~1,600 instructions on the boot path, dominated by ARM IHI 0048B's mandatory `ICENABLER` / `IPRIORITYR` / `ITARGETSR` walks. Three deferred concerns flagged: IPC round-trip wall-clock measurement, stack high-water-mark probes, `TrapFrame` slimming for ack-and-ignore handlers.
- **Master plan (** [`master-plan.md`](../../performance-optimization-reviews/master-plan.md) **)** — six-role hypothesis-driven cycle (Baseline / Hotspot / Proposal / Measurement / Regression-check / Reporter); a review without a hypothesis logs as a baseline artefact, not a full review. This Track D is *that* exploration — a holistic audit, no measurement, no implementation.

This Track D is therefore a **paper review** sitting above both prior artefacts. It identifies hotspots opened by B0/B1 code that the closure baseline did not propose against, and proposes (without implementing) optimisations sized so future hypothesis-driven cycles can pick one up at will.

## Hotspot candidates

Ranked by static-analysis weight, highest impact first. No new measurements — these are derived from source structure plus the B1-closure instruction counts.

1. **IPC round-trip momentary `&mut` materialisation** ([`kernel/src/sched/mod.rs:641–661`](../../../../kernel/src/sched/mod.rs), [`kernel/src/sched/mod.rs:729–737`](../../../../kernel/src/sched/mod.rs), [`kernel/src/sched/mod.rs:813–818`](../../../../kernel/src/sched/mod.rs)) — `ipc_send_and_yield` and `ipc_recv_and_yield` each materialise four `&mut` references inside an `unsafe` block (`s`, `arena_ref`, `queues_ref`, `table_ref`). The B1 closure called the cost out at "0–9 % upper bound, release-profile inlining likely absorbs most of it" (~166–177 instructions). The release-profile reality is that LLVM has visibility through the inner `&mut` derivation only because both functions are defined in the same crate; if a future move splits one of `arena`/`queues`/`table` into its own translation unit, the `&mut` materialisation becomes opaque to the call-site optimiser and the upper bound becomes the realised cost. Worth keeping under measurement once a wall-clock harness exists.
2. **`unblock_receiver_on` linear scan** ([`kernel/src/sched/mod.rs:294–314`](../../../../kernel/src/sched/mod.rs)) — O(N) over `TASK_ARENA_CAPACITY = 16` ([`kernel/src/obj/mod.rs:43`](../../../../kernel/src/obj/mod.rs)). At v1's three-task workload this is ~10 instructions; at full capacity it is ~50–70 instructions per IPC delivery. Already named in the A6 baseline (~10 instructions for N=2). The scan is over `task_states[]` which is 16 × `size_of::<TaskState>()` = 16 × 12 bytes = 192 bytes — fits in three 64-byte cache lines. Conversion to a per-endpoint waiter slot would change the cost from O(N) to O(1) but trades scan time for an ADR-0019 single-waiter-per-endpoint commitment that is already true in v1; the design ADR mentions multi-waiter as a Phase B+ open question, so the structure cannot be flipped without ADR work.
3. **IRQ trampoline 192-byte caller-saved spill** ([`bsp-qemu-virt/src/vectors.s:114–147`](../../../../bsp-qemu-virt/src/vectors.s)) — saves x0..x18 + x30 + ELR_EL1 + SPSR_EL1 (30 instructions, half the trampoline cost of an ack-and-ignore IRQ per the B1 closure decomposition). For v1's `irq_entry` body (timer-IRQ path: read GICC_IAR → check spurious → check IRQ ID → `msr cntv_ctl_el0` mask → write GICC_EOIR → return; ~20 instructions, no scheduler mutation), only x0–x4, x30, ELR_EL1, SPSR_EL1 are actually clobbered by `irq_entry`. The B1 closure recorded this as "deferred until preemption lands" with the explicit "Rejected for v1" rationale (a slimmer trampoline doubles churn when preemption arrives). This stands; the reasoning is correct.
4. **`CapabilityTable::cap_revoke` BFS scratch array** ([`kernel/src/cap/table.rs:337–390`](../../../../kernel/src/cap/table.rs)) — allocates a `[Index; CAP_TABLE_CAPACITY] = [u16; 64]` on stack (128 bytes) every revoke call, regardless of subtree depth. For a leaf revoke (ADR-0014's "no-op" case, exercised by the `cap_revoke_on_leaf_is_a_noop` test) the BFS allocates 128 bytes and walks zero descendants. Trivial cost on a 64 KiB stack, but `cap_revoke` is also a kernel hot-path candidate for Phase B (capability-driven system calls). A bounded walk that uses the existing tree-link fields would skip the array entirely; deferred for now.
5. **`CapabilityTable::is_full` and `references_object` linear scans** ([`kernel/src/cap/table.rs:471–501`](../../../../kernel/src/cap/table.rs)) — `is_full` is O(1) (`free_head.is_none()` check); `references_object` walks every slot (64 entries) every call. The latter is called from kernel-object destroy paths whose ADR-0016 reachability check pattern walks N capability tables × 64 entries = O(N × 64) per destroy. v1 has at most a handful of tables; not a hotspot today. Flag for revisit when a kernel-table registry materialises.
6. **`IpcQueues::reset_if_stale_generation` per-call generation check** ([`kernel/src/ipc/mod.rs:211–228`](../../../../kernel/src/ipc/mod.rs)) — every `state_of` / `peek_state` re-checks the slot's generation. In the cooperative IPC demo, both endpoint slots are stable for the program's lifetime, so the check is pure overhead (one `usize` compare + one branch). Inlining and branch prediction make this ~free; the cost is ~2 instructions per IPC operation. Listed for completeness; not actionable.
7. **`Scheduler::yield_now` post-pre-flight transient empty queue** ([`kernel/src/sched/mod.rs:563–571`](../../../../kernel/src/sched/mod.rs)) — when the ready queue contains only the running task, `yield_now` re-enqueues current, dequeues, sees its own handle, and returns without a switch. Both queue mutations are pure waste on this path. v1's idle-task discipline (ADR-0022) means the queue is never length-1 in practice — idle is always present — so the path is structurally unreachable in production. Cost: ~4 instructions, only on a path that does not run. Listed for completeness; rebalancing the early-out test is not justified.

## Proposals (with expected impact, NOT implemented)

Each proposal: scope, change, expected impact, risk. Implementation is deferred — these are the inventory, not the work order.

### P1. `#[cold]` annotation on error-return paths

- **Scope:** [`kernel/src/ipc/mod.rs`](../../../../kernel/src/ipc/mod.rs) (`IpcError::*` returns), [`kernel/src/sched/mod.rs`](../../../../kernel/src/sched/mod.rs) (`SchedError::Deadlock` return path at ~line 773–778), [`kernel/src/cap/table.rs`](../../../../kernel/src/cap/table.rs) (`CapError::CapsExhausted` / `InvalidHandle` returns).
- **Change:** mark the error-return functions / closures as `#[cold]`, or wrap the error-return arms in a `#[cold]` helper. Most precise form: extract the error-construction site into a `#[cold] #[inline(never)] fn make_<err>() -> IpcError` and call from the matched arm.
- **Expected impact:** moves error code out of the hot path's `i$`, tightens the success-path code-size, marginally improves branch prediction on the success arm. Order-of-magnitude estimate: success path 2–5 % smaller in `.text` instruction-bytes; success-path branch-prediction marginally better. No change to instruction count on the success path.
- **Risk:** very low. `#[cold]` is a hint to LLVM; it cannot regress semantic behaviour. Worst case is no measurable change.

### P2. `#[inline]` posture on hot helpers

- **Scope:** [`kernel/src/cap/table.rs`](../../../../kernel/src/cap/table.rs) `resolve_handle`, `entry_of`, `pop_free`; [`kernel/src/cap/rights.rs`](../../../../kernel/src/cap/rights.rs) `contains`, `union`, `intersection` (already `const fn` but no `#[inline]`); [`kernel/src/ipc/mod.rs`](../../../../kernel/src/ipc/mod.rs) `validate_ep_cap`, `validate_notif_cap`; [`kernel/src/obj/arena.rs`](../../../../kernel/src/obj/arena.rs) `Arena::get`, `Arena::get_mut`, `SlotId::index`, `SlotId::generation`; [`kernel/src/obj/task.rs`](../../../../kernel/src/obj/task.rs) `TaskHandle::slot`, etc.
- **Change:** add `#[inline]` to one-line accessor helpers; consider `#[inline(always)]` for the `validate_*_cap` helpers that always wrap a single `lookup` + rights check.
- **Expected impact:** in a cross-crate call (BSP → kernel) `#[inline]` cleared the cross-crate boundary the same way `pub` clears visibility. Within the kernel, LLVM's auto-inlining at release already handles small functions, so most of these are already inlined; the explicit attribute is documentation + a defence against future changes that reshape the call graph. Order-of-magnitude estimate: 0–3 % code-size win on cross-crate paths, no change on intra-crate paths. Audit-quality win: the `#[inline]` posture becomes part of the kernel's public ABI surface.
- **Risk:** low. Excessive `#[inline(always)]` can worsen `i$` pressure; restrict to one-line accessors and the `validate_*_cap` family (each a single match arm).

### P3. Const-eval invariant assertions

- **Scope:** [`kernel/src/cap/table.rs:105`](../../../../kernel/src/cap/table.rs) (already uses `const _: () = assert!(...)` for `CAP_TABLE_CAPACITY <= Index::MAX`); [`kernel/src/obj/arena.rs:90–95`](../../../../kernel/src/obj/arena.rs) (already uses `const { assert!(...) }` block-form); [`bsp-qemu-virt/src/exceptions.rs:77`](../../../../bsp-qemu-virt/src/exceptions.rs) (already uses `const _: () = assert!(core::mem::size_of::<TrapFrame>() == 192)`). The Phase A review's "migrate runtime `debug_assert` to `const { assert!(...) }`" recommendation appears to have been **acted on** — every site I inspected already uses the const-eval form.
- **Change:** none. Status: **already done**.
- **Expected impact:** N/A.
- **Risk:** N/A.

### P4. `core::hint::assert_unchecked` on already-`debug_assert`-checked invariants

- **Scope:** [`kernel/src/sched/mod.rs:581–584`](../../../../kernel/src/sched/mod.rs) and [`kernel/src/sched/mod.rs:786–789`](../../../../kernel/src/sched/mod.rs) (`current_idx != next_idx`); [`kernel/src/sched/mod.rs:765–769`](../../../../kernel/src/sched/mod.rs) (`prior_state == TaskState::Ready`); [`kernel/src/obj/arena.rs:129`](../../../../kernel/src/obj/arena.rs) (`(head as usize) < N`).
- **Change:** add `// SAFETY: invariant established by …` plus an `unsafe { core::hint::assert_unchecked(current_idx != next_idx) }` immediately after each `debug_assert_*`. The `assert_unchecked` lets LLVM elide downstream bounds checks on `contexts[idx]` indexing, which currently rely on the array's compile-time length (already a const). The biggest win is at the `(*sched).contexts.as_mut_ptr().add(current_idx)` site — LLVM cannot prove `current_idx < TASK_ARENA_CAPACITY` from the surrounding code; a hint would let it skip whatever defensive arithmetic it inserts.
- **Expected impact:** 1–3 instructions per context-switch entry; sub-1 % of the 27-instruction switch cost. Larger win: cleaner generated code that is easier to read in a future `objdump -d`-based review.
- **Risk:** medium. `assert_unchecked` is `unsafe`; a debug-mode assertion that is truly load-bearing on the unsafe path is *not* the same as the unsafe assertion in release mode. The migration is only safe for invariants that are *structurally* enforced (e.g. `current_idx != next_idx` because dequeue ran after the running task was removed from the queue). Each site needs a small SAFETY comment + audit-log Amendment. Per ADR-0024 / unsafe-policy, do not skip the audit step.

### P5. `CapEntry` slot packing

- **Scope:** [`kernel/src/cap/table.rs:62–77`](../../../../kernel/src/cap/table.rs).
- **Recompute `size_of::<Slot>()`:** `Slot { entry: Option<SlotEntry>, generation: u32, next_free: Option<u16> }`.
  - `SlotEntry { capability: Capability, parent: Option<u16>, first_child: Option<u16>, next_sibling: Option<u16>, depth: u8 }` — `Capability { rights: CapRights(u32), object: CapObject }`. `CapObject` is a 4-variant enum; each variant carries a `*Handle(SlotId { index: u16, generation: u32 })`. Discriminant + 6 bytes payload, padded to 8 → 8 bytes total. So `Capability` is 4 (rights) + 8 (object) = 12 bytes, aligned to 4.
  - `SlotEntry` payload: 12 (cap) + 3 × `Option<u16>` (4 bytes each with niche, 12 bytes total) + 1 (depth) → with alignment, ~32 bytes.
  - `Option<SlotEntry>` adds a discriminant byte that may push the alignment to 4 or 8 — call it 36 bytes (32 + 4 padding).
  - Total `Slot`: 36 (entry) + 4 (generation) + 4 (next_free) = **44 bytes** estimate. With trailing alignment, possibly 48 bytes.
  - 64 slots × 48 = **3,072 bytes per CapabilityTable**. The B1 closure §Metric 2 records 2,564 bytes; the discrepancy means the layout is tighter than this naive estimate (likely some `Option<u16>` niche packing landed). Either way, the `Option<u16>` and `Option<SlotEntry>` discriminants are visible in the layout.
- **Change candidates (each independent):**
  - **(a) Replace `Option<Index>` with `Index::MAX as sentinel`.** `next_free: Option<u16>` is 4 bytes (with discriminant + alignment); a `u16` with `0xFFFF` as "none" is 2 bytes. Savings: ~6 bytes per slot × 64 × 3 occurrences (`next_free`, `parent`, `first_child`, `next_sibling`) = ~1,150 bytes per CapabilityTable. With 2 tables in `.bss`, ~2.3 KiB.
  - **(b) Pack `depth: u8` into a spare byte** of the rights-discriminant byte or the `kind` enum. Savings: ~4 bytes per slot × 64 = 256 bytes per table.
  - **(c) Merge `next_free` and the "is-this-slot-occupied?" bit into a tagged `Index`** (high bit = occupied). Savings: 1 byte per slot. Marginal.
  - **(d) Switch `CapObject` to a `(kind: u8, slot: SlotId)` plain struct** instead of an enum-of-typed-handles. Savings: 0 bytes (the typed-handle enum is already as small as a tagged union). The change costs the compile-time kind-vs-handle pairing that ADR-0016 Decision §3 explicitly chose. **Rejected** on the soundness side.
- **Expected impact (combined a + b):** ~1.5 KiB per CapabilityTable, ~3 KiB total (`.bss`); 3 % reduction in kernel RAM at v0.0.2-track. Code-size: neutral or slightly larger (the manual sentinel-checks add a couple of compares per access, partially offset by smaller cache footprint).
- **Risk:** medium. The `Option<Index>` → sentinel migration is mechanically simple but eliminates the type-system's "is-it-occupied?" distinction for `entry`; the field-level invariant moves from compiler-checked to programmer-checked. Compatible with v1's zero-`unsafe` posture in the cap module if implemented behind a small newtype wrapper (`struct OccupancyIndex(u16)` with a `pub fn occupied(self) -> Option<u16>` accessor). **Defer** until a measurement-driven cycle picks up the RAM-reduction concern.

### P6. `Arena<T, N>` per-slot overhead reduction

- **Scope:** [`kernel/src/obj/arena.rs:60–72`](../../../../kernel/src/obj/arena.rs). Same shape as P5: `Slot<T> { entry: Option<T>, generation: u32, next_free: Option<u16> }`. For `T = Endpoint { id: u32 }`, the `Option<Endpoint>` discriminant + `Endpoint` payload + `generation` + `next_free` ≈ 16 bytes per slot. For N=16 slots, ~256 bytes per arena × 3 arenas = ~768 bytes.
- **Change:** identical to P5 (a, b, c).
- **Expected impact:** ~150–250 bytes per arena, ~500 bytes total. Marginal compared to P5; the kernel-object arenas are 16-slot each vs. CapabilityTable's 64-slot.
- **Risk:** identical to P5. Defer.

### P7. `d8–d15` save/restore in `context_switch_asm`

- **Scope:** [`bsp-qemu-virt/src/cpu.rs:347–398`](../../../../bsp-qemu-virt/src/cpu.rs) `context_switch_asm` — saves d8..d15 (8 STP pairs = 4 instructions save + 4 restore = 8 of 27 total).
- **AAPCS64 mandates** preserving the lower 64 bits of v8–v15 across function calls *iff* the callee uses NEON; if the kernel never uses NEON in tasks, the saves are dead. v1's stance per ADR-0024 / the boot.s `CPACR_EL1.FPEN = 0b11` is that NEON is enabled *at the privilege level* but tasks themselves are simple `loop { ipc_*_and_yield }` bodies that do not use SIMD. The compiler may still emit NEON ops opportunistically — AAPCS64 says "if d8–d15 are *touched* by anything across this call, save them", not "if SIMD is on". The boot path itself (`boot.s` BSS-zero loop) does not use NEON; the demo task bodies in `main.rs` do not use NEON; `irq_entry` does not use NEON.
- **Change candidates:**
  - **(a) Disable FP/SIMD in tasks** by setting `CPACR_EL1.FPEN = 0b00` in `boot.s` before `kernel_entry`, then drop d8–d15 from `Aarch64TaskContext`. Saves 64 bytes per context × 16 slots = 1 KiB in `.bss`; saves 8 instructions per switch (~30 % reduction in switch instruction count). **Rejected** because: any compiler-emitted NEON op in kernel code (e.g. `memcpy` on a large struct) would trap as Undefined Instruction; debugging that on QEMU virt is the kind of yak-shave that costs days. The `CPACR_EL1.FPEN = 0b11` write was added to `boot.s` precisely to *prevent* such traps when LLVM emits `movi` / `stp q-regs` for zero-initialisation (per `boot.s` step 4 comment).
  - **(b) Defer NEON save to lazy save-on-trap**: leave FPEN enabled, mark `Aarch64TaskContext` as not saving d8–d15, and rely on a future `Undefined Instruction` exception handler to save+restore on demand. **Rejected** because v1 has no Undefined Instruction handler; adding one is a substantial trap-frame rework.
  - **(c) Audit whether release-build kernel + tasks ever emit NEON ops** by `objdump -d` / grep for `movi` / `stp q` / `ldp q` in the release ELF. If zero, document the empirical absence and revisit (a) with hard data. **Recommended for a future hypothesis-driven cycle.** Until then, the d8–d15 saves are correct.
- **Expected impact (option a, gated on (c) showing zero NEON ops):** 1 KiB `.bss` save, ~30 % per-switch instruction count reduction (27 → 19). At ~10–20 ns per switch on a real Cortex-A72, ~3–6 ns saved per switch.
- **Risk:** as above — high-risk on (a) without the (c) audit; medium on (c) which is paper review.

### P8. `Scheduler::SchedQueue` `enqueue` arithmetic

- **Scope:** [`kernel/src/sched/mod.rs:76–91`](../../../../kernel/src/sched/mod.rs).
- **Observation:** `enqueue` currently does `self.head.wrapping_add(self.len) % N` and `self.len.wrapping_add(1)`. For `N = TASK_ARENA_CAPACITY = 16` (a power of two), `% N` compiles to `& (N - 1)`. Verified by the way the `wrapping_add` + `% N` pattern matches LLVM's idiom recogniser. **No change needed**; the modulo is already a fast bit-mask in release builds.
- **Change:** none.
- **Expected impact:** N/A.

### P9. Boot path `gic.init()` MMIO write count

- **Scope:** [`bsp-qemu-virt/src/gic.rs:153–247`](../../../../bsp-qemu-virt/src/gic.rs) — three MMIO loops over `GICD_TYPER`-reported line count. The B1 closure §Metric 5 records this as ~1,600 instructions, dominant on the new boot path.
- **Observation:** the architectural maximum is `GIC_MAX_IRQ = 1020`; QEMU virt's `GICD_TYPER` reports a much smaller `it_lines_field` (typically 4 for 160 IRQs, depending on the QEMU version). The loop bound is therefore `irq_count = (it_lines_field + 1) * 32` ≈ 160. Each of `ICENABLER` / `IPRIORITYR` / `ITARGETSR` is a 32-bit-word stride: ~5 + 40 + 40 = ~85 distinct MMIO writes, plus the four constructor + CTLR + PMR writes ≈ 90 writes. At ~10 cycles per MMIO write on a real GIC, that is ~900 cycles ≈ 0.5 µs at 1.8 GHz — imperceptible.
- **Change candidates:**
  - **(a) Skip the priority/target loop when `GICD_TYPER` reports a small line count** — already happens implicitly (the loop bound is the reported count). **No change needed.**
  - **(b) Vectorise the loop with `stp` of two 32-bit words** — an MMIO access cannot be coalesced; the `IPRIORITYR` registers are 32-bit-strided and a 64-bit store is not an architected access. **Rejected.**
- **Expected impact:** 0 instructions saved; existing implementation is correct and minimal.

### P10. Wall-clock IPC round-trip benchmark harness (deferred from B1 closure)

- **Scope:** new file (e.g. `kernel/src/sched/bench.rs` or a BSP-side ad-hoc harness) — not actually proposed for the kernel crate but flagged here because the B1 closure §Verdict listed it as "future hypothesis-driven cycle" and the harness's absence prevents anyone from validating proposals P1, P4, P7.
- **Change:** add a `pub fn ipc_roundtrip_bench(...) -> u64` that loops `ipc_send_and_yield ↔ ipc_recv_and_yield` N times under `cpu.now_ns()` measurement, returns `(now_ns - start_ns) / N`. Single-task call site in `kernel_entry`.
- **Expected impact:** unblocks every wall-clock proposal in this artefact. Cost: ~30 LOC + a `now_ns()` snapshot at start/end.
- **Risk:** low. The harness lives in a side path, not the production scheduler. The only risk is that someone forgets to remove it before B2 ship — gate it behind a `#[cfg(feature = "bench")]` and document in the BSP `Cargo.toml`.

### Asm hand-checks (verifications, no proposals)

- **`boot.s` BSS-zero loop** ([`bsp-qemu-virt/src/boot.s:118–127`](../../../../bsp-qemu-virt/src/boot.s)) — uses `str xzr, [x0], #8` (8-byte stride, post-increment); linker.ld guarantees `__bss_start` and `__bss_end` are 8-byte aligned via `.bss : ALIGN(8)` and the trailing `. = ALIGN(8)` after the section body ([`bsp-qemu-virt/linker.ld:39–45`](../../../../bsp-qemu-virt/linker.ld)). **Correct.** The post-increment form is also the cheapest possible per-iteration code: 1 store + 1 cmp + 1 branch = 3 instructions per 8 bytes.
- **`vectors.s` 16 vectors at 0x80 stride** ([`bsp-qemu-virt/src/vectors.s:42–82`](../../../../bsp-qemu-virt/src/vectors.s)) — `.balign 2048` for the table base, `.balign 0x80` between each entry, 16 entries total = 2 KiB exact. `linker.ld:26–27` re-aligns at the section level (`. = ALIGN(2048); KEEP(*(.text.vectors))`). VBAR_EL1 alignment requirement is 2 KiB. **Correct.** Each entry is a single `b <label>` (1 instruction in 0x80 = 128 bytes of slot, most of it padded zero). Trampoline minimality vs the dispatch table is appropriate: the trampoline is 32 instructions for the IRQ path (saves x0..x18 + x30 + ELR/SPSR, calls `irq_entry`, restores, `eret`); shrinking it would require reasoning about which registers `irq_entry` actually clobbers (proposal P? — explicitly Rejected for v1 by the B1 closure).
- **`context_switch_asm` callee-save set** ([`bsp-qemu-virt/src/cpu.rs:347–398`](../../../../bsp-qemu-virt/src/cpu.rs)) — saves x19..x28 + x29(fp) + x30(lr) + sp + d8..d15. Matches AAPCS64 callee-saved set exactly; the doc-comment cites ARM ARM. **Correct.** The naked-asm form (`#[unsafe(naked)]`) is load-bearing — without it the compiler-emitted prologue would push a frame and corrupt the saved sp; the doc comment names this trap. The d8–d15 save question is P7 above (verification cycle proposed, not implementation).

### Const-correctness inventory

- **`CapRights::from_raw`** ([`kernel/src/cap/rights.rs:67–70`](../../../../kernel/src/cap/rights.rs)) — `const fn`. ✅
- **`Message::default()`** ([`kernel/src/ipc/mod.rs:62`](../../../../kernel/src/ipc/mod.rs)) — derived `Default`; the `#[derive(Default)]` does not produce a `const fn` constructor in stable Rust today. **Could** be a `pub const fn new()` that returns the all-zero message, but the `Default::default()` call site does not benefit from `const fn` (it's a runtime construction in the IPC test scaffolding). Marginal at best.
- **`Aarch64TaskContext::default()`** ([`bsp-qemu-virt/src/cpu.rs:304–319`](../../../../bsp-qemu-virt/src/cpu.rs)) — derived `Default`; called once per task slot at scheduler construction (16 slots × `core::array::from_fn(|_| C::TaskContext::default())`). Same constraint as `Message::default()`: the `Default` derive isn't `const fn`. A hand-rolled `pub const fn zero() -> Self` would let `Scheduler::new` itself become `const fn` and live in `.rodata` rather than be runtime-initialised from the BSP. Code-size win: ~50–100 bytes of zero-init code in `.text`. Risk: low; status: **proposal** (call this P11 if you want a number on it).
- **`SchedQueue::new`** ([`kernel/src/sched/mod.rs:62–69`](../../../../kernel/src/sched/mod.rs)) — already `const fn`. ✅
- **`Capability::new`** ([`kernel/src/cap/mod.rs:111`](../../../../kernel/src/cap/mod.rs)) — `const fn`. ✅
- **`CapabilityTable::new`** ([`kernel/src/cap/table.rs:97–129`](../../../../kernel/src/cap/table.rs)) — **not** `const fn` (uses `core::array::from_fn(|i| ...)`, not `const`-evaluable today). The `const _: () = assert!(CAP_TABLE_CAPACITY <= Index::MAX as usize)` compile-time invariant assertion is in place. If `core::array::from_fn` becomes const-stable in a future `rustc`, `new` could be promoted; this is a Rust-toolchain-tracked future work, not a kernel work-item.
- **`Arena::new`** ([`kernel/src/obj/arena.rs:88–121`](../../../../kernel/src/obj/arena.rs)) — same shape and same `core::array::from_fn` constraint as `CapabilityTable::new`. Uses the `const { assert!(...) }` block-form (newer than `const _: () = assert!(...)`). ✅ on the `const` block, no on `const fn`.

## Findings (classified)

### Blocker

None. Performance reviews rarely produce blockers; the master plan reserves them for cases where a layout / asm bug breaks the model. None of the items inspected reach that bar.

### Non-blocking

- [`bsp-qemu-virt/src/cpu.rs:347–398`](../../../../bsp-qemu-virt/src/cpu.rs) — d8–d15 save/restore is correct under AAPCS64 but possibly unnecessary in v1. Defer to proposal P7's audit cycle (verify zero NEON ops in release ELF, then consider disabling FPEN). 1 KiB `.bss` + ~30 % switch-cost reduction is on the table behind a one-cycle audit.
- [`kernel/src/sched/mod.rs:294–314`](../../../../kernel/src/sched/mod.rs) — `unblock_receiver_on` linear scan over `task_states[]`. O(N) with N=16 is ~10 instructions today; a per-endpoint single-waiter slot (consistent with ADR-0019's single-waiter-per-endpoint v1 invariant) would make this O(1). Tied to a future ADR's multi-waiter posture; do not change unilaterally.
- [`kernel/src/cap/table.rs:62–77`](../../../../kernel/src/cap/table.rs) — `Slot` / `SlotEntry` use `Option<u16>` for the four tree-link fields. A sentinel-based representation (P5 / P6) recovers ~3 KiB across the kernel's two CapabilityTables and three kernel-object arenas. Defer to a hypothesis-driven RAM-reduction cycle.
- [`bsp-qemu-virt/src/cpu.rs:304–319`](../../../../bsp-qemu-virt/src/cpu.rs) — `Aarch64TaskContext::default()` is a derive, not `const fn`. Hand-rolling a `pub const fn zero()` would let `Scheduler::new` become `const fn` (when `core::array::from_fn` becomes const-stable, which is a `rustc` work-item), saving ~50–100 bytes of `.text` zero-init code today via direct `.rodata` placement. Tracked as proposal P11.
- [`kernel/src/sched/mod.rs:581–584`](../../../../kernel/src/sched/mod.rs), [`:786–789`](../../../../kernel/src/sched/mod.rs) — opportunities for `core::hint::assert_unchecked` (P4). Each carries a small `unsafe`-block + audit-log Amendment cost; the win is 1–3 instructions per context switch and a cleaner LLVM lowering. Worth picking up alongside any other `sched/mod.rs` change.

### Observation

- [`docs/analysis/reviews/performance-optimization-reviews/2026-04-28-B1-closure.md`](../../performance-optimization-reviews/2026-04-28-B1-closure.md) §Verdict's three deferred concerns — IPC round-trip wall-clock, stack high-water-mark probes, `TrapFrame` slimming — are still open. None can be closed by static analysis. The wall-clock harness (P10 above) is the precondition for the other two. Recommend the maintainer launch a hypothesis-driven cycle around P10 before committing to P1, P4, or P7.
- The Phase A code review's "migrate `debug_assert` → `const { assert!(...) }`" recommendations are honoured in [`kernel/src/cap/table.rs:105`](../../../../kernel/src/cap/table.rs), [`kernel/src/obj/arena.rs:90–95`](../../../../kernel/src/obj/arena.rs), and [`bsp-qemu-virt/src/exceptions.rs:77`](../../../../bsp-qemu-virt/src/exceptions.rs). No drift.
- [`kernel/src/cap/table.rs:337–390`](../../../../kernel/src/cap/table.rs) — `cap_revoke` allocates a 128-byte BFS scratch array on the kernel stack on every revoke, regardless of subtree size. At v1's 64 KiB stack budget this is invisible; at a future per-task 1 KiB stack target (after stack-watermark probes from B1's deferred concern #2) it becomes 12 % of stack. Worth keeping in view.
- [`kernel/src/sched/mod.rs:531–602`](../../../../kernel/src/sched/mod.rs) and the 76-line "Shared safety contract" comment block — the `unsafe fn` + `*mut`-parameter shape is correct per ADR-0021; the cost is one momentary `&mut` materialisation per phase per IPC bridge call. The B1 closure already framed this as 0–9 % upper-bound; nothing in the static analysis says otherwise.
- [`bsp-qemu-virt/src/exceptions.rs:145–228`](../../../../bsp-qemu-virt/src/exceptions.rs) — `irq_entry` body is straight-line: read GICC_IAR, spurious check (early `return`), IRQ ID branch, mask CNTV_CTL_EL0, EOI, return. The `compiler_fence(Ordering::SeqCst)` on the spurious path ([`:173`](../../../../bsp-qemu-virt/src/exceptions.rs)) is a documentation-quality fence; on aarch64 with volatile MMIO the architectural reordering rules already pin the GICC_IAR read before any subsequent memory access, so the fence is documentation rather than a wall-clock cost. No change needed; flag for future review if the IRQ path grows to multiple registers.
- The kernel's `IpcError`, `SchedError`, and `CapError` enums all derive `Eq, PartialEq` plus `Debug`. The `Debug` impl is the only one that shows up in `.text` (cold path); confirms the structure of the proposal P1 — the error-construction sites are the colourable boundaries.

## Cross-track notes

- → **Track A (kernel correctness):** the proposals P4 (`assert_unchecked`) and P5 (`Slot` packing) intersect with the kernel's zero-`unsafe` posture in `cap/` and `obj/`. Track A is the canonical owner of "should we add `unsafe` here?"; route any movement on P4/P5 through Track A first.
- → **Track C (security):** proposal P7 (drop d8–d15 save) intersects with the EL1 vector trampolines (UNSAFE-2026-0020) and the boot.s EL drop (UNSAFE-2026-0017). If the d8–d15 saves go away, the audit-log Amendment must capture the new contract — a task that uses NEON would then trap, which is a security-relevant behaviour change (DoS via Undefined Instruction). Defer until Track C signs off on the contract change.
- → **Track F (tests):** the wall-clock harness (P10) belongs under the test/bench surface, not production. Track F owns coverage + property/Miri telemetry; P10's harness shape is a natural fit for a new "bench" feature flag.
- → **Track H (infra):** if proposal P10 lands, the `Cargo.toml` gains a `bench` feature flag the kernel build would activate selectively. Track H owns the Cargo / feature-flag matrix.

## Sub-verdict

**Iterate (proposals queued).**

Static analysis finds zero blockers and zero outright correctness regressions; the B1 closure's three deferred concerns remain the primary work-list. Track D adds eleven proposal-shaped items (P1 through P11), of which P1 (`#[cold]` on error returns), P10 (wall-clock harness), and P4 (`assert_unchecked`) are the highest-ROI for a near-term hypothesis-driven cycle. P5 / P6 (`Slot` packing) and P7 (NEON-save audit) are higher-ceiling but require Track-C sign-off and a small audit-log story before any bytes change. P3 (const-eval invariant migration) is **already complete** — confirms the Phase A recommendation was acted on.

No source touched in this track. The proposals are the inventory; the hypothesis-driven cycles are the work.

---

Three-line summary:

- Track D recorded the B1-closure baselines, walked every hot path (IPC, sched, IRQ, cap, asm) by static analysis, and produced eleven sized proposals (P1 through P11) without implementing any of them.
- Highest-ROI near-term picks are P1 (`#[cold]` on error returns), P10 (wall-clock IPC harness — precondition for measuring P1/P4/P7), and P4 (`assert_unchecked` on already-checked split-borrow invariants); P7 (drop d8–d15 saves) and P5/P6 (`Slot` packing) are higher-ceiling but require Track-C / Track-A pre-clearance.
- Sub-verdict: Iterate (proposals queued). No blockers; correctness invariants from the prior baselines hold. Phase A's `const { assert!(…) }` migration is already done — confirmed at `cap/table.rs:105`, `obj/arena.rs:90–95`, `exceptions.rs:77`.
