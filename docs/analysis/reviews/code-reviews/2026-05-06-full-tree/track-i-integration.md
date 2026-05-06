# Track I — Cross-track integration

- **Agent run by:** Claude general-purpose agent (Opus 4.7, 1M context), 2026-05-06
- **Scope:** Inter-crate seams, trait-contract drift, ABI boundaries (kernel ↔ asm + linker), symbol mangling, phase ↔ ADR ↔ task ↔ audit-entry chain, skill-pointer health, inter-crate dependency edges.
- **HEAD reviewed:** `214052d`

This track inspects the *seams* the per-crate / per-axis tracks are too narrow to see. Inputs were read superficially for inter-crate edges; deep correctness lives in Tracks A / B / G / E.

---

## Trait-contract drift table

For each HAL trait the in-source signature in `hal/src/*.rs` was matched against the BSP impl in `bsp-qemu-virt/src/*.rs` and against the test-HAL fake in `test-hal/src/*.rs`. Methods are spot-checked by signature first, then by behavioural contract.

| HAL trait | Method | `hal/src` signature | `bsp-qemu-virt/src` impl signature | `test-hal/src` fake signature | Match? |
|---|---|---|---|---|---|
| `Console` | `write_bytes(&self, bytes: &[u8])` | `hal/src/console.rs:38` | `bsp-qemu-virt/src/console.rs:61` | `test-hal/src/console.rs:59` | yes |
| `Cpu` | `current_core_id(&self) -> CoreId` | `hal/src/cpu.rs:47` | `bsp-qemu-virt/src/cpu.rs:225` | `test-hal/src/cpu.rs:95` | yes |
| `Cpu` | `disable_irqs(&self) -> IrqState` | `hal/src/cpu.rs:53` | `bsp-qemu-virt/src/cpu.rs:240` | `test-hal/src/cpu.rs:99` | yes |
| `Cpu` | `restore_irq_state(&self, state: IrqState)` | `hal/src/cpu.rs:61` | `bsp-qemu-virt/src/cpu.rs:259` | `test-hal/src/cpu.rs:106` | yes |
| `Cpu` | `wait_for_interrupt(&self)` | `hal/src/cpu.rs:67` | `bsp-qemu-virt/src/cpu.rs:270` | `test-hal/src/cpu.rs:110` | yes |
| `Cpu` | `instruction_barrier(&self)` | `hal/src/cpu.rs:75` | `bsp-qemu-virt/src/cpu.rs:279` | `test-hal/src/cpu.rs:114` | yes |
| `ContextSwitch` | `type TaskContext: Default + Send` | `hal/src/context_switch.rs:31` | `bsp-qemu-virt/src/cpu.rs:403` (`Aarch64TaskContext`) | not on test-HAL — kernel-test-private impl in `kernel/src/sched/mod.rs:875` | yes |
| `ContextSwitch` | `unsafe fn context_switch(&self, current: &mut Self::TaskContext, next: &Self::TaskContext)` | `hal/src/context_switch.rs:50` | `bsp-qemu-virt/src/cpu.rs:405` | kernel-test-private at `sched/mod.rs:878` | yes |
| `ContextSwitch` | `unsafe fn init_context(&self, ctx: &mut Self::TaskContext, entry: fn() -> !, stack_top: *mut u8)` | `hal/src/context_switch.rs:64` | `bsp-qemu-virt/src/cpu.rs:417` | kernel-test-private at `sched/mod.rs:886` | yes |
| `Timer` | `now_ns(&self) -> u64` | `hal/src/timer.rs:36` | `bsp-qemu-virt/src/cpu.rs:437` | `test-hal/src/timer.rs:91` | yes |
| `Timer` | `arm_deadline(&self, deadline_ns: u64)` | `hal/src/timer.rs:43` | `bsp-qemu-virt/src/cpu.rs:484` | `test-hal/src/timer.rs:95` | yes |
| `Timer` | `cancel_deadline(&self)` | `hal/src/timer.rs:48` | `bsp-qemu-virt/src/cpu.rs:532` | `test-hal/src/timer.rs:99` | yes |
| `Timer` | `resolution_ns(&self) -> u64` | `hal/src/timer.rs:54` | `bsp-qemu-virt/src/cpu.rs:556` | `test-hal/src/timer.rs:105` | yes |
| `IrqController` | `enable(&self, irq: IrqNumber)` | `hal/src/irq_controller.rs:39` | `bsp-qemu-virt/src/gic.rs:316` | `test-hal/src/irq_controller.rs:94` | yes |
| `IrqController` | `disable(&self, irq: IrqNumber)` | `hal/src/irq_controller.rs:42` | `bsp-qemu-virt/src/gic.rs:342` | `test-hal/src/irq_controller.rs:98` | yes |
| `IrqController` | `acknowledge(&self) -> Option<IrqNumber>` | `hal/src/irq_controller.rs:51` | `bsp-qemu-virt/src/gic.rs:364` | `test-hal/src/irq_controller.rs:102` | yes |
| `IrqController` | `end_of_interrupt(&self, irq: IrqNumber)` | `hal/src/irq_controller.rs:58` | `bsp-qemu-virt/src/gic.rs:378` | `test-hal/src/irq_controller.rs:106` | yes |
| `Mmu` | (placeholder) | `hal/src/mmu.rs:208-264` | not impl'd by any BSP yet (B2 work) | not impl'd by `tyrne-test-hal` *as a fake of* `tyrne_hal::Mmu`; `FakeMmu` is exposed but ungated against B2 surface | yes (n/a in v1) |
| `Iommu` | (empty placeholder) | `hal/src/lib.rs:61` | not impl'd | not impl'd | yes (n/a in v1) |

