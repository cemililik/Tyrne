---
name: perform-code-review
description: Run a structured code-review pass on a pull request or branch, applying the Tyrne code-review standard.
when-to-use: Whenever the maintainer asks for a review of a PR or branch, or when an agent opens its own work for self-review before committing.
---

# Perform code review

## Inputs

- A **PR URL** or a **branch / commit range** to review.
- The PR description, if any.

## Procedure

1. **Read the PR description first.** Understand the stated intent before looking at the code. If the description is missing or vague, ask the author to write one.

2. **Identify risk class.**
   - Does the change touch any of the subsystems listed under [security-review.md — Scope](../../../docs/standards/security-review.md)?
     - Capabilities, IPC, syscalls, memory management, scheduler, boot, cryptography, authentication, `unsafe` regions, security-sensitive dependencies.
   - If **yes**, this skill alone is not sufficient — also run [perform-security-review](../perform-security-review/SKILL.md) as a separate pass.

3. **Read the tests first.**
   - Tests tell you what the author *thinks* the behavior is. Read them before the implementation.
   - Ask: are the tests covering the contract, or just the implementation? Would they catch a regression if someone refactored?
   - Flag missing test categories (per [testing.md](../../../docs/standards/testing.md)): public API tests, regression tests for fixes, error-path tests for new `Error` variants, QEMU smoke tests for new syscalls.

4. **Read the diff in topological order.**
   - New types → their impls → callers → tests.
   - Do not read files in the order GitHub presents them; read by dependency.

5. **Apply the checklist** from [code-review.md — Review checklist](../../../docs/standards/code-review.md). Work through every item:
   - Correctness (does it do what the description says; off-by-ones; missing error handling).
   - Capability semantics (trust-boundary checks in the right place).
   - Fault containment (can userspace faults reach kernel through this path).
   - `unsafe` discipline (every new `unsafe` meets [unsafe-policy.md](../../../docs/standards/unsafe-policy.md); defer to [justify-unsafe](../justify-unsafe/SKILL.md)).
   - Error handling (Result propagation, no new `unwrap`/`expect` on hot paths, errors converted at boundaries per [error-handling.md](../../../docs/standards/error-handling.md)).
   - Documentation (every new public item documented; `unsafe fn` has `# Safety`; ADR references where relevant).
   - Architectural principles ([architectural-principles.md](../../../docs/standards/architectural-principles.md) P1–P12).
   - Dependencies (new crates must have gone through [add-dependency](../add-dependency/SKILL.md)).
   - Commit messages ([commit-style.md](../../../docs/standards/commit-style.md): conventional format, trailers present, `Refs: ADR-NNNN` where applicable).

6. **Run it.** For non-trivial PRs, build locally and — if the change has a behavioral effect — run the QEMU smoke suite. CI is necessary, not always sufficient.

7. **Do not spend energy on:**
   - rustfmt output (CI handles it).
   - Clippy-catchable issues (CI handles it).
   - Preference bikeshedding ("I would have named it differently but yours is fine").
   - Out-of-scope refactor requests.

8. **Post the review.**
   - **Approve** if the change is ready; minor suggestions can remain as optional comments.
   - **Request changes** with specific, actionable items if something must be fixed before merge. "I'm uneasy" is not actionable; convert discomfort into a concrete item or escalate.
   - **Comment** without verdict if the review is partial or if you want discussion before committing to approve/reject.

9. **Record security-review outcome** (if applicable). A security-sensitive change must also have [perform-security-review](../perform-security-review/SKILL.md) executed; its outcome is a separate comment and a `Security-Review:` trailer on the commit.

## Acceptance criteria

- [ ] PR description read and understood.
- [ ] Risk class identified; security review path taken if applicable.
- [ ] Tests read before implementation.
- [ ] Full [code-review.md](../../../docs/standards/code-review.md) checklist worked.
- [ ] Each checklist item either confirmed OK or called out with a specific comment.
- [ ] A verdict posted (Approve / Request changes / Comment).

## Anti-patterns

- **LGTM without reading the code.** Do not approve to "unblock" the author.
- **Re-litigating style the tools already caught.**
- **Scope creep** ("while you're here, also refactor X").
- **Vague request-changes.** "Something feels off" is not a review.
- **Skipping tests.** An implementation review without a tests review is half a review.
- **Merging a security-sensitive change** without the second [perform-security-review](../perform-security-review/SKILL.md) pass.

## References

- [code-review.md](../../../docs/standards/code-review.md) — the standard.
- [security-review.md](../../../docs/standards/security-review.md) — when this skill alone is not enough.
- [testing.md](../../../docs/standards/testing.md) — what counts as a real test.
- [architectural-principles.md](../../../docs/standards/architectural-principles.md) — P1–P12.
- [commit-style.md](../../../docs/standards/commit-style.md) — commit format.
