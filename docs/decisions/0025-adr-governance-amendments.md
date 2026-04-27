# 0025 — ADR governance amendments: forward-reference contract, rider hygiene

- **Status:** Accepted
- **Date:** 2026-04-27
- **Deciders:** @cemililik

## Context

[ADR-0013](0013-roadmap-and-planning.md) settled the project's roadmap-and-planning process on 2026-04-20. Its §"Integration with ADRs" said: *"The roadmap does not replace ADRs. It sequences them. A task may require an ADR as an acceptance criterion; an ADR may result in new tasks being added."* That single paragraph was the entire normative content for how ADRs interact with the planning process.

The Phase A → B0 implementation arc — six days of work spanning T-006 / T-007 / T-009 — produced four ADRs (ADR-0021, ADR-0022) that needed post-Accept riders within their first week, plus one (ADR-0021) that needed a mid-proposal revision before Accept. Each rider's content traced back to one of two implicit rules that ADR-0013's framing had not made explicit:

1. **Forward-references that don't ground at a real T-NNN drift into purgatory.** ADR-0022's first rider claimed "T-009 wires a timer IRQ" without T-009 having a task file constraining its scope. The implementation discovered the conflation; a sub-rider was needed to disambiguate.
2. **Riders themselves get treated as failures.** When a third rider appears, the temptation is to rush the next ADR to "get it right this time" — which produces *more* riders, not fewer. The signal is the *rate* of riders, not their presence.

(A third rule — a 24-hour ADR cool-down between `Proposed` and `Accepted` — was drafted alongside these but withdrawn before Accept on maintainer feedback. See §Revision notes for the full reasoning. The substance the cool-down was meant to enforce — careful re-reading before Accept — remains a write-adr-skill responsibility, just without the enforced calendar-day delay.)

ADR-0013 was edited in-place on 2026-04-27 (commit `56fd9eb`) to add three subsections codifying these rules. That edit was itself an append-only-policy violation: ADR-0013 was already `Accepted`, and the new content rewrote its body rather than appending. The second-read review surfaced the contradiction (ADR-0013 was the document defining the append-only rule it was being edited in violation of). This ADR-0025 is the correction: extract the rules into a new ADR that stands on its own, leave ADR-0013's body intact except for a single rider/pointer.

## Decision drivers

- **Honour the append-only invariant.** ADR-0013 cannot be edited in place once Accepted. The rules need their own ADR to exist as a first-class decision, citable and revisable on its own terms.
- **Make the rules followable mechanically.** "Every forward-reference must point at a real T-NNN file" is a rule the maintainer (or an agent) can mechanically apply. So is "rider count > N is a signal". Lessons-as-prose got rediscovered four times across A → B0; lessons-as-rules need to fire on the next ADR.
- **Substance over ceremony.** Initial drafts of this ADR included a hard 24-hour cool-down between `Proposed` and `Accepted`. The maintainer judged that the *substance* the cool-down enforced (careful re-reading) is achievable through the existing review skills (write-adr's careful-review step, independent agent reviews, the dependency-chain section's forced upfront thinking) without a fixed calendar delay. Withdrawn before Accept; see §Revision notes.
- **Do not over-correct.** Riders are how implementation feedback enters the design record. Trying to eliminate them is the wrong target. The rules name what is acceptable (riders, dated and append-only) and what is not (in-place body rewrites, ungrounded forward-references).
- **Compatible with single-author + AI-agent reality.** Tyrne is solo development with AI-agent assistance. The rules do not assume a multi-person review board. They assume that the maintainer plus a reviewing agent (manually or via a skill) is the review surface — which means the rules must be cheap, mechanical, and self-checkable.

## Considered options

1. **Option A — Two rules in their own ADR (this ADR-0025; chosen).** ADR-0013 stays Accepted, body intact, with a single pointer rider. New rules are first-class, citable, supersedable on their own terms.
2. **Option B — Edit ADR-0013 in place.** What was attempted in commit `56fd9eb`; rejected because it violates the append-only invariant ADR-0013 is meant to define. Already reverted.
3. **Option C — `propose-standard-change` skill instead of an ADR.** The rules are arguably standards (process discipline), not architectural decisions. Could go in `docs/standards/adr-governance.md` instead. Rejected because the rules are *about ADRs* and need to be cited from inside ADR text — having them in a `standards/` file requires every ADR that cites them to dereference into a non-ADR file. Easier to keep ADR-internal cross-references inside the ADR set.
4. **Option D — Inline rules into the `write-adr` skill, no ADR.** Skills are how procedures are encoded; the skill update was necessary anyway (shipped in `56fd9eb`). But skills are agent-facing procedures; the *normative* statement of why those steps exist needs an ADR to cite. The skill update happens regardless; the ADR is the rationale.