**Behavioural contract spot checks:**

- `Cpu::disable_irqs` — HAL trait says "Mask CPU-level interrupts and return the previous mask state" (`hal/src/cpu.rs:49-53`); BSP implements via `MRS daif` then `MSR daifset, #0xf` (`bsp-qemu-virt/src/cpu.rs:240-257`); `FakeCpu` flips `irqs_enabled` and returns `IrqState(prev)` (`test-hal/src/cpu.rs:99-104`). All three honour the "captures prior state, restorable via `restore_irq_state`" semantics.
- `IrqController::acknowledge` — trait contract says "Returns `None` for spurious or race" (`hal/src/irq_controller.rs:46-50`); BSP folds `INTID 1023` to `None` (`bsp-qemu-virt/src/gic.rs:371-375`); `FakeIrqController` returns `None` on empty queue (`test-hal/src/irq_controller.rs:103`). Contract honoured.
- `Timer::arm_deadline` — trait contract says "Past deadlines fire promptly" and "Single armed deadline; arming a second replaces the first" (`hal/src/timer.rs:23-28`); BSP writes `CNTV_CVAL_EL0` then enables `CNTV_CTL_EL0` and `gic.enable(TIMER_IRQ)` (`bsp-qemu-virt/src/cpu.rs:484-530`); `FakeTimer` writes `armed_deadline = Some(deadline_ns)` overwriting prior value (`test-hal/src/timer.rs:95-97`). Both honour replace-prior semantics.
- `ContextSwitch::context_switch` `# Safety` — trait says interrupts must be disabled, contexts must outlive the switch, `next` must have been written by a prior switch or `init_context` (`hal/src/context_switch.rs:40-49`). BSP impl forwards directly to `context_switch_asm` and re-states the same invariants in its `// SAFETY:` comment (`bsp-qemu-virt/src/cpu.rs:405-415`). The safety contract round-trips.

No drift detected.

### Generic-vs-trait-object boundary

