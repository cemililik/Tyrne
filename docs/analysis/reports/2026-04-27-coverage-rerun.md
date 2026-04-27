# Host-test coverage re-run — 2026-04-27 (post-T-011)

Captured with `cargo llvm-cov --workspace --exclude tyrne-bsp-qemu-virt --summary-only` on the [T-011 — Missing tests bundle](../tasks/phase-b/T-011-missing-tests-bundle.md) tip. The 2026-04-23 baseline ([2026-04-23-coverage-baseline.md](2026-04-23-coverage-baseline.md)) was the trigger for T-011's targeted test additions; this re-run records the delta and confirms the task's coverage-gate acceptance criteria are met.

## Headline numbers

| Scope | Baseline (2026-04-23) | Post-T-011 (2026-04-27) | Δ |
|---|---|---|---|
| **Workspace (excl. BSP)** — Regions | 94.41 % | **96.33 %** | **+1.92 pp** |
| Workspace — Lines | 92.85 % | 95.30 % | +2.45 pp |
| Workspace — Functions | 90.37 % | 91.94 % | +1.57 pp |
| `kernel/src/sched/mod.rs` — Regions | 84.16 % | **93.97 %** | **+9.81 pp** |
| `kernel/src/cap/table.rs` — Regions | 96.84 % | 97.46 % | +0.62 pp |
| `kernel/src/ipc/mod.rs` — Regions | 97.73 % | 97.86 % | +0.13 pp |

Both T-011 acceptance gates met:

- ✅ `sched/mod.rs` regions ≥ 90 % (target hit at 93.97 %; the four largest scheduler gaps closed by the new tests).
- ✅ Workspace regions ≥ 96 % (target hit at 96.33 %; the headline metric also crossed 95 % on lines).

Host-test count: **130 → 143** (+13 across kernel: 4 IPC + 5 sched + 4 cap-table targeted-sweep). Miri pass remains clean across the full 143 tests (`cargo +nightly miri test --workspace --exclude tyrne-bsp-qemu-virt`).

## Per-file, sorted weakest to strongest

| File | Regions | Functions | Lines | Notes |
|------|---------|-----------|-------|-------|
| `hal/src/mmu.rs` | 40.82 % | 35.71 % | 38.64 % | Unchanged — trait declaration surface; no production impl yet (MMU lands in B2). Routed to B2. |
| `kernel/src/cap/mod.rs` | 89.47 % | 100.00 % | 88.89 % | Unchanged — 2 small regions; not load-bearing. |
| `kernel/src/obj/endpoint.rs` | 93.75 % | 88.89 % | 92.68 % | Unchanged — one `get_endpoint` helper unused outside tests. |
| **`kernel/src/sched/mod.rs`** | **93.97 %** | 81.67 % | 93.38 % | **+9.81 pp regions** — `start_prelude` extraction + `ipc_send_and_yield` three-case bundle closed the four largest baseline gaps. Remaining 65 region misses are concentrated in the post-switch unreachable-loop tail of `start` and a few raw-pointer error paths only reachable under abnormal aliasing (which Miri exercises). |
| `kernel/src/obj/notification.rs` | 96.33 % | 91.67 % | 95.38 % | Unchanged. |
| `test-hal/src/mmu.rs` | 95.93 % | 89.29 % | 94.74 % | Unchanged. |
| `kernel/src/obj/arena.rs` | 97.25 % | 100.00 % | 99.17 % | Unchanged. |
| `kernel/src/cap/rights.rs` | 97.50 % | 94.44 % | 96.00 % | Unchanged. |
| **`kernel/src/cap/table.rs`** | **97.46 %** | 98.28 % | 95.89 % | **+0.62 pp regions** — four targeted error-branch tests (cap_derive on full table, cap_copy on stale handle, lookup on stale handle, drop-first-child unlink path). |
| **`kernel/src/ipc/mod.rs`** | **97.86 %** | 100.00 % | 98.01 % | Three new tests added (ReceiverTableFull pre-flight; stale-generation reset paired guards). The region count is unchanged at 21 because the new tests exercise the *same* lines through a different code path — the value is correctness coverage, not line coverage. |
| `test-hal/src/cpu.rs` | 97.83 % | 94.74 % | 96.70 % | Unchanged. |
| `test-hal/src/irq_controller.rs` | 98.31 % | 94.74 % | 96.74 % | Unchanged. |
| `kernel/src/obj/task.rs` | 100.00 % | 100.00 % | 100.00 % | — |
| `hal/src/console.rs` | 100.00 % | 100.00 % | 100.00 % | — |
| `hal/src/cpu.rs` | 100.00 % | 100.00 % | 100.00 % | — |
| `hal/src/timer.rs` | 100.00 % | 100.00 % | 100.00 % | T-009 — held at 100 %. |
| `test-hal/src/console.rs` | 100.00 % | 100.00 % | 100.00 % | — |
| `test-hal/src/timer.rs` | 100.00 % | 100.00 % | 100.00 % | — |

## What T-011 changed

The thirteen new tests close the four highest-value gaps the baseline flagged.

### `kernel/src/sched/mod.rs` (the headline win, +9.81 pp regions)

