# T-008 — Architecture docs for scheduler + IPC + post-T-009 HAL update

- **Phase:** B
- **Milestone:** B0 — Phase A exit hygiene
- **Status:** In Review
- **Created:** 2026-04-27
- **Author:** @cemililik (+ Claude Opus 4.7 agent)
- **Dependencies:** T-006 (`Done` 2026-04-27), T-007 (`Done` 2026-04-27), T-009 (`Done` 2026-04-27) — the implementations whose design this task documents.
- **Informs:** T-013 (T-013's `boot.md` update lives in T-013's own DoD; T-013 will reference T-008's `scheduler.md` and `hal.md` Timer subsection for the EL drop's downstream context). T-012 (T-012's exception/IRQ work will reference T-008's `ipc.md` and `scheduler.md` for the wake-source / idle-WFI handoff).
- **ADRs required:** none — this task documents existing Accepted ADRs (ADR-0017, ADR-0019, ADR-0020, ADR-0021, ADR-0022, ADR-0010), it does not propose new ones.

---

## User story

As a future maintainer (or contributor) opening the codebase six months from now, I want the major Phase A design decisions captured under [`docs/architecture/`](../../../architecture/) — not just inside ADRs and code — so that the *how it fits together* story is one click away from the architecture overview, not scattered across eight Accepted ADRs and four kernel modules.

## Context

Tyrne's [`docs/architecture/`](../../../architecture/) folder describes *how the system is built*; [`docs/decisions/`](../../../decisions/) records *why each piece was chosen that way*. Today the architecture folder has four substantive documents — `overview.md`, `boot.md`, `hal.md`, `security-model.md` — plus a small `README.md` index. This set covers the system's outer shape and the boot path well; it does **not** cover the two largest pieces of Phase A engineering work:

1. **The scheduler.** ADR-0019 (scheduler shape), ADR-0020 (`ContextSwitch` + `Cpu` v2), and ADR-0022 (idle task + typed `SchedError::Deadlock`) jointly settle the design, but a reader who wants the "FIFO with idle, cooperative, deadlock-typed" picture has to read three ADRs and the kernel source. There is no single `scheduler.md` summarising the *how* — the ready-queue shape, the `ContextSwitch::switch_to` contract, the idle task's role, the deadlock detection path.
2. **The IPC primitive set.** ADR-0017 (IPC primitive set) and ADR-0021 (raw-pointer scheduler IPC bridge) describe the design; ADR-0021 specifically settles the unsafe-ness of the bridge API. But the *how the bridge is structured*, *what invariants the BSP must uphold*, *how send/receive map to scheduler state* — the overview a new reader needs — is not in the architecture folder.

The post-T-009 timer story needs a small carve-out in `hal.md`: T-009 implemented `Timer` for `QemuVirtCpu` via `CNTVCT_EL0`/`CNTFRQ_EL0`, and the register-family rationale (CNTVCT vs CNTPCT) lives in UNSAFE-2026-0015's two Amendments and ADR-0010 — but `hal.md`'s current Timer subsection (if any) predates this work. A 5–10 line update keeps `hal.md` synchronised with the BSP's actual implementation.

`overview.md` has one update item too: it should mention Phase A is complete and cross-link to the new scheduler.md / ipc.md so a reader landing on the overview finds the deeper docs.

[`boot.md`](../../../architecture/boot.md) is **explicitly out of scope** for T-008 — T-013's DoD already owns the EL drop documentation update there. Splitting that across two tasks would create exactly the kind of forward-reference handwave ADR-0025 §Rule 1 forbids.

## Acceptance criteria

