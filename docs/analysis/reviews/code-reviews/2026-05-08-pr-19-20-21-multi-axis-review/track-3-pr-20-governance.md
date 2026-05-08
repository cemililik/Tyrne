# Track 3 — PR #20 governance, T-016 scoping, audit-log forward-flags, docs

- **PR:** [#20](https://github.com/cemililik/Tyrne/pull/20)
- **Branch:** adr-0027-kernel-virtual-memory-layout
- **Commits reviewed:** dc4d92b + bb0a6ba + 8b6eef4
- **Reviewer:** Claude Opus 4.7 sub-agent (Track 3)
- **Verdict:** Approve-with-2-followups

## Summary

Governance discipline is largely clean — Propose / Accept are correctly separate commits, T-016 lands in the same commit as Propose (ADR-0025 §Rule 1 satisfied), and the four forward-flagged UNSAFE entries are well-described in T-016 with full Operation / Invariants / Rejected-alternatives shape. One **Major** drift: `phase-b.md` §B2 status block (line 111) and §Sub-breakdown step 1 (line 115) still say "Proposed 2026-05-08" at branch HEAD; the Accept commit only updated the ledger row at line 257. T-016 user-story scoping is solid — six bisectable commits with verifiable acceptance criteria and seven correctly-named out-of-scope deferrals.

## Findings

### Blocker

(none)

### Major

- **M1. `phase-b.md` post-Accept inconsistency.** At branch HEAD (post-bb0a6ba), `phase-b.md` line 111 §B2 status block still reads "ADR-0027 `Proposed` 2026-05-08; T-016 (MMU activation) `Draft` 2026-05-08", and §Sub-breakdown step 1 (line 115) still reads "*(Proposed 2026-05-08)*". The Accept commit only flipped the ADR ledger row at line 257 (`Proposed → Accepted`). Internal contradiction within the same file — fix in a follow-up commit on this branch *before* merge, or as a Minor rider. Block on merge if Track 3 verdict is to be a faithful "post-PR state".

### Minor

- **m1. `current.md` describes ADR-0027 §Simulation as "five-row Phase-2 §Simulation table"** (line 48). "Phase-2" is an ADR-0032 term (Phase 2 Deadlock branch of `ipc_recv_and_yield`) and is meaningless for the MMU SCTLR.M=1 transition. Copy-paste artefact from the ADR-0032 banner template. Drop the words "Phase-2" — the table walks five steps (0..4), not a "Phase 2".
- **m2. `current.md` line 48 ledger entry says "Accept will be a separate commit per `write-adr` §10"** — but Accept *already* landed in this PR (bb0a6ba). Stale future-tense wording in the post-Accept ledger; the bb0a6ba edit pass missed it. Edit to past tense ("Accept landed as a separate commit per `write-adr` §10") or drop the sentence.
- **m3. `8b6eef4` PR-number correction is minor process drift.** Track C of the prior 2026-05-07 review's lesson #6 was "trust gh as ground truth, not the prose"; the 2026-05-08 banner was authored before PR numbers were assigned, requiring the third commit. A pre-push step "verify PR numbers via `gh pr view N`" would close the loop. The same fix landed on PR #18 last week — recurrence suggests the banner-authoring step should be deferred until after `gh pr create`.

### Nit

- **n1. ADR-0027 §Context para 2 framing.** The excerpted ADR text reads (verbatim, link targets stripped to avoid mis-resolving from this track file's location):
  > ADR-0027 is the **first ADR drafted under `write-adr` skill §Simulation discipline going forward** (ADR-0026's table was the empirical retro-source; ADR-0032's was the first application; this is the first *non-recovery-primitive* state machine to use the rule).
  
  The body's framing ("first non-recovery-primitive") is more precise than current.md / phase-b.md / PR body's "first to apply the rule forward (rather than retro-extracted as for ADR-0026 / ADR-0032)". The latter is technically true if "retro-extracted" is read narrowly as "back-fitted after the fact", since ADR-0032's table did land in its Propose. Wording could align across all four artefacts.
- **n2. T-016 §Approach commit 5 lands `linker.ld` + `mmu_bootstrap` in one commit** — the linker reservation is `.bss`-internal (zero behavioural change to the running kernel until consumed), so the routine that consumes the symbols can land alongside without splitting commit 5 into 5a/5b. Bisectability holds.

### Praise

- **P1. T-016 §Audit-log section is excellent forward-flag draftability.** Each of UNSAFE-2026-0022/0023/0024/0025 has Operation / Invariants / Rejected-alternatives substance directly transcribable into `unsafe-log.md` at T-016 implementation time. The umbrella-vs-new-entry decision (4 new entries, not extending UNSAFE-2026-0014) is implicit but correct — the MMU surface is structurally distinct from the scheduler `&mut` umbrella.
- **P2. Companion `memory-management.md` (209 lines) is the design-first pattern done right.** Mirrors `scheduler.md` / `ipc.md` / `exceptions.md` lineage; will earn a code-refresh rider at T-016 §Approach commit 6 closure (already noted in T-016 §Documentation).
- **P3. Six-commit bisectable §Approach in T-016 is well-ordered.** Each commit is buildable and host-test-green; QEMU smoke deferred to commit 6 (correctly — intermediate commits leave the kernel un-MMU-activated). No backward dependencies between commits.
- **P4. `MapperFlush` token rationale is fully developed across ADR-0027 §(c), `memory-management.md`, and the ADR-0009 §Revision rider.** Rust ecosystem prior art (`x86_64::structures::paging::MapperFlush`) cited; rejection of `Drop`-based discipline in favour of explicit `flush()`/`ignore()` argued; ergonomic cost acknowledged in §Negative consequences.

## T-016 acceptance-criteria audit

| Criterion (selection) | Atomic? | Verifiable? | Conflicts/missing? |
|-----------------------|---------|-------------|--------------------|
| `MapperFlush` newtype + `#[must_use]` | yes | host-tests + lint | ok |
| `Mmu::map` return-type change | yes | type-check | ok |
| `bsp-qemu-virt/src/mmu.rs` impl with 7 named methods | yes | host-tests | ok |
| `.boot_pt` linker reservation + `__boot_pt_*` symbols | yes | ELF inspection | ok |
| `mmu_bootstrap` 3-step sequence | yes | code review + smoke | ok |
| 4 audit-log entries with full triplet shape | yes | grep `unsafe-log.md` | ok |
| Smoke trace = baseline + 1 new line | yes | smoke comparison | ok |
| `-d int,unimp,guest_errors` empty | yes | QEMU smoke | ok |

No missing criterion for the MAIR/TCR encoding pre-MMU-flip — the descriptor-encoding host tests in commit 2 cover the pre-flip arithmetic, the `mmu_bootstrap` review (commit 5) covers the register configuration, and the smoke trace + `-d int,unimp,guest_errors` discipline catches the post-flip moment. No conflicts between §Acceptance criteria and §Definition of done; the DoD adds three roadmap-promotion items not in §AC, which is the expected superset relationship.

## Forward-reference grounding

| Reference | Type | Grounded in this PR? | Pattern match? |
|-----------|------|----------------------|----------------|
| T-016 (`docs/analysis/tasks/phase-b/T-016-mmu-activation.md`) | T-NNN user-story | yes — added by dc4d92b (160 lines) | ADR-0025 §Rule 1: same-commit-as-Propose grounding ✓ |
| ADR-0033 placeholder (kernel high-half migration) | named-but-unallocated ADR slot | no file landed | Slot-naming pattern (ADR-0028/0029/0030/0031 in `phase-b.md` ledger have no files); **does NOT match the ADR-0023 placeholder-with-Deferred-file pattern**. Accept commit body justifies this choice; consistent with how other reserved B-phase slots are tracked. |
| UNSAFE-2026-0022 / 0023 / 0024 / 0025 | audit-log entries | no — to be opened by T-016 implementation | Pattern matches: T-016 §Audit-log entries section pre-drafts the triplet content; PR description does not explicitly say "not in this PR" but T-016 §Approach commits 3/4/5 schedule the additions. Recommend a one-sentence PR-body note at next push. |
| ADR-0009 §Revision notes rider | additive surface change | yes — landed by dc4d92b | ADR-0017 `ipc_cancel_recv` rider precedent ✓ |
| ADR-0012 §Open questions resolution | linkback | yes — landed by dc4d92b | strikethrough + resolution-paragraph form ✓ |

## Audit-log forward-flag adequacy

| Entry | Operation? | Invariants? | Rejected-alts? | Adequate for draft? |
|-------|-----------|-------------|----------------|---------------------|
| UNSAFE-2026-0022 (page-table frame writes in `mmu_bootstrap`) | yes — descriptor writes to four bootstrap frames | yes — page-aligned, exclusively-owned, pre-zeroed | yes — dynamic alloc rejected (no PMM yet); `core::ptr::write` rejected (more `unsafe`) | yes |
| UNSAFE-2026-0023 (`MAIR_EL1` / `TCR_EL1` / `TTBR{0,1}_EL1` / `SCTLR_EL1` writes) | yes — 5 named MSR sites | yes — MMU off when these run; configure regime that activates on M=1 | yes — piecemeal-via-functions rejected (loses must-run-in-order discipline) | yes |
| UNSAFE-2026-0024 (TLBI / IC IALLU / DSB / ISB asm) | yes — 5 named asm forms | yes — barrier ordering documented | yes — implementation-defined OoO behaviour rejected | yes |
| UNSAFE-2026-0025 (per-call `Mmu::map`/`unmap` page-table entry writes) | yes — block / table / page descriptor writes post-bootstrap | yes — target frames are valid PT frames in active AS; per-page TLBI is caller's responsibility (enforced via `MapperFlush`) | partial — the rationale for "this is distinct from 0022" is implicit (0022 is bootstrap-only single-call; 0025 is per-call post-bootstrap from a different code path / different invariant set) but not explicit | adequate; a one-line "distinct from 0022 because per-call invariants differ" would tighten it |

Numbering ✓ contiguous from existing UNSAFE-2026-0021. Umbrella-vs-new-entry decision (4 new, not extending 0014) correct — MMU surface is structurally separate from the scheduler-`&mut` umbrella. T-016 implicitly enforces this by listing the four entries as new acceptance criteria; an explicit "not extending UNSAFE-2026-0014" in T-016 §Audit-log entries §Notes would close the loop on PR #20's audit-discipline reading.

## Roadmap / governance consistency

| Artefact | Pre-merge claim | Precedent matches? |
|----------|----------------|--------------------|
| `current.md` 2026-05-08 banner: "ADR-0027 Accepted; T-016 Draft" | pre-merge | **yes — improves on ADR-0032 precedent.** ADR-0032's Propose commit (`4aa4b24`) did NOT update `current.md` at all; the banner-on-Propose discipline started post-ADR-0032. This PR's pattern (Propose touches banner with `Proposed`; Accept flips banner to `Accepted`) is the correct evolution. |
| `phase-b.md` ADR ledger row "Accepted 2026-05-08" | pre-merge | yes — Accept commit correctly flips the ledger from `Proposed` to `Accepted` (line 257). **But the §B2 status block (line 111) and Sub-breakdown step 1 (line 115) still say `Proposed`** — see Major M1. |
| T-016 status chain (Draft → In Progress post-merge) | per PR body | yes — T-016 frontmatter `Status: Draft` matches PR body "implementation moves Draft → In Progress post-merge"; bb0a6ba's body and current.md banner agree. |

## Commit-message hygiene

| Commit | Subject | Body | Trailers | Verdict |
|--------|---------|------|----------|---------|
| `dc4d92b` Propose | "docs(adr,arch,task): propose ADR-0027 — kernel virtual memory layout (B2 — identity-mapped MMU activation) + open T-016" — 122 chars (>72) | comprehensive; sub-decisions enumerated; updated artefacts itemised; verification listed | `Refs: ADR-0027, T-016, ADR-0009, ADR-0012` ✓; `Co-Authored-By: Claude Opus 4.7` ✓ | Solid. Subject is over 72 chars but `commit-style.md` allows the convention `subject:summary…` form for high-context entries; matches `4aa4b24` (ADR-0032 Propose) shape. |
| `bb0a6ba` Accept | "docs(adr): accept ADR-0027 — kernel virtual memory layout" — 58 chars | careful re-read checklist (4 bullets) per write-adr §10; §Simulation arithmetic verified | `Refs: ADR-0027` ✓; `Co-Authored-By:` ✓ | Excellent. Mirrors `db24d6d` (ADR-0032 Accept) exactly + adds explicit "L2_low[64..72] = 8 blocks" arithmetic check. |
| `8b6eef4` PR-number correction | "docs(roadmap): correct P10 harness PR reference (#20 → #21)" — 60 chars | one-line; documents drift cause | no `Refs:` (acceptable for a hygiene fix); `Co-Authored-By:` ✓ | OK. See Minor m3 — recurrence of the same drift suggests a process tweak. |

## References

- write-adr skill §10 (separate Propose / Accept commits)
- ADR-0025 §Rule 1 (forward-reference grounding contract)
- Prior ADR-0032 Propose (`4aa4b24`) + Accept (`db24d6d`) for precedent
- ADR-0023 placeholder-with-file pattern (`docs/decisions/0023-cross-table-capability-revocation-policy.md`) for the alternative-pattern comparison
- `docs/audits/unsafe-log.md` — UNSAFE-2026-0021 confirmed as latest extant entry; 0022..0025 numbering contiguous
- Track C of 2026-05-07 multi-axis review (lesson #6 — "trust gh as ground truth")