## Decision outcome

**Chosen: Option A — two rules in their own ADR.**

The two rules below are normative for every ADR drafted from this ADR's Accept date forward. They do not retroactively apply to ADRs already Accepted (ADR-0001 through ADR-0024 stand on their original bodies; their riders, where they exist, were written before this ADR codified the rider format and are grandfathered).

### Rule 1 — Forward-reference contract: every "future task" claim is grounded

If an ADR — in any section, including riders — states "task X will do Y", task X must be either:

- An existing T-NNN file (any status, including `Draft`), or
- Opened as part of the same commit that lands the ADR claim.

"Future, not-yet-opened task" wording is forbidden. The reason: forward-references that have no slot drift into purgatory. ADR-0022's first rider's claim "T-009 wires a timer IRQ" was wrong, in part, because no task file constrained T-009's actual scope at the moment the rider was written. The rider would have caught itself if the contract had required pointing at a real T-009 file.

When a future task genuinely cannot be opened yet (because its scope depends on something the maintainer has not decided), state that explicitly: *"see Open questions §X — task TBD pending decision Y"* — and the corresponding *Open questions* section must list the unresolved input. This is the only permitted form of un-grounded forward-reference, and it is paired with a visible "we know we don't know yet" marker.

### Rule 2 — Riders are not failures; their *frequency* is a signal

ADR riders — *Revision notes* entries appended after the original Accept, and Amendment blocks in the audit log — are valid records of learning. They are not failures of the ADR process; they are how implementation feedback enters the design history. Trying to eliminate them is overcorrection.

What *is* a signal is the **rate** of riders per ADR over time. An ADR that picks up 3+ riders in its first week of life indicates the original draft missed something structural. The rule is not "no riders"; it is "if rider rate climbs, audit the ADR-writing process, not the rider-writers."

Riders themselves are append-only by the same logic that makes the unsafe-log append-only ([`docs/standards/unsafe-policy.md §3`](../standards/unsafe-policy.md)): the original body stays intact; the rider explicitly states what it changes and why. In-place rewrites of an Accepted ADR's original body are forbidden — the same violation this ADR-0025 corrects in commit `56fd9eb`.

### Dependency chain

For the two rules above to be fully in effect:

1. **`write-adr` skill update** — shipped in commit `56fd9eb` (Dependency-chain procedure step) with a follow-up edit removing the cool-down step (see §Revision notes). The careful-review step replaces the cool-down's behavioural intent without imposing a calendar delay.
2. **`docs/decisions/template.md` update** — shipped in commit `56fd9eb`. Decision outcome gains a "Dependency chain" subsection.
3. **ADR-0024** (EL drop policy) — first ADR to use the dependency-chain section in production. Its `Proposed → Accepted` arc on 2026-04-27 is the first observable event under the new rules.
4. **No new task slot** — this ADR is normative-only; it does not require an implementation task.

## Consequences

### Positive

- **ADR-0013 stays Accepted with its original body intact.** The append-only invariant is preserved as the rule itself codifies.
- **The two rules are first-class and citable.** Future ADRs reference "ADR-0025 §Rule 1 (forward-reference contract)" rather than "ADR-0013 §X" for content that wasn't in ADR-0013's accepted body.
- **ADR-0024 is the first validation event.** It uses the dependency-chain section in production; if it lands without riders within a week, codified rules > rediscovered lessons.
- **The skill and template updates already shipped** in commit `56fd9eb` (with the cool-down step removed in the follow-up). Mechanically, the rules are in force.
- **Substance over ceremony.** The withdrawn cool-down rule was the heaviest of the three drafts; dropping it before Accept means the surviving rules are all mechanical and self-checkable, not calendar-gated.

### Negative

- **One more ADR to read for the meta-process.** A returning maintainer needs to read both ADR-0013 and ADR-0025 to understand the planning-and-decision process. *Mitigation:* ADR-0013's pointer rider names ADR-0025 explicitly; the cross-reference is one click away.
- **Rules need maintenance over time.** If "3+ riders in a week" turns out to be the wrong threshold, this ADR needs a rider of its own (or a successor). *Mitigation:* per Rule 2, a rider on this ADR is normal; the rules are not pretending to be permanent.
- **Carefulness becomes a soft commitment, not a calendar gate.** Without the cool-down, the careful-re-read step in write-adr depends on the maintainer (or agent) actually being deliberate. *Mitigation:* the dependency-chain section forces upfront thinking before Accept; the rider-frequency signal in Rule 2 catches the case where carefulness slipped.

