# Code review 2026-05-06 — Tyrne full-tree comprehensive at HEAD 214052d

- **Change:** entire committed source + documentation tree on branch `development` at `214052d`. Holistic re-review since the Phase A code review (2026-04-21, `cba5b16`); covers B0 (T-006/T-007/T-008/T-009/T-011) and B1 (T-012/T-013) in addition to re-reading Phase A.
- **Reviewer:** @cemililik (+ ten parallel Claude agents per the [full-tree review plan](2026-05-06-full-tree-comprehensive-review-plan.md))
- **Risk class:** Security-sensitive
- **Security-review cross-reference:** [per-track artefact](2026-05-06-full-tree/track-c-security.md) — verdict Approve.
- **Performance-review cross-reference:** [per-track artefact](2026-05-06-full-tree/track-d-performance.md) — verdict Iterate (11 proposals queued, P1 / P10 / P4 are the highest-ROI near-term picks).
- **Footprint at HEAD:** ~8 283 LOC source (kernel 3 654 + hal 1 198 + test-hal 998 + bsp 2 026 Rust + 333 asm + linker/build); 25 ADRs (0023 reserved-empty); 21 audit entries (0012 Removed); 149 host tests + 149 miri tests pass; QEMU smoke maintainer-launched.
- **Pre-flight gate:** see [`00-preflight.md`](2026-05-06-full-tree/00-preflight.md) — `cargo fmt`, `cargo host-clippy`, `cargo kernel-clippy`, `cargo host-test` (149/149), `cargo kernel-build`, `cargo +nightly miri test` (149/149) all clean.

## Correctness

Synthesis of [Track A — Kernel correctness](2026-05-06-full-tree/track-a-kernel.md) (Approve), [Track B — HAL & test-HAL](2026-05-06-full-tree/track-b-hal.md) (Comment), and [Track G — BSP & boot path](2026-05-06-full-tree/track-g-bsp.md) (Approve). Security-axis correctness (capability invariants, memory safety, kernel-mode discipline) is folded in from [Track C](2026-05-06-full-tree/track-c-security.md) (Approve).

### Kernel (Track A)

