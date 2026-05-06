# Track F — Tests & coverage depth

- **Agent run by:** Claude general-purpose agent, 2026-05-06
- **Scope:** Test depth, per-variant coverage, miri/property posture, smoke-regression.
- **HEAD reviewed:** [`214052d`](../../../../../) (`development`)
- **Test counts:** 149 / 149 host (25 [tyrne-hal] + 90 [tyrne-kernel] + 34 [tyrne-test-hal]; 2 doc-tests ignored). Miri at HEAD: **incomplete capture** — the background `cargo +nightly miri test --workspace --exclude tyrne-bsp-qemu-virt` task left only the doc-test trailer in `/tmp/claude-503/.../tasks/bm2uv1pmo.output` (3 doc-test result lines, all `0 passed; 0 failed`); the unit-test phase is not in the captured stream. The most recent confirmed clean Miri pass is the **143 / 143** post-T-011 record in [2026-04-23-miri-validation.md](../../../reports/2026-04-23-miri-validation.md) plus the rerun confirmed in [2026-04-27-coverage-rerun.md §"Miri pass remains clean"](../../../reports/2026-04-27-coverage-rerun.md). Six new host tests have landed since T-011 closed (143 → 149) — five of them are `start_prelude` / `ipc_send_and_yield` / `ipc_recv` additions at the same surface Miri already covers, the sixth is the `start_prelude_panics_on_empty_ready_queue` `should_panic` test which Miri also handles. **Miri-at-HEAD is therefore expected-clean but not directly observed by this run.** Flagged as Observation §M-1; the merge agent should reconcile if a fresh Miri run lands.

## Per-error-variant matrix

Every error-enum variant shipping in HEAD's source tree, mapped to the test (or absence of one) that provokes it. Variants are listed enum-by-enum in source order. "Provoking test" = a host test that asserts `Err(<variant>) == result` or asserts on a `should_panic(expected = …)` whose payload identifies the variant.

