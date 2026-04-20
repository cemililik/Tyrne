# Security review master plan

A security review is an adversarial pass. For every property the change might preserve, violate, or reveal, the reviewer asks: *what can a malicious caller do with this?* The review succeeds when every applicable axis has been examined and either cleared or explicitly flagged.

This plan is written to be parallelizable across multiple agents — up to eight roles, one per axis — but it is equally valid as a single-agent sequential walkthrough. The value of separating the axes is that each one is a focused checklist that does not lose state under context switch.

## Inputs

- The **change under review** — PR URL, commit range, or branch.
- The PR / change **description**, with security context flagged by the author.
- Access to: the source, the affected ADRs, [`../../../architecture/security-model.md`](../../../architecture/security-model.md), the [`../../audits/unsafe-log.md`](../../audits/unsafe-log.md), and any prior security reviews on the same subsystem.

## Output

A file at `docs/analysis/reviews/security-reviews/YYYY-MM-DD-<context>.md` following the shape below. Added to the index in this folder's [`README.md`](README.md). The corresponding commit's trailer is `Security-Review: <reviewer>` per [`commit-style.md`](../../../standards/commit-style.md); when the trailer convention matures, it may also cite the artifact path.

## Pre-flight: scope check

Confirm the change actually triggers a security review per [`../../../standards/security-review.md`](../../../standards/security-review.md). If **none** of the triggers apply, this review is not required and the maintainer or author is informed.

## Pre-flight: separation of passes

This review is performed **after** the code review, with a deliberate context switch (ideally hours; at minimum a full mental reset). The review starts with an empty checklist, not a continuation of the code review's findings.

## Agent roles

Each role produces a section of the final artifact. Within each role, every item receives one of three outcomes: **OK**, **flagged**, or **N/A** (with one-sentence justification).

### 1. Capability correctness

**Adversarial question per item:** can a caller perform this privileged action *without* holding the capability it should require?

- Every privileged operation introduced by the change requires a capability.
- The required capability is the narrowest sufficient one.
- Capability checks happen *before* observable side effects. A failed check leaks no state.
- Capability transfer is move-only; no accidental cloning.
- Capability duplication requires an explicit `Duplicate` authority.
- Capability revocation takes effect atomically.

### 2. Trust boundaries

**Adversarial question:** at each trust boundary the change crosses, what happens when untrusted input crosses it?

- Every input crossing userspace → kernel is validated before use.
- Userspace pointers are never dereferenced in kernel mode outside validated mappings.
- Buffer lengths from userspace are range-checked.
- Message contents from userspace are parsed into typed structures.
- Cross-task IPC does not grant authority the sender did not hold.

### 3. Memory safety

**Adversarial question:** can the change introduce a memory-safety violation?

- Any new `unsafe` meets [`unsafe-policy.md`](../../../standards/unsafe-policy.md): `SAFETY:` comment, audit entry, security-reviewer sign-off.
- Invariants in `# Safety` sections hold for every caller, not just the obvious one.
- No uninitialized memory exposed.
- No use-after-free; lifetimes on raw pointers reasoned about.
- No aliasing violations.

### 4. Kernel-mode discipline

**Adversarial question:** can the change stall, panic, or deadlock the kernel?

- No allocation in interrupt service routines.
- No unbounded loops in kernel mode.
- Critical sections are minimized.
- No new kernel panic on a hot path.
- Every allocation path returns a typed error, not a panic, on exhaustion.

### 5. Cryptography (when applicable)

**Adversarial question:** can an attacker exploit an algorithmic, implementation, or side-channel weakness?

- No roll-your-own primitives.
- Keys are never logged, never in error messages, never in `Debug` output.
- Constant-time comparisons where timing leaks matter.
- Randomness from an acceptable source.
- Nonces / IVs / salts handled per the primitive's contract.

### 6. Secrets and logging

**Adversarial question:** can the change leak a secret through a diagnostic channel?

- Secrets (keys, tokens, capability bits) never appear in logs, panic output, debug prints, or error types.
- `Debug` impls on security-sensitive types redact or are absent.

### 7. Dependencies

**Adversarial question:** does the change pull in trust that we have not earned?

- Any new dependency went through [`add-dependency`](../../../../.claude/skills/add-dependency/SKILL.md).
- The dependency's trust category is understood; build-time-only is very different from kernel-linked.
- `cargo-vet` trust decisions are updated.

### 8. Threat-model impact

**Adversarial question:** does the change reshape what the system defends against?

- Change is reconciled with [`security-model.md`](../../../architecture/security-model.md).
- If the change shifts the threat model, the update is in flight or linked as a follow-up task.

## Merge step

Combine the role outputs into a single artifact. The verdict is computed from them:

- **Approve** — every applicable axis returned OK; flagged items are all minor non-blocking.
- **Changes requested** — one or more axes returned a blocking `flagged` outcome. Each is specific and actionable.
- **Escalate** — the review surfaces an issue larger than this change (e.g., a trust-model gap that the subsystem exposes). A tracking task is opened via [`start-task`](../../../../.claude/skills/start-task/SKILL.md).

The verdict is propagated to the corresponding code-review artifact as a cross-reference.

## Acceptance criteria

- [ ] File at `docs/analysis/reviews/security-reviews/YYYY-MM-DD-<context>.md`.
- [ ] Frontmatter (change id, reviewer, date, separation-from-code-review note) filled.
- [ ] All eight sections present; each applicable item has OK / flagged / N/A with a justification.
- [ ] Verdict (Approve / Changes requested / Escalate) stated.
- [ ] `unsafe` audit-log cross-reference if any `unsafe` changed.
- [ ] Index in [`README.md`](README.md) updated.
- [ ] Commit trailer `Security-Review:` set on the approved change.

## Output template

```markdown
# Security review YYYY-MM-DD — <change identifier>

- **Change:** <PR URL / branch / commit range>
- **Reviewer:** @cemililik (+ any AI agent acting in the axes below)
- **Separation from code review:** <time gap or context-switch summary>
- **Unsafe audit cross-reference:** <UNSAFE-YYYY-NNNN list or "n/a">

## 1. Capability correctness

<findings per item: OK | flagged | N/A + justification>

## 2. Trust boundaries

<findings>

## 3. Memory safety

<findings>

## 4. Kernel-mode discipline

<findings>

## 5. Cryptography

<findings or "N/A — no crypto in this change">

## 6. Secrets and logging

<findings>

## 7. Dependencies

<findings>

## 8. Threat-model impact

<findings>

## Verdict

**Approve | Changes requested | Escalate**

<If Changes requested: list blockers. If Escalate: name the follow-up task.>
```

## Anti-patterns

- **Combining code and security review into one pass.** The separation is the point.
- **Waiving a checklist item without saying why.** Every N/A has a justification.
- **Approving because the change "looks small."** Small changes have large security effects regularly.
- **"Tested it" as a security argument.** Testing finds bugs, not absences of bugs.
- **Stopping at the happy path.** The review is about the malicious path.
- **Unrecorded outcome.** An unrecorded security review did not happen.

## Amendments

- _2026-04-20_ — initial version; derived from [`security-review.md`](../../../standards/security-review.md) standard; scaffolded by [ADR-0013](../../../decisions/0013-roadmap-and-planning.md).
