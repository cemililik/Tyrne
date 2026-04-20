# Security review

A dedicated review discipline for changes that touch security-sensitive subsystems. This standard defines what triggers a security review, who performs it, what they check, and how the outcome is recorded.

It extends — it does not replace — the ordinary code review process in [code-review.md](code-review.md). Every security-reviewed change still goes through normal review.

## Scope

Security review is **mandatory** for any change that touches:

- **Capabilities.** Capability types, the capability table, capability transfer across IPC, capability derivation, capability revocation.
- **IPC.** Message format, endpoint objects, send/receive entry points, buffer handling.
- **Syscalls.** Addition of a syscall, change to an existing syscall's signature or authority requirements.
- **Memory management.** Page tables, the MMU interface, physical allocator, virtual allocator, TLB invalidation, address-space construction / destruction.
- **Scheduler.** Priority, preemption, critical sections.
- **Boot.** Anything from reset vector through the point at which the first userspace task has been created.
- **Cryptography.** Any primitive (hash, cipher, signature, key derivation, random), any protocol, any key handling.
- **Authentication / authorization boundaries.** Service mutual authentication (eventually).
- **`unsafe` regions.** Any introduction, modification, or broadening of `unsafe` code.
- **Dependencies.** Adding a new dependency that touches any of the above, or upgrading one in a way that changes behavior.

A PR author declares security relevance in the PR description. A reviewer who spots security relevance the author missed escalates immediately.

## Who performs a security review

- **Phase-current rule (solo phase):** the maintainer performs the security-review pass as a **distinct** pass, separate in time (at least several hours, ideally a day) from the initial implementation and from the ordinary self-review. The separation is the discipline.
- **Phase-future rule (multi-contributor):** security-sensitive PRs require a second reviewer explicitly designated as the security reviewer. That reviewer's approval is in addition to the ordinary reviewer's approval.

The security reviewer is named in the PR commit's `Security-Review:` trailer (see [commit-style.md](commit-style.md)).

## Security-review checklist

The security reviewer works through every applicable item. "Not applicable" is a valid answer that must be justified briefly in the review comment.

### Capability correctness

- [ ] Every privileged operation introduced by this change requires a capability.
- [ ] The capability required is the **narrowest** one that permits the operation (no privilege escalation by accepting a broad capability where a narrow one would do).
- [ ] Capabilities are checked **before** observable side effects. A failed check does not leak information that the call was attempted.
- [ ] Capability transfer (if any) is move-only; no accidental cloning.
- [ ] Capability duplication (if intended) itself requires an explicit `Duplicate`-style authority.
- [ ] Capability revocation (if applicable) takes effect atomically; no partial state where the revocation is half-applied.

### Trust boundaries

- [ ] Every input crossing from userspace to the kernel is validated before use.
- [ ] Pointers from userspace are **never** dereferenced in kernel mode without going through a validated mapping.
- [ ] Buffer lengths provided by userspace are range-checked against the actual mapped region.
- [ ] Message contents from userspace are parsed into typed structures; no raw bytes are consumed in privileged code.
- [ ] Cross-task IPC does not grant the receiver any authority the sender did not have or did not explicitly transfer.

### Memory safety

- [ ] Any new `unsafe` block meets [unsafe-policy.md](unsafe-policy.md).
- [ ] Invariants stated in `# Safety` sections hold for every call site, not only "the usual" one.
- [ ] No uninitialized memory is exposed. Where a buffer is declared but only partially filled, the unused portion is zeroed or the buffer is returned as `MaybeUninit`.
- [ ] No use-after-free. Lifetimes on raw pointers are reasoned about explicitly.
- [ ] No aliasing violations. Mutable pointers to the same memory do not coexist.

### Kernel-mode discipline

- [ ] No allocation in interrupt service routines.
- [ ] No unbounded loops in kernel mode; every loop has a documented termination.
- [ ] Critical sections are minimized. Scheduler state is held for as short a time as possible.
- [ ] No new kernel panic introduced on a hot path.

### Cryptography (when present)

- [ ] No roll-your-own primitives. If a primitive is introduced, it is a well-known algorithm from a reviewed crate (or, if truly novel, it comes with a separate ADR and a cited proof or analysis).
- [ ] Keys are never logged, returned in error messages, or exposed via `Debug`.
- [ ] Constant-time comparisons are used where timing leaks matter.
- [ ] Randomness comes from an acceptable source — not `rand::thread_rng` with no seeding story.
- [ ] Nonces, IVs, and salts are handled correctly per the primitive's contract.

### Secrets and logging

- [ ] Secrets (keys, tokens, capability bits) are not included in logs, panic messages, debug output, or error types.
- [ ] `Debug` impls on security-sensitive types redact sensitive fields (or are not implemented at all).

### Dependencies

- [ ] Any new dependency has been evaluated under [infrastructure.md](infrastructure.md) dependency-addition rules.
- [ ] The dependency's trust model is understood. A build-time-only crate is very different from a kernel-linked crate.
- [ ] `cargo-vet` trust decisions are updated accordingly.

### Threat model impact

- [ ] The change is reconciled with the documented threat model (once `docs/architecture/security-model.md` exists — Phase 3).
- [ ] If the change reshapes the threat model, the PR includes the threat-model update or links to a follow-up PR scheduled imminently.

## Outcome

Every security review results in one of:

1. **Approved.** The security reviewer signs off explicitly, in a review comment that references the checklist outcome. The commit's `Security-Review:` trailer is populated.
2. **Changes requested.** The reviewer enumerates the specific items that block approval. Vague "I'm uneasy" is not a valid outcome; discomfort is elaborated into a concrete item or escalated into an ADR discussion.
3. **Escalated.** The reviewer identifies an issue whose resolution exceeds the PR — e.g. the trust model for a subsystem needs rework. The PR is held, a tracking issue is opened, and progress is made against that issue before the PR can proceed.

## Timing

- Security review is a **separate pass**, not a read-through of the code review checklist with a different mindset.
- The reviewer takes notes as they go. A completed checklist is the deliverable, attached to the PR as a comment.
- Security review time is not a code-review slowdown by default; a routine security change can be reviewed in an hour. A complex change (new syscall family, new IPC primitive) takes longer and is budgeted accordingly.

## Records

- Every security-reviewed change leaves a trail:
  - `Security-Review:` trailer in the commit.
  - Review comment with the checklist on the PR.
  - For changes that introduce or modify `unsafe`, the audit log entry per [unsafe-policy.md](unsafe-policy.md).
- Security advisories (if any emerge post-merge) cross-reference the review that approved the change. This is the project's internal feedback loop.

## Anti-patterns to reject

- Treating "it compiled and tests passed" as a security argument.
- Accepting "trust me" in a `SAFETY:` comment or a review reply.
- Approving a security-sensitive change as a courtesy to unblock the author.
- Waiving a checklist item without saying why it does not apply.
- Using the same pass for code review and security review — context collapse defeats the point.

## Tooling

- Manual checklist, applied per PR.
- `cargo-audit` — known vulnerabilities in dependencies (CI gate).
- `cargo-vet` — audited dependencies (CI gate).
- `cargo-geiger` — `unsafe` accounting (periodic).
- `miri` on host-runnable subsets, where practical.
- Static analysis for cryptographic primitives when introduced (subject of a future standard).

## References

- NIST Secure Software Development Framework (SSDF).
- Microsoft SDL practices (for review discipline, not specific tooling).
- OWASP guidance on code review (general principles that transfer).
- seL4 verification experience — what they found worth reviewing even in a verified kernel.
