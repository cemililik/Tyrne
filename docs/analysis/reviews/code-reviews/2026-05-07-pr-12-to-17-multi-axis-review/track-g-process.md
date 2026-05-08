# Track G — Process & governance (PR #12–#17 multi-axis review)

- **Agent:** Claude Opus 4.7 (1M context), 2026-05-07
- **Scope:** ADR-0025 §Rule 1 + §Rule 2 compliance, write-adr skill (especially §10 separate-Accept commit + §Simulation), commit-style trailer hygiene, master-plan AC additions, skill / rider chronology across PR #12–#17.
- **Merge SHA range:** `298b5d2a..8dc433ee` on main (PR #12 → PR #17).

## Executive summary

The 14-day window across PR #12–#17 is the project's **first full test** of the post-2026-04-27 ADR-governance regime (ADR-0025 + write-adr §10 careful re-read + ADR-0024 / ADR-0025 same-day-Accept precedent). It includes a supersession (ADR-0022 → ADR-0026), a Deferred placeholder body (ADR-0023), a fresh state-machine ADR (ADR-0032), and the **first project-side application of write-adr §10's "Accept is a separate commit" rule** as its own diff (PR #17, commit `db24d6d`).

The verdict is **Approve**, no Blockers, no Majors. Process discipline is consistently strong across the window: every commit body explains *why* not *what*; `Refs:` trailers are present on every ADR-touching commit and every cited ADR/T-NNN exists; review-rounds are clean per-PR amend cycles with explicit "applied" / "skipped with reason" sections. Three observation-class findings worth recording for the next-round retro: (a) ADR-0026's Propose-and-Accept landed in a single commit (`10dea48`) — technically permitted by the supersede-adr skill §7 solo-phase exception, but in tension with write-adr §10's separate-commit rule and the discipline §10 was reinforcing; (b) the §Simulation rule was codified in PR #16 commit `77a578a` *immediately before* commit `4aa4b24` proposed ADR-0032 with a Simulation table, and the ordering means the rule was retro-extracted from ADR-0026's experience and applied forward to ADR-0032 in the same PR — the chronology is honest in commit bodies but the artefacts read as if the rule pre-existed the table; (c) the master-plan AC addition ("no closure-trio without recorded smoke") landed only in `business-reviews/master-plan.md`; security and performance master plans were not touched, which is consistent with closure-trio coordination living at the business-master-plan layer in this project but is worth surfacing if a future security-review-only closure trigger is ever introduced.

PR #16 is the highest process-density PR by an order of magnitude (5 substantive doc commits + 1 review-round, codifying two skill / standard rules, writing one Deferred placeholder, proposing one new ADR, and updating the master-plan AC); it is also the cleanest in terms of commit organisation — each artefact is in its own commit with a focused subject and well-formed trailers.

## Findings

### Blocker

- *(none)*

### Major

- *(none)*

### Minor

- **MIN-G1 (PR #12).** ADR-0026 was created at `Status: Accepted` directly in commit `10dea48` ("docs(roadmap,audits,analysis): record B1 smoke regression + ADR-0026 supersedes ADR-0022"). There is no preceding `Proposed`-status commit. Per write-adr §10 + Acceptance criterion ("**Initial commit lands the ADR at `Proposed`.** The Propose commit is separate from any subsequent Accept commit so the careful-re-read pass shows up as its own diff"), this is a violation of the spirit of the rule even when supersede-adr §7's solo-phase clause ("one combined commit is acceptable if the decision is already settled") technically permits a single-commit form. The full skill stack at the moment of merge:
  - `supersede-adr` §7: *"In solo phase, one combined commit is acceptable if the decision is already settled."*
  - `write-adr` §10 + Acceptance criterion: *"Accept may follow same-day after the re-read of step 10 (no calendar gate per ADR-0025 §Revision notes), but never in the same commit as the initial draft."*

  These two clauses are in tension; supersede-adr's permission rests on "decision already settled," but the substance write-adr §10 protects (the careful-re-read showing up as its own diff) is exactly what supersede-adr's "one combined commit" form forecloses. The pragmatic justification on 2026-05-06 was that ADR-0026 was being landed as a smoke-regression hotfix and the "settled decision" framing held — `register_idle` is a near-mechanical re-organisation of an existing structural choice. Track G's reading is that this IS a process-rule violation by the strict reading of write-adr §10, but it is the *kind* of violation the supersede-adr §7 solo-phase clause was written to permit, so it does not block. **Recommended adjustment: a §Revision notes rider on ADR-0026 (or a §Open questions entry on ADR-0025) reconciling the two clauses, so future supersession ADRs know whether to follow supersede-adr §7's combined form or write-adr §10's separate-commit form.** The ambiguity is the bug, not the choice. **Severity rationale: minor — the actual decision content of ADR-0026 holds up under scrutiny (its Simulation table is now the canonical example of the discipline §Simulation codifies); only the *governance* form is in question, and the maintainer's intent (smoke-regression hotfix) is well-documented in the commit body. Real harm is zero; the rule-vs-rule contradiction needs surfacing in the docs before the next supersession.**

- **MIN-G2 (PR #16).** The §Simulation rule was codified in `.claude/skills/write-adr/SKILL.md` and `docs/decisions/template.md` in commit `77a578a` (2026-05-07 14:31:28); ADR-0032 with its Simulation table was proposed in commit `4aa4b24` (2026-05-07 14:32:52), 84 seconds later, in the same PR. ADR-0026's pre-existing Simulation table (2026-05-06) was the empirical motivation cited by both commits. The chronology means: (a) at the moment ADR-0026 was written (2026-05-06), the §Simulation rule did not yet exist; (b) the skill rule was retro-extracted from ADR-0026's experience between 2026-05-06 and 2026-05-07; (c) ADR-0032 is the first ADR drafted *under* the new rule. PR #16's commit body for `77a578a` is fully transparent about this ("Codified after ADR-0026's table caught what ADR-0022's prose-only reasoning had missed"), so the chronology is **not** hidden — a careful reader following the git log can reconstruct it. The risk is that a non-careful reader scanning the artefacts (skill + template + ADR-0032 + ADR-0026) without the commit log infers the rule pre-existed both ADRs, which would mis-credit the discipline as a forward-applied rule rather than a retro-extracted one. **Recommended adjustment: a brief note in `docs/decisions/0026-idle-dispatch-fallback.md` §Revision notes (or in ADR-0025 §References) that the §Simulation rule was extracted from ADR-0026's table on 2026-05-07, naming the codifying commit `77a578a`. The B1 closure retrospective at `docs/analysis/reviews/business-reviews/2026-05-07-B1-closure.md` (cited by `77a578a`) probably already says this; surfacing it at the artefact layer reduces the chronology-reconstruction cost.** **Severity rationale: minor — the chronology is honest in commit bodies; the risk is purely "future reader misreads the artefact ordering as predictive"; one-line rider mitigates fully.**

- **MIN-G3 (PR #16).** The master-plan AC addition ("no closure-trio without recorded smoke") landed only in `docs/analysis/reviews/business-reviews/master-plan.md` (commit `77a578a`, line 88). The commit body is explicit: *"`docs/analysis/reviews/business-reviews/master-plan.md` §Acceptance criteria gains a new rule: for milestone-completion and phase-closure triggers only..."*. Security-reviews/master-plan.md and performance-optimization-reviews/master-plan.md were not touched. This is internally consistent — closure-trio coordination is a business-master-plan responsibility, and the security / perf reviews land *as artefacts of* the closure trio, not as the trigger for it. But it leaves a latent gap: if a future security-review-only event (e.g., a fresh threat-model escalation that triggers a security review independent of a milestone closure) needs to claim "closure semantics," the smoke-trace AC does not extend there. The current scope ("for milestone-completion and phase-closure triggers only") is correctly bounded — but the bound lives only in business-master-plan prose, not in the security / perf master plans where a future independent-trigger event would land. **Recommended adjustment: a one-line cross-reference in security-reviews/master-plan.md and performance-optimization-reviews/master-plan.md pointing at the business-master-plan §AC's smoke-trace clause, so a future security-review-only event reads the rule before claiming closure semantics.** **Severity rationale: minor — no current event tests this gap; the bound is correct; the cross-reference is forward-protection.**

### Nit

- **NIT-G1 (PR #12).** Commit `c30f4ee` ("feat(kernel,bsp): T-014 — idle dispatch via separate fallback slot") has a trailer block mixing `Refs:` content with `Audit:` content awkwardly:

  ```text
  Refs: ADR-0026, ADR-0022, ADR-0021, T-014
  Audit: UNSAFE-2026-0014 (existing entry covers `register_idle`'s
  momentary-`&mut` pattern; explicit naming follows in a future
  review-fix commit if asked)
  ```

  The `Audit:` trailer is multi-line and the explanation should arguably be in the body, not a trailer continuation. commit-style.md §Trailers says "Each trailer is on its own line." Subsequent commits (`a29c8a3`, `9cbf578`) drop the parenthetical and the multi-line trailer is replaced with the cleaner `Audit: UNSAFE-2026-0019, UNSAFE-2026-0020` shape — the single-line discipline self-corrects within the same PR. **Severity: nit — pre-existing pattern, self-corrected in the same PR's later commits.**

- **NIT-G2 (PR #12, PR #14, PR #15).** Each PR's review-round commit ("fix: PR #N review-round — apply ...") uses an `applied / skipped` bullet structure that is excellent for review traceability, but the commit *subject* is inconsistent across the three: `"apply 8 of 10 findings (2 style nits skipped)"` (PR #12), `"close TyrneOS bare-name leftovers (2 of 3 findings)"` (PR #14), `"4 findings (3 line-ref drops + 1 metadata trim)"` (PR #15). The subject patterns vary in whether they front the count, the action, or the categorisation. Not a violation, but a future "review-round commit" sub-pattern in commit-style.md could codify a single shape for cross-PR readability. **Severity: nit — observation only.**

- **NIT-G3 (PR #16).** Commit `4aa4b24` ("docs(adr,roadmap): propose ADR-0032 ... + open T-015") body has a parenthetical about the README index update: *"docs/decisions/README.md — index row for ADR-0032 (Proposed 2026-05-07). [Updated in earlier δ.1 commit; this commit's diff covers only the entries directly attributable to this decision-pair landing.]"* This is correct (the README index was already updated in commit `7e530c0` for the ADR-0023 placeholder) but the bracket-prose form is unusual for a commit-style body. A cleaner form would be a separate paragraph: *"The decisions/README.md row for ADR-0032 was added in the preceding δ.1 commit `7e530c0` alongside the ADR-0023 row; this commit's diff therefore touches only the new files."* **Severity: nit — readability only.**

- **NIT-G4 (PR #17).** The Accept commit (`db24d6d`, "docs(adr): accept ADR-0032 — endpoint rollback + ipc_cancel_recv") body is a 3-bullet checklist of "what the careful re-read confirmed". This is a strong pattern-of-record for write-adr §10 (the body literally documents what the re-read pass checked) and should arguably be promoted into the skill's §10 procedure as the recommended commit-body shape for the Accept commit. The current §10 says "Accept is a separate commit from the initial Propose commit so that the careful-re-read pass shows up as its own diff" but does not prescribe what the Accept commit's *body* should contain. **Severity: nit — positive observation that could become a skill update; not a finding against PR #17.**

### Praise

- **PRAISE-G1 (PR #17, commit `db24d6d`).** **First project-side application of write-adr §10's "Accept is a separate commit" rule as its own diff.** The diff is exactly +2/-2 (Status: Proposed → Accepted on the ADR file + matching index row in `docs/decisions/README.md`); the commit body documents the careful-re-read checklist (forward-references grounded, negatives are real costs, Simulation table covers Phase 2 end-to-end). This is the discipline §10 codifies, executed cleanly. The pattern is exactly what ADR-0025 §Revision notes (cool-down withdrawal) said the careful-re-read step would protect: "the substance the cool-down enforced — careful re-reading before Accept — remains a write-adr-skill responsibility, just without the enforced calendar-day delay." `db24d6d` is the proof-of-concept that the substance-without-calendar-delay actually works in practice.

- **PRAISE-G2 (PR #16, commit ordering).** The PR #16 commit chain is the cleanest "process-density PR" the project has shipped to date. Six commits, each with a single focused subject, in dependency-order:
  1. `8dad9ab` — B1 closure trio (the "what landed" facts)
  2. `77a578a` — codify the rules the retro extracted (skill + master-plan AC)
  3. `bd11f3a` — closure-trio canonical-source callouts (sourcery review-feedback)
  4. `7e530c0` — ADR-0023 Deferred placeholder body (δ.1)
  5. `4aa4b24` — ADR-0032 Propose + T-015 Draft (δ.2)
  6. `74a40c3` — review-round (gemini Phase α / δ-as-subject wording)

  Each commit is independently bisectable; each cites the prior in its body where relevant; trailer blocks are well-formed; no commit bundles unrelated artefacts. The pattern is a textbook execution of commit-style.md §Granularity ("One logical change per commit"). When future PRs need to land multi-artefact governance updates, this PR's commit-organisation is the reference.

- **PRAISE-G3 (across all 6 PRs).** **Every forward-reference grounded.** ADR-0025 §Rule 1 ("forward-references that don't ground at a real T-NNN drift into purgatory") spot-check across the 6 PRs:
  - ADR-0026's Dependency chain (PR #12) names T-014 — exists at HEAD as `docs/analysis/tasks/phase-b/T-014-idle-dispatch-fallback.md`. ✅
  - ADR-0023's "Why deferred" (PR #16) names triggers tied to B-phase tasks (none yet opened); the placeholder explicitly states "no implementation task references this ADR" — correctly avoiding ungrounded forward-references. ✅
  - ADR-0032's Dependency chain (PR #16 / PR #17) names T-015 — opened in the same commit as ADR-0032 Propose (`4aa4b24`), grounded at HEAD as `docs/analysis/tasks/phase-b/T-015-endpoint-rollback-cancel-recv.md`. ✅
  - All `Refs: ADR-NNNN` trailers across all 6 PRs cite extant ADRs (0010, 0011, 0017, 0019, 0021, 0022, 0024, 0025, 0026, 0032 all exist at HEAD; 0023 lands as placeholder in the window).
  - All `Refs: T-NNN` trailers cite extant task files.
  - No `Refs: ADR-0099` "trailer that lies" anti-pattern (per commit-style.md §Anti-patterns line 115).

- **PRAISE-G4 (PR #16, commit `77a578a`).** The "rule was withdrawn, the discipline was not" pattern is now applied recursively: the §Simulation rule was *codified* (skill + template + AC) rather than left as a learning in a retrospective document, exactly as ADR-0025's §Revision notes said the careful-re-read substance should be — embedded in the skill where the next agent will encounter it, not just narrated in a retro nobody re-reads. The pattern-of-pattern: when a retro identifies a learning, the closing move is to push it into the skill / standard / master-plan AC layer where it executes mechanically next time. This PR demonstrates the move.

- **PRAISE-G5 (PR #17 / PR #16 review-round handling).** PR #16's review-round (`74a40c3`) caught the "Phase α adopts" / "δ writes the body" wording issue *before* PR #17 inherited the artefact. The fix is two one-line edits with a focused commit body: *"line 110: 'Phase α adopts' → 'B2 prep adopts' (α is PR alias, not project phase); line 149: 'δ writes the body' → 'This task provides the body' (δ as subject of action was confusing)."* This is exactly the discipline ADR-0025 §Rule 1 protects (Greek-letter PR aliases were drifting into ADR / task prose as if they were project phases) — the review-round caught the drift before it propagated. PR #17's body and ADR-0032 inherit the corrected wording.

- **PRAISE-G6 (PR #16, ADR-0023 placeholder).** The ADR-0023 placeholder body (commit `7e530c0`) is genuinely Deferred, not crypto-Accepted-with-Deferred-status. Reading the file:
  - Status: `Deferred`
  - "## Decision outcome (not applicable — Deferred)" — explicit non-decision
  - Four-option sketch (A/B/C/D) labeled "What a real ADR-0023 *would have to* settle" — future-tense, no choice made
  - "### Simulation: Not applicable — this ADR is a Deferred placeholder; no decision to simulate. The eventual replacement ADR will need the discipline."

  The body resolves the in-tree references (glossary, retros, phase-b ledger) without making any decision the future ADR-0023 author has to undo. The four-option sketch gives a starting point but is explicitly marked as not-yet a Pros / Cons treatment. PR #17 does not modify this file (verified via `git log --all -- docs/decisions/0023-...md` — only `7e530c0` touches it). **The placeholder discipline is exactly right: future-grounded scaffolding, no current decision content.**

## Cross-PR observations

### ADR-0025 §Rule 1 (forward-reference contract): unanimous compliance

Across the 6 PRs, every forward-reference in every ADR / task / commit is grounded. The closest call is ADR-0023's placeholder, which legitimately *has no T-NNN* because its decision is deferred — the explicit "no implementation task references this ADR" line in the placeholder body is the §Rule 1-compliant way to express "we know we don't know yet" (per ADR-0025's "the only permitted form of un-grounded forward-reference, and it is paired with a visible 'we know we don't know yet' marker"). PR #16 demonstrates the pattern correctly.

### ADR-0025 §Rule 2 (riders are not failures): rate is healthy

Riders added in this 14-day window:
- **ADR-0017 §Revision notes** (PR #17, commit `c258ee3`) — additive `ipc_cancel_recv` recovery primitive; user-observable IPC surface unchanged. Refinement, not contradiction.
- **ADR-0026** does not yet have any riders — its 1-day-old life has not yet generated implementation feedback. (Track G expects one when ADR-0030 / ADR-0027 lands and either crystallises or contradicts ADR-0026's `register_idle` as a HAL-vs-BSP boundary call; that would be a §Rule 2-healthy rider.)
- **ADR-0022** received its supersession callout (PR #12, commit `10dea48`) — body preserved unmodified, status flipped to `Superseded by 0026 (idle-task-location axis only; typed-error axis stands)`. Append-only convention honoured.
- **UNSAFE-2026-0014** receives its 3rd amendment (PR #12, `register_idle` site) and 4th amendment (PR #17, `ipc_cancel_recv` site). The audit-log Amendment frequency on a single entry is starting to feel high, but each Amendment is a real new code site, not a contradiction of a prior claim — exactly the §Rule 2 "rider is feedback" pattern.

No rider in this window crosses into in-place rewrite of an Accepted body (ADR-0022's body is preserved; ADR-0017's body is preserved; ADR-0021's body is preserved). Append-only invariant holds.

### Commit-style.md compliance: clean across the board

`git log --oneline 298b5d2a..HEAD` reveals 27 commits across the 6 PRs (including merge commits + review-round amends). Sample of the imperative-mood, English, body-explains-why discipline:
- ✅ "feat(kernel,bsp): T-014 — idle dispatch via separate fallback slot (ADR-0026)"
- ✅ "docs(adr): accept ADR-0032 — endpoint rollback + ipc_cancel_recv"
- ✅ "fix: PR #15 review-round — 4 findings (3 line-ref drops + 1 metadata trim)"

All commit subjects use lowercase Conventional Commits scope; bodies wrap at ~100 chars; bullet structures separate "applied" / "skipped" findings cleanly; trailers are at the bottom, blank-line-separated. One pre-existing pattern persists: PR-merge commits on `main` are merge-commits not squashes (`Merge pull request #N from cemililik/<branch>`), against commit-style.md §Merging strategy line 98 ("Merge commits are not used on `main`. The history is linear"). This is a pre-existing GitHub-PR-flow pattern from before the 14-day window (PR #1 through PR #11 all use the same merge-commit form), so it is not a finding against PRs #12-#17 — but the standard and the actual workflow are out of sync on this point and a future ADR should reconcile (either change the standard to permit GitHub merge-commits, or change the workflow to squash). Out of scope for this Track G review; flagging for the maintainer's awareness.

### Process density per PR (qualitative)

| PR | Process density | Governance artefacts |
|---|---|---|
| #12 | High | ADR-0026 Accept (single-commit, supersede-adr §7); ADR-0022 supersession callout; T-014 Draft → In Review; UNSAFE-2026-0014 3rd amendment; mini-retro 2026-05-06-B1-smoke-regression; comprehensive review artefact lands |
| #13 | Medium | 3 doc-commits closing 7 Track-E blockers + 3 non-blocker drift items; ADR-0023 index row added (placeholder body deferred to PR #16) |
| #14 | Low | URL rename sweep + Yüksek → High localisation; review-round catches 2 bare-name leftovers |
| #15 | Low | Code polish; review-round trims process-metadata tails from comments (correct sourcery-driven discipline — review-track names age poorly inside source) |
| #16 | **Highest** | B1 closure trio (3 review artefacts); §Simulation rule codified (skill + template + AC); business-master-plan smoke-trace AC; ADR-0023 placeholder body; ADR-0032 Propose; T-015 Draft; sourcery-driven canonical-source callouts |
| #17 | Medium | ADR-0032 Accept (separate-commit per write-adr §10 — first such application); T-015 implementation; UNSAFE-2026-0014 4th amendment; ADR-0017 §Revision notes rider; ipc.md state machine update |

PR #16 deserves special note as the most-governance-dense PR the project has shipped, *and* the cleanest in commit organisation. The "process commit chain" pattern is the model.

## Verdict

**Approve, with three minor adjustments queued for maintainer review** (MIN-G1 / MIN-G2 / MIN-G3 above — all rider / cross-reference-class fixes, no code or governance changes required). No Blockers. No Majors. Five Praise items recording patterns the project should keep doing.

The 14-day window establishes the post-2026-04-27 ADR-governance regime as **executable** — the rules survive contact with a smoke-regression hotfix (PR #12), a multi-artefact closure trio (PR #16), and the first separate-Accept-commit application (PR #17), and produce well-organised, traceable, append-only-respecting artefacts at every step. The one rule-vs-rule tension (write-adr §10 separate-commit vs supersede-adr §7 solo-phase combined-commit; MIN-G1) is a clarifying-rider opportunity, not a process failure.

## References

- [ADR-0025 — ADR governance amendments: forward-reference contract, rider hygiene](../../../../decisions/0025-adr-governance-amendments.md) — §Rule 1, §Rule 2, §Revision notes (cool-down withdrawal).
- [`.claude/skills/write-adr/SKILL.md`](../../../../../.claude/skills/write-adr/SKILL.md) — full procedure, especially §10 separate-commit Accept and §Simulation (added in PR #16 commit `77a578a`).
- [`.claude/skills/supersede-adr/SKILL.md`](../../../../../.claude/skills/supersede-adr/SKILL.md) — §7 solo-phase combined-commit clause invoked by ADR-0026's landing.
- [`docs/standards/commit-style.md`](../../../../standards/commit-style.md) — trailer hygiene + merging-strategy clauses.
- [`docs/analysis/reviews/business-reviews/master-plan.md`](../../business-reviews/master-plan.md) §Acceptance criteria — smoke-trace AC added by commit `77a578a`.
- [`docs/analysis/reviews/business-reviews/2026-05-07-B1-closure.md`](../../business-reviews/2026-05-07-B1-closure.md) — the retrospective that produced the §Simulation + smoke-trace rules.
- [`docs/decisions/0023-cross-table-capability-revocation-policy.md`](../../../../decisions/0023-cross-table-capability-revocation-policy.md) — Deferred placeholder body landed in PR #16 commit `7e530c0`.
- [`docs/decisions/0026-idle-dispatch-fallback.md`](../../../../decisions/0026-idle-dispatch-fallback.md) — single-commit Accept landing on 2026-05-06; pattern-of-record for MIN-G1.
- [`docs/decisions/0032-endpoint-rollback-and-cancel-recv.md`](../../../../decisions/0032-endpoint-rollback-and-cancel-recv.md) — first ADR drafted under the new §Simulation rule; first Accept commit under write-adr §10 separate-commit discipline.
- Track A (Kernel correctness) — sibling axis review for the same PR window.
- Track J (Hygiene) — predecessor pattern for the multi-axis review format (`docs/analysis/reviews/code-reviews/2026-05-06-full-tree/track-j-hygiene.md`).