Searched for `&dyn Cpu`, `Box<dyn Cpu>`, `&dyn ContextSwitch` across `kernel/src`, `bsp-qemu-virt/src`, `test-hal/src`. The Phase A post-mortem rule (the kernel's hot critical-section paths use `IrqGuard<C>` generic over a concrete `C: Cpu` rather than `&dyn Cpu` to avoid the `.rodata`-aliasing vtable hazard) is **upheld at HEAD**:

- Only matches: `FmtWriter<'a>(pub &'a dyn Console)` (intentional, ADR-0007 — formatted output adapter; not a hot path); doc-comments referring to `&dyn Cpu` / `&dyn IrqController` / `&dyn Timer` as the *kernel's* usage shape (which is correct — the kernel's core path uses `&dyn Timer` and `&dyn IrqController` only via *constructed-once-then-static* references, not in the cooperative critical section).
- The single `Box<dyn …>` reference (`kernel/src/sched/mod.rs:390`) is a **comment** explaining a rejected alternative, not a runtime use.

`IrqGuard<C: Cpu>` retains its concrete-type-parameter shape (`hal/src/cpu.rs:102`); no regression.

### ABI boundary

Three `#[repr(C)]` structs cross the kernel/BSP ↔ asm boundary:

| Struct | File | Asm consumer | Size | Compile-time size guard? |
|---|---|---|---|---|
| `Aarch64TaskContext` | `bsp-qemu-virt/src/cpu.rs:305` (in-source comment names `context_switch_asm` at line 293) | `context_switch_asm` (`bsp-qemu-virt/src/cpu.rs:347-398`) — same file, `naked_asm!` block | 168 B (10×8 + 3×8 + 8×8 = 168) | **No** — see Non-blocking finding below |
| `TrapFrame` | `bsp-qemu-virt/src/exceptions.rs:43` (in-source comment names `vectors.s` IRQ trampoline at lines 37-42 and 72-76) | `tyrne_irq_curr_el_trampoline` (`bsp-qemu-virt/src/vectors.s:114-147`) | 192 B | **Yes** — `const _: () = assert!(core::mem::size_of::<TrapFrame>() == 192)` at `bsp-qemu-virt/src/exceptions.rs:77` (added in PR #10 R2 per the master plan; **verified present** at HEAD) |
| `TaskStack` (`#[repr(C, align(16))]`) | `bsp-qemu-virt/src/main.rs:128` | not directly read by asm — but `top()` returns `*mut u8` consumed by `init_context` and the saved `sp` field. Indirect ABI consumer | 4096 B + 16 B align | n/a (alignment-only struct, no field offsets the asm references) |
| `AlignedStack<const N: usize>` (`#[repr(C, align(16))]`) | `kernel/src/sched/mod.rs:916` (test-only) | none — test scaffolding | varies | n/a |

**Field-offset audit, `Aarch64TaskContext` ↔ `context_switch_asm`:**

| Field | Offset (Rust calc) | Asm offset | Match? |
|---|---|---|---|
| `x19_x28: [u64; 10]` | 0 | `[x0, #0]` for x19/x20, …, `[x0, #64]` for x27/x28 | yes |
| `fp` (`x29`) | 80 | `[x0, #80]` (paired with `x30`) | yes |
| `lr` (`x30`) | 88 | `[x0, #80]` paired (low half = fp@80, high half = lr@88 by `stp` semantics) | yes |
| `sp` | 96 | `[x0, #96]` | yes |
| `d8_d15: [u64; 8]` | 104 | `[x0, #104]` … `[x0, #152]` | yes |

**Field-offset audit, `TrapFrame` ↔ `vectors.s`:** mirrored field-by-field at offsets 0x00, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90, 0xA0, 0xB0 — total 0xC0 = 192 bytes. Matches both source-file's documented layout and the compile-time size guard.

### Symbol mangling

Asm-callable Rust functions:

| Function | Site | Attributes at HEAD | Asm caller |
|---|---|---|---|
| `kernel_entry` | `bsp-qemu-virt/src/main.rs:504` | `#[unsafe(no_mangle)] pub extern "C" fn kernel_entry() -> !` | `boot.s:131` (`bl kernel_entry`) |
| `irq_entry` | `bsp-qemu-virt/src/exceptions.rs:145` | `#[unsafe(no_mangle)] pub unsafe extern "C" fn irq_entry(_frame: *mut TrapFrame)` | `vectors.s:131` (`bl irq_entry`) |
| `panic_entry` | `bsp-qemu-virt/src/exceptions.rs:257` | `#[unsafe(no_mangle)] pub unsafe extern "C" fn panic_entry(class: u64, esr: u64) -> !` | `vectors.s:167`, `vectors.s:184` |
| `context_switch_asm` | `bsp-qemu-virt/src/cpu.rs:348` | `#[unsafe(naked)] unsafe extern "C" fn context_switch_asm(...)` | called from Rust at `cpu.rs:410` |

Linker-exported asm symbol → Rust import:

| Symbol | Defined in | Imported in | Block syntax at HEAD |
|---|---|---|---|
| `tyrne_vectors` | `vectors.s:42` (`.global tyrne_vectors`) | `bsp-qemu-virt/src/main.rs:484-489` | `extern "C" { static tyrne_vectors: u8; }` (legacy block syntax — see Non-blocking finding below) |
| `_start` | `boot.s:43` (`.global _start`) | linker-only entry per `linker.ld:11` (`ENTRY(_start)`) | n/a — never named on the Rust side |
| `__bss_start`, `__bss_end`, `__stack_top` | `linker.ld:40,44,49` | `boot.s` only | n/a — asm-side only |
| `tyrne_vectors_end` | `vectors.s:197` | declared but unreferenced; comment at `vectors.s:194` says "linker.ld does not currently use these symbols but having them makes the section bounds easy to verify in objdump" | dead-but-documented |

**Verification of the PR #10 R2 conversion claim** (master plan: "PR #10 review converted these to `unsafe extern "C"`"): `irq_entry`, `panic_entry`, `context_switch_asm` are all `unsafe extern "C" fn` at HEAD. `kernel_entry` remains `pub extern "C" fn` — see the Non-blocking finding below for the asymmetry rationale.

---

## Phase ↔ ADR ↔ task ↔ audit chain walk

The roadmap `phase-b.md` plan claims a set of ADRs; those ADRs claim a set of tasks; those tasks claim a set of audit entries. The chain is walked end-to-end below. The B0/B1 era is the high-risk band per the master plan.

| Phase / Milestone | ADR(s) claimed by phase | Task(s) implementing the ADR(s) | Task status at HEAD | Audit entries created/touched | Chain intact? |
|---|---|---|---|---|---|
| A — capability foundations | ADR-0014 (cap representation), ADR-0016 (kernel-object storage), ADR-0017 (IPC primitives) | T-001 (cap table), T-002 (kernel-object storage), T-003 (IPC primitives) | all Done | (Phase A introduced UNSAFE-2026-0001..0011 in BSP; kernel-side T-001..003 all zero-`unsafe`) | yes |
| A — scheduler + demo | ADR-0019 (scheduler shape), ADR-0020 (`ContextSwitch`+`Cpu` v2) | T-004 (cooperative scheduler), T-005 (two-task IPC demo) | both Done | UNSAFE-2026-0006/0007/0008/0009/0010/0011 (BSP) introduced under T-004/T-005 | yes |
| A — capability scheme | ADR-0018 (badge + reply-recv) | n/a — Deferred (ADR is `Accepted` but explicitly marks the surface as deferred per the Phase A scope) | n/a | none | yes (deferral explicit) |
| B0 — Phase A exit hygiene | ADR-0021 (raw-pointer scheduler API), ADR-0022 (idle task + typed deadlock) | T-006 (raw-pointer refactor), T-007 (idle task + typed deadlock), T-008 (architecture docs), T-011 (missing-tests) | all Done | UNSAFE-2026-0012 → `Removed` (commit `f9b72f8` per audit-log entry); UNSAFE-2026-0013 introduced (`StaticCell::as_mut_ptr`); UNSAFE-2026-0014 introduced (sched free-fn momentary-`&mut`) | yes |
| B0 — timer init | ADR-0010 (existing) — implementation half | T-009 (timer init + `CNTVCT_EL0`) | Done | UNSAFE-2026-0015 introduced (`MRS CNTPCT/CNTFRQ` originally, then Amended 2026-04-23 to `CNTVCT_EL0`); UNSAFE-2026-0006 Amended 2026-04-23 to record post-T-009 struct shape | yes |
| B0 — cross-table CDT | ADR-0023 (cross-table CDT) — accept-deferred (no file at HEAD) | n/a | n/a | none | **partially intact** — ADR-0023 has no file but is referenced from prose. See Non-blocking finding |
| B1 — EL drop | ADR-0024 (EL drop policy) | T-013 (EL drop to EL1) | Done | UNSAFE-2026-0017 introduced (`boot.s` DAIF mask + EL2→EL1); UNSAFE-2026-0018 introduced (`tyrne_hal::cpu::current_el` helper); UNSAFE-2026-0016 Amended 2026-04-27 to record load-bearing-post-condition shift | yes |
| B1 — exception infrastructure | (no new ADR — ADR-0026 was reserved by T-012 but not used per `phase-b.md:246` and the B1 closure business review explicitly notes "ADR-0026 was not used"); ADR-0010 (`arm_deadline`/`cancel_deadline` real); ADR-0011 (`IrqController` real); ADR-0021 (Amended for IRQ-handler aliasing discipline); ADR-0022 (first rider's *Sub-rider* closed) | T-012 (exception + IRQ infrastructure) | Done | UNSAFE-2026-0019 (`QemuVirtGic` MMIO surface); UNSAFE-2026-0020 (vector-table install + asm trampolines); UNSAFE-2026-0021 (`CNTV_CTL`/`CVAL` writes); UNSAFE-2026-0014 Amended 2026-04-28 to name `irq_entry` as a future site of the same momentary-`&mut` pattern | yes |
| B1 — process | ADR-0025 (ADR governance amendments) | n/a — meta-process | n/a | none | yes |

**Walk verdict:** the phase ↔ ADR ↔ task ↔ audit chain is **intact** for every Done milestone. The sole anomaly is ADR-0023 (cross-table CDT, accept-deferred) which is referenced from prose but has no file; this is consistent with the "accept-deferred" disposition recorded in `phase-b.md:41` and tracked in `current.md` / B0 closure review, but the glossary's link target is dead — see Non-blocking #2.

**Audit-log integrity check:** entries 0001 through 0021 are contiguous (no holes). UNSAFE-2026-0012 is correctly marked `Removed` with commit `f9b72f8`. The phase-B-era additions (0013–0021) all correctly cite their introducing task's name, commit SHA, and either an active `// SAFETY:` block or a `Removed` flag with a removal commit. The two Amendments on UNSAFE-2026-0014 (T-011 `start_prelude` extension; T-012 `irq_entry` placeholder) are append-only and dated. UNSAFE-2026-0019 / 0020 / 0021 each carry the "Pending QEMU smoke verification" status note as expected per the Track C / Track G master-plan disclaimer.

---

## Findings

### Blocker

*(none)*

### Non-blocking

- **[bsp-qemu-virt/src/main.rs:484]** The `extern "C" { static tyrne_vectors: u8; }` block uses **legacy edition-2021 block syntax**, while the asm-callable Rust *functions* in the same crate (`irq_entry`, `panic_entry`, `context_switch_asm`) were converted to `unsafe extern "C" fn` in PR #10 R2. The two halves of the FFI surface drift in their stylistic discipline: the function-side carries the `unsafe` marker explicitly; the static-import block does not. Edition 2021 does not require `unsafe extern "C" { … }` block syntax (that is a Rust 1.82+ / 2024-edition feature), so this is *correct* code, but the *project-internal consistency* established by PR #10 R2 is broken at this one site. The same observation applies to `kernel_entry` itself — it is `pub extern "C" fn` rather than `pub unsafe extern "C" fn`, even though it is the most ABI-load-bearing function in the tree.
  - Suggested resolution: either (a) bump the workspace to `edition = "2024"` and convert both the block and `kernel_entry` to `unsafe extern "C"` in the same PR, or (b) leave at edition 2021 and add a one-line comment at `main.rs:484` documenting why the block intentionally uses the legacy form (no information leak; the maintainer's review-round-2 conversion preserved a Rust-edition-aware boundary).

- **[docs/glossary.md:25]** The CDT entry links to `decisions/0023-cross-table-capability-revocation-policy.md` — the file does not exist at HEAD (ADR-0023 is "reserved-empty / accept-deferred" per `phase-b.md:41`). The link's *intent* is documented ("when opened"), but a markdown reader following the link gets a 404. Other cross-references to ADR-0023 (B0 closure security review, business review, `phase-b.md` ledger) are *prose-only* and do not link to a file path.
  - Suggested resolution: either (a) drop the markdown link wrapping (keep the bare prose "see ADR-0023 when opened"), or (b) write a placeholder `0023-cross-table-capability-revocation-policy.md` whose body is "Status: Deferred per accept-deferred path; see B0 closure review and `phase-b.md` for context. To be authored when the first multi-task server arc surfaces in B3–B6." Track E should pick this up; Track I flags it as the place where the *chain-walk* breaks at one end.

- **[bsp-qemu-virt/src/cpu.rs:305]** `Aarch64TaskContext` has no compile-time `size_of` guard, while its sibling `TrapFrame` (`bsp-qemu-virt/src/exceptions.rs:77`) has `const _: () = assert!(core::mem::size_of::<TrapFrame>() == 192);`. The asm `naked_asm!` body in `context_switch_asm` (`bsp-qemu-virt/src/cpu.rs:365-397`) reads byte offsets `0`, `16`, `32`, `48`, `64`, `80`, `96`, `104`, `120`, `136`, `152` — drift between Rust `repr(C)` and asm offsets would corrupt every cooperative switch. Adding a parallel `const _: () = assert!(core::mem::size_of::<Aarch64TaskContext>() == 168);` would catch the drift at compile time, the same way the `TrapFrame` guard already does for the IRQ frame. PR #10 R2 added the `TrapFrame` guard but did not back-fill the older `Aarch64TaskContext` site.
  - Suggested resolution: append a `const _: () = assert!(core::mem::size_of::<Aarch64TaskContext>() == 168);` immediately after the `Aarch64TaskContext` definition. Optionally also `assert!(core::mem::offset_of!(Aarch64TaskContext, sp) == 96);` etc. for byte-perfect offset coverage, mirroring the discipline applied to `TrapFrame`.

### Observation

- **[Cargo.toml dependency graph]** All four inter-crate edges resolve as documented in [ADR-0006]: `tyrne-kernel → tyrne-hal` (production) + `tyrne-test-hal` (dev); `tyrne-bsp-qemu-virt → tyrne-hal + tyrne-kernel` (production); `tyrne-test-hal → tyrne-hal` (production); `tyrne-hal → ∅`. The master-plan checklist phrasing ("`tyrne-test-hal` is dev-dep on `tyrne-kernel`") inverts the actual edge direction — the correct phrasing is "`tyrne-kernel` dev-deps on `tyrne-test-hal`". The code is correct; only the plan's prose is loose.

- **[.claude/skills index ↔ disk]** The skill index at `.claude/skills/README.md` lists 15 skills; all 15 corresponding `SKILL.md` files exist on disk. No orphans, no broken cross-links. The four review master-plans referenced from `conduct-review/SKILL.md` (`business-reviews/master-plan.md`, `code-reviews/master-plan.md`, `security-reviews/master-plan.md`, `performance-optimization-reviews/master-plan.md`) all resolve.

- **[Skill ↔ master-plan pointers]** The Track I checklist phrase "`perform-code-review` and `perform-security-review` skills point at this directory's master plans" is slightly mis-targeted. Those two skills point at `docs/standards/code-review.md` and `docs/standards/security-review.md` (which exist and resolve cleanly); only `conduct-review/SKILL.md` actually points at the four review master plans. All resolutions are healthy; only the master-plan phrasing was imprecise.

- **[docs/decisions/README.md ↔ phase-b.md]** ADR-0023 is absent from the `docs/decisions/README.md` index table (jumps from 0022 → 0024) but listed in the `phase-b.md` "ADR ledger for Phase B" table as Deferred. The two indices disagree on whether a deferred-without-file ADR appears as a row. This is internal inconsistency, not breakage — but a future reader scanning the README index for ADR coverage might miss the deferred slot.

- **[`tyrne-test-hal` does not implement `ContextSwitch`]** `FakeCpu` implements `Cpu` only; the kernel's scheduler tests use a private `impl ContextSwitch for FakeCpu` inside `#[cfg(test)] mod tests` in `kernel/src/sched/mod.rs:875`. This is intentional per ADR-0020's "ContextSwitch is BSP-defined" framing — the test-HAL crate stays trait-surface-minimal — but it means *external consumers* of `tyrne-test-hal` (none today) could not test scheduler-shaped code without re-deriving that impl. Worth noting if the test-HAL ever ships externally.

- **[asm-side `tyrne_vectors_end` symbol unreferenced]** Defined at `vectors.s:197` as `.global tyrne_vectors_end`; comment at `vectors.s:194` admits the linker does not consume it ("makes the section bounds easy to verify in objdump"). Strictly: a public asm symbol exists with no consumer in source. Diagnostic-only, not a hazard.

---

## Cross-track notes (route to merge)

- → **Track E (docs accuracy)**: the dead glossary link to `decisions/0023-cross-table-capability-revocation-policy.md` (Non-blocking #2) is a documentation finding at heart; Track E owns the resolution. Track I flagged it because it is the place where the *chain walk* breaks at one end.
- → **Track E**: the inconsistency between `docs/decisions/README.md`'s ADR index (no row for deferred-without-file ADR-0023) and `phase-b.md`'s ADR ledger (row exists, marked Deferred) (Observation #4) is also an Track-E disposition: pick one indexing convention and apply it everywhere.
- → **Track G (BSP & boot)**: the `Aarch64TaskContext` size-guard gap (Non-blocking #3) is most naturally fixed inside Track G's BSP-side scope. Track I records it because Track G might focus on the deeper boot path and miss the symmetry against PR #10 R2's `TrapFrame` guard.
- → **Track H (build/infra)**: the edition-2021 vs edition-2024 question (Non-blocking #1) is partly a workspace toolchain decision. If Track H finds reason to bump the edition for unrelated reasons, the `unsafe extern "C"` consistency pass is a free rider.
- → **Track A (kernel correctness)**: trait-contract surface looks healthy from the seam side; Track A owns the deeper "are the kernel's *uses* of these traits correct?" question. Track I confirmed the surface contracts match end-to-end; Track A confirms the *callers* match.
- → **Track B (HAL surface)**: the test-HAL `FakeCpu`-does-not-`ContextSwitch` observation (Observation #5) is in Track B's deeper scope; Track I notes it from the seam side.
- → **Track J (umbrix→tyrne residue)**: not exercised by Track I's checklist; no cross-track item.

---

## Sub-verdict

**Approve** — with three Non-blocking items that the merge step should fold into the consolidated artifact's verdict-section follow-up list. No Blocker. The trait-contract surface, ABI boundaries, symbol-mangling discipline, phase-ADR-task-audit chain, skill index, and inter-crate dependency graph are all consistent and self-coherent at HEAD `214052d`. The three Non-blocking items are *project-internal-consistency* polish, not seam breakages.
