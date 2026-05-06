# Business review 2026-05-06 — B1 smoke regression mini-retro

- **Trigger:** maintainer-initiated. The comprehensive multi-agent review at HEAD `214052d` (this morning) surfaced a doc-drift block (Track E `Request changes` × 7); separately, the maintainer-side QEMU smoke that B1 closure had recorded as `Pending QEMU smoke verification` was finally run this afternoon. The smoke surfaced a kernel-side regression unrelated to Track E's doc drift but more severe in its blast radius: **B1's "implementation complete" claim of 2026-04-28 does not survive a full QEMU run**. This mini-retro records that reality before it goes stale.
- **Scope:** ad-hoc — *B1 implementation closure*, retroactive correction. Specifically the demo-flow regression introduced by T-007 (idle task, B0) and inherited unchanged through T-009 / T-011 / T-012 / T-013 to HEAD `214052d`.
- **Period:** 2026-04-28 (B1 implementation closed in PR #10 merge `7b42bbe`) → 2026-05-06 (today; smoke run + diagnosis).
- **Participants:** @cemililik (+ Claude Opus 4.7 agent as scribe; ten parallel review agents earlier today; one merge agent).

---

## What landed

### Smoke run

- **2026-05-06 ~21:35 (local)** — first full QEMU smoke executed by the maintainer at HEAD `214052d` via `tools/run-qemu.sh` (with a one-line bash-3.2 `set -u` fix to the script applied in-flight: `"${INT_LOG_FLAGS[@]}"` → `${INT_LOG_FLAGS[@]+"${INT_LOG_FLAGS[@]}"}`). Serial output captured to `/tmp/tyrne-serial.log`.
- **Observed trace (truncated, hung):**

  ```text
  tyrne: hello from kernel_main
  tyrne: timer ready (62500000 Hz, resolution 16 ns)
  tyrne: starting cooperative scheduler
  tyrne: task B — waiting for IPC
  tyrne: task A -- sending IPC
  ```

  Trace stops here; no panic banner (`!! tyrne panic !!` not present); no exception logged under `-d int`; no `guest_error` / `unimp` / PSCI event. QEMU continues running with the guest in a silent halt state.

- **Confirming diagnostic** (`-d exec,nochain` for ~3 s) — last executed Rust function names follow the chain:

  ```text
  ipc_send → ipc_send_and_yield → yield_now → disable_irqs → context_switch_asm → idle_entry → wait_for_interrupt
  ```

  The kernel **switched to `idle_entry`** instead of to `task_b` after Task A's `ipc_send_and_yield` delivered the message and unblocked Task B. Idle's body issues `WFI`; no IRQ source is armed in the v1 demo (the timer's `arm_deadline` is wired but never called by `task_a` / `task_b`); the core sleeps indefinitely.

### Root-cause analysis (commit-level)

- **Mechanism.** The scheduler's ready queue is a single FIFO. Idle is added to the queue at boot via `Scheduler::add_task(idle_entry, ...)` ([`bsp-qemu-virt/src/main.rs:701`](../../../../bsp-qemu-virt/src/main.rs#L701)). After Task A's `ipc_send` delivers and `unblock_receiver_on(ep)` enqueues the unblocked Task B at the queue's tail, `yield_now` re-enqueues Task A at the tail and dequeues the head — which by that point in the round-robin is **idle**, not Task B.
- **Why ADR-0022's analysis missed this.** [ADR-0022 §Decision outcome](../../../decisions/0022-idle-task-and-typed-scheduler-deadlock.md) chose **Option A — idle as a regular task in the ready queue** over Option B (idle in a dedicated `Option<TaskHandle>` slot, dispatched only as fallback). Its rationale (line 73) reads: *"when idle yields to itself (only task ready), `yield_now`'s existing 'only one ready task' early-return path handles the case without a context switch"*. That reasoning is correct **only when idle is the sole Ready task**; the demo flow has all three (B Ready after unblock, A Ready after yield, idle Ready since boot) simultaneously. The ADR did the math in prose without simulating the actual demo queue states; the bug is structural, not a coding error.
- **Why host tests pass.** `tyrne-test-hal::FakeCpu`'s `wait_for_interrupt` is a `count += 1` no-op (it does not block). The 90 kernel host tests use `FakeCpu` and so cannot model "idle's WFI hangs because no IRQ ever fires"; they exit `idle_entry`'s loop body promptly and record progress. Miri (149/149 clean today) inherits the same blindness — it is a memory-safety checker, not a liveness checker.
- **When it broke.** T-007 (B0; commit `b3f6d80`/`760c019`-era — see [T-007 task file](../../tasks/phase-b/T-007-idle-task-typed-deadlock.md)) introduced both the `idle_entry` registration and ADR-0022's Option A choice. The defect entered the tree at the same instant ADR-0022 was Accepted (2026-04-22). It was not a B1 regression in the strict sense — B1 (T-012/T-013) inherited it unmodified.

### Reviews and approvals that nominally cleared this code

- **Phase A code review (2026-04-21)** — predates T-007; out of scope for this regression.
- **B0 closure business review (2026-04-27)** — approved with QEMU smoke marked maintainer-side pending; no agent-side smoke runs.
- **B0 closure consolidated security review (2026-04-27)** — ADR-0022 Option A scrutinised on the eight axes of the security-review master plan; no liveness-axis adversarial flow ran. Verdict: Approve.
- **B1 closure business review (2026-04-28)** — approved with the same maintainer-side smoke gap; carried forward UNSAFE-2026-0019 / 0020 / 0021 `Pending QEMU smoke verification` notes.
- **B1 closure consolidated security review (2026-04-28)** — same.
- **B1 closure performance baseline (2026-04-28)** — by design did not exercise full demo trace; relied on inherited A6 baseline numbers.
- **2026-05-06 full-tree comprehensive review (this morning)** — Track A (Kernel correctness) approved with 7 non-blocking; Track C (Security) approved; Track G (BSP) approved. None traced the demo's queue-state machine; the regression was outside the static-analysis surface every track operated on.

The honest reading: **six retro/review artefacts approved a code path that does not work end-to-end on real silicon (or QEMU's faithful emulation of it).** The pattern is consistent — every reviewer relied on host tests + miri + the absence of a smoke regression as proxies for "demo works"; none of those proxies model the FIFO ordering between idle and unblocked receivers under a cooperative WFI idle.

## What changed in the plan

- **B1 milestone status — rolled back from "implementation complete" to "implementation incomplete; one regression open".** This is a retroactive correction of the 2026-04-28 closure call. The B1 closure trio (business + security + performance) stands as historical record of the state-of-knowledge on 2026-04-28, with this mini-retro and a status flip in [`current.md`](../../../roadmap/current.md) recording the correction.
- **ADR-0022 status — to flip to `Superseded by ADR-0026`** in Phase 2 of this fix arc. Its body remains intact per ADR-0025 §Rule 2 (riders/historical-record convention); a `> Superseded.` callout at the top points forward.
- **ADR-0026 — repurposed.** Phase-b.md row 246 reserved ADR-0026 for "Exception-vector-table / handler-dispatch shape (T-012, conditional)" and acknowledged it might go unused. T-012 absorbed the vector-table design without writing 0026, leaving the slot logically free. Phase 2 of this fix arc uses ADR-0026 for the idle-dispatch supersession; phase-b.md gets a row-amendment recording the repurpose.
- **T-014 — to open** as `Draft` in Phase 2 (with ADR-0026's `Dependency chain` section grounding it per ADR-0025 §Rule 1). Scope: refactor `Scheduler<C>` to add a separate `idle: Option<TaskHandle>` slot and a `register_idle` raw-pointer free function; idle is dispatched only when the ready queue is otherwise empty. Estimated diff: ~30-50 LOC kernel + 1 BSP `kernel_entry` line + 2-3 new host tests; existing `unblock_receiver_on_moves_task_to_ready` and `start_prelude_panics_on_empty_ready_queue` tests need to be re-read but should remain valid (idle is the answer to the latter's panic, not a regression cause).
- **B2 prep (ADR-0027 kernel virtual memory layout) — paused** until B1 truly closes. The 2026-04-28 closure note specifying that ADR-0027 drafting could run in parallel with smoke verification is no longer applicable: smoke surfaced a real B1 regression, so the smoke is no longer "maintainer's-clock-only verification" but a live blocker.
- **The comprehensive review's seven Track-E doc-drift blockers — still open.** Each remains a doc fix; they do not block T-014's land but should not be lost in the noise of the regression. Tracked in [`2026-05-06-full-tree-comprehensive.md`](../code-reviews/2026-05-06-full-tree-comprehensive.md).

## What we learned

**Smoke is the only liveness oracle Tyrne has, and the project has been treating it as a checkbox.** The 8-day gap between B1 implementation closure and the first full smoke run is the signal. Every prior closure trio recorded the smoke as a maintainer-side workitem and proceeded to "Done" in advance of it. That worked through Phase A because the A5/A6 demo's smoke had been run live during T-005's review-fix arcs. From T-007 (B0) onward, no closure has been gated by an executed smoke — only by the *intent* to run one. The ratio of "design + host-test + review approve" wall-time to "smoke-run wall-time" is now ~5 days : ~30 seconds; smoke is not the bottleneck, and the optics of "Done conditional on smoke" leaks back into "Done" silently. **Implication:** B-phase milestone closures should not promote past `In Review` to `Done` without a recorded smoke pass; the trio's verdict is a `Comment` with a smoke gate, not an `Approve`. This is a process change worth codifying as Phase 2 of this arc lands, not before — the decision belongs in B1's actual closure retro, not this regression mini-retro.

**ADR analysis is uncomfortable to simulate, and skipping the simulation is the most consistent failure mode of the project's ADR work.** ADR-0022's choice between Option A (idle in queue) and Option B (idle separate) was justified entirely in prose. The text correctly identified the dispatch ergonomics ("Option A keeps the FIFO simple") and the audit footprint ("Option A adds zero unsafe"); it did not run the demo's queue states forward through `unblock_receiver_on` + `yield_now`. The regression is exactly what a four-row table of (queue-state-pre, action, queue-state-post) would have surfaced before the ADR Accept date. The same critique fits every multi-step kernel ADR the project has shipped (ADR-0014 cap derivation tree, ADR-0017 IPC primitive set, ADR-0019 scheduler shape) — each was justified in prose and got its bug-discoveries via riders post-Accept rather than via simulation pre-Accept. **Implication:** the `write-adr` skill could grow a *Simulation* check item — for every multi-step state-machine ADR, the body must include a 3-5 row table walking the worst-case interaction through. Defer the codification until Phase 3 lands and we know whether the simulation discipline would have caught *this* bug specifically.

**The comprehensive multi-agent review's blind spot is precisely "did you actually run the program?"** Ten tracks ran in parallel this morning, four of them (A kernel correctness, C security, F tests, G BSP) each had a clean shot at the demo flow. None caught it. The reason is uniform: each track's checklist was about static-analysis-detectable properties. The flow trace ("after task A's IPC send, the next dispatch should be B; trace through the queue states to confirm") is dynamic, not static. Track F (Tests & coverage) came closest — its verdict noted "QEMU smoke is maintainer-only" as a Non-blocking finding §F-1. The find was correct; the severity classification ("Non-blocking") was not. **Implication:** for any future full-tree review, add a *Track K — Live execution* whose explicit job is "boot the kernel under QEMU and trace one demo flow end-to-end". This is essentially Track F's §F-1 promoted to a track in its own right. Defer the codification until the post-fix re-review proves the structure works.

**The audit-log "Pending QEMU smoke verification" pattern is load-bearing and was working as designed.** UNSAFE-2026-0019 / 0020 / 0021 each carried a `Pending QEMU smoke verification` status note from 2026-04-28 onward. Today's smoke could not lift those notes because the kernel hangs before any of those `unsafe` sites' contracts get fully exercised in production sequence (the timer's `arm_deadline` is never called by the demo, so UNSAFE-2026-0021's MMIO write site never fires; the GIC-CPU-interface IRQ path covered by UNSAFE-2026-0019 / 0020 is initialised but never traverses an actual interrupt). The Pending notation correctly reflected the verification gap. The win here: **the audit log retained the calibration the closure retros lost**. The discipline is real, and Phase 1.2 of this fix arc records that calibration formally as Amendments to those three entries.

## Adjustments

- [x] **Rolled back B1 closure status in [`current.md`](../../../roadmap/current.md) to reflect smoke-surfaced regression.** This mini-retro's Phase 1.3 (handled in same commit cluster).
- [x] **Append-only Amendments to UNSAFE-2026-0019 / 0020 / 0021** noting the smoke run reached but did not exercise their MMIO/asm sites, so the `Pending QEMU smoke verification` status persists. This mini-retro's Phase 1.2 (handled in same commit cluster).
- [ ] **Phase 2 — supersede ADR-0022 with ADR-0026** (see *What changed in the plan* above). Trigger: this mini-retro lands; ADR-0026 drafted next.
- [ ] **Phase 2 — open T-014 as `Draft`** with ADR-0026's *Dependency chain* section grounding it per ADR-0025 §Rule 1. Trigger: ADR-0026 in `Proposed` state.
- [ ] **Phase 3 — kernel scheduler refactor** per T-014. Trigger: ADR-0026 `Accepted` (same-day after careful re-read per ADR-0025 §Revision notes is allowed; the substance-of-the-step gate replaces the cool-down per ADR-0025).
- [ ] **Phase 3 — re-run QEMU smoke** post-refactor; expect the full demo trace through `tyrne: all tasks complete` plus the boot-to-end timing line. Trigger: T-014's implementation commit lands.
- [ ] **Phase 3 — final Amendment lifting `Pending QEMU smoke verification` on UNSAFE-2026-0019 / 0020 / 0021** when the smoke completes a full pass (the timer-IRQ path is still not exercised by the demo, so these Amendments will read "smoke-verified-pre-IRQ-arm; full IRQ-path coverage deferred to first preemption-using task" rather than blanket clearance — and that's the honest record). Trigger: smoke produces full trace.
- [ ] **Codify "no Done without recorded smoke" gate.** Defer to B1's actual (post-fix) closure retro; not a Phase 1-3 item.
- [ ] **Codify "Simulation" check in `write-adr` skill** for multi-step state-machine ADRs. Defer to post-Phase-3 review.
- [ ] **Add "Live execution" track to full-tree-review template.** Defer to post-Phase-3 review.

## Next

- **Active phase:** B (unchanged).
- **Active milestone:** **B1 — reopened.** B0 stays `closed` (T-007's design choice was the root cause but T-007 itself satisfies its task DoD; the supersession of ADR-0022 is the corrective action, not a B0 reopen).
- **Active task:** **T-014 to open in Phase 2** (`Draft`); replaces "B2 prep (ADR-0027 drafting)" as the next implementation work. ADR-0027 drafting paused until B1 closes for real.
- **Next review trigger:** **B1 closure (genuine).** Produces a full closure trio (business + consolidated security + performance baseline) once T-014 lands and the smoke passes through `tyrne: all tasks complete`. The 2026-04-28 closure trio remains the historical record of "what we believed on 2026-04-28"; the next closure trio records "what is actually true post-T-014".
