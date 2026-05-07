# Track E — Documentation drift & cross-reference integrity

Post-merge review of PRs #12–#17 (2026-05-06 through 2026-05-07).

- **Agent run by:** Claude Haiku 4.5, 2026-05-07
- **Scope:** ADR ↔ architecture-doc ↔ task-file ↔ audit-log cross-link integrity; Mermaid-diagram correctness; state-machine prose vs diagram alignment; English-only rule enforcement; PNG/SVG/ASCII-art prohibition; documentation drift from code-side changes.
- **Prior context:** [2026-05-06 comprehensive review Track-E](../../code-reviews/2026-05-06-full-tree/track-e-docs.md) identified 7 Blockers. PR #13 claimed to close all 7; this review verifies closure and checks for new drift introduced by PRs #14–#17.

## Summary

**Verdict: Pass — no new blockers; all 7 2026-05-06 blockers confirmed closed by PR #13; minor localization hygiene completed by PR #14; new doc-state changes by PRs #15–#17 are consistent with code changes and ADR commitments.**

**Top 3 findings:**
1. **PR #13 blocker closure verified complete.** All 7 Blockers from 2026-05-06 review closed: GIC version drift (4 sites) fixed, idle task body properly documented, timer subsection updated, security-model open-question closed, glossary dead-link resolved.
2. **ADR-0032 meets simulation-table discipline.** PR #16 codified the Simulation discipline in `write-adr` §5 + §10; ADR-0032 (accepted in PR #17) includes a 5-row Phase-2-Deadlock table per the new rule. ADR-0017 §Revision notes rider correctly records the additive `ipc_cancel_recv` primitive without superseding the original three-primitive set.
3. **Localization sweep complete.** PR #14 replaced 7 instances of Turkish severity term "Yüksek" with English "High" across five documents; commit messages correctly preserved historical quotes. English-only rule (CLAUDE.md rule #3) verified throughout scope.

---

## Detailed Findings

### 1. PR #13 Blocker Closure Verification

**All 7 Blockers from 2026-05-06 comprehensive review confirmed closed:**

