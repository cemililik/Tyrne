# Code review 2026-05-07 — PR #12 to PR #17 multi-axis post-merge sweep

- **Change:** PRs [#12](https://github.com/cemililik/Tyrne/pull/12), [#13](https://github.com/cemililik/Tyrne/pull/13), [#14](https://github.com/cemililik/Tyrne/pull/14), [#15](https://github.com/cemililik/Tyrne/pull/15), [#16](https://github.com/cemililik/Tyrne/pull/16), [#17](https://github.com/cemililik/Tyrne/pull/17) (the 14-day window 2026-04-23 → 2026-05-07 closing on `main`).
- **Merge SHA range:** `298b5d2a..8dc433ee` on `main`; HEAD at review time `c258ee3` (doc-side rollup commit on `t-015-endpoint-rollback-cancel-recv`).
- **Reviewer:** @cemililik (+ Claude Opus 4.7 multi-agent fan-out — 8 parallel axis agents).
- **Type:** Post-merge multi-axis sweep — *not* a merge gate. Findings flow into a follow-up PR; "Block" reserved for genuine regression/audit-discipline violation.
- **Risk class:** Security-sensitive (PR #17 touches IPC + scheduler invariants; cross-references the [2026-05-07 B1 closure security review](../security-reviews/2026-05-07-B1-closure.md), whose single forward-flag this PR closes).

> **2026-05-08 closure status:** all 9 hygiene items from §Follow-up backlog closed in [PR #18](https://github.com/cemililik/Tyrne/pull/18) (merge `aa7e6c5`). Forward-flagged item 11 (P10 wall-clock harness) closed by the [2026-05-08 B2-prep integration PR](https://github.com/cemililik/Tyrne/pull/22) (replaces the originally-opened #19 / #20 / #21). Items 10 / 12 / 13 remain forward-flagged on their downstream venues (ADR-0030 / ADR-0019; first userspace-destroy task; B5+ preemption ADR). See §Follow-up backlog at the bottom of this file for per-item closure annotations.

## Scope

Eight axes, each scanning all six PRs. Per-axis files under [`2026-05-07-pr-12-to-17-multi-axis-review/`](2026-05-07-pr-12-to-17-multi-axis-review/):

| Track | Axis | File | Verdict |
|-------|------|------|---------|
| A | Kernel correctness | [track-a-kernel.md](2026-05-07-pr-12-to-17-multi-axis-review/track-a-kernel.md) | Approve, 2 follow-ups |
| B | HAL + BSP | [track-b-hal-bsp.md](2026-05-07-pr-12-to-17-multi-axis-review/track-b-hal-bsp.md) | Approve |
| C | Security & capability discipline | [track-c-security.md](2026-05-07-pr-12-to-17-multi-axis-review/track-c-security.md) | Approve |
| D | Performance / footprint | [track-d-perf.md](2026-05-07-pr-12-to-17-multi-axis-review/track-d-perf.md) | Approve |
| E | Documentation drift | [track-e-docs.md](2026-05-07-pr-12-to-17-multi-axis-review/track-e-docs.md) | Approve |
| F | Test coverage & quality | [track-f-tests.md](2026-05-07-pr-12-to-17-multi-axis-review/track-f-tests.md) | Approve |
| G | Process & governance | [track-g-process.md](2026-05-07-pr-12-to-17-multi-axis-review/track-g-process.md) | Approve, 3 follow-ups |
| H | Audit-log discipline | [track-h-audit.md](2026-05-07-pr-12-to-17-multi-axis-review/track-h-audit.md) | Approve, 2 follow-ups |

## Verdict

**Approve, with seven Minor follow-ups for a single hygiene PR before ADR-0027 drafting.** Zero Blockers; zero Majors that block v1; one Major *forward-flagged* for the syscall-ABI ADR (ADR-0030) and the multi-waiter ADR (ADR-0019 §Open). The 14-day arc is the cleanest stretch of work the project has shipped: the smoke regression caught on 2026-05-06 was closed correctly (T-014 + ADR-0026), the doc/code/process polish landed in tight α/β/γ/δ pulses without re-introducing drift, and T-015 (PR #17) closed the 2026-05-07 B1-closure security review's single forward-flag exactly per the design promise (additive recovery primitive, zero new audit-log entry, byte-for-byte unchanged smoke trace).

## Headline numbers

| Severity | A kernel | B hal/bsp | C security | D perf | E docs | F tests | G process | H audit | **Total** |
|----------|---------:|----------:|-----------:|-------:|-------:|--------:|----------:|--------:|----------:|
| Blocker  | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | **0** |
| Major (forward-flagged, not v1) | 0 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | **1** |
| Minor    | 2 | 1 | 1 | 2 | 0 | 2 | 3 | 2 | **13** |
| Nit      | 1 | 1 | 1 | 1 | 0 | 0 | 4 | 2 | **10** |
| Praise   | 5 | — | — | 2 | — | 1+ | 6 | 1+ | **15+** |

The 0 / 1-forward / 13-Minor distribution is consistent with the prior 2026-05-06 comprehensive review's outcome (verdict *Request changes*, 7 doc-blockers, all closed by α/β/γ): polish landed where polish was asked for, and no new class of regression slipped in.

## Per-PR summary

| PR | Headline change | Highest-severity finding from this review |
|----|-----------------|-------------------------------------------|
| **#12** [`298b5d2a`](https://github.com/cemililik/Tyrne/pull/12) | T-014 idle-dispatch hotfix; ADR-0026 supersedes ADR-0022's *idle-task-location* axis (Option A → separate `Scheduler::idle: Option<TaskHandle>` slot, dispatched via `ready.dequeue().or(s.idle)`); UNSAFE-2026-0014 3rd Amendment names `register_idle`. | **Minor (G):** ADR-0026 Propose + Accept landed in a single commit (`10dea48`), in tension with `write-adr` §10 "separate commit" but technically permitted by `supersede-adr` §7 "solo-phase combined commit" — needs a clarifying rider reconciling the two skill clauses. |
| **#13** [`cfc49249`](https://github.com/cemililik/Tyrne/pull/13) | α doc-fix sweep — closes the 7 Track-E blockers from the 2026-05-06 comprehensive review (GIC v3→v2 in 3 sites, idle-body documentation per ADR-0026, security-model open-question, glossary dead-link). | None new; Track E confirmed all 7 blockers closed and Mermaid diagrams stay valid. |
| **#14** [`1cd810d9`](https://github.com/cemililik/Tyrne/pull/14) | Repo-wide `TyrneOS → Tyrne` URL rename (64 URLs) + `Yüksek → High` localization sweep (5 docs); review-round caught `cd TyrneOS` orphan + `TyrneOS repository` typo in SECURITY.md (both fixed in PR). | **Minor (H):** UNSAFE-2026-0016's body was edited in-place by the localization sweep, technically violating the introducing-commit-boundary discipline — fix is a small Amendment or a `unsafe-policy.md §3` exemption for mechanical localization. (Track B also notes the brief mis-attributed the `tools/run-qemu.sh` Bash 3.2 fix to PR #14 — actual introducing commit is `0f0c97c` on PR #12's branch.) |
| **#15** [`e9fa019a`](https://github.com/cemililik/Tyrne/pull/15) | γ code-side polish — Track A/B/F kernel + HAL + test, Track G/I BSP + integration; `register_idle` `debug_assert!` → `assert!`; γ.6 reverted "defensive loop after start()" (clippy::unreachable_code + too_many_lines); 4 line-ref drops + 1 metadata trim in review-round. | **Praise (B):** clean disposition of Track-G #4 by documented-rejection (`-> !` is the type-system belt-and-braces), plus `Aarch64TaskContext == 168` const-assert mirroring `TrapFrame == 192` extends the existing discipline. |
| **#16** [`95b15aa1`](https://github.com/cemililik/Tyrne/pull/16) | B1 closure trio (business + consolidated security + performance baseline 2026-05-07, replacing the 2026-04-28 trio's load-bearing role); δ items: ADR-0023 Deferred placeholder, ADR-0032 Propose, T-015 Draft, `write-adr` skill §Simulation codification, master-plan AC change "no closure-trio without recorded smoke." | **Minor (G):** the §Simulation rule was retro-extracted, not pre-existing — commit `77a578a` codifies it 84 seconds before commit `4aa4b24` proposes ADR-0032 with a Simulation table. Chronology is honest in commit bodies but artefacts read as if rule pre-existed; one-line rider in ADR-0026 §Revision notes naming the codifying commit closes it. Master-plan AC also landed only in `business-reviews/master-plan.md`; security and performance master-plans not cross-referenced. |
| **#17** [`8dc433ee`](https://github.com/cemililik/Tyrne/pull/17) | T-015 implementation — ADR-0032 Accept (separate commit per `write-adr` §10), new `ipc_cancel_recv` primitive, `ipc_recv_and_yield` Phase 2 Deadlock branch upgraded to symmetric scheduler + endpoint rollback; 6 new tests (5 IPC + 1 sched); UNSAFE-2026-0014 4th Amendment names the new Deadlock-branch momentary `&mut EndpointArena` + `&mut IpcQueues` site; ADR-0017 §Revision rider records additive recovery primitive (user-observable surface unchanged). | **Major forward-flag (C):** `RecvWaiting` unit-variant identity gap — kernel records no waiter identity; v1's depth-1 cooperative discipline makes it unobservable, but ADR-0030 (syscall ABI) and ADR-0019 (multi-waiter) must address it. ADR-0032 forecasts the `caller: TaskHandle` signature change correctly at [kernel/src/ipc/mod.rs:447-453](../../../../kernel/src/ipc/mod.rs#L447). **Tracked, no v1 action.** |

## Per-track summary

### Track A — Kernel correctness

Approve with 2 minor follow-ups + 1 nit. Zero blockers, zero majors. Five Praise items: `register_idle`'s release-`assert!` upgrade, the cleanly-disciplined two-stage Deadlock-rollback borrow split, the conservative `debug_assert!(cancel_result.is_ok())` guard, the exemplarily small `ipc_cancel_recv` body matching ADR-0032's "zero new audit-log entry" promise byte-for-byte, and the `SchedQueue::new` const-block migration. The 2026-05-06 baseline's deferred Deadlock-asymmetry non-blocker is **closed structurally** by T-015. Top finding: `ipc_cancel_recv` takes `&mut EndpointArena` but only calls immutable `Arena::get` — could be `&EndpointArena` (Minor; sweep when ADR-0030 finalises syscall ABI). See [track-a-kernel.md](2026-05-07-pr-12-to-17-multi-axis-review/track-a-kernel.md).

### Track B — HAL + BSP

Approve. Only PRs #12 and #15 carry HAL/BSP code; PR #14 is doc-only on this axis; PR #16/#17 don't touch HAL/BSP at all. PR #15 closes Track-B/G non-blockers from 2026-05-06 (`ContextSwitch: Send + Sync`, `Aarch64TaskContext == 168` size-assert mirroring `TrapFrame`'s discipline, three prose-only justifications). PR #12's idle-dispatch rewire substitutes `register_idle` for `add_task` without touching boot.s, vectors.s, GIC, or timer code. UNSAFE-2026-0019 / 0020 / 0021 partial-verification Amendments still match reality. Notable correction: brief mis-attributed the `tools/run-qemu.sh` Bash 3.2 fix to PR #14 — actual introducing commit `0f0c97c` lands on PR #12's branch. See [track-b-hal-bsp.md](2026-05-07-pr-12-to-17-multi-axis-review/track-b-hal-bsp.md).

### Track C — Security & capability discipline

Approve. PR #17 closes the 2026-05-07 B1-closure security review's single forward-flag ("`ipc_recv_and_yield` Deadlock endpoint-rollback asymmetry") in the way the review expected: ADR-0032 authored per ADR-0025 §Rule 1, primitive-based per Option A, lands ahead of B2 userspace-destroy work. No security regressions in PRs #12-#16. Top finding: **`RecvWaiting` unit-variant identity gap (Major forward-flag, tracked)** — `EndpointState::RecvWaiting` records no waiter identity; v1 depth-1 cooperative discipline makes it unobservable, but ADR-0030 / ADR-0019 must reckon with it. PR #14's SECURITY.md cleanup (TyrneOS rename + `cd TyrneOS` orphan + `TyrneOS repository` typo) all confirmed merged at HEAD `c258ee3`. PR #15's `debug_assert! → assert!` upgrade on `register_idle` is net-positive. See [track-c-security.md](2026-05-07-pr-12-to-17-multi-axis-review/track-c-security.md).

### Track D — Performance / footprint

Approve. T-015 contributes **+228 bytes `.text`, +0 `.rodata`, +0 `.bss`** — measured live by rebuilding at both `95b15aa` (post-PR #16) and `c258ee3` (post-PR #17): +12 bytes on `ipc_recv_and_yield<QemuVirtCpu>` (cold Deadlock arm) + 216 bytes for the new non-generic `ipc_cancel_recv`. PRs #13/#14 zero perf relevance. PR #15's `assert!` upgrade and γ.6 revert absorbed by the 2026-05-07 re-baseline. **Top finding: doc drift** — `current.md` and the 2026-05-07 perf re-baseline still quote `.text 21,792`; post-T-015 it is **22,020**. Needs a +228-byte amendment block on the re-baseline. PR #17's "boot-to-end ~4.1 ms" is a single-run anecdote ~25 % below the re-baseline's lowest run; almost certainly host-cache variance, but promotes the queued P10 wall-clock harness. See [track-d-perf.md](2026-05-07-pr-12-to-17-multi-axis-review/track-d-perf.md).

### Track E — Documentation drift

Approve. Zero blockers. All 7 blockers from the 2026-05-06 comprehensive review verifiably closed by PR #13: GIC version corrected v3→v2 in three sites, Timer subsection updated from `unimplemented!()`, idle-body documented as `wait_for_interrupt()` per ADR-0026 (×2 sites in scheduler.md), security-model DAIF-masking open-question closed, glossary ADR-0023 dead-link resolved by PR #16's placeholder. ADR-0032 (Accepted in PR #17) carries a proper 5-row Phase-2-Deadlock simulation table per the new `write-adr` §Simulation rule; ADR-0017 §Revision rider records the additive primitive without superseding the original three-primitive set. PR #14's localisation sweep replaces 7 instances of "Yüksek" with "High" across 5 documents; English-only rule (CLAUDE.md #3) verified. See [track-e-docs.md](2026-05-07-pr-12-to-17-multi-axis-review/track-e-docs.md).

### Track F — Test coverage & quality

Approve. 152 → 158 host tests + miri 158/158 clean post-T-015. Two-test discipline for ADR-0032 is textbook: the augmented T-007 test pins the scheduler axis, the brand-new `ipc_recv_and_yield_deadlock_rolls_back_endpoint_state` pins the endpoint axis (fails-before-fix verified by reading `ipc_recv`'s state machine: pre-T-015 the second `ipc_recv` would hit `RecvWaiting → Err(QueueFull)` and unwrap-panic). Top finding: **`cancel_recv` `RecvComplete` branch not directly tested** (Minor) — the 5 IPC tests cover `RecvWaiting` (success), `Idle` and `SendPending` (no-op), missing-RECV-right, idempotency, but not `RecvComplete`; ~25 LOC test closes it. **"Byte-for-byte smoke unchanged" structurally honest but tells nothing about T-015** — v1's `register_idle` makes `SchedError::Deadlock` unreachable, so the new cancel call site is exercised only by host tests + miri (adequate for v1; flag for B5+ preemption). Smoke variance band is wide enough (~30 % delta absorbed by "~4–6.5 ms typical") that a real upward regression of similar magnitude would also be absorbed. Zero test removals across all 6 PRs. See [track-f-tests.md](2026-05-07-pr-12-to-17-multi-axis-review/track-f-tests.md).

### Track G — Process & governance

Approve with 3 minor follow-ups + 4 nits. Zero blockers, zero majors. Six Praise items, including: PR #17's `db24d6d` is the **first project-side application of `write-adr` §10's separate-Accept-commit discipline** — clean +2/-2 status flip with a careful-re-read checklist body. PR #16 is the cleanest process-density PR the project has shipped (6 commits, each independently bisectable, all with grounded forward-references and clean trailers). Top findings: (1) **ADR-0026 Propose+Accept landed in a single commit** in PR #12 — tension between `write-adr` §10 ("separate commit") and `supersede-adr` §7 ("solo-phase combined commit"); (2) **§Simulation retro-extracted in PR #16** — codified 84 seconds before ADR-0032 Propose; needs a one-line rider in ADR-0026 §Revision notes naming the codifying commit; (3) **master-plan AC scope** — smoke-trace AC landed only in `business-reviews/master-plan.md`; security and performance master plans not cross-referenced. See [track-g-process.md](2026-05-07-pr-12-to-17-multi-axis-review/track-g-process.md).

### Track H — Audit-log discipline

Approve with 2 minor follow-ups. Zero blockers, zero majors. **All 21 entries (20 Active, 0012 Removed) align with kernel/HAL/BSP `unsafe` sites at HEAD `8dc433e`** — no orphan blocks, no orphan entries. PR #15 audit-log diff is empty, confirming current.md's "no new audit entries" claim. Top findings: (1) UNSAFE-2026-0014's 3rd / 4th Amendments reference the introducing commit by PR number rather than SHA — established back-fill precedent exists, so add SHAs `c30f4ee` (T-014) and `7a402cb` (T-015); (2) PR #14's localization sweep edited UNSAFE-2026-0016's body in-place — technically violates introducing-commit-boundary discipline; either append a localization-sweep Amendment or amend `unsafe-policy.md §3` to exempt mechanical localization. The three `Pending QEMU smoke verification` statuses (UNSAFE-2026-0019 / 0020 / 0021) correctly persist post-T-015 — PR #17 doesn't arm any deadline either, so the IRQ-dispatch verification gap is neither closed nor widened. See [track-h-audit.md](2026-05-07-pr-12-to-17-multi-axis-review/track-h-audit.md).

## Cross-cutting findings

Patterns repeating across two or more tracks — promote-to-process candidates:

1. **Cancel-on-cap-bearing-state semantics has three readers (Tracks A, C, F).** Track A wants doc clarification on the `SendPending { cap: Some(_) }` / `RecvComplete { cap: Some(_) }` no-op path; Track C wants destroy-drain semantics distinct from v1 no-op (forward-flag for B2+); Track F wants a regression test pinning the no-op (`cancel_recv_on_send_pending_with_cap_does_not_drop_cap`). One follow-up touching three artefacts: ADR-0032 §Open questions rider + ipc/mod.rs doc-comment + one new host test.

2. **Smoke variance band is too wide for true regression detection (Tracks D + F).** "~4–6.5 ms typical" envelope absorbs a ~30 % shift in either direction. PR #17's 4.1 ms vs post-T-014 mid-band 5.8 ms is unflagged because of it. **Promote to process:** the queued P10 wall-clock harness (k-of-N runs + tightened percentile band) is now load-bearing; should land before B2 ADR-0027 implementation rather than as a generic backlog item.

3. **Audit-log Amendment commit-references inconsistent (Track H + soft signal from Track G).** Some Amendments cite SHA, some cite PR number. Track H wants a back-fill pass and a one-line discipline note in `unsafe-policy.md §3` — Track G's read is the same (consistency would close a small process drift). Single-PR fix.

4. **`unsafe-policy.md §3` introducing-commit-boundary needs a localization-exemption (Track H + Track E).** PR #14's mechanical sweep edited UNSAFE-2026-0016's body in-place; this is *not* a discipline failure of the sweep but an unstated exemption in the policy. Either codify "mechanical localization edits are exempt from the boundary" or require an Amendment record per sweep.

5. **§Simulation discipline's chronology rider (Track G + soft signal from Track E).** §Simulation rule was retro-extracted in PR #16 mere seconds before its first application in ADR-0032; the artefacts read as if it pre-existed. Track E sees the rule applied cleanly to ADR-0032; Track G wants an honest rider naming the codifying commit. Single one-line edit in ADR-0026 §Revision notes.

6. **The brief's PR-numbering vs gh ground truth.** The user's prose described the PRs as α/β/γ/δ pulses but the gh-ground-truth numbering shifts by one (PR #12 is the smoke-regression hotfix, not the α doc-fix). All 8 agents trusted gh and discovered actual content from `gh pr view`. **No artefact misrouting** (e.g., "PR #14 fixed this" claims) survives in the track files. Worth recording in the next AGENTS.md / fan-out brief: trust gh as ground truth, not the prose description.

## Follow-up backlog

Severity-sorted, action-cumulative. **No Blocker**, **no v1 Major**. The Major C-1 is forward-flagged for ADR-0030 / ADR-0019 work and is *not* a B2-prep follow-up.

> **Status (2026-05-08): all 9 hygiene items closed in [PR #18](https://github.com/cemililik/Tyrne/pull/18) (merge `aa7e6c5`); item 11 (P10 wall-clock harness) closed in this branch's integration PR (replaces #19/#20/#21). Items 10 / 12 / 13 remain forward-flagged on the appropriate downstream venues. See per-item closure notes below.**

### Hygiene PR before ADR-0027 drafting (closed by PR #18)

1. ✅ **Update `current.md` + 2026-05-07 perf re-baseline** with `.text 22,020` (was 21,792); add a +228-byte amendment block citing PR #17. *(Track D Minor)* — closed by PR #18 commit `94a6c0f`; verified at HEAD via `grep "22,020 bytes" docs/roadmap/current.md`.
2. ✅ **Add `RecvComplete` no-op test** in `kernel/src/ipc/mod.rs` (`cancel_recv_on_recv_complete_does_not_drop_message_or_cap`, ~30 LOC — implementation went slightly stronger than the originally-recommended ~25 LOC by also pinning the cap-bearing-state property). *(Track F Minor)* — closed by PR #18 commit `25854a1`; verified at HEAD via `host-test 159/159`.
3. ✅ **Doc-rider on `ipc_cancel_recv`** clarifying `SendPending`/`RecvComplete` no-op semantics + cap-drain expectations for the future B2+ destroy caller. *(Track A Minor + Track C Minor consolidated)* — closed by PR #18 commit `25854a1`.
4. ✅ **Tighten cancel-block `// SAFETY:` comment** in `sched/mod.rs` (cancel block said `caller_table` was "exclusive" but actual borrow is `&CapabilityTable`). *(Track A Nit)* — closed by PR #18 commit `25854a1`.
5. ✅ **Back-fill SHAs in UNSAFE-2026-0014 Amendments 3 + 4** (`c30f4ee` for T-014, `7a402cb` for T-015). *(Track H Minor)* — closed by PR #18 commit `94a6c0f`.
6. ✅ **One-line rider in `unsafe-policy.md §3`** exempting mechanical localization edits from the introducing-commit-boundary. *(Track H Minor)* — closed by PR #18 commit `94a6c0f` (chose the standard-side fix over a per-entry Amendment).
7. ✅ **One-line rider in ADR-0026 §Revision notes** naming the codifying commit of `write-adr` §Simulation (the rule's chronology vis-à-vis ADR-0032 Propose). *(Track G Minor)* — closed by PR #18 commit `94a6c0f`.
8. ✅ **Cross-reference master-plan AC** ("no closure-trio without recorded smoke") into `security-reviews/master-plan.md` and `performance-optimization-reviews/master-plan.md`. *(Track G Minor)* — closed by PR #18 commit `94a6c0f`.
9. ✅ **Skill-clause reconciliation rider** between `write-adr` §10 ("separate Accept commit") and `supersede-adr` §7 ("solo-phase combined commit"), recording which rule wins for ADR-0026's situation in PR #12. *(Track G Minor)* — closed by PR #18 commit `94a6c0f` (rider lives in ADR-0026 §Revision notes alongside item 7's chronology rider).

### Forward-flagged (do *not* pull into this hygiene PR)

10. **`RecvWaiting` waiter-identity gap** — open in ADR-0019 §Open and ADR-0030 (placeholder) draft; v1 unobservable but must be addressed before any multi-waiter or syscall-ABI commit. *(Track C Major, tracked, no v1 action)* — **status unchanged 2026-05-08**: still forward-flagged for ADR-0030 / ADR-0019.
11. ✅ **P10 wall-clock harness for QEMU smoke** — should land before ADR-0027 implementation so B2 changes are measured against a tight band rather than the wide ~4–6.5 ms envelope. *(Track D Minor + Track F Minor consolidated; cross-cutting #2)* — closed in the [2026-05-08 integration PR](../../../analysis/reviews/code-reviews/2026-05-08-pr-19-20-21-multi-axis-review.md) (replaces #21). [`tools/perf-harness.sh`](../../../../tools/perf-harness.sh) lives; first measured baseline at HEAD pre-T-016: **p10=3.884 ms / p50=4.642 ms / p90=5.584 ms** over 20 iterations (see [`docs/analysis/reports/perf-baseline-2026-05-08-post-pr-19-pre-adr-0027.md`](../../reports/perf-baseline-2026-05-08-post-pr-19-pre-adr-0027.md)). The "should land before ADR-0027 implementation" gating condition is satisfied: P10 lands alongside ADR-0027 Accept in the same integration PR; T-016 implementation will be the first to measure against the new band.
12. **Cancel-on-cap-bearing-state destroy-drain ADR** — when B2+ userspace-destroy lands, the v1 no-op must be re-examined; either ADR-0032 §Revision rider or a new ADR. *(Track C Minor, Track A Minor; cross-cutting #1)* — **status unchanged 2026-05-08**: still forward-flagged for the first userspace-destroy task. The PR #18 doc-rider on `ipc_cancel_recv` (item 3 above) already names the future destroy-drain caller, so the forward-flag is documented at the source level too.
13. **B5+ preemption-rollback re-validation of ADR-0032's symmetric arc** — when preemption arrives, today's "structurally unreachable Deadlock branch" assumption disappears and the cancel arc starts being exercised in production. Re-read ADR-0032's "Negative consequences" rider. *(Track C, tracked)* — **status unchanged 2026-05-08**: still forward-flagged for B5+ preemption ADR.

## Self-critique — what this review may have missed

The 2026-05-06 ten-agent comprehensive review missed the T-014 smoke regression. That blind spot was **agent-level coverage of QEMU runtime behaviour**: every track read the source and the docs, but none ran the smoke. The same blind spot risk applies here. Track D rebuilt the kernel and measured `.text` (catching the +228-byte delta and the doc drift) but no track ran `tools/run-qemu.sh` to confirm the smoke actually traces clean post-T-015 across the wide variance band — we trusted PR #17's claim. If a *different* runtime regression slipped through (e.g., a subtle timing change in the IRQ dispatch path that PR #15 polish introduced and that miri can't see, or a stack-frame layout shift from the `register_idle` `assert!` upgrade interacting with the BSP's `task_a` / `task_b` start sequence), this review would not catch it. Mitigation: the queued P10 wall-clock harness is the structural fix; an interim mitigation is a single live smoke run before merging the hygiene follow-up PR. A second blind spot: only Track D measured live; the other seven tracks read source. If a follow-up PR introduces a perf change that no track is *primarily* responsible for, drift will accumulate again.

## References

- 8 axis files under [`2026-05-07-pr-12-to-17-multi-axis-review/`](2026-05-07-pr-12-to-17-multi-axis-review/) (track-a..h)
- PR pages: [#12](https://github.com/cemililik/Tyrne/pull/12) · [#13](https://github.com/cemililik/Tyrne/pull/13) · [#14](https://github.com/cemililik/Tyrne/pull/14) · [#15](https://github.com/cemililik/Tyrne/pull/15) · [#16](https://github.com/cemililik/Tyrne/pull/16) · [#17](https://github.com/cemililik/Tyrne/pull/17)
- Prior review baseline: [2026-05-06 full-tree comprehensive code review](2026-05-06-full-tree-comprehensive.md)
- B1 closure trio (2026-05-07): [business](../business-reviews/2026-05-07-B1-closure.md) · [security](../security-reviews/2026-05-07-B1-closure.md) · [performance](../performance-optimization-reviews/2026-05-07-B1-closure.md)
- Load-bearing ADRs: [0017](../../../decisions/0017-ipc-primitive-set.md) · [0019](../../../decisions/0019-scheduler-shape.md) · [0021](../../../decisions/0021-raw-pointer-scheduler-ipc-bridge.md) · [0022](../../../decisions/0022-idle-task-and-typed-scheduler-deadlock.md) · [0023](../../../decisions/0023-cross-table-capability-revocation-policy.md) · [0025](../../../decisions/0025-adr-governance-amendments.md) · [0026](../../../decisions/0026-idle-dispatch-fallback.md) · [0032](../../../decisions/0032-endpoint-rollback-and-cancel-recv.md)
- Standards: [code-style](../../../standards/code-style.md) · [unsafe-policy](../../../standards/unsafe-policy.md) · [testing](../../../standards/testing.md) · [commit-style](../../../standards/commit-style.md) · [security-review](../../../standards/security-review.md) · [documentation-style](../../../standards/documentation-style.md) · [code-review](../../../standards/code-review.md)
- Audit log: [`unsafe-log.md`](../../../audits/unsafe-log.md)
- Roadmap: [`current.md`](../../../roadmap/current.md) · [`phases/phase-b.md`](../../../roadmap/phases/phase-b.md)
