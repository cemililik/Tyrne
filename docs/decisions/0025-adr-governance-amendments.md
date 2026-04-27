# 0025 — ADR governance amendments: cool-down, forward-reference contract, rider hygiene

- **Status:** Proposed
- **Date:** 2026-04-27
- **Deciders:** @cemililik

## Context

[ADR-0013](0013-roadmap-and-planning.md) settled the project's roadmap-and-planning process on 2026-04-20. Its §"Integration with ADRs" said: *"The roadmap does not replace ADRs. It sequences them. A task may require an ADR as an acceptance criterion; an ADR may result in new tasks being added."* That single paragraph was the entire normative content for how ADRs interact with the planning process.

The Phase A → B0 implementation arc — six days of work spanning T-006 / T-007 / T-009 — produced four ADRs (ADR-0021, ADR-0022) that needed post-Accept riders within their first week, plus one (ADR-0021) that needed a mid-proposal revision before Accept. Every rider was, in retrospect, a gap that an extra calendar day of cool-down would have caught at near-zero cost. Every rider's content traced back to one of three implicit rules that ADR-0013's framing had not made explicit:

1. **Same-day `Proposed → Accepted` produces drafty decisions.** A draft that hasn't been re-read after sleeping on it carries assumptions the author did not stress-test. The four post-Accept riders in A → B0 each closed a gap that a second read would have surfaced.
2. **Forward-references that don't ground at a real T-NNN drift into purgatory.** ADR-0022's first rider claimed "T-009 wires a timer IRQ" without T-009 having a task file constraining its scope. The implementation discovered the conflation; a sub-rider was needed to disambiguate.
3. **Riders themselves get treated as failures.** When a third rider appears, the temptation is to rush the next ADR to "get it right this time" — which produces *more* riders, not fewer. The signal is the *rate* of riders, not their presence.

ADR-0013 was edited in-place on 2026-04-27 (commit `56fd9eb`) to add three subsections codifying these rules. That edit was itself an append-only-policy violation: ADR-0013 was already `Accepted`, and the new content rewrote its body rather than appending. The second-read review surfaced the contradiction (ADR-0013 was the document defining the append-only rule it was being edited in violation of). This ADR-0025 is the correction: extract the three rules into a new ADR that stands on its own, leave ADR-0013's body intact except for a single rider/pointer, and follow ADR-0025's own rules in landing it (Proposed today; Accepted ≥ 2026-04-28 per the cool-down rule this ADR codifies).

## Decision drivers

- **Honour the append-only invariant.** ADR-0013 cannot be edited in place once Accepted. The rules need their own ADR to exist as a first-class decision, citable and revisable on its own terms.
- **Make the rules followable mechanically.** "Sleep on it" is a rule the maintainer (or an agent) can mechanically apply. So is "every forward-reference must point at a real T-NNN file". So is "rider count > N is a signal". Lessons-as-prose got rediscovered four times across A → B0; lessons-as-rules need to fire on the next ADR (ADR-0024 is the validation event).
- **Do not over-correct.** Riders are how implementation feedback enters the design record. Trying to eliminate them is the wrong target. The rules name what is acceptable (riders, dated and append-only) and what is not (in-place body rewrites, ungrounded forward-references).
- **Compatible with single-author + AI-agent reality.** Tyrne is solo development with AI-agent assistance. The rules do not assume a multi-person review board. They assume that the maintainer plus a reviewing agent (manually or via a skill) is the review surface — which means the rules must be cheap, mechanical, and self-checkable.

## Considered options

1. **Option A — Three rules in their own ADR (this ADR-0025; chosen).** ADR-0013 stays Accepted, body intact, with a single pointer rider. New rules are first-class, citable, supersedable on their own terms.
2. **Option B — Edit ADR-0013 in place.** What was attempted in commit `56fd9eb`; rejected because it violates the append-only invariant ADR-0013 is meant to define. Already reverted.
3. **Option C — `propose-standard-change` skill instead of an ADR.** The rules are arguably standards (process discipline), not architectural decisions. Could go in `docs/standards/adr-governance.md` instead. Rejected because the rules are *about ADRs* and need to be cited from inside ADR text — having them in a `standards/` file requires every ADR that cites the cool-down rule to dereference into a non-ADR file. Easier to keep ADR-internal cross-references inside the ADR set.
4. **Option D — Inline rules into the `write-adr` skill, no ADR.** Skills are how procedures are encoded; adding the cool-down step to `write-adr` is necessary anyway (it has been done in commit `56fd9eb`). But skills are agent-facing procedures; the *normative* statement of why those steps exist needs an ADR to cite. The skill update happens regardless; the ADR is the rationale.