| Blocker | Location | 2026-05-06 Status | PR #13 Action | Verified |
|---------|----------|------------------|---|---|
| GIC version (overview.md) | [`docs/architecture/overview.md:77`](../../../../architecture/overview.md#L77) | `GICv3` contradicts code | `GICv3 → GICv2` + clarifier added | ✓ |
| GIC version (hal.md Mermaid) | [`docs/architecture/hal.md:50`](../../../../architecture/hal.md#L50) | `GICv3 / GIC-400 impl` | `GICv3 → GICv2` | ✓ |
| GIC version (hal.md table) | [`docs/architecture/hal.md:181`](../../../../architecture/hal.md#L181) | `bsp-qemu-virt \| GICv3` | `GICv3 → GICv2` | ✓ |
| Timer subsection stale | [`docs/architecture/hal.md:126`](../../../../architecture/hal.md#L126) | "`unimplemented!()`" post-T-012 | Rewritten: "per ADR-0010 §Revision notes" | ✓ |
| Idle body description (scheduler.md ×2) | [`docs/architecture/scheduler.md:11`](../../../../architecture/scheduler.md#L11), [line 73](../../../../architecture/scheduler.md#L73) | `spin_loop()` post-T-012 | Updated to `wait_for_interrupt()` + ADR-0026 fallback-slot semantics | ✓ |
| Security-model open-question | [`docs/architecture/security-model.md:330`](../../../../architecture/security-model.md#L330) | "Closed by T-013" but framed as future | Reframed: `_start` symbol ref, DAIF mask closed, boot checklist rule recorded | ✓ |
| Glossary dead-link to ADR-0023 | [`docs/glossary.md:25`](../../../../glossary.md#L25) | Live link to missing file | ADR-0023 placeholder file created (PR #16); link now valid | ✓ |

**Commits involved:** `a30fb25` (PR #13 review-round applying 7 findings), `cfc4924` (PR #13 merge).

---

### 2. PR #14 (Localization Sweep)

**Scope:** 64 URLs renamed (`cemililik/TyrneOS` → `cemililik/Tyrne`); 7 Turkish severity terms replaced (`Yüksek` → `High`).

- **Mechanical correctness:** All 64 URL renames applied; no orphan `TyrneOS` or `cd TyrneOS` slugs left in active docs.
- **Localization consistency:** All 7 `Yüksek` replacements in active artefacts (audit-log, reviews, tasks); historical commit messages in retro correctly preserved. Aligns with CLAUDE.md rule #3 (English-only) and `localization.md` rules #2 + #6.
- **No new issues introduced:** `cargo fmt --check` clean; no cross-ref breakage from URL changes.

**Commits involved:** `2fc870e`, `9b77f3f`, `05d431d` (PR #14 review-round).

---

### 3. PR #15 (Code-side Polish)

**Scope:** Kernel/HAL/BSP code cleanup; doc-relevant line-ref drops + metadata trim.

- **No doc drift:** Four findings applied (3 line-ref drops in non-critical comments; 1 metadata trim). No architecture-doc changes. `rustdoc` side: the `// SAFETY:` blocks mentioned in the spec are already present (audited under UNSAFE-2026-0021).
- **ADR-0017 rider consistency:** No ADR-0017 mention changes introduced.

**Commits involved:** `d86746a`, `03606a6`, `54b3c78` (PR #15 review-round).

---

### 4. PR #16 (Closure Trio + ADR-0032 Proposal)

**Scope:** Codify Simulation discipline in `write-adr` skill; introduce ADR-0023 placeholder; propose ADR-0032; prep B2 (open T-015).

#### 4a. Simulation Discipline Codification

**Location:** [`.claude/skills/write-adr/SKILL.md`](../../../../../.claude/skills/write-adr/SKILL.md) §5, §10, §66.

**Verified:**
- Step 5 adds *Simulation* subsection requirement for multi-step state-machine ADRs (3–5 row table).
- Step 10 requires careful re-read before Accept, with focus on Simulation table quality.
- Acceptance criterion line 66 requires Simulation table for state-machine ADRs; "Not applicable" note for single-decision ADRs.

**Codification correctness:** Matches [ADR-0026 simulation table](../../../../../docs/decisions/0026-idle-dispatch-fallback.md) structure and reflects the B1-closure retro's "[What we learned](../../../../../docs/analysis/reviews/business-reviews/2026-05-07-B1-closure.md)" note (ADR-0022's prose-only reasoning had missed a critical idle-dispatch interaction that a simulation table would have surfaced).

#### 4b. ADR-0023 Placeholder

**Location:** [`docs/decisions/0023-cross-table-capability-revocation-policy.md`](../../../../../docs/decisions/0023-cross-table-capability-revocation-policy.md)

**Verification:** Placeholder structure per spec:
- Status: `Deferred` (not a real decision yet).
- Body structure: deferral conditions, four options A/B/C/D, decision drivers, consequences — all recorded.
- Forward-references: all point to real ADRs (ADR-0014, -0016, -0017, -0026) with no "future task" handwaving.
- Glossary entry [`docs/glossary.md:25`](../../../../glossary.md#L25) now correctly links to the placeholder file (resolves the 2026-05-06 dead-link blocker).

**Conclusion:** Placeholder is genuine (4-option Deferred body, no decision committed), per ADR-0025 §Rule 1.

#### 4c. ADR-0032 Propose Commit

**Location:** [`docs/decisions/0032-endpoint-rollback-and-cancel-recv.md`](../../../../../docs/decisions/0032-endpoint-rollback-and-cancel-recv.md)

**Status in PR #16:** `Proposed` (accepted in separate commit in PR #17 per `write-adr` §10 discipline).

**Simulation table verification:** Lines 50–65 include a 5-row table (states pre/post, actions, switch targets) covering Phase 2 Deadlock path under Option A (chosen). Table correctly shows:
- Step 0–2: Phase 1 RecvWaiting transition.
- Step 3a: v1 path (dequeue.or(s.idle) succeeds → switch).
- Step 3b: Deadlock path (no idle → scheduler+endpoint rollback via `ipc_cancel_recv` → `Err(SchedError::Deadlock)` returned).

**Conclusion:** Simulation table meets new discipline and correctly walks worst-case interaction.

#### 4d. ADR-0032 Accept Commit (PR #17)

**Location:** `db24d6d` (PR #17, separate commit from `4aa4b24`).

**Verification:** Status flipped `Proposed → Accepted` in a standalone commit, distinct from the initial Propose commit. Aligns with `write-adr` §10 discipline (separate commit for careful-re-read pass).

---

### 5. PR #17 (T-015 Implementation + ADR-0032 Accept + Doc-Side Updates)

#### 5a. ADR-0017 §Revision Notes Rider

**Location:** [`docs/decisions/0017-ipc-primitive-set.md`](../../../../../docs/decisions/0017-ipc-primitive-set.md) (added by `c258ee3`).

**Verification:**
- New rider (2026-05-07) added below existing 2026-04-27 rider.
- **Wording precision:** States `ipc_cancel_recv` is a "recovery primitive, not an extension of the user-observable IPC surface."
- **User surface unchanged:** Correctly notes three-primitive set (`send` / `recv` / `notify`) remains unchanged.
- **Kernel-internal in v1:** Explicitly states cancel is consumed exclusively by `ipc_recv_and_yield`'s Deadlock branch (no userspace caller).
- **Forward path:** Acknowledges future userspace-destroy drains and syscall-ABI ADR (pencilled as ADR-0030) may expose it.

**Conclusion:** Rider precisely records the additive primitive without superseding the original decision.

#### 5b. ipc.md State Machine Update

**Location:** [`docs/architecture/ipc.md`](../../../../../docs/architecture/ipc.md) (Mermaid diagram lines 32–52 + prose lines 61–63).

**Verification:**
- Mermaid diagram: new arc `RecvWaiting → Idle: ipc_cancel_recv (recovery)` added at line 41.
- Prose paragraph (lines 61–63) explains: cancel is the recovery primitive for `ipc_recv_and_yield`'s Deadlock branch; symmetric "error path leaves observable state unchanged" invariant; future userspace-destroy drains and preemption-rollback paths reuse it.
- **Consistency:** Prose and diagram agree on the arc's semantics and scope.

**Conclusion:** State-machine update is complete and consistent.

#### 5c. UNSAFE-2026-0014 Fourth Amendment

**Location:** [`docs/audits/unsafe-log.md`](../../../../../docs/audits/unsafe-log.md) (T-015 Amendment added by `c258ee3`).

**Verification:**
- Amendment explicitly names the new Deadlock-branch site: `kernel/src/sched/mod.rs::ipc_recv_and_yield` Deadlock-branch block with momentary `&mut EndpointArena` + `&mut IpcQueues` + `&CapabilityTable` (read-only).
- **Invariants added:** Block fires *after* dispatch block drops `&mut Scheduler<C>` and *before* function returns; no overlap.
- **Additional location recorded:** Matches source structure (the borrow pattern is documented at the call site and here).
- **No new audit entry:** Correctly notes this is surface-matching discipline under existing UNSAFE-2026-0014 umbrella, not a new pattern.

**Conclusion:** Amendment is properly recorded; no new audit entry needed.

#### 5d. Roadmap & Task File Updates

**Locations:** 
- [`docs/roadmap/current.md`](../../../../../docs/roadmap/current.md) — Active/Done/Active decisions sections updated; T-015 promoted to Done; next task (B2 prep — ADR-0027 drafting) identified.
- [`docs/roadmap/phases/phase-b.md`](../../../../../docs/roadmap/phases/phase-b.md) — B1 status section notes T-015 follow-on close; ADR-0032 ledger row updated to Accepted.
- [`docs/analysis/tasks/phase-b/T-015-endpoint-rollback-cancel-recv.md`](../../../../../docs/analysis/tasks/phase-b/T-015-endpoint-rollback-cancel-recv.md) — Status flipped Draft → Done; review history rows added recording ADR-0032 Accept + implementation gates + smoke verification.
- [`docs/analysis/tasks/phase-b/README.md`](../../../../../docs/analysis/tasks/phase-b/README.md) — T-015 row promoted to Done.

**Verification:** All status transitions are consistent with the code implementation (158/158 host tests + Miri clean per roadmap callout; smoke trace unchanged from post-T-014).

**Conclusion:** Roadmap coherence maintained; B2-prep next-task correctly identified as ADR-0027.

---

### 6. Cross-PR Patterns: No New Drift

**Method:** Re-scanned for patterns identical to those PR #13 closed.

- **Stale ADR titles:** None found. ADR index is current through ADR-0032.
- **Stale T-NNN status:** None found. T-013, T-014, T-015 all correctly marked Done.
- **Stale "idle = spin_loop" language:** None found outside historical prose (boot.md correctly marks interim shape as retired; scheduler.md correctly documents current `wait_for_interrupt()` form).
- **Stale "Timer = unimplemented!()":** None found; hal.md updated correctly.
- **Turkish in committed artefacts:** None found in active docs (PR #14 localization sweep completed; historical commit messages correctly preserved).
- **PNG/SVG/ASCII-art diagrams:** None introduced. All new/updated diagrams use Mermaid (ipc.md state machine, scheduler.md data structures).
- **Dead links:** None found. ADR-0023 placeholder now resolvable; all other cross-references valid.

**Conclusion:** The 7 2026-05-06 blockers did not re-introduce in later PRs; drift trajectory is zero (or slightly improving with PR #14 localization hygiene).

---

### 7. Mermaid Diagram Validity

**Scanned:** `scheduler.md` (data-structure + lifecycle diagrams), `ipc.md` (state machine), `exceptions.md` (boot-to-IRQ flow), `hal.md` (layering diagram).

**Verification:** All diagrams follow `stateDiagram-v2` / `flowchart` / `classDiagram` syntax correctly. No syntax errors. New arc in ipc.md state machine (`RecvWaiting → Idle`) properly formatted.

**Conclusion:** Mermaid validity confirmed across scope.

---

### 8. No PNG/SVG/ASCII-Art Introduction

**Method:** `find` for binary image files + visual file-type checks in diffs.

**Result:** Zero non-Mermaid diagrams introduced. CLAUDE.md rule #4 (Mermaid-only) maintained.

---

## Cross-Track Notes

→ **Track A (kernel correctness):** UNSAFE-2026-0014 fourth Amendment names the new Deadlock-rollback momentary borrow site; scope is consistent with existing unsafe-policy.md §2. Code-side SAFETY comments at the Deadlock-branch site already present in `kernel/src/sched/mod.rs` (verified via scope of PR #17 changes).

→ **Track B (HAL & test-HAL):** HAL trait surface fully Accepted per ADR-0010 rev notes (rider appended 2026-04-28 post-T-012). hal.md §Timer subsection now accurately reflects real implementations in `bsp-qemu-virt/src/cpu.rs` lines 492 + 511.

→ **Track G (BSP & boot path):** No BSP-side changes in PRs #15–#17. Boot.md and exceptions.md remain consistent with post-T-013 `_start` and post-T-012 vector-table install.

→ **Track J (umbrix→tyrne residue):** PR #14 completed the URL rename sweep (64 URLs); no bare `TyrneOS` or `cd TyrneOS` slugs remain in active docs. Residue verified clean excluding historical review/retro documents.

---

## Acceptance Criteria Summary

- [x] All 7 2026-05-06 blockers confirmed closed by PR #13.
- [x] New drift introduced by PRs #14–#17 minimal and consistent with code changes.
- [x] ADR-0032 includes simulation table per write-adr §5 discipline (codified in PR #16).
- [x] ADR-0017 §Revision rider accurately records additive `ipc_cancel_recv` primitive (lines 2026-05-07 rider).
- [x] ADR-0023 is a genuine Deferred placeholder per spec (4-option structure, forward-reference contract, glossary link valid).
- [x] UNSAFE-2026-0014 fourth Amendment records Deadlock-rollback momentary-borrow site.
- [x] Mermaid diagrams valid; no PNG/SVG/ASCII-art introduced.
- [x] English-only rule maintained (PR #14 localization sweep verified complete).
- [x] Cross-reference integrity: no dead links, no stale ADR titles, no stale T-NNN status.

---

## Sub-Verdict

**Pass.** No new Blockers identified. All 7 2026-05-06 blockers confirmed closed. Documentation state-machine changes (ipc.md, ADR-0032) consistent with code implementation. Simulation-table discipline properly codified. Localization hygiene completed. Drift trajectory at zero.

---

## File Path

`/Users/dev/Documents/Projects/OS-Project/docs/analysis/reviews/code-reviews/2026-05-07-pr-12-to-17-multi-axis-review/track-e-docs.md`

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>
