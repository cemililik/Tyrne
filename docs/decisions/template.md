# NNNN — <Title>

- **Status:** Proposed
- **Date:** YYYY-MM-DD
- **Deciders:** @cemililik

<!--
Status enum (use one):
  - Proposed                — drafted, awaiting Accept after careful re-read.
  - Accepted                — settled; the project follows this decision.
  - Deferred                — recognised as needed but explicitly postponed; no file body required if filed-but-deferred (see ADR-0018, ADR-0023).
  - Deprecated              — historical; followed for a time but no longer.
  - Superseded by NNNN      — overridden by a later ADR; old body preserved for the historical record (per supersede-adr skill).
-->


## Context

<What is the situation, the problem, or the question? What constraints apply? What are the stakes of getting this wrong?>

## Decision drivers

- <Driver 1 — what force pushes us toward one answer or away from another?>
- <Driver 2>
- <Driver 3>

## Considered options

1. **<Option A>** — one-sentence description.
2. **<Option B>** — one-sentence description.
3. **<Option C>** — one-sentence description.

## Decision outcome

Chosen option: **<Option X>**.

<Why this one. Usually a paragraph or two connecting the decision drivers to the chosen option.>

### Simulation

<For multi-step state-machine ADRs (capability flows, IPC handshakes,
scheduler dispatch, exception entry, MMU / TLB transitions, syscall ABI
handshakes, etc.) include a 3–5 row table walking the worst-case
interaction through the proposed shape. Each row records `(state-pre,
action, state-post, switch target / observable effect)`. This subsection
exists to prevent the failure mode the 2026-05-06 B1 smoke regression
surfaced: ADR-0022 §Decision outcome chose its option on prose-only
reasoning ("yield_now's only-one-ready early-return handles the case")
that was correct only when one task was Ready; the demo's three-task
moment broke the assumption and the kernel hung in WFI. ADR-0026
§Decision outcome shows the shape in production. Codified after the
[2026-05-07 B1 closure retro §"What we learned"](../analysis/reviews/business-reviews/2026-05-07-B1-closure.md).

For ADRs whose subject is *not* a multi-step state machine (process /
governance / dependency policy / single-decision shape), this subsection
is omitted with a one-line note ("Not applicable — this ADR settles a
single-shape decision; no state-machine to simulate.").>

### Dependency chain

<List every task / piece of infrastructure / prior decision that must already
exist for this ADR's chosen option to be **fully** in effect, in
implementation order. Each line either points at an existing T-NNN file or
flags it as a gap that must be opened before the ADR claims its full benefit.
This subsection exists to prevent the failure mode the A → B0 arc rediscovered
four times: an ADR's "future task X will do Y" handwave going unverified until
implementation surfaces the gap (see ADR-0025 §Rule 1 (forward-reference contract)).>

Example (placeholders only — replace with the real T-NNNs your decision names):
```text
For this decision to be fully in effect:
1. <Subsystem A read path> — T-NNN (Status)
2. <Subsystem B programming step> — T-NNN (Status)
3. <Driver C configuration> — T-NNN
4. <Vector D install> — T-NNN

The first task closes only step 1. Remaining steps are scoped under
the second task, opened in the same commit as this ADR if it does
not yet exist.
```

If a step has no T-NNN slot, the ADR cannot Accept until one is opened (per
ADR-0025 §Rule 1 (forward-reference contract)).

## Consequences

### Positive

- <Benefit 1>
- <Benefit 2>

### Negative

- <Cost or risk 1 — and how we plan to live with or mitigate it>
- <Cost or risk 2>

### Neutral

- <Side effect that is neither a clear benefit nor a clear cost>

## Pros and cons of the options

### Option A

- Pro: …
- Con: …

### Option B

- Pro: …
- Con: …

### Option C

- Pro: …
- Con: …

## References

- <Link 1>
- <Link 2>
