---
name: perform-security-review
description: Run the dedicated security-review pass for a change that touches security-sensitive subsystems.
when-to-use: Whenever a PR touches capabilities, IPC, syscalls, memory management, scheduler, boot, cryptography, authentication boundaries, `unsafe` regions, or security-sensitive dependencies. Runs in addition to, not instead of, normal code review.
---

# Perform security review

## Inputs

- A **PR URL** or **branch / commit range** whose scope falls under the triggers in [security-review.md — Scope](../../../docs/standards/security-review.md).
- The PR description and any security-relevant context the author flagged.

## Procedure

1. **Confirm scope.** Re-read [security-review.md](../../../docs/standards/security-review.md)'s trigger list against the diff. If **none** of the triggers apply, this skill is not needed — return to the maintainer to confirm or drop the security-review requirement.

2. **Separate this pass from code review.** Do not combine them.
   - If you just finished [perform-code-review](../perform-code-review/SKILL.md) on this change, take a deliberate break (ideally hours, at minimum a full context switch) before starting this pass.
   - Open the security-review checklist fresh.

3. **Work the full checklist** from [security-review.md — Security-review checklist](../../../docs/standards/security-review.md). Every item gets an explicit outcome: `OK`, `flagged`, or `N/A` with one-sentence justification.

   Checklist areas (skill applies all of them when in scope):
   - Capability correctness.
   - Trust boundaries.
   - Memory safety.
   - Kernel-mode discipline.
   - Cryptography (when present).
   - Secrets and logging.
   - Dependencies.
   - Threat model impact.

4. **For each item, ask the adversarial question.** Do not just verify that the happy path works; verify that a **malicious caller cannot** abuse the path.
   - For capability checks: *Can I call this without the capability and get anything useful, including a side channel?*
   - For memory operations: *Can I make an aliasing mutable pointer? Can I observe uninitialized memory?*
   - For IPC: *Can the sender grant themselves or the receiver authority beyond what they held?*
   - For cryptography: *Is there a timing side channel? Is the nonce handling correct under a replay?*

5. **Cross-check against architectural principles** (see [architectural-principles.md](../../../docs/standards/architectural-principles.md)). A change that passes the checklist but violates P1 (no ambient authority), P3 (drivers in userspace), or P7 (no proprietary blobs) is not approved.

6. **Check the audit log** if the change introduces or modifies `unsafe`. Every `unsafe` change has a `UNSAFE-YYYY-NNNN` entry — see [justify-unsafe](../justify-unsafe/SKILL.md) skill. Reconcile the entry against the code.

7. **Decide.**
   - **Approve.** The change is safe to merge from a security perspective. Post a review comment with the completed checklist. Record a `Security-Review:` trailer (or instruct the author to add one) on the commit per [commit-style.md](../../../docs/standards/commit-style.md).
   - **Changes requested.** The checklist identified specific items that block approval. Each item is concrete and actionable.
   - **Escalate.** The change exposes an issue larger than the PR — e.g., a trust model gap for a subsystem. Open a tracking issue; hold the PR pending resolution.

8. **Record the outcome.**
   - Post the checklist on the PR as a review comment.
   - If the outcome is approve, the commit gets `Security-Review: @<reviewer>`.
   - If the outcome is changes-requested or escalated, the tracking issue links back to the PR.

## Acceptance criteria

- [ ] Separate pass, not combined with code review.
- [ ] Every applicable checklist item worked with explicit OK / flagged / N/A.
- [ ] Adversarial question posed for each item (what could a malicious caller do?).
- [ ] Cross-check against architectural principles done.
- [ ] `unsafe` audit log reconciled if applicable.
- [ ] Outcome posted on PR as a structured comment.
- [ ] `Security-Review:` trailer recorded if the outcome is approve.

## Anti-patterns

- **Combining code and security review into one pass.** The whole point of the separate pass is fresh eyes.
- **Approving because the change "looks small".** Small changes can have large security effects.
- **Waiving a checklist item without saying why.** Every N/A has a justification.
- **Accepting "I tested it" as a security argument.** Testing finds bugs, not absences of bugs.
- **Stopping at the happy path.** Security is about the malicious path.
- **Not recording the outcome.** An unrecorded security review did not happen for audit purposes.

## References

- [security-review.md](../../../docs/standards/security-review.md) — the standard, including the full checklist.
- [architectural-principles.md](../../../docs/standards/architectural-principles.md) — P1, P2, P3, P4, P7 are especially relevant.
- [unsafe-policy.md](../../../docs/standards/unsafe-policy.md) — for `unsafe` review details.
- [justify-unsafe](../justify-unsafe/SKILL.md) — the author-side companion to this skill.
- [commit-style.md](../../../docs/standards/commit-style.md) — `Security-Review:` trailer.