- [x] **`docs/architecture/scheduler.md` exists** and covers:
  - The cooperative FIFO scheduler shape (ready queue as a `heapless::Vec`-backed slot array; round-robin via `ScheduleStep`).
  - The `ContextSwitch` trait (per ADR-0020) and the per-task context-state slot the BSP allocates (`Cpu::ContextState`).
  - The idle task's role (FIFO head when no other task is ready; current `spin_loop` body, future `wait_for_interrupt` activation gated on T-012).
  - The typed `SchedError::Deadlock` path (per ADR-0022) and how the resume-time check distinguishes "all tasks blocked" from "no tasks registered".
  - The raw-pointer IPC-bridge API surface (per ADR-0021) — *what the BSP sees*; details of *why the API is unsafe* stay in ADR-0021.
  - Cross-references to ADR-0019, ADR-0020, ADR-0021, ADR-0022, T-004, T-006, T-007.
- [x] **`docs/architecture/ipc.md` exists** and covers:
  - The Send/Recv primitive set per ADR-0017; the `IpcEndpoint` data layout; `IpcError` taxonomy.
  - How send/receive map onto scheduler state — the "send blocks unless receiver ready, receive blocks unless message queued" contract — and the resume-after-block path.
  - The raw-pointer bridge from the BSP into the scheduler (`ipc_send_and_yield` / `ipc_recv_and_yield` free functions over `*mut Scheduler<C>`); the BSP-side discipline (UNSAFE-2026-0013 / 0014 patterns).
  - Cross-references to ADR-0017, ADR-0021, T-003, T-005, T-006.
- [x] **`docs/architecture/hal.md` updated** with a Timer subsection (or expansion of an existing one) describing:
  - The `Timer` trait surface (now/arm_deadline/cancel_deadline/resolution_ns) per ADR-0010.
  - The CNTVCT_EL0 vs CNTPCT_EL0 register-family choice (one paragraph; cite UNSAFE-2026-0015 + ADR-0010).
  - That `arm_deadline` / `cancel_deadline` are deferred to T-012 (IRQ delivery half) — the BSP's current implementation is `unimplemented!()` for the IRQ-armed half.
- [x] **`docs/architecture/overview.md` updated** with:
  - One paragraph noting Phase A is complete (kernel boots on QEMU virt; two-task IPC demo runs A6).
  - Cross-links to scheduler.md and ipc.md from wherever the kernel-internals are first mentioned.
- [x] **`docs/architecture/README.md` index** lists the two new docs.
- [x] **No new ADR.** This task documents existing Accepted ADRs; if the writing surfaces a *new* design question (something not already settled in an ADR), stop and write the ADR via `write-adr` — do not silently embed a decision in an architecture doc.
- [x] **No diagram-format violations.** Any diagrams are inline Mermaid (per CLAUDE.md non-negotiable rule 4).
- [x] **Documentation style** per [`docs/standards/documentation-style.md`](../../../standards/documentation-style.md) — paragraph-first prose, no bullet-stacking, English in the repo (per CLAUDE.md non-negotiable rule 3).

## Out of scope

