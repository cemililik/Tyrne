# Track F — Test coverage & test quality (PRs #12–#17 post-merge)

- **Agent run by:** Claude Opus 4.7 (1M context) agent, 2026-05-07
- **Scope:** Host-test coverage of the new behaviour landed by PRs #12–#17, miri Stacked-Borrows results, QEMU smoke discriminating power, regression-test discipline (fails-before-fix / passes-after), v1-reachable vs unreachable code-path coverage, and the "byte-for-byte smoke unchanged" claim's coverage implication.
- **Tip reviewed:** [`8dc433e`](../../../../../) (PR #17 merge — `main`). Branch in working tree: `t-015-endpoint-rollback-cancel-recv` (post-merge identical to `main`).
- **Test counts at tip:** **158 / 158 host** (149 → 152 via PR #12 / T-014; 152 → 158 via PR #17 / T-015), **158 / 158 miri** clean per [`current.md` 2026-05-07 update](../../../../roadmap/current.md#L7) and the [T-015 review-history rows](../../../tasks/phase-b/T-015-endpoint-rollback-cancel-recv.md#L113-L114).
- **Prior axis run:** [2026-05-06-full-tree/track-f-tests.md](../2026-05-06-full-tree/track-f-tests.md) — 149/149 host tip; §F-1 (QEMU smoke not CI-wired), §F-2 (`ObjError::StillReachable` no producer), §F-3 (three obj-test modules missing two clippy-allows). §F-3 is **closed by PR #15** (see Cross-PR audit below). §F-1 / §F-2 still open at this tip.

## PR-by-PR test-touch matrix

| PR | Tip commit | Test-touch | Notes |
|----|-----------|-----------|-------|
| #12 (T-014 idle dispatch) | [`298b5d2`](../../../../../) | **+3 host** in [`kernel/src/sched/mod.rs`](../../../../../kernel/src/sched/mod.rs) — `register_idle_stores_handle_in_idle_slot_and_not_in_ready_queue`, `dispatcher_picks_idle_only_when_ready_queue_empty`, `unblock_after_yield_dispatches_unblocked_receiver_not_idle` | 149 → 152. Each pins a behaviour that was *added* by ADR-0026's separate-fallback-slot refactor; the second is the smoke-regression's actual fail-shape (idle dispatched ahead of a ready task). |
| #13 (Track-E doc-fix) | [`cfc4924`](../../../../../) | **Zero source / zero test diff.** | 13 doc files only; 0 of `kernel/`, `hal/`, `test-hal/`, `bsp-qemu-virt/`. Verified via `git show --stat cfc4924`. |
| #14 (URL rename + Yüksek → High) | [`1cd810d`](../../../../../) | **Zero test-body diff.** | Touches code comments in `bsp-qemu-virt/src/*.rs` and `hal/src/*.rs` for stale `cemililik/TyrneOS` link rewrites; no changed `#[test]` body anywhere. Verified via stat-diff. |
| #15 (γ code polish) | [`e9fa019`](../../../../../) | **Zero `#[test]` additions or removals**; **+lint-allow drift closed** in [`kernel/src/obj/{task,endpoint,notification}.rs`](../../../../../kernel/src/obj/) test modules (added `clippy::expect_used` + `clippy::panic` to the `#[allow]` blocks). | Closes 2026-05-06 §F-3 cleanly. No test removed; the file-level diffs are inline `const { assert!(...) }` migrations, doc-comments, and a Size-proof comment in `cap_revoke`. **Verified — no silent test removal.** |
| #16 (B1 closure trio + δ docs) | [`95b15aa`](../../../../../) | **Zero test diff.** | Adds business / security / performance review files + the T-015 task user story. No source. |
| #17 (T-015 endpoint rollback) | [`8dc433e`](../../../../../) | **+5 IPC unit tests** in [`kernel/src/ipc/mod.rs`](../../../../../kernel/src/ipc/mod.rs#L1148-L1262) + **+1 sched regression test** in [`kernel/src/sched/mod.rs`](../../../../../kernel/src/sched/mod.rs#L1380-L1431); existing T-007 `ipc_recv_and_yield_returns_deadlock_when_ready_queue_empty` test grew an endpoint-state assertion (lines [1367-1377](../../../../../kernel/src/sched/mod.rs#L1367-L1377)) | 152 → 158. See §"PR #17 — the six new tests" below. |

**Cross-PR test-removal audit:** zero test bodies were removed across PRs #12–#17. PR #15's diff was hand-walked function-by-function (γ commits `d86746a` + `03606a6` + the `54b3c78` review-round fix); only doc-comment and `const { assert!(...) }` style migrations, plus the §F-3 close. No test fn was renamed or replaced silently.

## PR #17 — the six new tests

ADR-0032's *Decision outcome* simulation table row 3b (the Phase 2 Deadlock branch with `register_idle` not installed) is the contract. The six tests below (5 IPC + 1 sched) plus the augmented T-007 test pin that contract from two angles.

### IPC `cancel_recv` unit tests ([`kernel/src/ipc/mod.rs`](../../../../../kernel/src/ipc/mod.rs#L1148-L1262))

| # | Test | What it pins | Fails-before-fix? | Could pass without `ipc_cancel_recv` body? |
|---|------|--------------|-------------------|--------------------------------------------|
| 1 | [`cancel_recv_clears_recv_waiting_back_to_idle`](../../../../../kernel/src/ipc/mod.rs#L1158-L1178) | The success arc `RecvWaiting → Idle`; verified empirically — a follow-up `ipc_recv` returns `Pending` rather than `QueueFull`. | **Yes.** Without the `if matches!(state, RecvWaiting) { *state = Idle }` body, the slot would stay `RecvWaiting` and the follow-up `ipc_recv` would error `QueueFull` → `unwrap()` panic. | No — the assertion is *behavioural* (clean Idle re-registration), not a state-equality peek. |
| 2 | [`cancel_recv_on_idle_is_noop`](../../../../../kernel/src/ipc/mod.rs#L1181-L1192) | Idempotency on the `Idle` pre-state; ADR-0032 row 0. | **No** — passes both pre- and post-fix because `Idle` is the no-op branch. **Documents the contract**, does not catch a regression. | Yes — would pass even with an empty `ipc_cancel_recv` body that just returns `Ok(())`. |
| 3 | [`cancel_recv_on_send_pending_does_not_drop_message`](../../../../../kernel/src/ipc/mod.rs#L1195-L1232) | The "no-op on `SendPending`" branch + the parked sender's message survives the cancel call (the value-preserving property). | **Yes** in the negative direction — if `ipc_cancel_recv` blanket-cleared `state` to `Idle` regardless of pre-state, the parked SendPending message would be silently dropped and the follow-up `ipc_recv` would return `Pending` instead of the expected `Received(123)`. | No — asserts the message body matches `test_msg(123)` after cancel. |
| 4 | [`cancel_recv_without_recv_right_fails`](../../../../../kernel/src/ipc/mod.rs#L1235-L1246) | Rights enforcement — cancel needs the same `RECV` right that authorised the original recv. | **Yes** — without `validate_ep_cap(table, ep_cap, CapRights::RECV)?`, a SEND-only cap would succeed at clearing arbitrary endpoints' RecvWaiting slots (a confused-deputy hazard the security axis cares about). | No — uses `unwrap_err` + `assert_eq!(IpcError::InvalidCapability)`. |
| 5 | [`cancel_recv_is_idempotent`](../../../../../kernel/src/ipc/mod.rs#L1249-L1262) | Two consecutive cancels on the same slot — the second is a no-op (matches case 2 once the first has fired). | **No** in the strict sense — passes even without `ipc_cancel_recv` doing anything because the *first* cancel already passes case 1's behavioural check. **Documents the property**, does not catch a regression *by itself*. | Partly — the second cancel's `unwrap()` would still succeed against any `Ok(())`-returning body. |

**State-machine coverage of `cancel_recv` against the brief's checklist:**

| Pre-state of called endpoint | Test that exercises | Status |
|--|--|--|
| `RecvWaiting → Idle` (success path) | #1, #5 | Covered |
| Called from `Idle` (no-op error path) | #2 | Covered |
| Called from `SendPending` (no-op) | #3 | Covered |
| Called from `RecvComplete` (no-op) | **None directly.** | **Coverage gap §F-1 (Minor)** — see Findings. The branch is structurally the same code path as `SendPending` (the `if matches!(state, RecvWaiting)` returns false for both), so a single combined check would normally be sufficient, but `RecvComplete` is the third post-rendezvous state and the brief explicitly lists it. |
| Bad-cap path — stale generation | **None.** | **Observation §M-1** — `validate_ep_cap` returns `IpcError::InvalidCapability` for a stale lookup; `ep_arena.get(slot)` rejects a stale-generation `EndpointHandle`. Both paths are exercised by other IPC tests' setup but not by a dedicated cancel-stale test. |
| Bad-cap path — missing right | #4 | Covered |
| Interleaved cancel (cancel after another op) | #5 (cancel-after-cancel); #3 (cancel-after-send) | Covered |

The five-test suite is **adequate for the v1 ADR-0032 contract** (the production caller is the Deadlock branch, which always reaches cancel from `RecvWaiting`). The two gaps above are non-blocking — both branches are covered by code-share with already-tested branches; they would become real gaps when multi-waiter wake (ADR-0019 §Open questions) lands and `cancel_recv` grows a `caller: TaskHandle` parameter.

### Scheduler regression test — [`ipc_recv_and_yield_deadlock_rolls_back_endpoint_state`](../../../../../kernel/src/sched/mod.rs#L1387-L1431)

**Pre-T-015 state.** The 2026-05-06 [Track A non-blocker](../2026-05-06-full-tree/track-a-kernel.md) noted *"the endpoint state was already moved to RecvWaiting and is not reversed"* as a deferred non-blocker. Verified by `git show 95b15aa1:kernel/src/sched/mod.rs` — at PR #16 tip the only Deadlock test was T-007's `ipc_recv_and_yield_returns_deadlock_when_ready_queue_empty`, which asserted scheduler state restoration only (`sched.current`, `sched.task_states[0]`, `sched.ready.is_empty()`) and did **not** check the endpoint slot.

**Fails-before-fix verification.** The pre-T-015 behaviour was: after `Err(SchedError::Deadlock)`, `IpcQueues::states[0] == RecvWaiting`. The new test asserts that a follow-up `ipc_recv` returns `RecvOutcome::Pending`. From [`ipc_recv` lines 365-381](../../../../../kernel/src/ipc/mod.rs#L365-L381), the second call's `match old { ... EndpointState::RecvWaiting => Err(IpcError::QueueFull) }` would fire → `.unwrap()` would panic with `QueueFull`. **The test is genuinely fails-before-fix** — the inline comment at lines [1419-1423](../../../../../kernel/src/sched/mod.rs#L1419-L1423) calls this out explicitly: *"Pre-fix behaviour: a second `ipc_recv` would observe `RecvWaiting` (set by Phase 1) and return `QueueFull`."*

**Could it pass without the implementation?** No. The scheduler-side Phase 2 Deadlock branch must call `ipc_cancel_recv` (lines [983-988](../../../../../kernel/src/sched/mod.rs#L983-L988)) **and** `ipc_cancel_recv` must reset the slot. Both code paths are required for the assertion to clear.

**Symmetric rollback empirically pinned.** ADR-0032 §Simulation row 3b is now backed by an executable test, satisfying the [B1 closure retro's *Adjustments*](../../business-reviews/2026-05-07-B1-closure.md) "Simulation row → host test" discipline.

### T-007 augmentation — `ipc_recv_and_yield_returns_deadlock_when_ready_queue_empty`

The original T-007 test grew **lines [1367-1377](../../../../../kernel/src/sched/mod.rs#L1367-L1377)** — an `ipc_recv(...)` call against the same `ep_cap` after the `Err(Deadlock)` return, asserting `RecvOutcome::Pending`.

**Assertion placement.** The new assertion is *after* the result `matches!(Err(Deadlock))` check and *after* the scheduler-state restoration assertions (`sched.current == prior_current`, `task_states[0] == prior_state`, `ready.is_empty()`). Placement is correct — it observes the kernel post-Deadlock-return state and pins the *endpoint* axis as a peer to the *scheduler* axis already pinned by T-007. Adding it inside the result-match instead would have meant short-circuiting before the existing assertions; appending it preserves both axes' regression-guard.

**Why two tests for one property?** The dedicated `ipc_recv_and_yield_deadlock_rolls_back_endpoint_state` test exists alongside the augmented T-007 because:
1. T-007's name pins the *scheduler* deadlock contract (Err typing + scheduler rollback). Adding endpoint-state to its name would muddy the regression intent.
2. The new test's name explicitly calls out *endpoint state rollback*, making the ADR-0032 regression guard discoverable from the name alone (per [`docs/standards/testing.md` §Test naming](../../../../standards/testing.md#L64-L77): *"specific enough that a reader can diagnose a failure from the name alone"*).

The two-test approach is **correct discipline**, not duplication. Praise §P-1 below.

## Miri (Stacked Borrows) — 158/158 clean

The CI `miri` job ([`.github/workflows/ci.yml` lines 107-130](../../../../../.github/workflows/ci.yml#L107-L130)) runs `cargo +nightly miri test --workspace --exclude tyrne-bsp-qemu-virt`, which exercises every kernel host test including the six new T-015 tests.

**What miri actually validates at this tip:**

- The IPC-side `cancel_recv` tests (#1-#5) all run through `&mut EndpointArena` + `&mut IpcQueues` + `&CapabilityTable` — pure Rust references, no raw-pointer split borrow on the IPC-side surface. Stacked Borrows here checks that `validate_ep_cap` does not invalidate any `&mut` held by the caller; given `validate_ep_cap` takes `&CapabilityTable` (immutable) and the state mutation flows through `queues.state_of(...)`'s `&mut self`, no aliasing hazard exists. Miri's clean pass on these five is straightforward but valuable as a regression guard.
- The new sched test `ipc_recv_and_yield_deadlock_rolls_back_endpoint_state` is the **load-bearing one for Stacked Borrows.** It runs through `ipc_recv_and_yield`'s Phase 2 Deadlock branch, which is now exactly the pattern the brief calls out:
  - **First momentary `&mut`**: scoped block `{ let s = unsafe { &mut *sched }; ...; }` (lines [939-969](../../../../../kernel/src/sched/mod.rs#L939-L969)). Drops at end of block — explicit `// `s: &mut Scheduler<C>` drops here.` comment at [line 969](../../../../../kernel/src/sched/mod.rs#L969).
  - **Second momentary `&mut`**: a *separate* `unsafe { let arena_ref = &mut *ep_arena; let queues_ref = &mut *queues; let table_ref = &*caller_table; ipc_cancel_recv(...) }` block (lines [983-988](../../../../../kernel/src/sched/mod.rs#L983-L988)). The two scopes do not overlap — the scheduler `&mut` has already been dropped at line 969 before the cancel block opens at line 983. This is a textbook Stacked-Borrows-correct split.
  - **Miri pass = correctness signal.** The 158/158 miri-clean status tells us the two `&mut` scopes are genuinely non-overlapping at runtime, the raw `*mut` parameters do not alias, and `caller_table` is reborrowed from `*mut` to `&` (immutable) for the cancel call without violating `*caller_table`'s shared/exclusive discipline.
- The augmented T-007 test exercises the same Phase 2 path but via the original test setup; same guarantee.

**No new audit-log entry.** UNSAFE-2026-0014's umbrella covers the new momentary-`&mut` site; the [2026-05-07 fourth Amendment](../../../../audits/unsafe-log.md) names the Phase 2 Deadlock-branch site explicitly. Miri's verdict on this surface is the *runtime check* corresponding to the static UNSAFE-2026-0014 claim — both must hold for the rollback discipline to be sound.

**Coverage of the new raw-pointer split-borrow site by miri: confirmed.** This closes the brief's question §5.

## QEMU smoke — discriminating power

**Smoke claim ("byte-for-byte unchanged").** Per [T-015 review-history row 2](../../../tasks/phase-b/T-015-endpoint-rollback-cancel-recv.md#L114): *"QEMU smoke verified — full demo trace through `tyrne: all tasks complete` + `boot-to-end elapsed = 4088000 ns` (~4.1 ms; matches the post-T-014 baseline's ~5.5–6.5 ms typical envelope, byte-for-byte identical message sequence)."* The post-T-014 baseline ([2026-05-07 perf re-baseline §Metric 3](../../performance-optimization-reviews/2026-05-07-B1-closure.md#L60-L67)) recorded ~5.8 ms (γ verification) and ~5.5–6.5 ms range.

**Coverage implication of "byte-for-byte unchanged".** v1's BSP installs the idle task via `register_idle` ([T-014 / ADR-0026](../../../../decisions/0026-idle-dispatch-fallback.md)). With idle present, Phase 2's `s.ready.dequeue().or(s.idle)` always finds *some* task to switch to — `SchedError::Deadlock` is structurally unreachable in the demo. The new Phase 2 Deadlock branch (the `ipc_cancel_recv` call site at [lines 971-995](../../../../../kernel/src/sched/mod.rs#L971-L995)) **is never executed in the v1 smoke run.** That is the precise reason the trace is byte-for-byte identical.

**Conclusion: the smoke offers zero discriminating power for T-015's behavioural change.** The cancel arc is verified entirely by host tests + miri. The smoke remains a regression guard for the *rest* of the demo (T-006 / T-007 / T-009 / T-012 / T-013 / T-014 stack), not for T-015.

**Is this adequate for v1?** Yes. The Deadlock branch is structurally unreachable; a smoke that exercised it would require building a "no-idle" boot mode purely to test a path that the v1 production configuration prevents. The host regression test (`ipc_recv_and_yield_deadlock_rolls_back_endpoint_state`) plus miri's Stacked-Borrows pass plus the IPC-unit-test cancel-state-machine pin together provide stronger guarantees than a smoke variant could. **Flagged as Minor §F-2 below for B5+ preemption work** — when preemption lands, the cancel arc *will* be reachable from real production paths (preempted-mid-Phase-2 case per ADR-0032 §Context), and a smoke variant or QEMU-driven preemption test will be the natural venue.

## Smoke variance band

**The 4.1 ms vs 5.8 ms gap.** [`current.md` line 7](../../../../roadmap/current.md#L7) states *"~4–6.5 ms typical on QEMU-default Cortex-A72"*. The post-T-015 single-run is 4.088 ms; the post-T-014 perf baseline is 5.8 ms (γ verification single run); the post-T-014 envelope is 5.5–6.5 ms (closure-trio confirmation). **The 4.1 ms post-T-015 run is ~30 % below the post-T-014 mid-band.**

**Variance source.** The [perf re-baseline §Methodology notes](../../performance-optimization-reviews/2026-05-07-B1-closure.md#L73-L75) explicitly records: *"The QEMU host-clock gives a ~10–15 % variance on boot-to-end timing across cold-vs-warm host caches."* A 30 % under-shoot exceeds that documented band. Plausible mechanisms: (a) host-cache warmth (the closure-trio runs were sequential cold-start, whereas T-015's smoke run may have followed a recent build); (b) host-CPU dynamic frequency scaling; (c) different macOS state.

**Regression-detection threshold.** With the band stated as "~4–6.5 ms typical," a real 30 % regression upward (to ~7.5 ms) would *not* be caught by the band's upper edge — it would land at the lip and be plausibly attributed to host variance. **The variance band is too wide to catch sub-30 %-shaped regressions.** This intersects Track D's open P10 proposal (IPC round-trip benchmark harness with iteration + variance, gated by a `bench` feature flag). Per [perf re-baseline §Hotspot](../../performance-optimization-reviews/2026-05-07-B1-closure.md#L78-L90), P10 remains queued.

**Severity for this Track:** Minor §F-3. Not a Track F problem to solve (perf P10 is the venue), but the wide band affects what smoke can catch as regression — a coverage concern.

## CI gate posture

| Gate | Status at tip | Catches T-015 regression? |
|------|--------------|---------------------------|
| `cargo host-test` | 158/158 green | **Yes** — six new tests (5 IPC + 1 sched) + augmented T-007 directly fail on regression. |
| `cargo +nightly miri test` | 158/158 clean | **Yes** — Stacked Borrows on the new Phase 2 Deadlock branch's two-momentary-`&mut` pattern is the load-bearing aliasing check (UNSAFE-2026-0014 fourth Amendment site). |
| `cargo host-clippy` | clean (per T-015 row 1) | Indirectly — would fire on `cancel_recv` lint drift. |
| `cargo kernel-clippy` | clean | Same. |
| `cargo kernel-build` | clean | Build-side compatibility check; no T-015-specific behaviour. |
| QEMU smoke | maintainer-launched, not CI | **No** — Deadlock branch is structurally unreachable in the v1 demo. T-015's behaviour is **not** smoke-detectable by design. The "byte-for-byte unchanged" claim is correct but tells us nothing about T-015. |

The QEMU-smoke-not-CI-wired finding from [2026-05-06 §F-1](../2026-05-06-full-tree/track-f-tests.md#L162) **remains open** — neither PR #15 nor PR #17 added a `qemu-smoke` job. Carried forward as §F-4 below for visibility; not re-opening as a new finding.

## Findings

### Blocker

None.

### Major

None.

### Minor

**§F-1 — `cancel_recv` `RecvComplete` branch not directly tested.** The [`ipc_cancel_recv` body lines 477-479](../../../../../kernel/src/ipc/mod.rs#L477-L479) treats `RecvComplete { .. }` as a no-op via the `if matches!(state, RecvWaiting)` guard — code-share with the `SendPending` branch tested by [`cancel_recv_on_send_pending_does_not_drop_message`](../../../../../kernel/src/ipc/mod.rs#L1195-L1232). The brief lists `RecvComplete` as a checklist branch; the doc-comment at [lines 437-444](../../../../../kernel/src/ipc/mod.rs#L437-L444) names both `SendPending` and `RecvComplete` together. **Recommend:** add a one-screen `cancel_recv_on_recv_complete_is_noop` test that drives the endpoint to `RecvComplete` (sender-then-receiver-arrived) before calling cancel, asserts `Ok(())`, and verifies the parked message is still deliverable on the next `ipc_recv`. Cost: ~25 LOC.

**§F-2 — No smoke variant exercises the Deadlock-branch cancel arc.** v1's idle task makes `SchedError::Deadlock` structurally unreachable; the new cancel call site at [lines 971-995](../../../../../kernel/src/sched/mod.rs#L971-L995) is exercised only by host tests + miri. Adequate for v1 (per §"QEMU smoke" above), but worth queueing for B5+ preemption work — when preemption lands, the cancel arc will be reachable from real production paths (preempted-mid-Phase-2 per ADR-0032 §Context bullet 3), and a smoke variant or preemption-test will be the natural venue. **Recommend:** open a phase-B5 follow-up note in [phases/phase-b.md](../../../../roadmap/phases/phase-b.md) tagging "Deadlock-cancel arc gains a real exerciser when preemption lands."

**§F-3 — Smoke-variance band is wider than the 30 % T-015 single-run delta.** Post-T-015 single run is 4.088 ms; post-T-014 mid-band is 5.8 ms (~30 % delta). [`current.md`](../../../../roadmap/current.md#L7)'s "~4–6.5 ms typical" envelope absorbs the delta, so it is not a regression call-out — but the same wide band would absorb a real 30 % upward regression if it ever happened. **Recommend:** Track D's P10 (IPC round-trip benchmark harness with iteration + variance) — already queued in [Track D's 2026-05-06 review](../2026-05-06-full-tree/track-d-performance.md). Not a Track F deliverable; recorded for cross-track visibility.

### Observation

**§M-1 — Stale-generation cap path on `cancel_recv` not directly tested.** `validate_ep_cap` returns `IpcError::InvalidCapability` for a stale lookup; `ep_arena.get(slot)` rejects a stale-generation `EndpointHandle`. Both paths are exercised by other IPC tests' setup (e.g. `recv_with_invalid_handle_returns_invalid_capability`) but not by a dedicated cancel-stale test. The branches are code-shared with the validation paths already covered, so this is not a real gap — recorded for matrix completeness.

**§M-2 — Test #2 (`cancel_recv_on_idle_is_noop`) and #5 second-cancel arm of (`cancel_recv_is_idempotent`) would pass against an empty `Ok(())`-only `ipc_cancel_recv` body.** They document the contract but do not catch a regression *by themselves*. Test #1 (`clears_recv_waiting_back_to_idle`) and test #3 (`on_send_pending_does_not_drop_message`) are the load-bearing fails-before-fix tests in the IPC-unit suite; #2 and #5 are documentation-grade. Reasonable trade — the contract is small and the documentation has clear value — but the regression-detection power of the unit suite is concentrated in 2 of the 5 tests.

**§M-3 — `register_idle_stores_handle_in_idle_slot_and_not_in_ready_queue` (PR #12) tests a structural property; the smoke regression that prompted T-014 was *behavioural* (idle dispatched ahead of ready task).** The companion test `dispatcher_picks_idle_only_when_ready_queue_empty` does pin the behavioural shape directly. The structural test is a complementary regression guard but on its own would not have caught the original smoke regression. Both tests together are correct discipline; recorded for transparency on what each pins.

**§M-4 — Carried-forward findings from 2026-05-06 Track F.** §F-1 (QEMU smoke not CI-wired) and §F-2 (`ObjError::StillReachable` no producer) **remain open** at this tip. PR #15 closed §F-3 (three obj-test modules' clippy-allow drift). Recommend the maintainer's next phase-boundary review (B2 closure trio) reconciles the carry-forward state explicitly.

### Praise

**§P-1 — Two-test discipline for the ADR-0032 contract.** The augmented T-007 (`returns_deadlock_when_ready_queue_empty` — pins scheduler axis) plus the new `ipc_recv_and_yield_deadlock_rolls_back_endpoint_state` (pins endpoint axis) is a textbook example of the testing.md §Test naming principle: each test's name diagnoses its failure mode unambiguously, and neither test is duplicative — they pin different axes of the same ADR's symmetric-rollback claim.

**§P-2 — IPC unit tests verify behavioural shape, not state-equality peeks.** All five `cancel_recv_*` tests use a follow-up `ipc_recv` call to *observe* the post-cancel state via the public IPC surface, rather than reaching into `IpcQueues::states[i]` directly. This honours the testing.md anti-pattern *"Tests that take the exact code under test and restate it — test the *behavior*, not the implementation."* The implementation could refactor `EndpointState` internally without breaking these tests.

**§P-3 — `should_panic` discipline maintained.** Track-F's 2026-05-06 audit showed every `#[should_panic]` annotation in the workspace carries `expected = "..."`; PR #17 added zero `should_panic` annotations and preserves the property. PRs #12–#17 introduced no naked `#[should_panic]`.

**§P-4 — Test-helper hygiene preserved.** [`setup_single_task_with_recv_cap`](../../../../../kernel/src/sched/mod.rs#L1292-L1315) is a new helper introduced by T-015 (or by PR #12 — the lineage isn't explicit, but it's `#[cfg(test)]`-gated and shared by both T-007 and the new ADR-0032 regression test). It is `pub(crate)`-equivalent (defined inside `mod tests`) and never reaches the release build. Zero leak.

## Cross-track notes

- **→ Track A (kernel correctness):** the scheduler-side ADR-0032 regression test directly mirrors the [2026-05-06 Track A non-blocker](../2026-05-06-full-tree/track-a-kernel.md) about endpoint-state asymmetry. Track A's deferred non-blocker is closed by Track F's evidence.
- **→ Track B (HAL):** no HAL test changes in PRs #12–#17. No coordination needed.
- **→ Track C (security):** §M-2 (test rights enforcement) intersects security — `cancel_recv_without_recv_right_fails` is the load-bearing rights-enforcement test for `cancel_recv`, sharing the `validate_ep_cap` discipline of `ipc_recv` / `ipc_send`. Track C should weigh whether the `RECV` right is the *correct* right for cancel (vs e.g. a hypothetical `CANCEL` right) — Track F notes the test pins what ADR-0032 specifies.
- **→ Track D (perf):** §F-3 (smoke-variance band wider than T-015 single-run delta) is the test-side framing of Track D's P10 proposal; Track D owns the resolution.
- **→ Track E (docs):** ADR-0032 §Simulation row 3b is empirically backed by the new sched test. The "Simulation row → host test" discipline is now visibly applied; Track E may want to record this as a reference example for future ADRs (the [`write-adr` skill §Simulation discipline](../../../../../.claude/skills/write-adr/SKILL.md) already codifies the discipline).
- **→ Track G (BSP):** no BSP test changes in PRs #12–#17 (BSP is `no_std` / `no_main`; never built under host-test target). PR #15 polished BSP comments only. No coordination needed.
- **→ Track H (infrastructure):** §F-4 carry-forward (QEMU smoke not CI-wired) is Track H's perennial gap. Same recommendation as 2026-05-06: open a B2-prep task to add a `qemu-smoke` CI job.
- **→ Track I (integration):** the post-T-015 smoke trace's "byte-for-byte unchanged" property is the integration-axis evidence that T-015 is non-disruptive in v1. Track I's verdict on this should mirror Track F's: **byte-for-byte unchanged is correct AND tells us nothing about the cancel arc.**
- **→ Track J (hygiene):** PR #15 closed Track-F-2026-05-06 §F-3 (clippy-allow consistency) — Track J should record this as a closed hygiene drift.

## Sub-verdict

**Approve.**

PR #17's six new tests + augmented T-007 + miri-158/158-clean is **correct discipline** for the ADR-0032 contract. The fails-before-fix property is genuinely held by tests #1, #3, the augmented T-007 endpoint-state assertion, and the new `ipc_recv_and_yield_deadlock_rolls_back_endpoint_state` regression test; the other two IPC tests document the no-op contract. The miri pass on the new two-momentary-`&mut` Phase 2 Deadlock branch validates the Stacked-Borrows-correct split borrow that UNSAFE-2026-0014's fourth Amendment statically claims. The "byte-for-byte unchanged" smoke is structurally honest — Deadlock is unreachable in v1 — and the host tests + miri together fully replace what a smoke variant could provide today.

PRs #13 and #14 were doc-only and PR #16 was reviews-only; verified zero source-code/test impact. PR #15 (γ polish) **closed 2026-05-06 §F-3** (lint-allow drift in three obj test modules) and added zero test removals — a clean roll-up. PR #12 (T-014) added 3 host tests covering the structural and behavioural shape of ADR-0026's separate-fallback-slot dispatch.

Three Minor findings (§F-1 missing `RecvComplete` direct test, §F-2 no Deadlock-cancel-arc smoke variant for B5+ preemption, §F-3 smoke variance band wider than T-015 single-run delta) and four Observations are queued for the next pass; none block the work just landed. Two carry-forwards from 2026-05-06 Track F (§F-1 / §F-2 in that report) remain open and reconcile naturally at B2 closure.

**Track F: Approve.**

---

## Summary (3 lines)

- 6 new tests landed by PR #17 (5 IPC `cancel_recv` unit tests + 1 sched regression for the ADR-0032 symmetric Deadlock rollback) plus an endpoint-state assertion appended to existing T-007 — fails-before-fix verified for tests #1, #3, the augmented T-007, and the new sched test; tests #2 and #5 document the no-op/idempotency contract without catching regressions on their own.
- Miri 158/158 clean validates the new two-momentary-`&mut` split-borrow at the Phase 2 Deadlock cancel call site (UNSAFE-2026-0014's fourth Amendment territory); the "byte-for-byte unchanged" smoke is structurally honest because v1's idle task makes `SchedError::Deadlock` unreachable — the cancel arc is exercised only by host tests + miri, which is adequate for v1 but flagged for B5+ preemption work (§F-2).
- Three Minor findings (missing `RecvComplete` direct test §F-1; no Deadlock-cancel smoke variant §F-2; smoke variance band wider than 30 % single-run delta §F-3) plus four Observations; no test removed in PRs #12–#17 (PR #15 closed 2026-05-06 §F-3 cleanly). **Track F: Approve.**