- **Capabilities ([`kernel/src/cap/`](../../../../kernel/src/cap/)):** `cap_copy` peer-depth, `cap_derive` saturating depth cap, `cap_revoke` BFS reachability under release-mode `break`, `cap_take` ordering vs `free_slot`, `CapRights::from_raw` reserved-bit masking, `CapabilityTable::new` build-time capacity assertion — every Phase A invariant still holds. The `Capability` type remains move-only at the type level (no `Copy`, no `Clone`; verified by Track C §1).
- **Kernel objects ([`kernel/src/obj/`](../../../../kernel/src/obj/)):** `Arena<T,N>::new` is correct for `N == 0`; generation reuse exercised by `free_then_allocate_bumps_generation`; `destroy_*` reachability contract documented as caller-enforced via `CapabilityTable::references_object` (kernel-side enforcement remains a Phase B follow-up). Test handles correctly `#[cfg(test)] + pub(crate)`.
- **IPC ([`kernel/src/ipc/mod.rs`](../../../../kernel/src/ipc/mod.rs)):** cap-take-before-state-mutation atomicity preserved; `ReceiverTableFull` pre-flight covered by `recv_with_full_table_preserves_pending_cap` (closes the Phase A test gap); `IpcQueues::reset_if_stale_generation` `debug_assert!` catches `Some(cap)` payload drops in tests but in release leaks silently — acceptable until userspace destroy paths land. `ipc_notify` correctly takes `&CapabilityTable` (immutable). v2 `Reply` / `ReplyRecv` (ADR-0018) verified absent.
- **Scheduler ([`kernel/src/sched/mod.rs`](../../../../kernel/src/sched/mod.rs)):** `SchedQueue<0>` structurally legal but type-level invariant unstated (Phase A non-blocker still applies). `yield_now` raw-pointer split-borrow correctness via `ctx_ptr.add(idx)` confirmed; `IrqGuard<C>` held across `cpu.context_switch` (UNSAFE-2026-0008). `ipc_recv_and_yield` Phase 1/2/3 separation verified; the post-resume re-call now returns the typed `Err(SchedError::Ipc(IpcError::PendingAfterResume))` per ADR-0022 (the Phase A `debug_assert!(!Pending)` recommendation is **superseded** at [`kernel/src/sched/mod.rs:820-826`](../../../../kernel/src/sched/mod.rs#L820)). `unblock_receiver_on` is single-waiter by `return` after first match — correct for v1 depth-1 endpoints; multi-waiter remains an ADR-0019 open question. The Deadlock rollback restores `s.current` and `s.task_states[current_idx]` but does **not** reverse the endpoint state from `RecvWaiting` to `Idle` — benign in v1 because the path is structurally unreachable with idle registered, queue an ADR for "endpoint rollback / `ipc_cancel_recv`" before B2.
- **Top-level lib ([`kernel/src/lib.rs:38-43`](../../../../kernel/src/lib.rs#L38)):** kernel-stricter denylist (`clippy::panic`, `unwrap_used`, `expect_used`, `todo`, `arithmetic_side_effects`, `float_arithmetic`) layers cleanly on the workspace `[lints]` set without redundant re-statement. Phase A "double-stated" nit effectively resolved.

### HAL & test-HAL (Track B)

- All five HAL traits (`Console`, `Cpu`, `ContextSwitch`, `Timer`, `IrqController`) match their ADRs (ADR-0007/0008/0010/0011/0020). `Mmu` ([`hal/src/mmu.rs`](../../../../hal/src/mmu.rs)) has a complete v1 trait surface but no production impl yet — dormant for B2 / ADR-0027.
- **`ContextSwitch` lacks `Send + Sync` bound** ([`hal/src/context_switch.rs:25`](../../../../hal/src/context_switch.rs#L25)): every other HAL trait carries `Send + Sync`, this one does not. Adding `: Send + Sync` matches the ADR-0020 "compiler-checked, so multi-core safety is not left to convention" discipline. Non-blocker.
- **`FakeCpu` does not implement `ContextSwitch`** ([`test-hal/src/cpu.rs:94`](../../../../test-hal/src/cpu.rs#L94)): the test-HAL claims "all five fakes present" but kernel scheduler tests reimplement a parallel local `FakeCpu` + `FakeTaskContext` at [`kernel/src/sched/mod.rs:850-908`](../../../../kernel/src/sched/mod.rs#L850). ADR-0020 §"`TestHal` fake" sketches the correct shape; lifting `FakeTaskContext` into `tyrne-test-hal` is the highest-value Track B follow-up.
- **`IrqState(pub usize)` synthetic-construction** ([`hal/src/cpu.rs:22`](../../../../hal/src/cpu.rs#L22)): widened in usage — two test-only `Cpu` impls in the kernel construct `IrqState(0)` directly. Phase A non-blocker still applies; either widen the doc-comment or narrow the field to `pub(crate)` with a `BspConstruct` helper.
- The Phase A `IrqGuard<C>` post-mortem comment ([`hal/src/cpu.rs:86-91`](../../../../hal/src/cpu.rs#L86)) is preserved verbatim into the B1 era; no `&dyn Cpu` regression.
- Production builds cannot pull in fakes — `tyrne-test-hal` is `[dev-dependencies]`-only on the kernel and absent from BSP `Cargo.toml`.

### BSP & boot path (Track G)

- All six rows of [`bsp-boot-checklist.md`](../../../standards/bsp-boot-checklist.md) pass: EL2→EL1 drop with HCR_EL2/SPSR_EL2 literals; `msr daifset, #0xf` as the literal first instruction at [`boot.s:47`](../../../../bsp-qemu-virt/src/boot.s#L47); `CPACR_EL1.FPEN = 0b11` + ISB after the EL drop; VBAR install before GIC init; SP 16-byte aligned at first `bl`; BSS zeroed with 8-byte stride; `context_switch_asm` `#[unsafe(naked)]` per checklist item 6.
- Boot ordering at [`bsp-qemu-virt/src/main.rs:526-598`](../../../../bsp-qemu-virt/src/main.rs#L526): banner → VBAR_EL1 install → ISB → GIC `new` + `init` → `daifclr #0x2`. Reordering would silently hang; current order is safe-by-construction.
- GIC v2 driver ([`bsp-qemu-virt/src/gic.rs`](../../../../bsp-qemu-virt/src/gic.rs)): `GIC_MAX_IRQ = 1020` range-asserted in `enable`/`disable` (line 78, 318, 344); `acknowledge` folds `INTID 1023` to `None` per spec; `end_of_interrupt` writes raw IRQ ID. Each `acknowledge`→`end_of_interrupt` pairing upheld by every branch of `irq_entry`.
- `arm_deadline` / `cancel_deadline` ([`bsp-qemu-virt/src/cpu.rs:484-554`](../../../../bsp-qemu-virt/src/cpu.rs#L484)) order is correct (CVAL before CTL before GIC enable; symmetric for cancel). `CurrentEL` self-check at [`cpu.rs:135-139`](../../../../bsp-qemu-virt/src/cpu.rs#L135) is now load-bearing post-condition of the EL drop per ADR-0024.
- Three Track G non-blockers worth listing:
  - `irq_entry` spurious branch's `compiler_fence(Ordering::SeqCst)` is structurally redundant ([`exceptions.rs:166-174`](../../../../bsp-qemu-virt/src/exceptions.rs#L166)) — drop it or document as defence-in-depth.
  - The IRQ trampoline saves into a 192-byte `TrapFrame` whose Rust-side size is guarded by `const _: () = assert!(size_of::<TrapFrame>() == 192)`; the asm side has no parallel guard ([`vectors.s:114-147`](../../../../bsp-qemu-virt/src/vectors.s#L114)).
  - `kernel_entry`'s call to `start(...)` is `-> !` but lacks a defensive `loop { spin_loop }` after — current code is correct under the contract; belt-and-braces would catch a refactor regression.

### Security correctness (Track C, full eight-axis sweep)

Every "OK" in [Track C](2026-05-06-full-tree/track-c-security.md) is justified by static reasoning over source at HEAD. Highlights:

- Every `unsafe` block traces 1:1 to an active audit entry (UNSAFE-2026-0001..0021; 0012 Removed); the row-by-row audit cross-reference table in Track C §3 confirms zero drift between the 21 audit entries and the in-code SAFETY blocks.
- The UNSAFE-2026-0012 removal is **complete** — verified that no orphaned `&mut`-aliasing-across-yield site survives under any other audit tag (`grep -n "assume_init_mut" bsp-qemu-virt/src/main.rs` returns zero hits).
- UNSAFE-2026-0019 / 0020 / 0021 retain their "Pending QEMU smoke verification" status notes per audit body; **no item in the Track C verdict depends on the smoke completing**. The Pending notation is correctly in place.
- The kernel crate is zero-`unsafe-{` in `cap/`, `obj/`, `ipc/`, `lib.rs`; the 14 inner blocks in `sched/mod.rs` all cite UNSAFE-2026-0008/0009/0014.
- No allocation in `irq_entry`; no heap allocation anywhere; bounded kernel state; allocation paths return typed errors (the three remaining `panic!`s in `sched/mod.rs` lines 307 / 440 / 559 are documented unreachable-invariant guards, not userspace-reachable).
- The Phase A `panic!` in `ipc_recv_and_yield`'s deadlock path is **resolved at HEAD** — replaced by `Err(SchedError::Deadlock)` per ADR-0022.
- Capability bits, generation counters, raw indices never appear in log output reachable from any panic path.
- Workspace remains zero-extern (`Cargo.lock` contains only the four in-tree workspace crates).
- Threat-model load-bearing shifts (B0/B1 inherited): EL1 as structural property post-T-013; aliasing model post-T-006; IRQ delivery as structural property post-T-012; IRQ-handler aliasing discipline post-ADR-0021 2026-04-28 Amendment — all four are net improvements; HEAD adds nothing that worsens any.

### Performance correctness (Track D, paper review)

[Track D](2026-05-06-full-tree/track-d-performance.md) finds **no correctness regressions**. Eleven proposals (P1–P11) queued; P3 (const-eval invariant migration) **already complete** (the Phase A `const { assert!(...) }` recommendation has been applied at [`kernel/src/cap/table.rs:105`](../../../../kernel/src/cap/table.rs#L105), [`kernel/src/obj/arena.rs:90-95`](../../../../kernel/src/obj/arena.rs#L90), [`bsp-qemu-virt/src/exceptions.rs:77`](../../../../bsp-qemu-virt/src/exceptions.rs#L77)). Asm hand-checks (BSS-zero loop, vectors 16×0x80, callee-save set d8–d15) all verify correct.

## Style

- [`kernel/src/cap/table.rs:105`](../../../../kernel/src/cap/table.rs#L105) — `CAP_TABLE_CAPACITY <= Index::MAX` uses `const _: () = assert!(...)`, not the inline `const { assert!(...) }` form used in `Arena::new`. Phase A non-blocker still applies (Track A).
- [`kernel/src/sched/mod.rs:48`](../../../../kernel/src/sched/mod.rs#L48) — `SchedQueue<0>` should carry a build-time `const { assert!(N > 0, ...) }` rather than relying on implicit short-circuit semantics (Track A).
- [`bsp-qemu-virt/src/console.rs:71-78`](../../../../bsp-qemu-virt/src/console.rs#L71) — `Pl011Uart::write_bytes` uses plain `+` for `self.base + offset` rather than `wrapping_add`. Phase A non-blocker still applies; no overflow possible at base `0x0900_0000` with offsets ≤ `0x18` (Track G).
- [`bsp-qemu-virt/src/main.rs:484`](../../../../bsp-qemu-virt/src/main.rs#L484) — `extern "C" { static tyrne_vectors: u8; }` uses legacy edition-2021 block syntax while `irq_entry` / `panic_entry` / `context_switch_asm` were converted to `unsafe extern "C" fn` in PR #10 R2; FFI-surface stylistic discipline drifted at this one site. Resolution: bump to edition 2024 + convert, or add a one-line comment justifying the legacy form (Track I).
- [`bsp-qemu-virt/src/exceptions.rs:142-145`](../../../../bsp-qemu-virt/src/exceptions.rs#L142) — `irq_entry`'s `_frame` parameter exists only for the trampoline calling convention; current doc hints at this in passing but does not explicitly justify the unused parameter (Track G).
- [`docs/analysis/tasks/phase-b/T-009-...md`](../../../tasks/phase-b/T-009-timer-init-cntvct.md) and seven other committed English docs use the Turkish severity adjective `Yüksek` while [`docs/standards/security-review.md`](../../../standards/security-review.md) and [`docs/standards/code-review.md`](../../../standards/code-review.md) use English severity labels (Critical / High / Medium / Low) (Track J §J-NB2).
- Three test modules ([`kernel/src/obj/task.rs:104`](../../../../kernel/src/obj/task.rs#L104), [`obj/endpoint.rs:96`](../../../../kernel/src/obj/endpoint.rs#L96), [`obj/notification.rs:111`](../../../../kernel/src/obj/notification.rs#L111)) omit `clippy::expect_used` and `clippy::panic` from their `#[allow(...)]` blocks — the rest of the kernel allows the trio together (Track F §F-3).

## Test coverage

Synthesis of [Track F — Tests & coverage](2026-05-06-full-tree/track-f-tests.md) (Approve) and Track A's test-coverage routing.

- **Per-error-variant matrix:** 23 variants reviewed; **17 covered** by in-tree tests; **6 documented-unreachable / no-producer** (`SchedError::QueueFull`, `ObjError::StillReachable`, `MmuError::{MisalignedAddress, OutOfFrames, InvalidFlags}`).
- **Phase A code-review's named gap (`IpcError::ReceiverTableFull`) is closed by T-011** at [`kernel/src/ipc/mod.rs:1004`](../../../../kernel/src/ipc/mod.rs#L1004) — `assert_eq!(err, IpcError::ReceiverTableFull)` plus a recovery half asserting the in-flight cap survived the failed recv.
- **`SchedError::Deadlock` and `IpcError::PendingAfterResume`** (the typed-error replacements per ADR-0022) covered by `ipc_recv_and_yield_returns_deadlock_when_ready_queue_empty` and `ipc_recv_and_yield_resume_pending_returns_typed_err`. **`start_prelude` empty-queue panic** covered by `start_prelude_panics_on_empty_ready_queue`. The three Phase A test gaps are closed.
- **Per-subsystem coverage thresholds:** every kernel-core file is comfortably above the [`testing.md`](../../../standards/testing.md) 80 % soft floor at the 2026-04-27 rerun (sched 93.97 %, ipc 97.86 %, cap/table 97.46 %, cap/mod 89.47 %, obj/* 93.75–100 %, cap/rights 97.50 %); 9 days stale, monotonic-up trend, recommend rerun at B2 entry.
- **Miri:** at HEAD pre-flight reports 149 / 149 pass clean; Track F's background capture only got the doc-test trailer, but the surface is the same Miri already covered (143/143 post-T-011 plus six tests at the same surface). Expected-clean.
- **Property / fuzz:** zero `proptest` / `quickcheck` / `bolero` / `cargo-fuzz` in workspace. `CapabilityTable` derivation-tree invariants are the highest-yield fit; defer to a B2-or-later roadmap task.
- **`should_panic` discipline:** every `should_panic` in the workspace carries `expected = "..."` (5/5).
- **Test-helper hygiene:** every test helper is `#[cfg(test)] + pub(crate)`; `EndpointHandle::test_handle` and `NotificationHandle::test_handle` carry `#[allow(dead_code, reason = "symmetric with TaskHandle::test_handle")]` — verified accurate. No test helper leaks into release.
- **Smoke-as-regression gap (§F-1):** `tools/run-qemu.sh` is maintainer-launched; no `qemu-smoke` job in [`.github/workflows/ci.yml`](../../../../.github/workflows/ci.yml). The "Pending QEMU smoke verification" notes on UNSAFE-2026-0019/0020/0021 cannot self-clear; the two-task-demo's six-line expected-output table is a regression contract nothing exercises. Recommend a B2-prep task to add a `qemu-smoke` CI job.
- **`ObjError::StillReachable` has no producer (§F-2):** declared at [`kernel/src/obj/mod.rs:68`](../../../../kernel/src/obj/mod.rs#L68); per [`testing.md`](../../../standards/testing.md) the rule is "every variant has a test that provokes it". Two acceptable resolutions: delete the variant (rely on `#[non_exhaustive]`) or extend the standard with a "documented future variant" exception.

## Documentation

Synthesis of [Track E — Docs & ADRs](2026-05-06-full-tree/track-e-docs.md) (**Request changes — 7 blocker-class items**), [Track J — Localization & hygiene](2026-05-06-full-tree/track-j-hygiene.md) (Comment), and [Track H — Build/infra](2026-05-06-full-tree/track-h-infra.md) doc-vs-state drift items.

### Track E blockers (all listed in §Verdict below)

Seven blocker-class doc drift items. Each is a factual statement that has fallen out of sync with the code at HEAD; none is security-relevant; all are single-edit fixes in single files.

### Non-blocking documentation drift

- **Root docs out of date:** [`README.md:39-55`](../../../../README.md) repository-layout tree omits the four crates that exist in the workspace and says "source code layout will be added after the architecture phase"; [`CONTRIBUTING.md:13-14`](../../../../CONTRIBUTING.md) says "no source code to extend or refactor meaningfully yet"; [`SECURITY.md:7`](../../../../SECURITY.md) says "There is no runnable kernel yet" and "(planned, Phase 2)" for the threat model — kernel boots on QEMU virt and security-model.md exists Accepted (Track E).
- **Standards index missing one entry:** [`docs/standards/README.md`](../../../standards/README.md) lists 13 docs; directory contains 14 (`bsp-boot-checklist.md` is real, in use, but absent from the index) (Track E).
- **Architecture overview status banner stale:** [`docs/architecture/README.md:13`](../../../architecture/README.md) and [`overview.md`](../../../architecture/overview.md) status banner mention Phase A only; align when the GIC-version blockers are resolved (Track E).
- **Two-task demo guide gaps:** [`docs/guides/two-task-demo.md:38-45`](../../../guides/two-task-demo.md) expected-output table omits the post-T-009 `tyrne: timer ready (...)` and `tyrne: boot-to-end elapsed = ... ns` lines; lines 47-59 do not mention the idle task; line 47 still describes a "Phase B" wfe path that B1 already lit (Track E).
- **`overview.md:67-73`** — "(final form documented in `hal.md`, planned)" parenthetical is stale; hal.md is Accepted and shipped (Track E).
- **`docs/decisions/template.md`** Status enum still lists `Proposed | Accepted | Deprecated | Superseded by NNNN`, missing `Deferred` (a real state per ADR-0018 / ADR-0023) (Track E).
- **`infrastructure.md` Configuration files section** ([`docs/standards/infrastructure.md:150-153`](../../../standards/infrastructure.md)) lists `supply-chain/config.toml`, `supply-chain/audits.toml`, and `.github/dependabot.yml` as if present; none exist on disk. Either re-shape the table or move under "Planned (when first extern dep lands)" (Track H, routed to Track E).
- **`infrastructure.md` Continuous integration "Required gates"** lists `cargo audit` and `cargo vet check` as merge-blockers; neither runs in [`ci.yml`](../../../../.github/workflows/ci.yml). Either ship the gates or relax the standard (Track H, routed to Track E).
- **64 stale `cemililik/TyrneOS` URLs** across 27 files (`Cargo.toml`, `SECURITY.md`, every `tyrne-kernel`/`tyrne-hal`/`tyrne-bsp-qemu-virt` rustdoc cross-reference, `docs/guides/run-under-qemu.md`'s clone command). Four newest docs use `cemililik/Tyrne` instead; local `git remote` points at `cemililik/UmbrixOS`. Resolve canonicality, then sweep (Track J §J-NB1).
- **`Yüksek` in eight committed English docs** — Turkish severity term in seven non-quoted references and one quoted commit-subject reference; standards use English (Critical / High / Medium / Low) (Track J §J-NB2).
- **`docs/decisions/README.md` ADR index** jumps 0022 → 0024; `phase-b.md` ADR ledger lists ADR-0023 as Deferred. Pick one indexing convention (Track I).
- **`docs/glossary.md`** dead link pattern (also blocker #5 below) is the place where the chain walk breaks at one end (Track I §Non-blocking #2).
- **`Aarch64TaskContext` lacks compile-time size guard** ([`bsp-qemu-virt/src/cpu.rs:305`](../../../../bsp-qemu-virt/src/cpu.rs#L305)) — its sibling `TrapFrame` has one; PR #10 R2 added the latter but did not back-fill the older site (Track I §Non-blocking #3).

### Audit-log integrity (Track E + Track H + Track I)

- **All 21 entries** observe `unsafe-policy.md §3` discipline; introducing-commit-vs-merge boundary codified in 0017's "Discipline note" and observed by 0006 / 0011 / 0014 / 0015 / 0016 / 0017 / 0018 Amendments. Indexing contiguous (no holes besides 0012 archived-Removed).
- **Audit ↔ source 1:1 correspondence** verified by Track E's full table (cited line counts: 0019 has 24 distinct citation sites in `gic.rs`; 0020 spans `vectors.s` + `exceptions.rs` + `main.rs`; 0014 has nine kernel + seven BSP citations) and Track H's grep verification.
- **Umbrix→tyrne residue clean** in source / standards / ADRs / architecture / guides / glossary / root docs; remaining `Umbrix` mentions are legitimate historical narrative in retro / business review prose (Track J).

## Integration

Synthesis of [Track I — Cross-track integration](2026-05-06-full-tree/track-i-integration.md) (Approve), [Track H — Build/toolchain](2026-05-06-full-tree/track-h-infra.md) (Approve), and cross-track notes routed during merge.

### Trait-contract surface (Track I)

- Every HAL trait method's signature in [`hal/src/*.rs`](../../../../hal/src/) matches its impl in [`bsp-qemu-virt/src/*.rs`](../../../../bsp-qemu-virt/src/) and its fake in [`test-hal/src/*.rs`](../../../../test-hal/src/). 17 method rows checked; zero drift.
- Behavioural contracts spot-checked: `Cpu::disable_irqs` save-and-restore, `IrqController::acknowledge` spurious→None, `Timer::arm_deadline` replace-prior, `ContextSwitch::context_switch` `# Safety` round-trip — all four honoured across HAL / BSP / test-HAL.
- Generic-vs-trait-object boundary (Phase A post-mortem rule): only matches for `&dyn Cpu` / `&dyn IrqController` / `&dyn Timer` are documentation-side or `FmtWriter<'a>(pub &'a dyn Console)` (intentional per ADR-0007). `IrqGuard<C: Cpu>` retains its concrete-type-parameter shape; no regression.

### ABI boundary (Track I)

- Three `#[repr(C)]` structs cross the kernel/BSP ↔ asm boundary: `Aarch64TaskContext` (168 B, **no compile-time size guard** — non-blocker), `TrapFrame` (192 B, guarded at [`exceptions.rs:77`](../../../../bsp-qemu-virt/src/exceptions.rs#L77)), `TaskStack` (alignment-only, no field offsets).
- Field-offset audit `Aarch64TaskContext` ↔ `context_switch_asm` (offsets 0/80/88/96/104) and `TrapFrame` ↔ `vectors.s` (0x00..0xC0) — both match.

### Symbol mangling (Track I)

- `kernel_entry` / `irq_entry` / `panic_entry` / `context_switch_asm` carry the expected `#[unsafe(no_mangle)]` + `unsafe extern "C"` attributes per PR #10 R2; `kernel_entry` remains `pub extern "C" fn` (no `unsafe` qualifier). The `extern "C" { static tyrne_vectors: u8; }` import at [`bsp-qemu-virt/src/main.rs:484`](../../../../bsp-qemu-virt/src/main.rs#L484) uses legacy edition-2021 block syntax — see Style above.
- Linker-exported symbols (`tyrne_vectors`, `_start`, `__bss_start`, `__bss_end`, `__stack_top`) all resolve. `tyrne_vectors_end` is exported but unreferenced (defensive / objdump convenience; documented at [`vectors.s:194`](../../../../bsp-qemu-virt/src/vectors.s#L194)).

### Phase ↔ ADR ↔ task ↔ audit chain (Track I)

- The phase ↔ ADR ↔ task ↔ audit chain is **intact** for every Done milestone (Phase A capability foundations, scheduler+demo, capability scheme deferral; B0 raw-pointer refactor, idle task, timer init; B1 EL drop, exception infrastructure).
- Sole anomaly: ADR-0023 (cross-table CDT, accept-deferred) referenced from prose (`phase-b.md`, `current.md`, B0 closure review) but has no file at HEAD. The glossary's hyperlink target is dead — see Documentation §Blocker #5.
- ADR cross-references all bidirectional: ADR-0010 ↔ T-009/T-012, ADR-0021 ↔ T-006 / UNSAFE-2026-0012/0013/0014, ADR-0022 ↔ T-007/T-009/T-012, ADR-0024 ↔ T-013/UNSAFE-2026-0017/0016/0018, ADR-0025 ↔ rider-discipline precedents.

### Build, toolchain, supply-chain (Track H)

- **CI exists** at [`.github/workflows/ci.yml`](../../../../.github/workflows/ci.yml) (created 2026-04-23) with four jobs: `lint-and-host-test`, `kernel-build`, `miri`, `coverage`. The Phase A "no CI workflow" framing is **stale** (see plan-level amendment in §Verdict).
- CI does **not** run `cargo audit` or `cargo vet check` — both listed as required merge-blockers in [`infrastructure.md`](../../../standards/infrastructure.md). Today this is a no-op (zero external deps), but the gate-vs-policy mismatch is real. Routed to Track E for the documentation side.
- `lint-and-host-test` and `kernel-build` jobs install stable Rust then implicitly defer to `rust-toolchain.toml`'s nightly pin — works but pollutes cache key; fix by switching to `dtolnay/rust-toolchain@stable` honouring the toolchain file.
- The linker rustflag `-T<absolute>/linker.ld` lives in [`bsp-qemu-virt/build.rs`](../../../../bsp-qemu-virt/build.rs) (not in [`.cargo/config.toml`](../../../../.cargo/config.toml)) — intentional design for absolute-path resolution; well-commented; the plan checklist already anticipated this.
- `[workspace.lints]` matches `code-style.md §Lints` exactly; no per-crate file silently relaxes a workspace deny. `Cargo.lock` contains exactly four in-tree workspace crates (zero externals). All five Cargo aliases resolve. `default-members` correctly excludes `bsp-qemu-virt`.
- Skill index ↔ disk: 15 skills in [`.claude/skills/README.md`](../../../../.claude/skills/README.md) match exactly 15 `SKILL.md` files on disk. No orphans, no broken cross-links.

### Inter-crate dependency graph (Track I)

- All four edges resolve as documented in ADR-0006: `tyrne-kernel → tyrne-hal` (production) + `tyrne-test-hal` (dev); `tyrne-bsp-qemu-virt → tyrne-hal + tyrne-kernel` (production); `tyrne-test-hal → tyrne-hal` (production); `tyrne-hal → ∅`. Plan prose phrased the kernel↔test-hal edge in the wrong direction; code is correct.

---

## Verdict

**Request changes**

Per the [code-review master plan §Merge step](master-plan.md#merge-step), any single track-level blocker forces Request changes. [Track E](2026-05-06-full-tree/track-e-docs.md) returned seven blocker-class doc-drift items; all other tracks returned Approve / Comment / Iterate (no blockers). The blockers are **all single-edit fixes in single files; none requires a code change**.

### Blockers (must fix before approval)

1. **Doc drift — GIC version (overview.md)** — [`docs/architecture/overview.md:77`](../../../architecture/overview.md) BSP table claims `bsp-qemu-virt` uses **GICv3**, but T-012 shipped a GIC v2 driver ([`bsp-qemu-virt/src/gic.rs`](../../../../bsp-qemu-virt/src/gic.rs); ADR-0011 + `exceptions.md` + UNSAFE-2026-0019 all say "GIC v2"). **Resolution:** correct the GIC version in the BSP table row.
2. **Doc drift — GIC version (hal.md Mermaid box)** — [`docs/architecture/hal.md:50`](../../../architecture/hal.md) BSP layering Mermaid box reads `BIrq["GICv3 / GIC-400 impl"]`. There is no GICv3 impl in tree. **Resolution:** swap the box label to `"GICv2 / GIC-400 impl"`.
3. **Doc drift — GIC version (hal.md table)** — [`docs/architecture/hal.md:181`](../../../architecture/hal.md) `bsp-qemu-virt` table row "Interrupt controller | GICv3" duplicates the same drift; the `bsp-pi4` row at line 198 correctly notes GIC-400 (v2 subset). **Resolution:** correct the `bsp-qemu-virt` row to `GICv2`.
4. **Doc drift — Timer trait status** — [`docs/architecture/hal.md:126`](../../../architecture/hal.md) Timer subsection still says `arm_deadline` / `cancel_deadline` are `unimplemented!()`. T-012 implemented them under UNSAFE-2026-0021 (bodies at [`cpu.rs:492`](../../../../bsp-qemu-virt/src/cpu.rs#L492) / [`:511`](../../../../bsp-qemu-virt/src/cpu.rs#L511)); ADR-0010 §Revision notes already records this. **Resolution:** flip the subsection prose, cite ADR-0010 §Revision notes 2026-04-28.
5. **Doc drift — scheduler idle path** — [`docs/architecture/scheduler.md:11`](../../../architecture/scheduler.md) and [`scheduler.md:73`](../../../architecture/scheduler.md) still describe idle's body as `core::hint::spin_loop()` with the `wait_for_interrupt` form framed as a future T-012 deliverable. T-012 landed 2026-04-28; [`bsp-qemu-virt/src/main.rs:276`](../../../../bsp-qemu-virt/src/main.rs#L276) shows `cpu.wait_for_interrupt();` in `idle_entry`. **Resolution:** update the idle-loop description in both passages and cite UNSAFE-2026-0021.
6. **Doc drift — security-model DAIF question** — [`docs/architecture/security-model.md:330`](../../../architecture/security-model.md) Open-question bullet still says v1's `boot.s` does not explicitly `msr daifset, #0xf` before stack/BSS setup. T-013 (commit `f289d4d`, ADR-0024, audited under UNSAFE-2026-0017) added exactly this `msr daifset, #0xf` as the first instruction of `_start` ([`boot.s:84`](../../../../bsp-qemu-virt/src/boot.s#L84)). **Resolution:** close the question with a citation to T-013 / ADR-0024 / UNSAFE-2026-0017, or convert the bullet to a closure-rider note.
7. **Glossary dangling link to ADR-0023** — [`docs/glossary.md:25`](../../../glossary.md) links `(decisions/0023-cross-table-capability-revocation-policy.md)` which does not exist (ADR-0023 reserved-empty). Every other reference to ADR-0023 (`phase-b.md`, `security-model.md`, B0/B1 closure reviews) is prose-only without a hyperlink. **Resolution:** drop the markdown link wrapping (keep prose "see ADR-0023 (deferred per Phase B0 closure)"), or write the deferred placeholder ADR-0023 file with a `Status: Deferred` body.

### Non-blocking follow-ups (clustered)

#### Kernel correctness (Track A)

- Migrate `CAP_TABLE_CAPACITY <= Index::MAX` to inline `const { assert!(...) }` form ([`kernel/src/cap/table.rs:105`](../../../../kernel/src/cap/table.rs#L105)).
- Add `const { assert!(N > 0, ...) }` at the top of `SchedQueue::new` ([`kernel/src/sched/mod.rs:48`](../../../../kernel/src/sched/mod.rs#L48)).
- File an ADR introducing `IpcError::WrongObjectKind` / `IpcError::MissingRight` (and symmetric transfer variants) before B2 begins.
- Queue an ADR for "endpoint rollback / `ipc_cancel_recv`" before Phase B2 to address the Deadlock-rollback endpoint-state asymmetry at [`sched/mod.rs:773-778`](../../../../kernel/src/sched/mod.rs#L773).
- Route `Some(cap)` payloads in `IpcQueues::reset_if_stale_generation` through the destroyer's table for graceful drain when endpoint destruction lands.
- Add a one-line comment at [`kernel/src/cap/table.rs:329-336`](../../../../kernel/src/cap/table.rs#L329) explaining the "descendants fit because every live node appears at most once" size proof.

#### HAL contracts (Track B)

- Add `: Send + Sync` to the `ContextSwitch` trait declaration ([`hal/src/context_switch.rs:25`](../../../../hal/src/context_switch.rs#L25)).
- Lift `FakeCpu` / `FakeTaskContext` from `kernel/src/sched/mod.rs` into `tyrne-test-hal::cpu` so `FakeCpu: ContextSwitch` per ADR-0020.
- Either widen the `IrqState(pub usize)` doc-comment to acknowledge "BSP impls and test-only Cpu impls" or narrow the field to `pub(crate)` with a `BspConstruct` helper.

#### Security inherited (Track C — all non-blocking, all forward-flagged)

1. Cross-table revocation gap (ADR-0023 deferred; B3–B6 first-multi-task-server arc).
2. `u32` generation overflow (B-late, long-running services).
3. K3-9 `Capability::Debug` redaction (B5 syscall-ABI design venue).
4. BSP task-body `.expect`/`panic!` (B-first-userspace-driver pre-requisite; covered by `error-handling.md §8`).
5. `arm_deadline` / `cancel_deadline` race windows (no v1 caller; B5+ `time_sleep_until` venue).
6. `cargo-vet init` baseline (K3-8 prerequisite).
7. PL011 `UARTFR_TXFF` spin attempt-cap (low priority, Phase B BSP-wide).
8. UNSAFE-2026-0019 / 0020 / 0021 Pending-QEMU-smoke closure (maintainer-side workitem).

#### Performance proposals queued by ID (Track D)

- **P1** `#[cold]` annotation on error-return paths in `ipc/mod.rs`, `sched/mod.rs`, `cap/table.rs` — highest-ROI near-term, success-path 2–5 % `.text` reduction, very low risk.
- **P2** `#[inline]` posture on hot helpers (`resolve_handle`, `entry_of`, `pop_free`, `validate_*_cap`, accessor helpers) — 0–3 % cross-crate code-size, intra-crate already inlined.
- **P3** Const-eval invariant assertions — **already complete**.
- **P4** `core::hint::assert_unchecked` on already-`debug_assert`-checked invariants — 1–3 instructions per context-switch entry; medium risk (each site needs a SAFETY comment + audit-log Amendment per ADR-0024 / unsafe-policy).
- **P5** `CapEntry` slot packing (`Option<u16>` → sentinel) — ~3 KiB `.bss` reduction; requires a small `OccupancyIndex` newtype to preserve the v1 zero-`unsafe` posture in `cap/`.
- **P6** `Arena<T,N>` per-slot overhead — ~500 bytes total across three arenas; same shape as P5.
- **P7** Drop d8–d15 saves in `context_switch_asm` — 1 KiB `.bss` + ~30 % per-switch instruction reduction; **gated on a paper-review audit (P7c) confirming zero NEON ops in release ELF**.
- **P8** `SchedQueue::enqueue` arithmetic — already a fast bit-mask in release; no change.
- **P9** Boot path `gic.init()` MMIO write count — already minimal; no change.
- **P10** Wall-clock IPC round-trip benchmark harness — **precondition for measuring P1, P4, P7**; ~30 LOC, low risk; gate behind a `bench` feature flag.
- **P11** Hand-rolled `pub const fn zero()` for `Aarch64TaskContext` — ~50–100 bytes `.text` reduction once `core::array::from_fn` becomes const-stable; tracked as deferred const-fn migration.

#### Test coverage (Track F)

- **§F-1** wire QEMU smoke into CI as a `qemu-smoke` job (closes the "Pending QEMU smoke verification" self-clear gap on UNSAFE-2026-0019/0020/0021; future-proofs against new `unsafe` blocks accumulating their own notes).
- **§F-2** decide `ObjError::StillReachable`: delete + rely on `#[non_exhaustive]`, or extend [`testing.md`](../../../standards/testing.md) with a "documented future variant" exception.
- **§F-3** roll-up cleanup: add `clippy::expect_used` + `clippy::panic` to the allow-blocks in [`obj/task.rs:104`](../../../../kernel/src/obj/task.rs#L104), [`obj/endpoint.rs:96`](../../../../kernel/src/obj/endpoint.rs#L96), [`obj/notification.rs:111`](../../../../kernel/src/obj/notification.rs#L111).
- **§M-2** open a B2-or-later roadmap task to add `proptest` (or `bolero`) under `[dev-dependencies]` of `tyrne-kernel` and write **one** property test against `CapabilityTable`'s derivation-tree invariants.
- **§M-3** rerun `cargo llvm-cov` at next phase-boundary closure (B2 entry); name the artifact `docs/analysis/reports/<ISO>-coverage-rerun.md`.

#### BSP & boot (Track G)

- Drop the redundant `compiler_fence(Ordering::SeqCst)` in `irq_entry`'s spurious-IRQ branch ([`exceptions.rs:166-174`](../../../../bsp-qemu-virt/src/exceptions.rs#L166)) or document as defence-in-depth.
- Add a comment cross-reference in `vectors.s` pointing at the [`exceptions.rs:77`](../../../../bsp-qemu-virt/src/exceptions.rs#L77) `TrapFrame` size assertion so the asm/Rust frame-size contract is paired in the maintainer's mental model.
- Append a defensive `loop { core::hint::spin_loop(); }` after `start(SCHED.as_mut_ptr(), cpu);` in `kernel_entry` ([`main.rs:716-721`](../../../../bsp-qemu-virt/src/main.rs#L716)).
- Add `// Receives the saved-frame pointer in x0 even though v1 ignores it; future arcs will read e.g. ELR/SPSR.` before `irq_entry` to justify the unused `_frame` parameter.
- Add `const _: () = assert!(core::mem::size_of::<Aarch64TaskContext>() == 168);` immediately after the type definition at [`bsp-qemu-virt/src/cpu.rs:305`](../../../../bsp-qemu-virt/src/cpu.rs#L305) to mirror the `TrapFrame` discipline (Track I §Non-blocking #3).

#### Infra / CI (Track H, routed to Track E for doc-side resolution)

- Add a `supply-chain` job that runs `cargo install cargo-audit && cargo audit` (no-op today, future-safe) **or** amend [`infrastructure.md` §Continuous integration](../../../standards/infrastructure.md) to mark `cargo audit` / `cargo vet check` as conditional on `Cargo.lock` containing at least one external entry.
- Replace the bespoke `rustup` block in CI with `dtolnay/rust-toolchain@stable` honouring `rust-toolchain.toml`; drop the orphan `rustup component add rustfmt clippy` against `default stable`.
- In [`infrastructure.md` §Configuration files](../../../standards/infrastructure.md), prepend "(present once supply-chain tooling lands per Phase 5)" to the four aspirational rows, or move them under a sub-heading "Planned (when first extern dep lands)".
- In `infrastructure.md`, add a one-line back-pointer "lint set canonical at [`code-style.md` §Lints](../../../standards/code-style.md)" so the two standards stay obviously in sync.
- Note in `infrastructure.md` §Toolchain that `miri` is added on-demand by the CI job (workspace-local invocation requires `rustup component add miri` once).

#### Hygiene / URLs / `Yüksek` labels (Track J)

- **§J-NB1** — 64 stale `cemililik/TyrneOS` URLs across 27 files. Resolve canonical repo URL first (`git remote -v` says `UmbrixOS`, four newest docs say `Tyrne`, 64 older references say `TyrneOS`), then sweep with a single `docs(refs):` commit similar in shape to `10e3351`. Cross-coordinated with Tracks A / G / H / B.
- **§J-NB2** — replace `Yüksek` with `High` in seven non-quoted committed English docs ([T-009-...md:109/110](../../../tasks/phase-b/T-009-timer-init-cntvct.md), [B0-closure (business)](../business-reviews/2026-04-27-B0-closure.md), [B0-closure (security)](../security-reviews/2026-04-27-B0-closure.md), [security-reviews/README.md:35](../security-reviews/README.md), [unsafe-log.md:263 + :271](../../../audits/unsafe-log.md)). Two commit-message-quoted instances are immutable.
- **§J-OBS1** — consider lifting `#![deny(clippy::todo)]` from `kernel/src/lib.rs` to `[workspace.lints.clippy]` so `hal`, `test-hal`, `bsp-qemu-virt` carry the same posture.

#### Integration drifts (Track I)

- Decide edition-2021 vs edition-2024 for the `extern "C" { static tyrne_vectors: u8; }` block at [`bsp-qemu-virt/src/main.rs:484`](../../../../bsp-qemu-virt/src/main.rs#L484); either bump + convert both the block and `kernel_entry` to `unsafe extern "C"`, or leave at edition 2021 with a one-line comment justifying the legacy form.
- Decide ADR-0023 indexing: `docs/decisions/README.md` ADR index jumps 0022 → 0024 while `phase-b.md`'s ADR ledger lists ADR-0023 as Deferred; pick one indexing convention and apply everywhere (closes the doc-glossary blocker #7 in the same edit).
- The plan's prose at §5 Track I ("`tyrne-test-hal` is dev-dep on `tyrne-kernel`") inverts the actual edge direction. Code is correct; only plan prose is loose.

### Plan-level amendments worth recording in the plan's §12

- **CI absence claim is stale (Track H, Track J).** The plan's §5 Track H + §11 still describe "the largest documented gap" as CI absence. CI exists at [`.github/workflows/ci.yml`](../../../../.github/workflows/ci.yml) since 2026-04-23 (four jobs: `lint-and-host-test`, `kernel-build`, `miri`, `coverage`). The actual gap is narrower: CI lacks `cargo audit` / `cargo vet check` (claimed merge-blockers in [`infrastructure.md`](../../../standards/infrastructure.md)), and the `supply-chain/` config files standards/infrastructure.md mentions are not on disk.
- **Track B's prompt named phantom HAL methods (Track B).** The Track B checklist names `now_ticks` / `freq_hz` / `enable_irqs` / `set_priority` etc. The actual landed trait surfaces match ADR-0010 / ADR-0011 / ADR-0008 cleanly: `Timer` is `now_ns` / `arm_deadline` / `cancel_deadline` / `resolution_ns` (no `now_ticks`, no `freq_hz`); `IrqController` is `enable` / `disable` / `acknowledge` / `end_of_interrupt` (no `set_priority`); `Cpu` is `disable_irqs` + `restore_irq_state` (no `enable_irqs`). Plan amendment: name methods exactly to avoid future agents chasing phantom regressions.

### Cross-references propagated

- **Security review:** see [`track-c-security.md`](2026-05-06-full-tree/track-c-security.md) — Approve. Eight axes pass; row-by-row audit cross-reference table verified clean for all 21 entries; UNSAFE-2026-0012 removal complete; UNSAFE-2026-0019/0020/0021 Pending-QEMU-smoke notation in place; eight forward-flagged items (all non-blocking, all inherited from prior reviews).
- **Performance review:** see [`track-d-performance.md`](2026-05-06-full-tree/track-d-performance.md) — Iterate. 11 proposals queued; P1 / P10 / P4 highest-ROI near-term picks; P3 already complete.
- **Per-track artefacts:** [a-kernel](2026-05-06-full-tree/track-a-kernel.md) (Approve), [b-hal](2026-05-06-full-tree/track-b-hal.md) (Comment), [c-security](2026-05-06-full-tree/track-c-security.md) (Approve), [d-performance](2026-05-06-full-tree/track-d-performance.md) (Iterate), [e-docs](2026-05-06-full-tree/track-e-docs.md) (Request changes), [f-tests](2026-05-06-full-tree/track-f-tests.md) (Approve), [g-bsp](2026-05-06-full-tree/track-g-bsp.md) (Approve), [h-infra](2026-05-06-full-tree/track-h-infra.md) (Approve), [i-integration](2026-05-06-full-tree/track-i-integration.md) (Approve), [j-hygiene](2026-05-06-full-tree/track-j-hygiene.md) (Comment).
- **Pre-flight:** [`00-preflight.md`](2026-05-06-full-tree/00-preflight.md) — gate clean.