## Decision outcome

**Chosen: Option A — three rules in their own ADR.**

The three rules below are normative for every ADR drafted from this ADR's Accept date forward. They do not retroactively apply to ADRs already Accepted (ADR-0001 through ADR-0024 stand on their original bodies; their riders, where they exist, were written before this ADR codified the rider format and are grandfathered).

### Rule 1 — ADR cool-down: no same-day Accept

An ADR drafted today does not move from `Proposed` to `Accepted` today. A minimum of one calendar day separates the two states; the maintainer (or a reviewing agent) re-reads the draft after sleeping on it, then accepts it.

This rule exists because four ADRs in the A → B0 arc (ADR-0021 mid-proposal revision, ADR-0021 post-Accept rider, ADR-0022 first rider, ADR-0022 first-rider sub-rider) had implementation-detected gaps that an extra day of reading would have caught at near-zero cost.

The cool-down does **not** apply to status flips that don't change content (e.g. fixing a typo, reformatting). It applies to "the decision the ADR records is final" transitions.

The cool-down also applies to **this** ADR. ADR-0025 is `Proposed` 2026-04-27; Accept happens 2026-04-28 at earliest, in a separate commit, after re-reading. ADR-0024 (proposed 2026-04-27, scheduled for Accept 2026-04-28) is the first ADR to use this rule; ADR-0025 itself is paired with it on the same Date — both Accept commits land on the same day in separate commits.

### Rule 2 — Forward-reference contract: every "future task" claim is grounded

If an ADR — in any section, including riders — states "task X will do Y", task X must be either:

- An existing T-NNN file (any status, including `Draft`), or
- Opened as part of the same commit that lands the ADR claim.

"Future, not-yet-opened task" wording is forbidden. The reason: forward-references that have no slot drift into purgatory. ADR-0022's first rider's claim "T-009 wires a timer IRQ" was wrong, in part, because no task file constrained T-009's actual scope at the moment the rider was written. The rider would have caught itself if the contract had required pointing at a real T-009 file.

When a future task genuinely cannot be opened yet (because its scope depends on something the maintainer has not decided), state that explicitly: *"see Open questions §X — task TBD pending decision Y"* — and the corresponding *Open questions* section must list the unresolved input. This is the only permitted form of un-grounded forward-reference, and it is paired with a visible "we know we don't know yet" marker.

### Rule 3 — Riders are not failures; their *frequency* is a signal

ADR riders — *Revision notes* entries appended after the original Accept, and Amendment blocks in the audit log — are valid records of learning. They are not failures of the ADR process; they are how implementation feedback enters the design history. Trying to eliminate them is overcorrection.

What *is* a signal is the **rate** of riders per ADR over time. An ADR that picks up 3+ riders in its first week of life indicates the original draft missed something structural. The rule is not "no riders"; it is "if rider rate climbs, audit the ADR-writing process, not the rider-writers."

Riders themselves are append-only by the same logic that makes the unsafe-log append-only ([`docs/standards/unsafe-policy.md §3`](../standards/unsafe-policy.md)): the original body stays intact; the rider explicitly states what it changes and why. In-place rewrites of an Accepted ADR's original body are forbidden — the same violation this ADR-0025 corrects in commit `56fd9eb`.

### Dependency chain

For the three rules above to be fully in effect:

