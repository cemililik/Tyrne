# Code review 2026-05-08 — PR #19 / #20 / #21 multi-axis pre-merge sweep

- **Change:** PRs [#19](https://github.com/cemililik/Tyrne/pull/19) (path-drift sweep), [#20](https://github.com/cemililik/Tyrne/pull/20) (ADR-0027 + T-016 open), [#21](https://github.com/cemililik/Tyrne/pull/21) (P10 perf-harness + first measured baseline).
- **Branches:** `doc-hygiene-2026-05-06-path-drift-sweep` / `adr-0027-kernel-virtual-memory-layout` / `p10-wall-clock-bench-harness`.
- **Reviewer:** @cemililik (+ Claude Opus 4.7 multi-agent fan-out — 4 parallel track agents).
- **Type:** **Pre-merge** multi-axis sweep — these PRs together open the B2 milestone; quality is load-bearing for the T-016 implementation arc that follows. "Block" reserved for genuine regression / discipline violation; "Approve-with-followups" means a one-line same-branch fixup or a hygiene-PR rider closes the finding.
- **Risk class:** Mixed. PR #19 mechanical / low risk; PR #20 design-heavy / high risk (drives T-016); PR #21 small but real bash + awk code / medium risk (load-bearing for B2 regression detection).

## Scope

Four tracks, non-overlapping eyeball. Per-track files under [`2026-05-08-pr-19-20-21-multi-axis-review/`](2026-05-08-pr-19-20-21-multi-axis-review/):

| Track | Axis | PR | File | Verdict |
|-------|------|----|------|---------|
| 1 | PR #19 mechanical sweep — path resolution + content preservation + sweep completeness + untouched-files claim | #19 | [track-1-pr-19-mechanical.md](2026-05-08-pr-19-20-21-multi-axis-review/track-1-pr-19-mechanical.md) | Approve |
| 2 | PR #20 ADR-0027 design correctness + §Simulation arithmetic + MAIR / TCR field-by-field + future-compat | #20 | [track-2-pr-20-design.md](2026-05-08-pr-19-20-21-multi-axis-review/track-2-pr-20-design.md) | Approve-with-3-followups |
| 3 | PR #20 governance, T-016 scoping, audit-log forward-flags, doc structure, cross-reference integrity | #20 | [track-3-pr-20-governance.md](2026-05-08-pr-19-20-21-multi-axis-review/track-3-pr-20-governance.md) | Approve-with-2-followups |
| 4 | PR #21 perf-harness implementation — bash + awk correctness, statistical formulas, edge cases, portability, security | #21 | [track-4-pr-21-perf-harness.md](2026-05-08-pr-19-20-21-multi-axis-review/track-4-pr-21-perf-harness.md) | Approve-with-2-followups |

## Per-PR verdict

- **PR #19 — Approve.** Pure mechanical sweep; all 193 link instances (83 unique targets) resolve post-sweep; zero broken; content byte-stable across all 7 affected files; the 3 untouched files (track-c, track-f, track-i) confirmed absent from diff.
- **PR #20 — Approve-with-1-fix-on-branch + 3-flow-to-hygiene-PR + small-rider-set.** No design blockers. **One Major must be fixed on this branch before merge** (T3-M1: `phase-b.md` §B2 status block + §Sub-breakdown step 1 still say `Proposed 2026-05-08` while the ADR ledger row at line 257 correctly says `Accepted 2026-05-08` — internal contradiction). Three Track-2 Majors are forward-flagged to T-016 / a hygiene PR (escape-hatch documentation, MMU-instance binding rationale, named-ADR-slot for kernel-image section permissions). §Simulation arithmetic is fully correct; MAIR / TCR encodings verified bit-by-bit against ARM DDI 0487.
- **PR #21 — Approve as-is.** No blockers, no Majors. Two Minors flow to a follow-up hygiene PR (orphan-process trap on Ctrl-C, p99-suppression-at-small-N reporting hygiene). Bash 3.2 + BSD awk clean; statistical conventions named in *both* code and report; baseline numbers reproduce from raw samples to the integer ns.

## Headline numbers

| Severity | Track 1 #19 mech | Track 2 #20 design | Track 3 #20 gov | Track 4 #21 perf | **Total** |
|----------|-----------------:|-------------------:|----------------:|-----------------:|----------:|
| Blocker  | 0 | 0 | 0 | 0 | **0** |
| Major    | 0 | 3 | 1 | 0 | **4** |
| Minor    | 1 | 2 (incl. 1 self-withdrawn) | 3 | 2 | **8** (+1 withdrawn) |
| Nit      | 0 | 2 | 2 | 1 | **5** |
| Praise   | 1 | 4 | 4 | 5 | **14** |

The 0-Blocker / 4-Major distribution is consistent with the prior 2026-05-07 multi-axis review's outcome (0 Blocker / 1-forward-flag Major). The Majors here are different in character: 3 are *content-additive* (one-line riders to ADR-0027 / T-016 to record what the design is doing implicitly), 1 is a *fix-on-branch* roadmap-internal-consistency drift.

## Per-track summary

### Track 1 — PR #19 mechanical sweep validation

Approve. 193 link instances across 7 files, 83 unique targets, all resolve post-sweep. Pre-sweep state had 58 broken 4-level paths + 4 redundant-`docs/`-prefix forms; post-sweep 0 broken, 0 mis-corrected. Content preservation perfect — every diff hunk differs only in `..` count or in the redundant-prefix collapse. The 3 untouched files (track-c-security.md, track-f-tests.md, track-i-integration.md) are confirmed absent from the diff per the PR description's claim. **One Minor:** the "180/180 fixed" phrasing in the PR body is ambiguous — likely refers to total link instances (~180), not the 83 unique targets; substance is correct, wording could be tightened. **Praise:** the commit message's path-math rationale is accurate ("repo-root targets need 5 `../`, docs-relative targets need 4 `../`") and matches the actual diff. See [track-1-pr-19-mechanical.md](2026-05-08-pr-19-20-21-multi-axis-review/track-1-pr-19-mechanical.md).

### Track 2 — PR #20 ADR-0027 design correctness + §Simulation arithmetic

Approve-with-3-followups. **§Simulation arithmetic verified end-to-end:** VA→indexing for `0x0800_0000`/`0x0900_0000`/`0x4000_0000`/`0x4800_0000` matches ADR's claims; 4-frame bootstrap budget holds (1 L0, 1 L1, 2 L2); GIC=8 / UART=1 / RAM=64 block counts arithmetically correct; barrier ordering (`TLBI VMALLE1; DSB ISH; IC IALLU; DSB ISH; ISB; SCTLR.{M,I,C}=1; ISB`) is conventional and ARM-ARM-correct; PC at `0x4008_NNNN` maps cleanly into `L2_high[0]`'s 2-MiB block post-MMU-flip. **MAIR / TCR field-by-field:** Attr0=`0x00`=device-nGnRnE ✓, Attr1=`0xFF`=normal-cached-WB-WA-IS ✓, T0SZ=16, IPS=`0b010` (40-bit PA, sufficient for 0..2 GiB v1 RAM), AS=0 (8-bit ASID size — wording could be tightened, see Minor). All correct per ARM DDI 0487 §D5/§D8/§D13. **Three Majors flow forward** (none design-killing): (1) ADR §Decision outcome (c) does not name `mem::forget`/`ManuallyDrop` as `#[must_use]` escape hatches — workspace `Cargo.toml:38` has `unused_must_use = "deny"` so the lint has teeth, but the escape hatches are silent; (2) `MapperFlush::flush(self, mmu: &impl Mmu)` accepts any `Mmu`, not the minting one — non-issue in v1 single-`Mmu` reality but worth a one-line forward-flag for B3+ multi-`AddressSpace` work; (3) future kernel-image section permissions (.text RX vs .rodata R vs .bss/.data RW) is deferred without a named-ADR slot the way ADR-0033 (high-half) is — recommend "ADR-0034 placeholder — Kernel-image section permissions" alongside the ADR-0033 bullet. **Praise:** §Simulation table is the right shape (5 rows, state-pre/action/state-post/observable-effect); Option A–D analysis is honest (Option D is marginal-but-defensible vs B; Option C is correctly priced as premature); ADR-0009 §Revision rider mirrors the ADR-0017 §Revision rider precedent for `ipc_cancel_recv`. See [track-2-pr-20-design.md](2026-05-08-pr-19-20-21-multi-axis-review/track-2-pr-20-design.md).

### Track 3 — PR #20 governance, T-016 scoping, audit-log forward-flags

Approve-with-2-followups (one of which is the lone fix-on-branch Major across all 4 tracks). **Major M1 — `phase-b.md` post-Accept internal contradiction.** At branch HEAD: line 110 (§B2 status block) and line 115 (§Sub-breakdown step 1) still say `Proposed 2026-05-08`; line 257 (ADR ledger row) correctly says `Accepted 2026-05-08`. The Accept commit `bb0a6ba` only flipped the ledger row. **Verified directly against `git show origin/adr-0027-kernel-virtual-memory-layout:docs/roadmap/phases/phase-b.md`.** Same-branch one-commit fixup before merge. **Three Minors:** (m1) `current.md` line 49 calls ADR-0027's table a "**Phase-2** §Simulation table" — copy-paste artefact from the ADR-0032 banner template; "Phase-2" is meaningless for SCTLR.M=1 (the table walks Steps 0..4); (m2) same line still says "Accept *will be* a separate commit per `write-adr` §10" in future tense, but Accept already landed in `bb0a6ba`; (m3) `8b6eef4` PR-number correction is the second occurrence in 2 weeks (PR #18 had a similar one-commit fixup) — banner authoring should defer until after `gh pr create` returns the actual PR number. **Praise:** T-016 §Audit-log section is excellent forward-flag draftability — UNSAFE-2026-0022 / 0023 / 0024 / 0025 each have full Operation / Invariants / Rejected-alternatives substance directly transcribable into `unsafe-log.md`; six-commit bisectable §Approach is well-ordered; `MapperFlush` rationale is fully developed across ADR-0027 §(c), `memory-management.md`, and the ADR-0009 §Revision rider. **Forward-reference grounding clean:** T-016 lands in same commit as Propose (ADR-0025 §Rule 1 ✓); ADR-0033 placeholder follows the named-but-unallocated slot-naming pattern (consistent with ADR-0028..0031 in the phase-b.md ledger); UNSAFE-2026-0022..0025 numbering contiguous from extant 0021. See [track-3-pr-20-governance.md](2026-05-08-pr-19-20-21-multi-axis-review/track-3-pr-20-governance.md).

### Track 4 — PR #21 perf-harness implementation review

Approve-with-2-followups. Paper review only — harness was *not* executed by the sub-agent; the maintainer owns the runtime. **Bash 3.2 + BSD awk clean by construction:** no `declare -A`, no `[[ -v ]]`, no `${var,,}`, insertion sort lives inside awk rather than relying on gawk-only `asort`. **`set -euo pipefail` discipline correct** with explicit `set +e`/`set -e` brackets around `wait "$cmd_pid"` and around the per-iter `OUTPUT=$(run_with_timeout ...)`. **Path traversal blocked structurally:** `--report=CONTEXT` is rejected against `[A-Za-z0-9._-]` at `tools/perf-harness.sh:101-106` *before* it ever flows into a path. **Awk statistics correct:** nearest-rank percentile (`idx = ceil((p/100) * n)`, clamped to `[1, n]`), single-pass mean, **population** stddev (n divisor) with `var < 0` round-off guard. **Convention named in both code and report's Methodology section** — no future archaeologist will have to reverse-engineer which of the seven percentile flavours is in use. **Statistical sanity check on the baseline passes all six checks:** ordering ✓ (3.862 ≤ 3.884 ≤ 4.642 ≤ 5.584 ≤ 6.558 ≤ 6.558), range = 2.696 ms < 6×stddev = 4.254 ms (no heavy tails, clean unimodal-ish distribution), CV = 15 % (tight for QEMU TCG); raw-samples block in the report independently re-derives `a[2]=3884000`, `a[10]=4642000`, `a[18]=5584000`, `a[20]=6558000` to the integer ns. **Two Minors:** (1) no `trap '...' EXIT INT TERM` cleanup — Ctrl-C between iterations can leave the in-flight QEMU + watchdog alive for up to `TIMEOUT_S` seconds; (2) `p99 == max` at N=20 is an inevitable property of nearest-rank at small N (`ceil(19.8) = 20 = a[n]`) — harmless, but the report's Metric table would be more honest if it noted "p99 collapses to max for N < 100" or if the harness simply suppressed p99 when `n < 100`. **Filename / title dedup is glob-anchored** at start of context (`[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]-*`), so `foo-2026-05-08` does *not* over-match. See [track-4-pr-21-perf-harness.md](2026-05-08-pr-19-20-21-multi-axis-review/track-4-pr-21-perf-harness.md).

## Cross-PR observations

Patterns that span two or more PRs / tracks — these are the lessons worth carrying forward into the T-016 implementation arc and beyond.

1. **PR-numbering hygiene drift recurrence (Track 3 m3 + soft signal from PR #21).** PR #20's `8b6eef4` is the second one-commit "PR number was wrong in the banner" fixup in 2 weeks (PR #18 had the same shape). PR #21 *avoided* the issue by not citing a PR number in its current.md banner at all — a defensible choice against future renumber drift, and one PR #20 could have followed. **Process tweak:** banner authoring (in current.md, phase-b.md, or any artefact that names a PR) should defer until *after* `gh pr create` returns the actual PR number; equivalently, the banner can refer to the branch slug (e.g. "P10 wall-clock harness branch") and let a follow-up commit promote to a PR number. Worth a one-line note in `commit-style.md` or the `start-task` skill.

2. **Convention consistency between PR #19's sweep and PR #20's new files.** PR #19 codifies the 5-`../`-for-repo-root / 4-`../`-for-docs convention for files at `docs/analysis/reviews/code-reviews/2026-05-06-full-tree/` (5 levels deep). PR #20's new files (`docs/decisions/0027-...md`, `docs/architecture/memory-management.md`, `docs/analysis/tasks/phase-b/T-016-...md`) live at different depths and use the depth-appropriate convention; spot-checked the relative-path links — no drift. **PR #20 does not reintroduce the bug PR #19 sweeps.**

3. **ADR-0032 vs ADR-0027 §Simulation discipline framing.** ADR-0032 (PR #17) Propose commit *did* land with a §Simulation table; ADR-0027 §Context para 2 calls itself the "first non-recovery-primitive state machine to use the rule" (precise) but `current.md` line 49 / phase-b.md line 110 / PR #20 body all use the looser "first to apply forward (rather than retro-extracted as for ADR-0026 / ADR-0032)". The latter is technically defensible only if "retro-extracted" is read narrowly as "back-fitted *after* the fact"; ADR-0032's table did land in its Propose. Track 3 Nit n1; align all four artefacts to the same precise framing in the same hygiene PR.

4. **PR #21 baseline label tightly couples to PR #20 / PR #19 merge order.** The baseline is named `perf-baseline-2026-05-08-post-pr-19-pre-adr-0027.md`. If merge order is **#19 → #21 → #20**, the label remains literally true. Any other order makes the label drift (#20 first → "pre-adr-0027" is false; #21 first → "post-pr-19" is anticipatory). Recommend merging in this exact order so the baseline file's label matches its measurement context.

5. **All 4 sub-agents trusted `gh` as ground truth.** Per the prior 2026-05-07 review's lesson #6 ("trust gh as ground truth, not the prose description"), Tracks 1–4 used `git show origin/<branch>:<path>` and `gh pr view N` consistently and caught one PR-body-vs-content gap (Track 1's "180/180" Minor — the PR body's count refers to instances, the diff verifies unique targets). Pattern continues to work; worth keeping in the next fan-out brief.

6. **No design Major from Track 2 forces a same-day fixup; the only same-branch-fix Major is governance (Track 3 M1).** This is the right severity distribution for a load-bearing-but-doc-only PR: design discipline is good enough to start T-016, while the roadmap-internal-consistency drift is mechanical and trivially closable.

## Follow-up backlog

Severity-sorted; action-sentence form. Items 1–4 are the **fix-before-merge** set (one of which is on PR #20's branch, three are flow-to-hygiene-PR or T-016 riders). Items 5–11 are **flow-to-hygiene-PR**. Items 12–15 are nits / non-blocking improvements.

> **Closure status (2026-05-08):** all 15 items closed in the integration-PR branch (replaces #19/#20/#21). Closures land across commits `59c08e9` (review artefacts + T3-M1 same-branch fix), `b482fdb` (Track-2 Majors + Track-3 / Track-2 / Track-4 Minors + Track-2 Nits + 2026-05-07 Track-H NIT-1), and the wrap-up commit on integration branch (Track-3 path-drift + commit-style anchor + 158→159 narrative + hal.md hedge + ADR-0027 ≈18 MiB + TCR_EL1.A1 + glossary entries + LC_ALL=C + this annotation pattern). Per-item closure annotations follow the [2026-05-07 review's `8b6147d`-style pattern](2026-05-07-pr-12-to-17-multi-axis-review.md): each item gains ✅ + closing-commit SHA.

### Fix on PR #20 branch before merge

1. ✅ **(Major / Track 3 M1)** Edit `docs/roadmap/phases/phase-b.md` line 110 (§B2 status block) + line 115 (§Sub-breakdown step 1) from `Proposed 2026-05-08` to `Accepted 2026-05-08`, matching the ADR ledger row at line 257. One-line same-branch commit. The Accept-commit careful-re-read pass missed the in-section status mentions. — **closed by `59c08e9`**.

### Flow to hygiene PR (or T-016 §Audit-log section riders) before T-016 commit-1 lands

2. ✅ **(Major / Track 2 M1)** Add a one-bullet entry under ADR-0027 §Consequences/Negative or §Decision outcome (c): "`mem::forget`, `ManuallyDrop::new`, and `let _ = ...` deliberately silence the `#[must_use]` lint on `MapperFlush`; this matches the `x86_64::structures::paging::MapperFlush` precedent and is the only documented path for a caller to drop the token without invoking `flush()` or `ignore()`." — **closed by `b482fdb`**.
3. ✅ **(Major / Track 2 M2)** Add a one-line note to ADR-0027 §Consequences/Neutral or §Decision outcome (c): "The flush token does not bind the minting `Mmu` instance; multi-`Mmu` deployments (B3+ per-task `AddressSpace`, B5+ multi-CPU) will need a stronger token type. Out of scope for v1." — **closed by `b482fdb`**.
4. ✅ **(Major / Track 2 M3)** Add an "ADR-0034 placeholder — Kernel-image section permissions (.text RX / .rodata R / .bss/.data RW)" entry to ADR-0027 §Decision outcome alongside the ADR-0033 bullet, *and* a corresponding row in `phase-b.md`'s ADR ledger. Currently the deferral is mentioned in T-016 §Out of scope and `memory-management.md` §map but has no named-ADR slot. — **closed by `b482fdb`**.

### Flow to hygiene PR (governance / wording polish)

5. ✅ **(Minor / Track 3 m1)** `docs/roadmap/current.md` line 49: drop the words "Phase-2" before "§Simulation table" (the ADR-0027 table walks Steps 0..4, not a "Phase 2"; copy-paste from the ADR-0032 banner). — **closed by `b482fdb`**.
6. ✅ **(Minor / Track 3 m2)** Same line: change "Accept will be a separate commit" to "Accept landed as a separate commit" or drop the sentence entirely (Accept already landed in `bb0a6ba`). — **closed by `b482fdb`**.
7. ✅ **(Minor / Track 3 m3)** Add one line to `commit-style.md` (or the `start-task` / `write-adr` skill): "Banner authoring that names a PR should defer until after `gh pr create` returns the PR number; alternatively reference the branch slug." Closes the recurrence pattern. — **closed by `b482fdb`**.
8. ✅ **(Minor / Track 1)** PR #19 PR description: clarify "180/180" — likely refers to link *instances* across the 7 files (~180) vs unique targets (83); substance is correct, phrasing is imprecise. — **closed by editing the integration PR #22 description** (`gh pr edit 22 --body-file ...`) plus a parallel addendum in `docs/roadmap/current.md` 2026-05-08 banner item 3. The original PR #19's gh description remains as-was (closed PR is immutable from this branch); the integration PR's description and the roadmap banner now state the precise metric: ~180 link instances (~193 by the 2026-05-08 multi-axis review's stricter re-count) across 7 files, 83 unique targets.
9. ✅ **(Minor / Track 2 #4)** ADR-0027 §Simulation Step 3: one-sentence rationale for `DSB ISH` vs Linux's `DSB NSH` (forward-compatible with eventual SMP boot; v1 single-core would be fine with NSH). Optional — current choice is correct, just underdocumented. — **closed by `b482fdb`**.
10. ✅ **(Minor / Track 2 #5)** Tighten ADR-0027 line 59 wording: "TCR_EL1.AS = 0 (8-bit ASID size; v1 leaves TTBR0_EL1.ASID = 0 globally)" — the current wording reads as if "AS=0" *is* the ASID value. — **closed by `b482fdb`** (plus the integration wrap-up commit's TCR_EL1.A1 forward-flag for ADR-0033).
11. ✅ **(Nit / Track 3 n1)** Align "first to apply §Simulation forward" framing across ADR-0027 §Context, current.md banner, phase-b.md §B2 banner, and PR #20 body. ADR-0032's Propose did land with a table; "first non-recovery-primitive state machine" is the precise framing. — **closed by `b482fdb`**.

### Flow to a future hygiene PR (PR #21 follow-ups)

12. ✅ **(Minor / Track 4 #1)** Add `trap 'kill -KILL "$cmd_pid" "$watchdog_pid" 2>/dev/null' EXIT INT TERM` (with PIDs tracked in shell globals) to `tools/perf-harness.sh` so Ctrl-C between iterations doesn't leak in-flight QEMU + watchdog for up to `TIMEOUT_S` seconds. — **closed by `b482fdb`** (`cleanup_in_flight()` + globals + EXIT INT TERM trap).
13. ✅ **(Minor / Track 4 #2)** Either suppress p99 in the report when `n < 100` (it collapses to `max` under nearest-rank), or add a one-line note in the Methodology section: "p99 collapses to max for N < 100 under the nearest-rank convention." Reporting hygiene; underlying number is correct. — **closed by `b482fdb`** (Methodology note added).

### Nits / non-blocking

14. ✅ **(Nit / Track 2 #7+#8)** ADR-0027 line 17 §-citation for "first application"; `memory-management.md` line 88 bit-field diagram cosmetic confusion (block-descriptor PA field is bits[47:21] not [47:12]). — **closed by `b482fdb`** (line 17 framing tightened to "first non-recovery-primitive state-machine ADR"; memory-management.md page-table descriptor diagram redrawn with correct bit ranges + L1/L3 variants noted).
15. ✅ **(Nit / Track 4 #3)** `tools/perf-harness.sh:266-273` `read_stats` re-parses awk output via four `echo | awk` invocations — one `read` loop or a single `eval "$(awk ... | sed 's/^/STAT_/')"` would shave ~7 forks. Runs once per harness invocation; trivial. — **already closed** by PR #21 review-round commit `ef30b5c` (8 echo|awk subshells became one while-read loop).

## Recommended merge order

**#19 → #21 → #20**, with the same-branch fixup on #20 (item 1 above) before #20 merges.

Rationale:
- #19 is purely mechanical and unblocks nothing — merge first to clear the path-drift backlog.
- #21 produces `perf-baseline-2026-05-08-post-pr-19-pre-adr-0027.md`; the label is literally true only when the file lands *after* #19 and *before* #20.
- #20 lands the load-bearing ADR; the §B2 status-block fix (item 1) is a one-line same-branch commit that closes the only same-branch Major from this review.

A hygiene PR after #20 merges captures items 2–11; the perf-harness follow-ups (items 12–13) wait for the next perf-relevant PR.

## References

- Track 1 (mechanical): [track-1-pr-19-mechanical.md](2026-05-08-pr-19-20-21-multi-axis-review/track-1-pr-19-mechanical.md)
- Track 2 (design): [track-2-pr-20-design.md](2026-05-08-pr-19-20-21-multi-axis-review/track-2-pr-20-design.md)
- Track 3 (governance): [track-3-pr-20-governance.md](2026-05-08-pr-19-20-21-multi-axis-review/track-3-pr-20-governance.md)
- Track 4 (perf-harness): [track-4-pr-21-perf-harness.md](2026-05-08-pr-19-20-21-multi-axis-review/track-4-pr-21-perf-harness.md)
- PR diffs: [`gh pr view 19`](https://github.com/cemililik/Tyrne/pull/19) / [`gh pr view 20`](https://github.com/cemililik/Tyrne/pull/20) / [`gh pr view 21`](https://github.com/cemililik/Tyrne/pull/21)
- ADR-0027 (kernel virtual memory layout): [`docs/decisions/0027-kernel-virtual-memory-layout.md`](../../../decisions/0027-kernel-virtual-memory-layout.md)
- T-016 (MMU activation): [`docs/analysis/tasks/phase-b/T-016-mmu-activation.md`](../../tasks/phase-b/T-016-mmu-activation.md)
- ADR-0009 §Revision rider, ADR-0012 §Open questions resolution
- Companion architecture: [`docs/architecture/memory-management.md`](../../../architecture/memory-management.md)
- write-adr skill §10 (separate Propose / Accept commits) + §Simulation discipline; ADR-0025 §Rule 1 (forward-reference grounding)
- ARM *Architecture Reference Manual* (ARM DDI 0487) — §D5 (VMSAv8 translation), §D8.5 (memory attributes), §D13.2.131 (TCR_EL1)
- Prior 2026-05-07 PR #12-#17 multi-axis review: [`2026-05-07-pr-12-to-17-multi-axis-review.md`](2026-05-07-pr-12-to-17-multi-axis-review.md)

## Self-critique — what this review may have missed

This review is paper-only. The four sub-agents read source files, ran arithmetic by hand against ARM DDI 0487, and cross-checked claims against `git show` and `gh pr view`, but **none of them executed code**. The blind-spots that survive:

1. **No QEMU smoke ran.** ADR-0027's §Simulation step 3 — the SCTLR.M=1 transition — cannot fail in this review because no kernel was booted with the page-tables described. T-016's commit-6 smoke verification is where that arithmetic actually gets exercised; if the bit-field math is subtly off (e.g. the AP encoding row in `memory-management.md` that Track 2 finding #6 self-withdrew after re-checking — the *kind* of error the prior review missed live-execution-wise), only the smoke catches it.
2. **The perf-harness was not run.** Track 4 verified the awk formulas statically and reproduced the percentile indices against the raw-samples block in the baseline report, but if the harness has a runtime regression (a sed-portability quirk on a different macOS / Linux box, a watchdog race that fires < 1 % of the time), this review cannot see it. The maintainer's local 20-iteration run is what produced the baseline; this review trusts that run.
3. **ARM ARM section numbers are revision-dated.** Track 2 cites §D5.5 / §D8.5 / §D13.2.131 / §D5.4.5. ARM occasionally renumbers sections across major-version revisions; the citations are correct for revision G.b but the project should not treat the section numbers as stable. The *encoded values* (Attr0=`0x00`, Attr1=`0xFF`, etc.) are stable; the section numbers are not.
4. **Cross-PR merge-order assumption.** The recommended #19 → #21 → #20 order is a label-correctness optimisation; nothing tested what happens if PR #20 lands before PR #21 (the baseline file's "pre-adr-0027" claim becomes false; a small relabel would close it). If the maintainer merges in a different order, the only consequence is a stale label, not a regression.
5. **The prior 2026-05-07 review's lesson was "live execution catches what paper review misses".** This review honors that lesson by *naming* the gap (here) but cannot close it; T-016's commit-6 smoke and the next perf-harness re-run after T-016 lands are the natural close-points.

The review covers what paper review *can* cover well: design correctness, governance discipline, statistical formula correctness, and roadmap-internal consistency. The remainder is for the maintainer's smoke + the next review cycle.