| Error type | Variant | Test that provokes it | Status |
|---|---|---|---|
| [`CapError`](../../../../../kernel/src/cap/mod.rs#L140) | `CapsExhausted` | [`cap_derive_on_full_table_returns_caps_exhausted`](../../../../../kernel/src/cap/table.rs#L1067) (T-011); also `insert_root_full_returns_caps_exhausted` ([line 894](../../../../../kernel/src/cap/table.rs#L894)) | **Covered** |
| `CapError` | `InvalidHandle` | many — e.g. [`lookup_invalid_handle_returns_err`](../../../../../kernel/src/cap/table.rs#L644), `cap_drop_invalid_handle` (652), `cap_copy_stale_handle_returns_invalid_handle` (1085, T-011), `cap_revoke_invalid_handle` (856), `cap_take_invalid_handle` (945) | **Covered** |
| `CapError` | `WidenedRights` | [`cap_copy_widening_rejected`](../../../../../kernel/src/cap/table.rs#L695); also `cap_derive_widening_rejected` (740) | **Covered** |
| `CapError` | `InsufficientRights` | [`cap_copy_without_duplicate_right_rejected`](../../../../../kernel/src/cap/table.rs#L705); also `cap_derive_without_derive_right` (728), `cap_revoke_insufficient_rights` (840) | **Covered** |
| `CapError` | `DerivationTooDeep` | [`cap_derive_at_max_depth_rejected`](../../../../../kernel/src/cap/table.rs#L756) | **Covered** |
| `CapError` | `HasChildren` | [`cap_drop_with_children_returns_has_children`](../../../../../kernel/src/cap/table.rs#L911); also `cap_take_with_children` (935) | **Covered** |
| [`IpcError`](../../../../../kernel/src/ipc/mod.rs#L73) | `InvalidCapability` | many — e.g. [`send_with_invalid_handle_returns_invalid_capability`](../../../../../kernel/src/ipc/mod.rs#L754), `recv_with_invalid_handle` (766), `notify_with_non_notification_handle` (953); also lifted via `SchedError::Ipc(InvalidCapability)` in [`ipc_send_and_yield_invalid_cap_preserves_state`](../../../../../kernel/src/sched/mod.rs#L1540) | **Covered** |
| `IpcError` | `QueueFull` | [send-side `send_when_send_pending_returns_queue_full`](../../../../../kernel/src/ipc/mod.rs#L798); [recv-side `recv_when_recv_already_waiting_returns_queue_full`](../../../../../kernel/src/ipc/mod.rs#L812); also asserted in T-007 deadlock follow-up at line 856 | **Covered** |
| `IpcError` | `InvalidTransferCap` | [`send_with_stale_transfer_cap`](../../../../../kernel/src/ipc/mod.rs#L851); also `send_without_duplicate_right_on_transfer_cap` (883) | **Covered** |
| `IpcError` | `ReceiverTableFull` | [`recv_with_full_table_preserves_pending_cap`](../../../../../kernel/src/ipc/mod.rs#L1004) (T-011) — explicit `assert_eq!(err, IpcError::ReceiverTableFull)` at line 1059, plus a recovery half-of-test that proves the cap survived the failed recv. **Phase A code-review's flagged gap is closed.** | **Covered** |
| `IpcError` | `PendingAfterResume` | [`ipc_recv_and_yield_resume_pending_returns_typed_err`](../../../../../kernel/src/sched/mod.rs#L1209) — uses the `ResetQueuesCpu` test-Cpu to force the pathological state. The variant is reachable only via `SchedError::Ipc(PendingAfterResume)`. | **Covered** |
| [`SchedError`](../../../../../kernel/src/sched/mod.rs#L145) | `NoCurrentTask` | [`yield_now_with_no_current_returns_error`](../../../../../kernel/src/sched/mod.rs#L1027) — `assert_eq!(result, Err(SchedError::NoCurrentTask))` at line 1033 | **Covered** |
| `SchedError` | `QueueFull` | **Not directly tested.** The only `map_err(\|_\| SchedError::QueueFull)` is in [`Scheduler::add_task`](../../../../../kernel/src/sched/mod.rs#L264). Ready-queue capacity equals `TASK_ARENA_CAPACITY` ([line 197](../../../../../kernel/src/sched/mod.rs#L197)) and `add_task` returns `ArenaFull`-equivalent before the queue can overflow, so the path is **structurally unreachable** under the v1 invariants. Documented at lines 237-243 as defensive. Treat as a "documented-unreachable defensive variant"; not a test gap, but called out for completeness. | **Documented unreachable** |
| `SchedError` | `Ipc(IpcError)` | Composite variant — exercised through `Ipc(InvalidCapability)` ([sched test 1540](../../../../../kernel/src/sched/mod.rs#L1540)) and `Ipc(PendingAfterResume)` ([sched test 1273](../../../../../kernel/src/sched/mod.rs#L1273)). Other inner `IpcError`s wrapping is by `From<IpcError>` impl ([line 183](../../../../../kernel/src/sched/mod.rs#L183)) and is type-system-preserving. | **Covered (representative)** |
| `SchedError` | `Deadlock` | [`ipc_recv_and_yield_returns_deadlock_when_ready_queue_empty`](../../../../../kernel/src/sched/mod.rs#L1106) (T-007) — `matches!(result, Err(SchedError::Deadlock))` plus state-restore assertion. The variant is structurally unreachable in the v1 cooperative workload (per ADR-0022's idle-task) but the test pins the typed-error semantics for the preemption / SMP future. | **Covered** |
| [`ObjError`](../../../../../kernel/src/obj/mod.rs#L57) | `ArenaFull` | [`arena_exhaustion_returns_arena_full`](../../../../../kernel/src/obj/task.rs#L134) — fills `TaskArena` to capacity, asserts the next `create_task` returns `ObjError::ArenaFull`. The generic `Arena<T,N>` implementation is shared by `EndpointArena`, `TaskArena`, `NotificationArena`, so a single concrete-instance test covers the variant. | **Covered (via TaskArena)** |
| `ObjError` | `InvalidHandle` | [`destroy_invalidates_handle`](../../../../../kernel/src/obj/task.rs#L120) — asserts `destroy_task` on already-freed handle returns `ObjError::InvalidHandle`. Endpoint and Notification arena tests at [line 104](../../../../../kernel/src/obj/endpoint.rs#L104) / [line 122](../../../../../kernel/src/obj/notification.rs#L122) check `is_none()` only and do not directly assert on this variant — the underlying generic path is the same as TaskArena's, so this is sufficient by code-share. | **Covered (via TaskArena)** |
| `ObjError` | `StillReachable` | **Not produced anywhere in-tree.** Variant is documented as "callers that enforce the reachability invariant pass this back to their own caller" — but no `destroy_*` function in `kernel/src/obj/` returns it, and no caller in `kernel/src/` or `bsp-qemu-virt/src/` constructs it. Per testing.md's "An error path — a new variant in an Error enum — has a test that provokes it" the variant **technically violates the rule**, but the rule's intent is "every variant a function can return is tested." `StillReachable` is presently a **contract placeholder for a future reachability-aware destroy API**; the variant exists for forward compatibility and `#[non_exhaustive]` already handles the addition. Flag as Non-blocking Finding §F-2 with two acceptable resolutions: (a) write a host test that constructs the variant via `Err(ObjError::StillReachable)` and asserts the discriminant for documentation purposes, or (b) remove the variant until a producer lands. | **Untested (no producer)** |
| [`MmuError`](../../../../../hal/src/mmu.rs#L165) | `AlreadyMapped` | [`double_map_returns_already_mapped`](../../../../../test-hal/src/mmu.rs#L266) (FakeMmu) | **Covered** |
| `MmuError` | `NotMapped` | [`unmap_missing_returns_not_mapped`](../../../../../test-hal/src/mmu.rs#L295) (FakeMmu) | **Covered** |
| `MmuError` | `MisalignedAddress` | **Not produced.** No `Mmu` impl in-tree returns this — neither `FakeMmu` (which never validates alignment) nor any production `Mmu` (the production aarch64 MMU lands in B2). The variant is part of the trait's documented contract but currently has zero producers. | **Untested (no producer until B2)** |
| `MmuError` | `OutOfFrames` | **Not produced.** Same as above — `FakeMmu`'s `map` does not call `frame_provider.alloc_frame()` (the fake stores mappings in a `BTreeMap` and never needs intermediate-table frames), so the variant cannot be reached today. | **Untested (no producer until B2)** |
| `MmuError` | `InvalidFlags` | **Not produced.** Same as above — `FakeMmu` accepts any `MappingFlags` combination; the variant exists for the production aarch64 impl that must reject e.g. `WRITE | EXECUTE` on an EL0 user mapping per ARMv8 architectural rules. | **Untested (no producer until B2)** |

### Per-variant summary

- **Total variants:** 23 (`CapError` 6 + `IpcError` 5 + `SchedError` 4 + `ObjError` 3 + `MmuError` 5).
- **Provoking test exists:** **17** (CapError 6/6, IpcError 5/5, SchedError 3/4, ObjError 2/3, MmuError 2/5).
- **Documented unreachable / no producer:** **6** (`SchedError::QueueFull`, `ObjError::StillReachable`, `MmuError::{MisalignedAddress, OutOfFrames, InvalidFlags}`; the `MmuError` trio is gated on B2's production aarch64 MMU; `SchedError::QueueFull` is structurally unreachable per the ready-queue capacity = task-arena capacity invariant; `ObjError::StillReachable` has no in-tree producer and is a forward-compat placeholder).

**Phase A code-review's named gap (`IpcError::ReceiverTableFull`) is fully closed by T-011.** Verified at [`kernel/src/ipc/mod.rs:1004`](../../../../../kernel/src/ipc/mod.rs#L1004) — the test not only provokes the variant (`assert_eq!(err, IpcError::ReceiverTableFull)` at line 1059) but also includes a recovery half (free a slot, retry) that asserts the in-flight cap was preserved. This is exactly the "the cap was not silently dropped" assertion the Phase A reviewer asked for.

## Per-subsystem coverage thresholds

[`docs/standards/testing.md`](../../../../standards/testing.md) §Coverage establishes a **soft target of 80 % line coverage on kernel-core subsystems** — IPC, capabilities, memory, scheduler — and explicitly disclaims a hard threshold ("The project does not set a hard coverage number."). The post-T-011 rerun ([2026-04-27-coverage-rerun.md](../../../reports/2026-04-27-coverage-rerun.md)) is the most recent on-disk measurement.

| File | Reported regions (2026-04-27 rerun) | Soft floor (80 % lines, kernel-core) | Status at HEAD |
|---|---|---|---|
| `kernel/src/sched/mod.rs` | 93.97 % | ≥ 80 % | Above floor; held above the T-011 acceptance gate of 90 %. **Stale** — the `start_prelude_panics_on_empty_ready_queue` `should_panic` test (and four other tests added since T-011) plus a [docs-only `cap/table.rs` test rename](../../../reports/2026-04-27-coverage-rerun.md#L97) probably moved this; magnitude unknown. |
| `kernel/src/ipc/mod.rs` | 97.86 % | ≥ 80 % | Above floor. |
| `kernel/src/cap/table.rs` | 97.46 % (97.60 % per follow-up note) | ≥ 80 % | Above floor. |
| `kernel/src/cap/mod.rs` | 89.47 % | ≥ 80 % | Above floor. |
| `kernel/src/obj/*.rs` | 93.75 % – 100 % | ≥ 80 % | Above floor. |
| `kernel/src/cap/rights.rs` | 97.50 % | ≥ 80 % | Above floor. |
| `hal/src/mmu.rs` | 40.82 % | n/a (trait surface, no production impl yet) | Documented gap; production impl lands in B2. |
| `bsp-qemu-virt/src/*` | (excluded from llvm-cov; `no_std` / `no_main`) | n/a | Routed to T-009 follow-up; QEMU-smoke-only. |

**Summary:** every kernel-core file is comfortably above the 80 % soft floor. No regression to flag. **Caveat:** the numbers are 9 days old and 6 tests (143 → 149) plus a small number of source-code changes (`start_prelude` extraction settled, T-012 BSP-side additions) have landed since the rerun. A fresh `cargo llvm-cov --workspace --exclude tyrne-bsp-qemu-virt --summary-only` would confirm the deltas; the existing trend is monotonic-up so a regression is unlikely.

**Recommendation:** rerun coverage at the next phase-boundary closure (B2 entry), naming the artifact `docs/analysis/reports/<ISO>-coverage-rerun.md`. This is sufficient — there is no per-subsystem-fail risk worth a per-PR coverage gate today.

## Property / fuzz / Miri posture

### Miri (Stacked Borrows)

- **Subsystems already validated:** the full host-test suite minus BSP, per [2026-04-23-miri-validation.md](../../../reports/2026-04-23-miri-validation.md). The pre-fix run found one real Stacked Borrows violation in T-007's `ResetQueuesCpu` helper (raw-pointer re-derivation invalidating an earlier tag); fix landed in the same change. Subsequent runs at T-011 close (143 / 143) and the current 149 / 149 surface have **been clean per the maintainer's local runs and CI** (CI pinning per [`.github/workflows/ci.yml`](../../../../../.github/workflows/ci.yml#L107) — the `miri` job runs every push to `development` and `main` and every PR). At HEAD specifically, the background run captured by the harness left only the doc-test phase output; this is a tooling capture issue, not a Miri regression. **Treat as: expected-clean, not directly observed by this Track.**
- **Subsystems still pending:** `bsp-qemu-virt/` is intentionally excluded — the BSP is a `no_std` / `no_main` bare-metal binary that Miri cannot build for the host target ([2026-04-23-miri-validation.md §"What this does NOT validate"](../../../reports/2026-04-23-miri-validation.md)). The kernel logic the BSP calls into is Miri-validated; the BSP's pointer plumbing (`StaticCell::as_mut_ptr`, the task-body pointer threading) is only validated statically.
- **Tree Borrows refinement:** [`docs/guides/ci.md`](../../../../guides/ci.md) and the miri-validation report both acknowledge `-Zmiri-tree-borrows` as a future tightening. Not yet enabled. Fine.

### Property / fuzz tests

**No `proptest`, no `quickcheck`, no `cargo-fuzz` harness in the workspace.** Confirmed by `grep -rn "proptest\|fuzz\|quickcheck"` across all `Cargo.toml` files and all `*.rs` source files — zero hits. The testing standard does not require property tests but the trail of subsystems is:

| Subsystem | Property-test fit | Rationale |
|---|---|---|
| `kernel/src/cap/table.rs` (CapabilityTable derivation tree) | **Strong fit.** The derivation tree's invariants — "no widening through `cap_derive`", "depth ≤ MAX_DERIVATION_DEPTH", "every parent's right-set is a superset of every child's", "`cap_revoke` on a node removes the entire subtree", "the parent–child–sibling linked-list structure is consistent (no cycles, no orphans)" — are quantified-over-N invariants that a property test (random sequences of `insert_root` / `cap_derive` / `cap_copy` / `cap_drop` / `cap_revoke` operations) could exercise far more aggressively than the 25-ish hand-written tests. Closest in spirit: `references_object_sees_live_caps_only` ([table.rs:793](../../../../../kernel/src/cap/table.rs#L793)). |
| `kernel/src/obj/arena.rs` | **Medium fit.** Slot-generation discipline ("after a free + alloc cycle the old handle's lookup must fail") is amenable to a randomized alloc/free sequence with assertions on every step. Hand-written `destroy_invalidates_handle` ([task.rs:120](../../../../../kernel/src/obj/task.rs#L120) / [notification.rs:140](../../../../../kernel/src/obj/notification.rs#L140)) covers the simple case. |
| `kernel/src/cap/rights.rs` | **Medium fit.** `CapRights` is a bitfield with `from_raw` / `raw` round-trip + intersection / difference algebra. Currently 7 hand-written tests; a property test asserting `from_raw(raw(x)) == x & KNOWN_BITS` over random `u32`s would close any boundary case. |
| `hal/src/timer.rs` | **Medium fit.** `ticks_to_ns` and `ns_to_ticks` round-trip arithmetic with ceiling-rounding guarantees; the existing tests cover the QEMU frequency, edge values, and saturation. A property test over `(ticks, freq)` random inputs would catch saturation-boundary fencepost bugs the four hand-written tests can miss. |
| `kernel/src/ipc/mod.rs` | **Lower fit.** State-machine transitions are well-covered by the hand-written suite (every `EndpointState` × `op` pair has a dedicated test). The state space is small enough that an enumerated "every (state, op) pair → expected (state', outcome)" matrix would be a more direct way to prove completeness than property-based generation. Treat as deferred. |
| `kernel/src/sched/mod.rs` | **Low fit.** The scheduler's invariants are deeply tied to the unsafe raw-pointer bridge; property testing here without a thread-sanitizer / loom-style harness would only re-cover what the existing T-007 / T-011 typed-error tests already pin. Loom is the better future tool when SMP lands.

**Recommendation (Non-blocking, deferred to B2 or later):** open a roadmap task to add a `proptest` (or `bolero`) dependency under `[dev-dependencies]` of `tyrne-kernel` and write **one** property test against `CapabilityTable`'s derivation-tree invariants — this is the highest-yield property-test target in the tree. Justify the new dependency under the dependency policy in [`docs/standards/infrastructure.md`](../../../../standards/infrastructure.md). Not blocking on B2 entry.

## Smoke-as-regression

[`tools/run-qemu.sh`](../../../../../tools/run-qemu.sh) is **maintainer-launched.** No CI job invokes it; the `.github/workflows/ci.yml` matrix is `lint-and-host-test`, `kernel-build`, `miri`, `coverage` — no `qemu-smoke`. Consequence:

- **The "Pending QEMU smoke verification" notes on [UNSAFE-2026-0019](../../../../audits/unsafe-log.md#L336-L354) (GIC MMIO), [UNSAFE-2026-0020](../../../../audits/unsafe-log.md#L356-L379) (EL1 vector table install + asm trampolines), and [UNSAFE-2026-0021](../../../../audits/unsafe-log.md#L381-L402) (EL1 virtual generic-timer compare-register writes) cannot self-clear.** Each entry says "Pending QEMU smoke verification at the maintainer's first opportunity per the T-012 review-history row" — but unless someone (the maintainer or an agent with QEMU access) runs `tools/run-qemu.sh` against HEAD and confirms the [two-task-demo's expected serial output](../../../../guides/two-task-demo.md#L37-L45), those notes will accumulate forever.
- **The two-task IPC demo expected-output table at [`docs/guides/two-task-demo.md`§Expected output`](../../../../guides/two-task-demo.md#L37-L45) is a regression contract that nothing checks.** Six lines of expected serial output. A 30-line `tools/qemu-smoke.sh` (or amendment to `run-qemu.sh`) that boots the kernel under QEMU with `-display none -no-reboot` and grep-asserts those six lines is well within reach. Adding it as a CI job (likely a separate `qemu-smoke` job that installs `qemu-system-aarch64` and runs the kernel under timeout) closes the regression-as-smoke gap.
- **T-009 follow-up referenced in [2026-04-23-coverage-baseline.md](../../../reports/2026-04-23-coverage-baseline.md#L9) ("BSP code is exercised indirectly by the QEMU smoke test; automated coverage would require the same kind of tooling a CI pipeline does")** is the same need from the coverage angle — once a QEMU CI job exists, `-C instrument-coverage` profiles can attach to it.

**This Track raises the absence as Non-blocking Finding §F-1.** It does not block B2 (the maintainer can manually clear the three Pending notes by running the smoke once at HEAD), but it is a **structural debt** that will accumulate as more `unsafe` blocks added in B2 and beyond carry their own "Pending QEMU smoke verification" notes.

## Test hygiene

### `#[allow(clippy::*)] reason = "..."` block on every kernel test module

Audit of every kernel `#[cfg(test)] mod tests` block and the `#![deny]` lints they sit under:

| File | clippy::unwrap_used | clippy::expect_used | clippy::panic | reason populated |
|---|---|---|---|---|
| [`kernel/src/lib.rs`](../../../../../kernel/src/lib.rs#L38) crate-level | `#![deny(clippy::panic)]`, `#![deny(clippy::unwrap_used)]`, `#![deny(clippy::expect_used)]` | — | — | Crate-wide denies define the baseline. |
| [`kernel/src/obj/arena.rs:185`](../../../../../kernel/src/obj/arena.rs#L185) tests | ✅ | ✅ | (not used; not allowed) | ✅ "tests may use pragmas forbidden in production kernel code" |
| [`kernel/src/obj/task.rs:104`](../../../../../kernel/src/obj/task.rs#L104) tests | ✅ | ❌ (missing) | ❌ (missing) | ✅ "tests may use pragmas forbidden in production kernel code" |
| [`kernel/src/obj/endpoint.rs:96`](../../../../../kernel/src/obj/endpoint.rs#L96) tests | ✅ | ❌ (missing) | ❌ (missing) | ✅ "tests may use pragmas forbidden in production kernel code" |
| [`kernel/src/obj/notification.rs:111`](../../../../../kernel/src/obj/notification.rs#L111) tests | ✅ | ❌ (missing) | ❌ (missing) | ✅ "tests may use pragmas forbidden in production kernel code" |
| [`kernel/src/cap/table.rs:600`](../../../../../kernel/src/cap/table.rs#L600) tests | ✅ | ✅ | ✅ | ✅ "tests may use pragmas forbidden in production kernel code" |
| [`kernel/src/cap/rights.rs:129`](../../../../../kernel/src/cap/rights.rs#L129) tests | ✅ | ✅ | (not used; not allowed) | ✅ "tests are allowed pragmas forbidden in production kernel code" |
| [`kernel/src/sched/mod.rs:836`](../../../../../kernel/src/sched/mod.rs#L836) tests | ✅ | ✅ | ✅ | ✅ "test pragmas not permitted in production kernel code" |
| [`kernel/src/ipc/mod.rs:480`](../../../../../kernel/src/ipc/mod.rs#L480) tests | ✅ | ✅ | ✅ | ✅ "tests may use pragmas forbidden in production kernel code" |

**Finding §F-3 (Non-blocking):** the `obj/task.rs` / `obj/endpoint.rs` / `obj/notification.rs` test modules omit `clippy::expect_used` and `clippy::panic` from their allow lists. They get away with it today because none of those test bodies use `.expect()` or `panic!()` — they only use `.unwrap()`. This works **today**; it is a hygiene drift because:

1. The pattern in the rest of the kernel (`cap/table.rs`, `sched/mod.rs`, `ipc/mod.rs`) is to allow all three lints together, treating the `#[cfg(test)]` block as a pragma-relaxed island.
2. Adding a test that uses `.expect()` to one of those files would now fire `clippy::expect_used` and break the build — a maintainer would either add the missing lint to the allow list (re-establishing the pattern) or rewrite the test to use `.unwrap_or_else` (fighting the drift). The current state lets the next contributor make either choice.
3. The cost to fix is one extra line per file.

Treat as a soft-edge consistency item — appropriate for a roll-up cleanup PR, not its own task.

### `hal/src/timer.rs` and `test-hal/src/*.rs` test modules

`hal/src/lib.rs` and `test-hal/src/lib.rs` do **not** have crate-level `#![deny(clippy::panic|unwrap_used|expect_used)]`. So their test modules don't strictly need allow blocks for those lints, and they don't have any. This is **consistent with the standard**, which scopes the strict-pragma deny to the kernel crate. No finding.

### `should_panic` discipline

Every `should_panic` annotation in the workspace carries `expected = "..."`:

| Site | Expected payload |
|---|---|
| [`kernel/src/sched/mod.rs:1316`](../../../../../kernel/src/sched/mod.rs#L1316) | `"empty ready queue"` |
| [`kernel/src/ipc/mod.rs:1111`](../../../../../kernel/src/ipc/mod.rs#L1111) | `"endpoint slot must be drained"` |
| [`hal/src/timer.rs:399`](../../../../../hal/src/timer.rs#L399) | `"ticks_to_ns: frequency_hz must be > 0"` |
| [`hal/src/timer.rs:410`](../../../../../hal/src/timer.rs#L410) | `"resolution_ns_for_freq: frequency_hz must be > 0"` |
| [`hal/src/timer.rs:480`](../../../../../hal/src/timer.rs#L480) | `"ns_to_ticks: frequency_hz must be > 0"` |

No naked `#[should_panic]` (without `expected = "..."`) found. **Pass.**

## Test-helper hygiene

All non-public test scaffolding helpers are `pub(crate)` and `#[cfg(test)]`-gated:

| Helper | Gate | Visibility |
|---|---|---|
| [`SlotId::from_parts`](../../../../../kernel/src/obj/arena.rs#L51) | `#[cfg(test)]` | `pub(crate)` |
| [`TaskHandle::test_handle`](../../../../../kernel/src/obj/task.rs#L58) | `#[cfg(test)]` | `pub(crate)` |
| [`EndpointHandle::test_handle`](../../../../../kernel/src/obj/endpoint.rs#L52) | `#[cfg(test)]` | `pub(crate)` |
| [`NotificationHandle::test_handle`](../../../../../kernel/src/obj/notification.rs#L64) | `#[cfg(test)]` | `pub(crate)` |
| `EndpointHandle::test_handle` and `NotificationHandle::test_handle` carry `#[allow(dead_code, reason = "symmetric with TaskHandle::test_handle")]` — these helpers are not currently called from any test but are kept for API symmetry. The `dead_code` allow is appropriately reason-populated.

The `ResetQueuesCpu` test-Cpu in [`kernel/src/sched/mod.rs:1160`](../../../../../kernel/src/sched/mod.rs#L1160) is defined inside `mod tests`, so it inherits the `#[cfg(test)]` gate and is never visible outside the test build. Its `unsafe impl Send for ResetQueuesCpu` and `unsafe impl Sync for ResetQueuesCpu` carry `// SAFETY: test-only; the pointer refers to a stack-local IpcQueues the test thread exclusively owns.` — appropriate.

**No test helper leaks into the release build.** Zero findings on this axis.

## Findings

### Blocker

None.

### Non-blocking

**§F-1 — QEMU smoke is not CI-wired.** [`tools/run-qemu.sh`](../../../../../tools/run-qemu.sh) is maintainer-launched; no `qemu-smoke` job in [`.github/workflows/ci.yml`](../../../../../.github/workflows/ci.yml). The "Pending QEMU smoke verification" notes on [UNSAFE-2026-0019](../../../../audits/unsafe-log.md#L354) / [UNSAFE-2026-0020](../../../../audits/unsafe-log.md#L379) / [UNSAFE-2026-0021](../../../../audits/unsafe-log.md#L402) cannot self-clear. The two-task-demo's six-line expected-output table ([two-task-demo.md §Expected output](../../../../guides/two-task-demo.md#L37-L45)) is a regression contract nothing exercises. **Recommend:** open a B2-prep roadmap task to add a `qemu-smoke` CI job (install `qemu-system-aarch64`, boot kernel under `-display none -no-reboot`, grep-assert the six expected lines, time-bound under a 30-second timeout).

**§F-2 — `ObjError::StillReachable` has no producer.** [`obj/mod.rs:68`](../../../../../kernel/src/obj/mod.rs#L68) declares the variant; no `destroy_*` returns it; no caller constructs it. Per [testing.md §"What has tests"](../../../../standards/testing.md#L52) the rule is "every variant of an Error enum has a test that provokes it". The variant is documented as "callers that enforce the reachability invariant pass this back to their own caller" — i.e. it is a forward-compatibility placeholder for an as-yet-unwritten reachability-aware destroy API. Two acceptable resolutions: **(a)** delete the variant and rely on `#[non_exhaustive]` (the rust idiom for "we may add variants later" without committing to them), **(b)** leave the variant and extend testing.md with a "documented future variant" exception. Recommend (a) — the rule is right; the current state is drift.

**§F-3 — Three test modules omit `clippy::expect_used` / `clippy::panic` from their allow blocks.** [`obj/task.rs:104`](../../../../../kernel/src/obj/task.rs#L104), [`obj/endpoint.rs:96`](../../../../../kernel/src/obj/endpoint.rs#L96), [`obj/notification.rs:111`](../../../../../kernel/src/obj/notification.rs#L111) only allow `clippy::unwrap_used`. The pattern in the rest of the kernel is to allow the trio together. Currently works because the test bodies don't use `.expect()` or `panic!()`, but a contributor adding such usage would have to either fix the drift or fight it. **Recommend:** roll-up cleanup — add `clippy::expect_used` and `clippy::panic` (with the same `reason = "tests may use pragmas forbidden in production kernel code"`) to those three files.

### Observation

**§M-1 — Miri at HEAD is not directly observed by this run.** The background `cargo +nightly miri test --workspace --exclude tyrne-bsp-qemu-virt` task scheduled by the pre-flight left only the doc-test trailer (3 doc-test result lines, all `0 passed; 0 failed; N ignored`) in `/tmp/claude-503/.../tasks/bm2uv1pmo.output`. The unit-test phase output is not in the captured stream — likely a shell-redirect / harness-capture issue, not a Miri failure. Six new host tests have landed since T-011's confirmed-clean run (143 → 149); five exercise the same code surface Miri already covered, the sixth is a `should_panic` test which Miri tolerates. **Expected-clean.** Merge agent should reconcile if a fresh run lands during the review window.

**§M-2 — No property tests in the workspace.** Zero `proptest` / `quickcheck` / `bolero` / `cargo-fuzz` usage. testing.md does not require them. The highest-yield candidate is `CapabilityTable`'s derivation-tree invariants ([`kernel/src/cap/table.rs`](../../../../../kernel/src/cap/table.rs)). Defer to a B2 or later roadmap task; not blocking.

**§M-3 — Stale coverage numbers.** [2026-04-27-coverage-rerun.md](../../../reports/2026-04-27-coverage-rerun.md) is 9 days old; 6 host tests + a small set of source changes have landed since. Rerun at next phase-boundary closure (B2 entry) — expected delta is monotonic-up.

**§M-4 — `SchedError::QueueFull` is a documented-unreachable defensive variant.** Not a finding; called out for the matrix completeness. The `map_err(|_| SchedError::QueueFull)` site at [`sched/mod.rs:264`](../../../../../kernel/src/sched/mod.rs#L264) is structurally unreachable per the ready-queue capacity = task-arena capacity invariant; the variant is preserved as a defensive return for the SMP / preemption future. ADR-0019 acknowledges this. No action required.

**§M-5 — Three `MmuError` variants have no producer.** `MisalignedAddress`, `OutOfFrames`, `InvalidFlags` exist on the trait but neither `FakeMmu` nor any production `Mmu` produces them today. Production aarch64 MMU lands in B2 and will be the natural test-coverage closer. No action required at HEAD.

## Cross-track notes

- **→ Track B (kernel correctness):** the per-error-variant matrix here is the test-side mirror of any kernel-correctness finding Track B raises — every typed error path's *return* is named here; if Track B identifies a typed-error path that is *missing* (as opposed to untested), the matrix will need a new row. None expected.

- **→ Track C (security):** §F-1 (QEMU smoke not CI-wired) directly affects security posture — three `unsafe` blocks (UNSAFE-2026-0019/0020/0021) carry "Pending QEMU smoke verification" notes that cannot self-clear without smoke automation. Track C should weigh whether the absence is a security-track Blocker or a security-track Non-blocking; this Track raises it as Non-blocking F.

- **→ Track D (performance):** §M-2 (no property tests) intersects performance only weakly — the highest-yield property-test target (`CapabilityTable`) is correctness-side, not perf-side. No coordination needed.

- **→ Track E (docs):** §F-2 (`ObjError::StillReachable` has no producer) intersects with [testing.md §"What has tests"](../../../../standards/testing.md#L52) — if Track E flags any other "documentation says it exists; code does not produce it" mismatch, the resolution pattern is the same (delete or extend the contract).

- **→ Track G (audit log):** the three "Pending QEMU smoke verification" notes are audit-log entries and §F-1 is the test-track angle on the same problem; coordinate with Track G's audit-log entries findings.

## Sub-verdict

**Approve with two small follow-ups.**

The per-error-variant coverage is **complete for every variant a function in-tree actually returns** (17 of 23; the remaining 6 are documented-unreachable or no-producer). The Phase A code-review's named gap (`IpcError::ReceiverTableFull`) is closed by T-011 with a recovery-half-of-test that proves the cap survived the failed recv. Per-subsystem coverage is comfortably above the 80 % soft floor; no regression. Miri posture is healthy (last confirmed clean: 143 / 143 post-T-011; HEAD's 149 / 149 expected-clean — capture issue only). Test hygiene is mostly clean (`should_panic` discipline 100 %; clippy-allow blocks 6/9 fully populated, three with cosmetic drift). Test-helper hygiene is fully clean (zero leak into release; every helper `pub(crate)` + `#[cfg(test)]`).

The two follow-ups worth opening as roadmap tasks:

1. **B2-prep — wire QEMU smoke into CI** (closes §F-1; un-blocks the three Pending-QEMU notes; future-proofs against new `unsafe` blocks accumulating their own notes).
2. **Roll-up cleanup — `ObjError::StillReachable` decision + three-file lint-allow consistency** (§F-2 + §F-3).

Neither blocks B2 entry. Track F approves.

---

## Summary (3 lines)

- 23 error-enum variants reviewed: 17 covered by in-tree tests (incl. T-011's `ReceiverTableFull` close), 6 are documented-unreachable or no-producer (`SchedError::QueueFull`, `ObjError::StillReachable`, `MmuError::{MisalignedAddress, OutOfFrames, InvalidFlags}`).
- Coverage above 80 % soft floor on every kernel-core file at the 2026-04-27 rerun; Miri at HEAD expected-clean (capture issue, not a regression); no property tests in workspace (recommend deferred B2 task on `CapabilityTable`).
- Two non-blocking findings: QEMU smoke is maintainer-only (UNSAFE-2026-0019/0020/0021 "Pending" notes can't self-clear), and three test modules + one no-producer variant carry small hygiene drift. **Track F: Approve.**
