# Security reviews

Deep security pass on changes that touch capabilities, IPC, syscalls, memory, scheduler, boot, cryptography, `unsafe`, or security-sensitive dependencies. The code review ([`../code-reviews/`](../code-reviews/)) covers ordinary quality concerns; this folder covers the adversarial pass.

## When to conduct

A security review is **mandatory** for any change that falls under the trigger list in [`../../../standards/security-review.md`](../../../standards/security-review.md) — reproduced here for convenience:

- Capabilities (types, table, transfer, derivation, revocation).
- IPC (message format, endpoint objects, send/receive entry points, buffer handling).
- Syscalls (addition, signature change, authority change).
- Memory management (page tables, MMU, allocators, TLB invalidation).
- Scheduler (priority, preemption, critical sections).
- Boot (reset vector through first userspace task creation).
- Cryptography.
- Authentication / authorization boundaries.
- `unsafe` (introduction, modification, broadening).
- Security-sensitive dependencies.

## What this review produces

A dated file `YYYY-MM-DD-<context>.md` in this folder, following the shape in [`master-plan.md`](master-plan.md). Sections correspond to the security checklist: capability correctness, trust boundaries, memory safety, kernel discipline, cryptography (when applicable), secrets handling, dependencies, threat-model impact.

## Relationship to the `perform-security-review` skill

[`perform-security-review`](../../../../.claude/skills/perform-security-review/SKILL.md) describes how to **conduct** the review during development. This folder holds the resulting artifact.

A security review is a **separate pass** from the code review — it is performed with fresh eyes, after a deliberate context switch. See [`../../../standards/security-review.md`](../../../standards/security-review.md).

## Index

_No reviews yet._ Security-review artifacts will begin once changes reach subsystems that trigger a security pass (starting with T-001 — the capability table, which is security-sensitive by construction).

| Date | Scope | File |
|------|-------|------|
| _pending_ | T-001 (capability table) | — |