### Neutral

- **Skill updates, template updates, and CLAUDE.md text** that reference "ADR-0013 §..." for the new rules now reference "ADR-0025 §...". One commit's churn during the rebuild from the revert; no recurring cost.
- **Existing Accepted ADRs are grandfathered.** ADR-0001 through ADR-0024's bodies remain as written. Riders on them follow the new rules from this ADR's Accept date forward.

## Pros and cons of the options

### Option A — Two rules in their own ADR (chosen)

- Pro: ADR-0013's body stays intact (append-only invariant honoured).
- Pro: Rules are first-class, citable, supersedable.
- Pro: ADR-0025 itself follows its own rules (dependency chain provided; forward-references grounded).
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

## Revision notes

- **2026-04-27 — Pre-Accept revision: cool-down rule withdrawn.** The first draft of this ADR included a third rule, "ADR cool-down: no same-day Accept", which would have required ≥ 1 calendar day between `Proposed` and `Accepted` for every ADR. After re-reading the draft alongside ADR-0024 (the first ADR slated to use the rule), the maintainer judged the cool-down disproportionate to its benefit in the project's single-author + AI-agent context: the substance the cool-down enforced (a careful, deliberate re-read before Accept) is achievable through the existing review surface (the write-adr skill's careful-review step, independent agent reviews, the dependency-chain section's forced upfront thinking), without imposing a calendar delay that doubles the wall-clock cost of every decision.
  
  Effects of the withdrawal, applied in the same commit that lands this revision:
  - The Decision-outcome §Rule 1 (cool-down) section is removed; what was §Rule 2 becomes §Rule 1, what was §Rule 3 becomes §Rule 2.
  - The write-adr skill's cool-down step (step 10 in the post-`56fd9eb` form) is removed; its acceptance-criterion box for "Status lands at Proposed, not Accepted" and its anti-pattern entry "Same-day Proposed → Accepted" are removed.
  - All cross-references in T-009 / T-012 / T-013 / ADR-0024 / ADR-0022 / phase-b.md / current.md / mini-retro that pointed at "ADR-0025 §Rule 2" are renumbered to "§Rule 1"; references to "§Rule 3" become "§Rule 2"; references to "§Rule 1 (cool-down)" are removed (along with the "Accepted ≥ 2026-04-28" wording the cool-down imposed on ADR-0024).
  - ADR-0024 and ADR-0025 are accepted on the same day (2026-04-27) in separate commits, no longer gated on a 24-hour delay.
  
  This withdrawal is logged here rather than in a §"Revision notes" rider on a future commit because the rule is being removed *before* Accept — the historical record of the rule's existence + reason for withdrawal is the value, not the rule itself. Per the project's rider conventions ([ADR-0021](0021-raw-pointer-scheduler-ipc-bridge.md) §Revision notes uses the same pattern), pre-Accept revisions are recorded inline in the §Revision notes section of the same ADR.

## References

- [ADR-0013 — Roadmap and planning process](0013-roadmap-and-planning.md) — the parent ADR these rules amend.
- [`docs/standards/unsafe-policy.md §3`](../standards/unsafe-policy.md) — the audit-log append-only policy whose pattern the ADR rider rule mirrors.
- [`.claude/skills/write-adr/SKILL.md`](../../.claude/skills/write-adr/SKILL.md) — updated in commit `56fd9eb` to encode the dependency-chain procedure; cool-down step removed in the follow-up.
- [`docs/decisions/template.md`](template.md) — updated in commit `56fd9eb` to include the "Dependency chain" subsection.
- [T-009 mini-retro](../analysis/reviews/business-reviews/2026-04-27-T-009-mini-retro.md) — the retrospective that produced the rules.
- [ADR-0021](0021-raw-pointer-scheduler-ipc-bridge.md) and [ADR-0022](0022-idle-task-and-typed-scheduler-deadlock.md) — the four-rider data points that motivated all three drafts (including the withdrawn cool-down).
- [ADR-0024](0024-el-drop-policy.md) — the first ADR to use the dependency-chain section in production; its Accept event is the first observable validation under the new rules.
