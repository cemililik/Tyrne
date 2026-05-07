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

| Date | Scope | Verdict | File |
|------|-------|---------|------|
| 2026-04-21 | Tyrne project → Phase A exit (Phase 1–4c bootstrap + A1–A6 kernel core) | Changes requested (3 Phase-B blockers; no Phase-A exit blocker) | [2026-04-21-tyrne-to-phase-a.md](2026-04-21-tyrne-to-phase-a.md) |
| 2026-04-27 | B0 closure consolidated pass (T-006/T-007/T-008/T-009/T-011/T-013 + ADR-0021/0022/0024/0025 + UNSAFE-2026-0013..0018) | Clean — no High findings; pre-existing items (cross-table revocation, generation overflow) tracked at original severity | [2026-04-27-B0-closure.md](2026-04-27-B0-closure.md) |
| 2026-04-28 | B1 closure consolidated pass (T-012 — GIC v2 + EL1 vector table + IRQ-handler dispatch + idle WFI; ADR-0021 2026-04-28 Amendment; UNSAFE-2026-0019/0020/0021 + UNSAFE-2026-0014 commit-`28c5ce9` Amendment) | Approve — no high-severity findings; two forward-flagged non-blocking items (`arm_deadline` CVAL+CTL race + `cancel_deadline` CTL+GIC race), both only relevant when a future caller exists; pre-closure work-items (QEMU smoke + Miri) maintainer-side | [2026-04-28-B1-closure.md](2026-04-28-B1-closure.md) |
| 2026-05-07 | B1 closure post-T-014 consolidated pass (T-014 + ADR-0026 idle-dispatch supersession of ADR-0022 §Decision-outcome Option A; UNSAFE-2026-0014 third Amendment for `register_idle`; UNSAFE-2026-0019/0020 partial-verification + post-T-014-smoke Amendments; UNSAFE-2026-0021 no-verification Amendment) | Approve — eight axes pass; no new attack surface; smoke trace clean for full ~6 ms boot-to-end run; pre-existing forward-flagged items unchanged | [2026-05-07-B1-closure.md](2026-05-07-B1-closure.md) |
