# Code review 2026-04-21 — Tyrne project → Phase A exit

- **Change:** all committed code from project inception through Phase A exit (Phase 1–4c bootstrap + A1–A6 kernel core). Branch: `development` (HEAD: `cba5b16`), compared to empty-repo baseline.
- **Reviewer:** @cemililik (+ Claude agent acting in all five roles)
- **Risk class:** Security-sensitive (capabilities, IPC, scheduler, boot, context switch, `unsafe` x12)
- **Security-review cross-reference:** [docs/analysis/reviews/security-reviews/2026-04-21-tyrne-to-phase-a.md](../security-reviews/2026-04-21-tyrne-to-phase-a.md)
- **Footprint:** 5 370 LOC across `kernel/` (3 030), `hal/` (688), `bsp-qemu-virt/` (872), boot assembly (54); 20 ADRs; 109 host tests + QEMU smoke.

## Correctness

### Capability subsystem ([kernel/src/cap/](../../../../kernel/src/cap/))

- **Non-blocking** — `CapabilityTable::cap_copy` correctly places peer copies under the *source's* parent with depth equal to source's (table.rs:207-213). Depth propagation is identical to the source rather than `source.depth + 1`, which is the right choice for "peer" semantics and matches ADR-0014. Covered by [`copy_of_a_child_shares_parent`](../../../../kernel/src/cap/table.rs#L860).
- **Non-blocking** — `cap_derive` enforces the depth cap via `saturating_add(1)` → `> MAX_DERIVATION_DEPTH`, so a `parent_depth` at `u8::MAX` still reports `DerivationTooDeep` rather than wrapping. [table.rs:268-271](../../../../kernel/src/cap/table.rs#L268).
- **Non-blocking — minor** — `cap_revoke`'s BFS is a two-pass expansion where children are enumerated *during* the scan loop ([table.rs:361-385](../../../../kernel/src/cap/table.rs#L361)). The correctness relies on `descendants[]` only holding indices whose grandchildren are still reachable via `first_child` at scan time. Because `free_slot` is only called *after* the scan (line 388), this invariant holds. The `debug_assert!(desc_len < CAP_TABLE_CAPACITY)` + release-mode `break` on overflow is the right defensive shape. Worth leaving a one-line comment *"descendants fit because every live node appears at most once in the derivation tree"* — today implied by the parent/sibling invariants but not asserted.
- **Non-blocking** — `cap_take` correctly extracts the entry via `Option::take()` *before* calling `free_slot`, then free_slot sees `entry == None` and short-circuits the clear. [table.rs:454-463](../../../../kernel/src/cap/table.rs#L444). Tested by `cap_take_middle_sibling_preserves_list_integrity`.
- **Non-blocking** — `CapRights::from_raw` masks reserved bits (rights.rs:67-70); regression for the "widened rights via reserved bit" class addressed in the T-001 round-2 review (`2e1d943`). Good.
- **Correctness gap (minor, non-blocking)** — `CapabilityTable::new()` panics would be unreachable by construction, but `CAP_TABLE_CAPACITY <= Index::MAX` is only a `const` assertion, **not** a compile-time error if someone bumps the constant past `u16::MAX`. Prefer `const { assert!(...) }` (as already used in `Arena::new`, [arena.rs:90-95](../../../../kernel/src/obj/arena.rs#L90)) to make the violation a hard build failure. [table.rs:105](../../../../kernel/src/cap/table.rs#L105).

### Kernel-object subsystem ([kernel/src/obj/](../../../../kernel/src/obj/))

- **Non-blocking** — `Arena<T, N>::new()` correctly handles `N == 0` via `free_head: if N > 0 { Some(0) } else { None }` ([arena.rs:119](../../../../kernel/src/obj/arena.rs#L119)). Covered by [`empty_capacity_arena_has_no_free_slot`](../../../../kernel/src/obj/arena.rs#L256).
- **Non-blocking** — `Arena::free` returns `None` on any of (out-of-bounds index, generation mismatch, already-free slot). Good three-way guarded lookup; matches the handle-fail contract of both `CapabilityTable::resolve_handle` and ADR-0016.
- **Non-blocking** — `destroy_task` / `destroy_endpoint` / `destroy_notification` do **not** themselves check reachability against capability tables; callers must check `references_object` first. This is stated in the module doc-comment ([obj/mod.rs:23-30](../../../../kernel/src/obj/mod.rs#L23)) and in each function's `# Errors` block. In practice, the A6 demo never destroys objects, so the gap is untested in integration. File a follow-up task (Phase B): bundle the check once a table registry exists, as the doc-comment already anticipates.
- **Non-blocking (hygiene)** — `TaskHandle::test_handle` is `#[cfg(test)]` + `pub(crate)` and exposed for cross-module tests; symmetric for `Endpoint`/`Notification`. The `#[allow(dead_code)]` markers on Endpoint/Notification test helpers ([endpoint.rs:53](../../../../kernel/src/obj/endpoint.rs#L53), [notification.rs:65](../../../../kernel/src/obj/notification.rs#L65)) are honest and well-commented.

### IPC subsystem ([kernel/src/ipc/mod.rs](../../../../kernel/src/ipc/mod.rs))

- **Non-blocking (important)** — the `ipc_recv` atomicity pre-flight is the strongest correctness detail in the file. [ipc/mod.rs:317-324](../../../../kernel/src/ipc/mod.rs#L317) checks `pending_has_cap && caller_table.is_full()` *before* `core::mem::replace(state, Idle)`, guaranteeing the cap is never dropped on the floor. The comment explicitly anchors this invariant to `install_cap_if_some`'s current error semantics; any refactor of that helper must revisit this point. Consider a `#[must_use = "..."]` or a property test to lock the coupling.
- **Non-blocking** — the `ipc_send` state-preservation on a failed `cap_take` ([ipc/mod.rs:256-259](../../../../kernel/src/ipc/mod.rs#L256)) is also important: taking the cap *before* the `state_of` mutation means a `HasChildren` transfer-cap error leaves `RecvWaiting` intact. Explicitly covered by [`send_with_bad_transfer_cap_preserves_recv_waiting`](../../../../kernel/src/ipc/mod.rs#L769).
- **Non-blocking** — `IpcQueues::sync_generation` ([ipc/mod.rs:181-189](../../../../kernel/src/ipc/mod.rs#L181)) resets slot state on endpoint reuse. This is the fix for the post-T-003 finding about stale `RecvWaiting` leaking across destroy/create cycles; covered by [`stale_queue_state_reset_on_slot_reuse`](../../../../kernel/src/ipc/mod.rs#L840). Good.
- **Potential issue (non-blocking, watch)** — `IpcError::InvalidCapability` is used for three distinct failure modes in `validate_ep_cap`: (a) stale handle, (b) missing rights, (c) wrong object kind. Identical variant collapse makes tests for right-enforcement impossible to distinguish from lookup failure, and impedes post-mortem debugging when a future caller conflates these. ADR-0017 is silent on this granularity. Consider adding `IpcError::WrongObjectKind` and `IpcError::MissingRight` in a follow-up ADR. Not a blocker — behaviour is correct and conservative.
- **Non-blocking** — `ipc_notify` takes `&CapabilityTable` immutably while `ipc_send` / `ipc_recv` take `&mut` (because `cap_take` / `insert_root` need it). Signature is consistent with operation semantics.
- **Non-blocking** — `Message` is `#[derive(Eq, PartialEq)]` with `#[derive(Default)]` — confirm this is intended. Default `Message` has label 0 and zero params; if `label == 0` ever becomes a sentinel "no message" value a user could confuse with `Message::default()`. Currently unused; park it.

### Scheduler ([kernel/src/sched/mod.rs](../../../../kernel/src/sched/mod.rs))

- **Non-blocking (important)** — `SchedQueue::enqueue`'s wrap math guards `clippy::arithmetic_side_effects` with an `#[allow(…, reason = "N > 0 enforced by caller…")]` on [sched/mod.rs:70-74](../../../../kernel/src/sched/mod.rs#L70). The `SchedQueue<0>` case is not statically forbidden at the type level; `new()` would panic-free, `enqueue` would short-circuit on `self.len == N` (0 == 0), `dequeue` would return None. So `SchedQueue<0>` is *behaviourally* a zero-capacity queue, not a bug. Fine, but a `const { assert!(N > 0) }` in `new` (analogous to `Arena::new`'s check) would make the invariant a type-level contract. Deferred.
- **Non-blocking** — `yield_now` handles the single-ready-task case by returning without switching ([sched/mod.rs:291-296](../../../../kernel/src/sched/mod.rs#L291)). The `let _ = self.ready.enqueue(current_handle);` on line 286 silently discards an error. The comment above justifies this: the running task was not in the queue so at most `TASK_ARENA_CAPACITY-1` tasks are queued. That invariant is load-bearing; consider a `debug_assert!(self.ready.len() < TASK_ARENA_CAPACITY)` before the enqueue to catch a bug-class violation in tests.
- **Non-blocking** — Split-borrow pattern in `yield_now` and `ipc_recv_and_yield` (raw-pointer `ctx_ptr.add(current_idx)` + `ctx_ptr.add(next_idx)` to obtain two non-aliasing mutable references). Correct only because `current_idx != next_idx`; this is comment-asserted but not `debug_assert!`-asserted. Adding `debug_assert_ne!(current_idx, next_idx)` before the `unsafe` block would catch a regression if a future change stops dequeuing the running task before re-adding it. [sched/mod.rs:309-316, 406-411](../../../../kernel/src/sched/mod.rs#L309).
- **Non-blocking** — `ipc_recv_and_yield` resumes after `cpu.context_switch` and *re-calls* `ipc_recv` with `debug_assert!(!matches!(result, Ok(RecvOutcome::Pending)))` ([sched/mod.rs:413-422](../../../../kernel/src/sched/mod.rs#L413)). This is the right guard — if a sender unblocks us without delivering (a scheduler bug) it fires in debug and the released-mode behaviour is merely "re-park silently" which is safer than a UB crash. Good defensive pattern.
- **Non-blocking (important)** — `unblock_receiver_on` is **single-waiter** per endpoint ([sched/mod.rs:444-464](../../../../kernel/src/sched/mod.rs#L444)). The doc-comment calls this out and points at an ADR-0019 open question. The A5 `IpcQueues` state machine only tracks one receiver per endpoint (depth 1), so this is correct-by-construction. Acceptable, provided ADR-0019's multi-waiter follow-up is prioritised before Phase C (real drivers).
- **Non-blocking** — `Scheduler::start` panics the kernel if no tasks are registered ([sched/mod.rs:251-253](../../../../kernel/src/sched/mod.rs#L251)); acceptable — the contract forbids that path and the kernel panics there cleanly via the panic handler (UNSAFE-2026-0002).
- **Non-blocking** — the `start()` IRQ-state doc ([sched/mod.rs:237-241](../../../../kernel/src/sched/mod.rs#L237)) is clear that tasks *begin* with IRQs masked. This is a documented quirk, not a bug, and the two-task demo operates correctly with IRQs masked since there are no interrupt sources in A5. Re-examine in Phase B when a timer arrives.

### HAL and context switch ([hal/src/context_switch.rs](../../../../hal/src/context_switch.rs), [bsp-qemu-virt/src/cpu.rs](../../../../bsp-qemu-virt/src/cpu.rs))

- **Non-blocking (important)** — `context_switch_asm` is `#[unsafe(naked)]` + `naked_asm!` per the code-style rule in [unsafe-policy.md §5a](../../../standards/unsafe-policy.md#5a-context-switch-functions-must-use-unsafenaked). Saves the AAPCS64 callee-save set + sp + d8–d15 exactly, retains sp via x8 scratch, and never touches the stack. Matches the `Aarch64TaskContext` `#[repr(C)]` offsets documented inline (cpu.rs:194-201).
- **Non-blocking** — `init_context` writes only `lr` and `sp`; every other register is zero from `Default`. A task's first `ret` consumes the written `lr`. Correct and minimal.
- **Non-blocking** — `IrqGuard` is generic over `C: Cpu` (not `&dyn Cpu`). The doc-comment on [cpu.rs:86-91](../../../../hal/src/cpu.rs#L86) explains *why*: trait-object coercion at deep inlining produced vtable references that aliased `.rodata`. This is exactly the kind of post-mortem comment a reader needs; preserve it verbatim.
- **Non-blocking** — `disable_irqs` uses `mrs daif / msr daifset, #0xf` — the comment explicitly notes the distinction from `msr daif, #imm`. Good. `restore_irq_state` takes `IrqState` as opaque but `IrqState(pub usize)` permits synthetic construction in safe code. A future (non-v1) hardening: make the inner field `pub(crate)` on the HAL crate with a `BspConstruct` escape hatch for BSP impls. Non-blocking.

### Boot / BSP ([bsp-qemu-virt/src/main.rs](../../../../bsp-qemu-virt/src/main.rs), [boot.s](../../../../bsp-qemu-virt/src/boot.s))

- **Non-blocking** — `boot.s` sets `CPACR_EL1.FPEN = 0b11` with `ISB`, then zeros BSS in 8-byte strides. Linker script guarantees 8-byte alignment (linker.ld:33-39). Order is correct: SP before CPACR (so any NEON spill in the asm can't trap early), CPACR before BSS-zero (so the loop's NEON-capable code path doesn't trap), BSS-zero before `bl kernel_entry` (so Rust's zero-init assumptions hold).
- **Non-blocking** — `kernel_entry` order of operations is correct: console → CPU → kernel objects → capability tables → IPC state → scheduler → start. Task B is added *before* Task A so B reaches `RecvWaiting` before A sends, producing `SendOutcome::Delivered` on the first send (cf. guide at [two-task-demo.md:51](../../../../docs/guides/two-task-demo.md#execution-trace)).
- **Potential issue (non-blocking)** — `Pl011Uart::write_bytes` computes `self.base + UARTFR` and `self.base + UARTDR` without a checked add ([console.rs:72, 76](../../../../bsp-qemu-virt/src/console.rs#L72)). Safe today because `base == 0x0900_0000` and offsets are ≤ `0x18`, but `clippy::arithmetic_side_effects` is **not** `deny` in the BSP crate (only in kernel). A `wrapping_add` or typed offsets would make the intent explicit and future-proof the code against constant changes.
- **Non-blocking** — `task_a` / `task_b` use `expect(...)` on IPC results. Consistent with their `fn() -> !` nature and with `clippy::expect_used` being kernel-scoped only (not BSP-scoped). Acceptable in demo tasks; tighten when tasks grow.
- **Potential issue (non-blocking, flagged in audit)** — `UNSAFE-2026-0012` covers the `&mut` aliasing pattern across cooperative yields ([main.rs task_a/task_b, all `assume_init_mut()` calls](../../../../bsp-qemu-virt/src/main.rs#L186)). Already documented as Medium tech debt in [A6 business review](../business-reviews/2026-04-21-A6-completion.md#technical-debt-entering-phase-b); the remediation (raw-pointer API) is queued behind a future ADR. Soundness under v1 single-core cooperative is acceptable; this **must** be resolved before SMP or preemption. Not reflagging — business review owns the tech-debt entry.

## Style

- **Non-blocking** — Module documentation density is consistent across the kernel. Each `mod.rs` opens with a one-paragraph summary, cites the relevant ADR (backed by a literal `[adr-NNNN]: https://...` link at the bottom), and calls out v1 scope. This is the pattern [documentation-style.md](../../../standards/documentation-style.md) asks for; good.
- **Non-blocking** — Test modules uniformly use `#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, reason = "tests may use pragmas forbidden in production kernel code")]`. The `reason = "..."` is populated every time. Good; matches [code-style.md](../../../standards/code-style.md#panics-in-kernel-code).
- **Non-blocking** — `unsafe fn` doc comments universally carry `# Safety` per [unsafe-policy.md §2](../../../standards/unsafe-policy.md#2-unsafe-fn-requires-a--safety-section-in-its-doc-comment). Verified for `QemuVirtCpu::new`, `init_context`, `context_switch`, `context_switch_asm`, `Pl011Uart::new`, `Scheduler::add_task`, `TaskStack::top`. Zero exceptions.
- **Non-blocking** — Every `unsafe` block / impl carries a `// SAFETY:` block with (invariants, alternatives, audit tag). Audit tag format `UNSAFE-2026-NNNN` is consistent. See §Documentation below for cross-reference.
- **Non-blocking (nit)** — `kernel/src/lib.rs` duplicates the kernel-wide lint deny list (`#![deny(clippy::panic)]` etc., lib.rs:36-43) that the **workspace** also sets via `[workspace.lints.clippy]` in `Cargo.toml`. Harmless but double-stated. If the workspace deny set is authoritative, the per-crate `#![deny(...)]` can go; or document that kernel-specific denies layer *on top of* the workspace set.
- **Non-blocking (nit)** — `bsp-qemu-virt/src/main.rs` `#![allow(unreachable_pub, reason = "binary crate; pub items are for the linker")]` is well-reasoned. Good.
- **Non-blocking (nit)** — `kernel/src/sched/mod.rs:530-536` defines the `FakeCpu` test fixture; test also defines `AlignedStack<N>` with `#[repr(C, align(16))]`. Consider hoisting these to a `sched/testing.rs` once a second scheduler test file exists.
- **Non-blocking (nit)** — Unicode em-dash `\u{2014}` in task A/B strings ([main.rs:170](../../../../bsp-qemu-virt/src/main.rs#L170)) but ASCII `--` elsewhere. Intentional given the business review's expected QEMU trace; no action.

## Test coverage

### Present

- **75 kernel tests** (cap: 48 — 18 rights + 30 table; obj: 11 — arena 7, task 3, endpoint 1, notification 2; ipc: 14; sched: 11). **34 test-hal tests** across Console, Cpu, Timer, IrqController, Mmu fakes. Total **109**; matches the [A6 business review tally](../business-reviews/2026-04-21-A6-completion.md#test-counts-at-phase-a-closure).
- Capability table: all error paths exercised (`CapsExhausted`, `InvalidHandle`, `WidenedRights`, `InsufficientRights`, `DerivationTooDeep`, `HasChildren`); depth cap tested at exactly `MAX_DERIVATION_DEPTH + 1`; generation reuse tested; middle-sibling unlink tested for both `cap_drop` and `cap_take`; `references_object` tested across insert/drop/revoke.
- IPC: all four state-machine edges (`Idle → SendPending`, `Idle → RecvWaiting`, `RecvWaiting → RecvComplete`, `RecvComplete → Idle`) exercised; cap-transfer sender-first and receiver-first paths tested; `QueueFull` on both second-send and second-recv; `InvalidTransferCap` on `HasChildren`; `ReceiverTableFull` edge flagged by comment but **not explicitly provoked by test** — see below.
- Scheduler: `SchedQueue` FIFO, wrap-around, empty, full-error all exercised; `add_task` sets `Ready` and records handle; `yield_now` switches contexts (via `FakeCpu::context_switch` marker); `unblock_receiver_on` both-directions; `NoCurrentTask` error variant hit.
- QEMU smoke: the A6 two-task demo output in the [business review](../business-reviews/2026-04-21-A6-completion.md#phase-a-exit-bar--verdict) is the end-to-end behaviour test — all the context-switch / IPC / cap-gate paths are exercised in real aarch64.

### Missing (non-blocking)

- **`IpcError::ReceiverTableFull` is not provoked by any test.** The pre-flight guard at [ipc/mod.rs:317-324](../../../../kernel/src/ipc/mod.rs#L317) is correct-by-reading, but no test drives a sender into `SendPending { cap: Some(...) }` with a full receiver table to confirm `ReceiverTableFull` comes back and the cap is not lost. Add a test that (a) fills the receiver table to `CAP_TABLE_CAPACITY`, (b) sends with a transfer cap, (c) receives, (d) asserts `ReceiverTableFull` + verify the sender's cap is still ownable by the endpoint (i.e. a subsequent recv with a free slot still hands it over). This would lock in the atomicity invariant.
- **`Scheduler::start` panic on empty queue** — no test. Not critical (panic on bootstrap programming error is acceptable) but a `#[should_panic(expected = "empty ready queue")]` test is cheap.
- **Deadlock panic in `ipc_recv_and_yield`** ([sched/mod.rs:393-395](../../../../kernel/src/sched/mod.rs#L393)) — no test. A `#[should_panic(expected = "deadlock:")]` test would document the failure mode for future agents.
- **Generic slot-reuse after cap transfer** — we have slot-reuse tests for `CapabilityTable` and `Arena`, but no composite test that runs `ipc_send (with transfer) → destroy endpoint → create endpoint in same slot → verify IpcQueues resets`. The `stale_queue_state_reset_on_slot_reuse` test exercises the state reset, but not with a pending transfer cap in the destroyed slot — there is an implicit assumption that destroying an endpoint with a pending `SendPending { cap: Some(_) }` will drop the cap. Document this explicitly (destruction loses in-flight caps) and add a test, or file as a future ADR for "in-flight-cap cleanup on endpoint destroy."
- **QEMU smoke as a regression gate** — the smoke currently lives in the `two-task-demo.md` guide and in the A6 business review. There is no CI invocation and no artefact check. A Phase B task should add a scripted `qemu-system-aarch64 ... | grep "all tasks complete"` as a smoke regression (the `run-qemu.sh` flag already exists; wire it to CI when CI lands).

## Documentation

### Present and correct

- All 20 ADRs (0001–0020 minus 0018 Deferred) are Accepted or Deferred per their frontmatter; the ADR index aligns with the audit log's attribution of UNSAFE-2026-0006..0012 to "T-004 / A5".
- Every `pub` item on the review surface carries a doc-comment (spot-checked `CapabilityTable`, `Arena`, `Message`, `RecvOutcome`, `Scheduler`, `ContextSwitch`, `IrqGuard`, `Pl011Uart`). `#[deny(missing_docs)]` at the workspace level is the guarantor.
- Every `Error` enum variant has a `///` line explaining when it fires. The `# Errors` sections on public functions enumerate the variants — `CapabilityTable::cap_copy`, `cap_derive`, `cap_revoke`, `cap_drop`, `cap_take`, `ipc_send`, `ipc_recv`, `ipc_notify`, all `create_*` / `destroy_*`.
- Every `unsafe fn` has a `# Safety` section: `QemuVirtCpu::new`, `ContextSwitch::{context_switch, init_context}`, `Scheduler::add_task`, `Pl011Uart::new`, `TaskStack::top`.
- `docs/guides/two-task-demo.md` accurately reflects the shipped A6 behaviour — the expected output table matches the four `writeln!` / `write_bytes` sites in `main.rs`, the execution trace is consistent with the `ipc_send_and_yield` / `ipc_recv_and_yield` actual flow, and the "known limitations" section mirrors the business review's tech-debt table.

### unsafe-log vs code cross-reference

Verified each UNSAFE-2026-NNNN entry against its in-code `SAFETY:` comment:

| Audit tag | Location(s) | In-code comment | Consistent? |
|---|---|---|---|
| 0001 | main.rs:348, 356, 361 | `// SAFETY: 0x0900_0000 is ... kernel in v1.` | Yes |
| 0002 | main.rs:454 (panic) | `// SAFETY: constructing a fresh Pl011Uart in the panic path ...` | Yes |
| 0003 | console.rs:51 | `// SAFETY: PL011 MMIO is hardware-synchronized ...` | Yes |
| 0004 | console.rs:58 | `// SAFETY: same reasoning as Send above ...` | Yes |
| 0005 | console.rs:71 | `// SAFETY: UARTFR and UARTDR are PL011 MMIO registers ...` | Yes |
| 0006 | cpu.rs:56, 64 | `// SAFETY: QemuVirtCpu is a zero-size marker ...` | Yes |
| 0007 | cpu.rs:72, 90, 107, 116, 124 | five SAFETY blocks on MRS/MSR/WFI/ISB | Yes |
| 0008 | cpu.rs:189 (asm), sched/mod.rs:260, 304, 400 | SAFETY on context-switch callers | Yes |
| 0009 | cpu.rs:259 (init_context), sched/mod.rs:211 (add_task) | SAFETY on init_context callers | Yes |
| 0010 | main.rs:79 (StaticCell), 167, 203, 246, 260, 310 | SAFETY on every `assume_init_ref` | Yes |
| 0011 | main.rs:105 (TaskStack) | `// SAFETY: single-core cooperative kernel ...` | Yes |
| 0012 | main.rs (task_a/task_b `assume_init_mut` calls): 186, 220, 272, 292 | `// SAFETY (aliasing): ...` | Yes |

No drift. The audit log is the source of truth and matches the code verbatim.

### Documentation gaps (non-blocking)

- **`docs/architecture/` not updated for A3–A6.** The index and individual subsystem docs (security-model, hal) were last touched in Phase 4c. Subsystems introduced in A2–A6 — the kernel-object arenas, IPC state machine, scheduler — have no architecture-doc coverage (all design lives in ADRs and code doc-comments). Per [master-plan §4](master-plan.md), when a change affects architecture, the architecture doc should update. Queue a follow-up task: `docs/architecture/kernel-objects.md`, `docs/architecture/ipc.md`, `docs/architecture/scheduler.md`. The `write-architecture-doc` skill is the right entry.
- **No `# Panics` section** on `Scheduler::start`, `Scheduler::ipc_recv_and_yield` — both panic under documented deadlock conditions. The `ipc_recv_and_yield` doc *does* have `# Panics` ([sched/mod.rs:362-365](../../../../kernel/src/sched/mod.rs#L362)); `Scheduler::start`'s `# Panics` is also present ([sched/mod.rs:243-245](../../../../kernel/src/sched/mod.rs#L243)). **Correction: both are present.** No action.
- **Audit log's UNSAFE-2026-0008 location list** ([unsafe-log.md:95](../../../../docs/audits/unsafe-log.md#L95)) mentions `Scheduler::start, yield_now, ipc_recv_and_yield` but not `Scheduler::ipc_send_and_yield` — which **transitively calls `yield_now`** (which itself contains UNSAFE-2026-0008). Not a hole because `yield_now` is the actual site; but the composed call graph could be clearer. Non-blocking.
- **CONTRIBUTING / SECURITY cross-link** — the root `CLAUDE.md` points at `SECURITY.md` and `docs/standards/`; the audit log (`unsafe-log.md`) points at `unsafe-policy.md` and `security-review.md`. All reachable. No broken link seen on spot-check.

## Integration

- **Dependencies.** No external crates in any of `kernel`, `hal`, `bsp-qemu-virt`. Workspace only depends on `tyrne-hal`, `tyrne-kernel`, `tyrne-test-hal` internally. The `add-dependency` skill has not been exercised — no external dependency has been added. Good; matches the ADR-0006 stance and the "no proprietary blobs" CLAUDE.md rule.
- **Workspace layout.** `[workspace] members = [kernel, hal, bsp-qemu-virt, test-hal]` and `default-members` correctly excludes `bsp-qemu-virt` (no_std + no_main, requires aarch64 target). `cargo test` at workspace root runs host tests only; `cargo kernel-build` (alias) builds the bare-metal image. Matches [infrastructure.md](../../../standards/infrastructure.md) and `.cargo/config.toml` rustflags (panic=abort scoped to `aarch64-unknown-none`).
- **Trait surface.** `tyrne-hal` exports `Console`, `Cpu`, `ContextSwitch`, `IrqController`, `Mmu`, `Timer`, `IrqGuard`, `IrqState`, `CoreId`, `FmtWriter`. The kernel depends on `Cpu`, `ContextSwitch`, `IrqGuard` only; HAL exports used by the BSP: `Console` (via Pl011Uart impl), `Cpu` + `ContextSwitch` (via QemuVirtCpu impl). No dead imports.
- **Kernel is zero-`unsafe`.** Verified: the 12 UNSAFE-2026-NNNN entries all live in `bsp-qemu-virt/src/{main.rs, console.rs, cpu.rs}` or in `kernel/src/sched/mod.rs` where the `unsafe` blocks *invoke* the BSP's `ContextSwitch`/`init_context` traits (i.e. the unsafety is the trait contract, not the kernel crate's own code). `kernel/src/cap/`, `kernel/src/obj/`, `kernel/src/ipc/` contain zero `unsafe` occurrences. Matches the A6 business-review claim.
- **Test-hal linkage.** `tyrne-test-hal` is `[dev-dependencies]` only on `kernel`. Correct — host tests can wire in `FakeCpu`/`FakeConsole`/etc; production builds cannot pick up the fakes.
- **CI coverage.** No `.github/workflows` or equivalent CI config is present at HEAD. `run-qemu.sh` is invoked manually per the `two-task-demo` guide. This is flagged as Phase B work in both the business review and in this review's Test Coverage section. Non-blocking for Phase A exit.
- **ADR consistency.** ADR-0018 (badge scheme) is Deferred — verified in `0018-badge-scheme-and-reply-recv-deferral.md`; no code references a badge primitive today. ADR-0015 (AI-integration stance, kernel-neutral) — no AI-specific dependency or code path, consistent. ADR-0020 (`ContextSwitch` split from `Cpu`) — code structure matches: `hal/src/context_switch.rs` is its own module, `Cpu` remains object-safe (`&dyn Cpu` used in `sched::Scheduler::resolve_ep_cap`'s signature indirectly and doc-comments; concrete `C: ContextSwitch + Cpu` generic used for `Scheduler` struct).
- **Downstream callers.** No downstream consumer of any of these crates exists outside the workspace; pre-alpha. API-break scope is internal only. The `Send`/`Sync` bounds on `Cpu` are compiler-checked.
- **Commits follow commit-style.** Spot-checked: `feat(a5)`, `docs(adr)`, `fix(review)`, `chore(...)` prefixes used consistently; Claude-authored commits carry the `Co-Authored-By: Claude Sonnet 4.6 ...` trailer expected by the project convention.

## Verdict

**Approve** (with four follow-up tasks and one cross-reference dependency).

All five passes returned either "clean" or "minor, non-blocking" findings. No blocker. The change is internally consistent with all 19 Accepted ADRs, every `unsafe` is audited and cross-verified, 109 host tests pass, the Phase A exit bar is demonstrated on real QEMU, and the kernel crate itself is zero-`unsafe` as designed.

**Approval is conditional on the paired security review** ([docs/analysis/reviews/security-reviews/2026-04-21-tyrne-to-phase-a.md](../security-reviews/2026-04-21-tyrne-to-phase-a.md)) also returning Approve. Per [master-plan §Pre-flight](master-plan.md#pre-flight-risk-class), a security-sensitive change cannot ship on a code-review Approve alone.

**Follow-up tasks (non-blocking, Phase B):**

1. Add a test for `IpcError::ReceiverTableFull` that provokes the pre-flight guard and asserts cap retention (see Test coverage §Missing, bullet 1).
2. Author `docs/architecture/{kernel-objects, ipc, scheduler}.md` via `write-architecture-doc` skill (see Documentation §Gaps, bullet 1).
3. Lock the UNSAFE-2026-0012 remediation (raw-pointer API) behind a new ADR before Phase B preemption or MMU-on work — already tracked in the A6 business review's tech-debt table; surface here for the code-review paper trail.
4. Decide whether `IpcError::InvalidCapability` should split into `WrongObjectKind` / `MissingRight` / `StaleHandle` (see Correctness §IPC, bullet 4) — needs an ADR amendment to 0017 if accepted.

No findings require code changes before merge.