1. **`start_prelude` extraction + dedicated test pair** — the dequeue + state-mutation half of `start` was structurally untestable from the host because `cpu.context_switch` is ABI assembly. Extracting `start_prelude` as a no-context-switch helper — semantically unchanged for callers — turned ~50 lines of formerly-dead-from-host coverage into an actively-exercised path. The paired test (`start_prelude_dispatches_head_and_marks_ready` + `start_prelude_panics_on_empty_ready_queue`) covers both the happy and the panic branches.
2. **`ipc_send_and_yield` three-case bundle** — the bridge had three terminal shapes (Delivered, Enqueued, Err) and the baseline only indirectly exercised a subset. The new bundle pins each terminal explicitly. The `Err` test (`ipc_send_and_yield_send_error_preserves_scheduler_state`) is symmetric to T-007's `ipc_recv_and_yield_returns_deadlock_when_ready_queue_empty` state-restore test — it closes the symmetric send-side gap.

### `kernel/src/cap/table.rs` (+0.62 pp regions)

Four targeted error-branch tests closed:

1. `cap_derive` on a full table → `CapsExhausted` from the `pop_free()` failure (a distinct call site from `insert_root`'s exhaustion test).
2. `cap_copy` on a stale handle → `InvalidHandle` (third entry point; previously covered for `cap_drop` and `cap_take` only).
3. `lookup` on a stale handle → `InvalidHandle` direct (the validation branch every other API delegates to).
4. `cap_drop` on a first-child → exercises `unlink_from_siblings`'s "head of list" branch. The mid-list branch was already covered by `drop_middle_sibling_preserves_list_integrity`; this completes the unlink path.

### `kernel/src/ipc/mod.rs` (correctness, not regions)

Three tests were added but the region count stayed at 21 because the tests exercise the *same* code through different state shapes. Their value is correctness, not coverage:

1. `recv_with_full_table_preserves_pending_cap` — confirms `ipc_recv`'s pre-flight guard does not silently drop the in-flight capability when the receiver's table is at capacity.
2. `stale_send_pending_with_some_cap_panics_in_debug` — confirms `IpcQueues::reset_if_stale_generation`'s `debug_assert!` fires on the leak case (slot reused with `Some(cap)` left behind).
3. `stale_recv_waiting_resets_silently` + `stale_send_pending_without_cap_resets_silently` — the must-not-panic guards proving the assert's predicate is not over-broad.

The first item closes a Phase-A code-review §Test coverage finding; the other three are the paired-test guard that ADR-0021's *Decision drivers* §"Invariant assertions must be testable" calls out as project policy.

## Remaining gaps (not in T-011's scope)

- `hal/src/mmu.rs` 40.82 % — trait surface only; production impl lands in **B2**.
- `kernel/src/sched/mod.rs` 65 region misses concentrate in the post-switch tail of `start` (the `loop { spin_loop() }` defensive guard reached only if `cpu.context_switch` returns, which it does not by construction) and a few raw-pointer error paths Miri exercises but `llvm-cov` cannot. Additional work here is diminishing returns.
- BSP coverage (`bsp-qemu-virt`) — excluded by design until a CI runner can drive QEMU smoke; routed to T-009 follow-up.
- 80–90 % coverage on `kernel/src/cap/mod.rs` and `kernel/src/obj/endpoint.rs` — small files with one or two low-value branches; not in T-011 scope.

## Reproducing this run

```sh
cargo llvm-cov --workspace --exclude tyrne-bsp-qemu-virt --summary-only
cargo llvm-cov --workspace --exclude tyrne-bsp-qemu-virt --lcov \
    --output-path docs/analysis/reports/lcov-2026-04-27.info
```

Pin: `cargo-llvm-cov 0.6.16` (matches CI per [docs/guides/ci.md](../../guides/ci.md)). Nightly: `nightly-2026-01-15` (matches `NIGHTLY_PIN` in `.github/workflows/ci.yml`).

## Verdict

T-011's coverage acceptance criteria are met without exception. The headline scheduler-coverage cliff identified in the baseline triage is closed; the long tail (BSP, MMU stub, post-switch defensive code) is correctly scoped to later phases.

---

## Follow-up note (added 2026-04-27, post-PR-#9 review-round 2)

A second-pass review of T-011's tip flagged that the `cap/table.rs` numbers in the table above are now slightly stale relative to the actual post-fix tip. The drift is positive (coverage went up) and is the direct effect of the second review-round's `drop_first_child_…` test fix (commit `9a8e312`): the test was renamed-and-retargeted to drop `last` (the actual list head) instead of `first` (the list tail), which moved the exercised branch from mid-list to head-of-list in `unlink_from_siblings` and added a region not previously hit.

Drift, measured against the post-fix tip:

| File | This report | Post-fix observed | Δ |
|---|---|---|---|
| `kernel/src/cap/table.rs` regions | 97.46 % | 97.60 % | +0.14 pp |
| Workspace regions | 96.33 % | 96.37 % | +0.04 pp |
| `kernel/src/sched/mod.rs` regions | 93.97 % | 93.97 % | unchanged |
| `kernel/src/ipc/mod.rs` regions | 97.86 % | 97.86 % | unchanged |

Both AC gates remain comfortably met (sched ≥ 90 %, workspace ≥ 96 %); the shift just makes the existing margin slightly larger. The original tables above are intentionally not rewritten — the headline numbers describe the state at T-011's commit (`761af95`) and the post-fix delta lives here in this follow-up so the report stays a true historical artifact. A future B0-closure consolidated coverage rerun (in its own report) is the natural place to re-measure once T-008 / T-013 promote to Done.