- **`boot.md` update for T-013's EL drop.** T-013 owns this; T-008 does not touch boot.md.
- **`docs/architecture/capabilities.md`.** Phase B6+ will rewrite the capability subsystem (cap-set generations, derive_from, revocation per ADR-0023's expected outcome); writing capabilities.md *now* would be near-instantly stale. Deferred until B6 lands.
- **`docs/architecture/memory.md`.** B2 (MMU activation) is the natural birthplace; writing it now would be guesswork.
- **A standalone `timer.md`.** The Timer story is small enough to live as a subsection of `hal.md`; a top-level timer document is overkill for the current scope.
- **Updating ADRs in place.** T-008 *reads from* ADRs and synthesises them; it does not edit ADR bodies. Per ADR-0025 §Rule 2, ADR bodies are append-only; corrections are riders, not in-place edits — and T-008 has no rider-worthy corrections to make.
- **Architectural diagrams beyond Mermaid sketches.** A scheduler state-machine diagram or an IPC sequence diagram in Mermaid is in scope; anything heavier is not.

## Approach

Two new documents written from scratch; two existing documents lightly amended. Order:

1. **Read the source.** scheduler.md is mostly synthesis of ADR-0019/0020/0022 + the kernel's `scheduler.rs` + the BSP's `task_a` / `task_b`. ipc.md is ADR-0017/0021 + `ipc.rs` + the bridge functions. The synthesis is the value; do not paraphrase ADRs blindly — explain the *interaction* the ADRs separately decide on.
2. **Draft scheduler.md.** Outline: opening paragraph (what the scheduler is + what it isn't); FIFO + ContextSwitch contract; idle task; deadlock detection; the raw-pointer bridge entry points; cross-references. Aim for ~150–250 lines, like `boot.md` and `hal.md`'s existing structure.
3. **Draft ipc.md.** Same shape: opening paragraph; Send/Recv primitive set; endpoint data layout; how send/receive thread through the scheduler; the bridge functions and the BSP discipline; cross-references. Same ~150–250 line target.
4. **Update hal.md.** Add or expand the Timer subsection. ~10–15 lines net change.
5. **Update overview.md.** One paragraph + 2–3 cross-link line additions. ~5–10 lines net.
6. **Update README.md index.** Two rows.

Mermaid diagrams: at least one per new doc, not more than two. scheduler.md gets a state diagram for task states (Ready / Running / Blocked-on-IPC / Idle); ipc.md gets a sequence diagram for the send-blocks-then-receive-resumes path. Both are fewer than 30 lines of Mermaid each.

## Definition of done

Beyond the acceptance criteria:

- [x] `cargo fmt --all -- --check` clean (no code changes expected; this is doc-only, but the gate is cheap).
- [x] `cargo host-clippy` clean (same reasoning).
- [x] `cargo host-test` passes (no test changes; same reasoning). (Delivered: 143 / 143.)
- [x] `cargo kernel-build` clean (same reasoning).
- [x] No new `unsafe` block — this is a doc-only task; if it touches `unsafe`, scope crept.
- [x] Cross-references go *both* ways: ADRs cited from architecture docs are the same ADRs whose §References sections cite (or will cite, in their next rider) the new architecture docs. Bidirectional citation prevents the "no one knows where to look" failure mode. (Delivered: ADR-0010 / 0017 / 0019 / 0020 / 0022 each gain a §Revision notes pointer at the architecture doc that synthesises them, in the same PR.)
- [x] Commit messages follow [`commit-style.md`](../../../standards/commit-style.md). Likely one or two commits — one for the new docs (scheduler.md + ipc.md), one for the existing-doc updates. (Delivered: bundled commit `c658c3d`.)
- [x] Task status updated to `In Review` when ready; [`docs/roadmap/current.md`](../../../roadmap/current.md) updated; phase-b.md §B0 sub-breakdown item for T-008 marked complete. (`In Review` since 2026-04-27.)

## Design notes

- **Why two new docs and not one combined `kernel.md`?** Scheduler and IPC are the two halves of "kernel-internal control flow" but they have distinct invariants and distinct ADR ancestry. A single `kernel.md` would either be too long or paper over the seam. Two focused docs is the architecture-docs convention (see how `boot.md` and `hal.md` are separate today).
- **Why is `boot.md` not in T-008?** T-013 will rewrite the boot flow's EL transition section when the asm lands. If T-008 wrote that section today, T-013 would either re-write it (in-place edit, potentially violating its own invariants) or T-008's work would go stale within a week. Cleaner to let T-013 own its update.
- **Why no ADR for the Timer story?** ADR-0010 already settles the Timer trait. UNSAFE-2026-0015's Amendment + ADR-0010's text together cover the CNTVCT vs CNTPCT register-family question. T-008 reads from those, it does not re-decide.
- **What if writing surfaces a design gap?** Stop, write the ADR. The archetype: while writing scheduler.md, the author realises the deadlock-detection path's exact wake-condition isn't in any ADR. That's an ADR moment; either ADR-0022 needs a rider or a new ADR is required. T-008's mandate is *documenting decided choices*, not *deciding new ones in disguise*.
- **Length budget check.** Two new ~200-line docs + ~25 lines of edits + index updates ≈ 425 lines net. Within range; comparable in scope to `hal.md` (316 lines) plus a smaller doc.

## References

- [ADR-0010 — Timer trait](../../../decisions/0010-timer-trait.md) — Timer subsection source.
- [ADR-0017 — IPC primitive set](../../../decisions/0017-ipc-primitive-set.md) — ipc.md source.
- [ADR-0019 — Scheduler shape](../../../decisions/0019-scheduler-shape.md) — scheduler.md source.
- [ADR-0020 — `ContextSwitch` trait and `Cpu` v2](../../../decisions/0020-cpu-trait-v2-context-switch.md) — scheduler.md source.
- [ADR-0021 — Raw-pointer scheduler IPC-bridge API](../../../decisions/0021-raw-pointer-scheduler-ipc-bridge.md) — both new docs cite.
- [ADR-0022 — Idle task and typed scheduler deadlock error](../../../decisions/0022-idle-task-and-typed-scheduler-deadlock.md) — scheduler.md source.
- [`docs/standards/documentation-style.md`](../../../standards/documentation-style.md) — required reading before writing.
- [`docs/architecture/overview.md`](../../../architecture/overview.md) — outer-shape doc the new docs link back to.
- [`docs/architecture/hal.md`](../../../architecture/hal.md) — receives the Timer subsection update.
- [T-004 user story](../phase-a/T-004-cooperative-scheduler.md) — scheduler implementation.
- [T-006 user story](T-006-raw-pointer-scheduler-api.md) — raw-pointer bridge implementation.
- [T-009 user story](T-009-timer-init-cntvct.md) — Timer implementation.

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-04-27 | @cemililik (+ Claude Opus 4.7 agent) | Opened with status `Draft`. Last B0 task to gain a file (per the T-009 mini-retro's *Adjustments* item 3, now ✅ in the second follow-up note). Scope deliberately narrow: two new docs (scheduler.md + ipc.md) + small updates to hal.md and overview.md. boot.md update is T-013's, not T-008's. |
| 2026-04-27 | @cemililik (+ Claude Opus 4.7 agent) | Implementation complete in a single arc. <br/>**New docs:** <br/>• `docs/architecture/scheduler.md` (~180 lines) — FIFO + `ContextSwitch` + idle task + typed `SchedError::Deadlock` + raw-pointer bridge; synthesises ADR-0019/0020/0021/0022 + T-004/T-006/T-007/T-011; two Mermaid diagrams (data-structure + state machine). <br/>• `docs/architecture/ipc.md` (~190 lines) — three-primitive set per ADR-0017 + endpoint state machine + slot-generation reset + capability-transfer pre-flight + scheduler-bridge wrappers + BSP discipline; three Mermaid diagrams (state machine, generation-reset flow, bridge sequence). <br/>**Updated docs:** <br/>• `docs/architecture/hal.md` — Timer subsection expanded with the post-T-009 picture (CNTVCT register family per ADR-0010, helper functions, IRQ-armed half deferred to T-012). <br/>• `docs/architecture/overview.md` — Phase-A-complete status banner + cross-links to scheduler.md / ipc.md from the responsibilities list and the IPC capability-transfer paragraph. <br/>• `docs/architecture/README.md` index — scheduler.md and ipc.md added as Accepted; planned `kernel-core.md` and `scheduling.md` rows removed (subsumed). <br/>**Scope discipline:** No new ADR — the task documents existing Accepted ADRs only, per AC. No code changes — `boot.md` is explicitly out of scope; T-013 owns its EL-drop update. <br/>**Verification:** host tests 143/143 green; `cargo fmt` / `host-clippy` / `kernel-clippy` / `kernel-build` clean. <br/>**State transition:** Status → `In Review`. |