1. **`write-adr` skill update** — already shipped in commit `56fd9eb` (the part that wasn't reverted). Procedure step 5 covers the dependency-chain requirement; step 10 covers cool-down; acceptance criteria gain the corresponding boxes; anti-patterns gain the corresponding entries.
2. **`docs/decisions/template.md` update** — already shipped in commit `56fd9eb`. Decision outcome gains a "Dependency chain" subsection.
3. **ADR-0024** (EL drop policy, currently `Proposed` per commit `0f970ea`) — first real test of the cool-down rule. Accept happens 2026-04-28 at earliest. If ADR-0024 lands clean (no riders within its first week), that is positive evidence that the rules work; if it picks up riders quickly, the rules need refining.
4. **No new task slot** — this ADR is normative-only; it does not require an implementation task. ADR-0024's Accept commit is the first observable event under the new rules.

## Consequences

### Positive

- **ADR-0013 stays Accepted with its original body intact.** The append-only invariant is preserved as the rule itself codifies.
- **The three rules are first-class and citable.** Future ADRs reference "ADR-0025 §Rule 1 (cool-down)" rather than "ADR-0013 §X" for content that wasn't in ADR-0013's accepted body.
- **The next ADR's Accept (ADR-0024) is the validation event.** If it lands clean, codified rules > rediscovered lessons.
- **The skill and template updates already shipped** in commit `56fd9eb`. Mechanically, the rules are already in force; this ADR is the missing rationale layer.

### Negative

- **One more ADR to read for the meta-process.** A returning maintainer needs to read both ADR-0013 and ADR-0025 to understand the planning-and-decision process. *Mitigation:* ADR-0013's pointer rider names ADR-0025 explicitly; the cross-reference is one click away.
- **Rules need maintenance over time.** If "3+ riders in a week" turns out to be the wrong threshold, this ADR needs a rider of its own (or a successor). *Mitigation:* per Rule 3, a rider on this ADR is normal; the rules are not pretending to be permanent.

### Neutral

- **Skill updates, template updates, and CLAUDE.md text** that reference "ADR-0013 §..." for the new rules now reference "ADR-0025 §...". One commit's churn during the rebuild from the revert; no recurring cost.
- **Existing Accepted ADRs are grandfathered.** ADR-0001 through ADR-0024's bodies remain as written. Riders on them follow the new rules from this ADR's Accept date forward.

## Pros and cons of the options

### Option A — Three rules in their own ADR (chosen)

- Pro: ADR-0013's body stays intact (append-only invariant honoured).
- Pro: Rules are first-class, citable, supersedable.
- Pro: ADR-0025 itself follows its own rules (cool-down active; dependency chain provided).
- Con: One more ADR to read. Mitigated by ADR-0013's pointer rider.

### Option B — Edit ADR-0013 in place

- Pro: Single ADR; no cross-reference.
- Con: Violates the append-only invariant ADR-0013 is meant to define. Self-contradictory.
- Con: Already attempted (commit `56fd9eb`) and reverted.

### Option C — Standards file instead of an ADR

- Pro: Standards are the natural home for process discipline.
- Con: Every ADR that cites the rules has to cross into `standards/`, breaking the "ADRs cite ADRs" pattern.
- Con: Standards files don't have the same review-and-Accept ritual; loses the "we considered alternatives" record.

### Option D — Skill-only update, no ADR

- Pro: Skills are the agent-facing procedure.
- Pro: The skill update is required anyway and has already shipped.
- Con: Skills are *how*, not *why*. Without an ADR to cite, future readers cannot find the rationale for why the skill says what it says.
- Con: Skill updates are not append-only; they evolve continuously. The rationale needs a stabler home.

## References

- [ADR-0013 — Roadmap and planning process](0013-roadmap-and-planning.md) — the parent ADR these rules amend.
- [`docs/standards/unsafe-policy.md §3`](../standards/unsafe-policy.md) — the audit-log append-only policy whose pattern the ADR rider rule mirrors.
- [`.claude/skills/write-adr/SKILL.md`](../../.claude/skills/write-adr/SKILL.md) — already updated in commit `56fd9eb` to encode the cool-down + dependency-chain procedure.
- [`docs/decisions/template.md`](template.md) — already updated in commit `56fd9eb` to include the "Dependency chain" subsection.
- [T-009 mini-retro](../analysis/reviews/business-reviews/2026-04-27-T-009-mini-retro.md) — the retrospective that produced the rules.
- [ADR-0021](0021-raw-pointer-scheduler-ipc-bridge.md) and [ADR-0022](0022-idle-task-and-typed-scheduler-deadlock.md) — the four-rider data points the cool-down rule was learned from.
- [ADR-0024](0024-el-drop-policy.md) — the first ADR to use the cool-down rule; its Accept (2026-04-28+) is the first validation event.
