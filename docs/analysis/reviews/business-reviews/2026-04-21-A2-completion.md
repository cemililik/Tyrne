# Business review 2026-04-21 — A2 completion

- **Trigger:** milestone-completion
- **Scope:** Milestone A2 — Capability table foundation
- **Period:** 2026-04-20 (roadmap-system establishment, commit `abe1b94`) → 2026-04-21 (T-001 landed on `main` via PR #1)
- **Participants:** @cemililik (+ Claude Opus 4.7 agent as scribe)

## What landed

### Commits (reverse chronological, A2-relevant)

| SHA | Date | Subject | Advances |
|-----|------|---------|----------|
| `75ca576` | 2026-04-21 | `docs(roadmap): T-001 → Done; A2 closed; advance current.md` | closes A2 |
| `e937537` | 2026-04-21 | `docs(adr): propose ADR-0016 — Kernel object storage` | forward into A3 |
| `a587761` | 2026-04-21 | `docs(roadmap): open T-002 — Kernel object storage foundation (Draft)` | forward into A3 |
| `2e1d943` | 2026-04-21 | `docs: apply second-round PR review nits` | T-001 review round 2 |
| `cd8511c` | 2026-04-21 | `fix(cap): apply PR review findings to the capability subsystem` | T-001 review round 1 (code) |
| `d7ff460` | 2026-04-21 | `docs: apply PR review feedback, renumber reserved ADRs, add WOSR-derived patterns` | T-001 review round 1 (docs) + forward patterns |
| `e58a235` | 2026-04-21 | `chore: gitignore local technical-analysis notes` | housekeeping |
| `95db0f4` | 2026-04-20 | `docs(roadmap): AI integration stance (ADR-0015) + Phase J + supporting updates` | Plan C accepted |
| `8fe59d0` | 2026-04-20 | `feat(kernel): implement capability table (T-001)` | T-001 main implementation |
| `c574a12` | 2026-04-20 | `docs(adr): propose ADR-0014 — capability representation; T-001 → In Progress` | T-001 start |

### ADRs

- **ADR-0014 — Capability representation** (Accepted 2026-04-20). Index-based arena with generation-tagged handles, move-only `Capability`, narrowing-only rights.
- **ADR-0015 — AI integration stance** (Accepted 2026-04-20). Kernel stays AI-neutral; AI features live opt-in in userspace per Plan C.
- **ADR-0016 — Kernel object storage** (Proposed 2026-04-21 — snapshot; Accepted the same day). Per-type fixed-size-block arenas with typed handles, mirroring the capability-table shape.

### Tasks reaching `Done`

- **T-001 — Capability table foundation.** Shipped `cap::CapabilityTable`, `cap::CapHandle`, `cap::CapRights`, `cap::Capability`, `cap::CapError`. Zero `unsafe`, no heap, 29 host tests (14 rights, 15 table) green. `CapObject` placeholder encapsulated via `new`/`raw`.

## What changed in the plan

- **Roadmap + analysis system established** (commit `abe1b94`, predates this review period but relevant context): ten phases, per-phase task folders, four typed review folders each with a master plan. See [ADR-0013](../../../decisions/0013-roadmap-and-planning.md).
- **Phase J added** alongside ADR-0015. The phase plan grew from nine phases to ten; [`phases/README.md`](../../../roadmap/phases/README.md) and [`roadmap/README.md`](../../../roadmap/README.md) were updated to match.
- **ADR renumber cascade.** ADR-0015 was originally reserved for A3 "Kernel object storage" but was taken by the AI-integration decision that landed out of sequence. A3/A4/A5 reservations shifted +1 (A3 → 0016, A4 IPC → 0017, A4 Badge → 0018, A5 Scheduler → 0019, A5 Cpu v2 → 0020); Phase B–I reservations shifted +1 each. The contiguous reservation range is now 0012–0057.
- **WOSR-derived pattern notes inserted** into three phases' sub-breakdowns:
  - Phase A3 — fixed-size-block allocator per kernel-object kind (applied in ADR-0016).
  - Phase B2 — typed `MapperFlush`-analog acknowledgement token on the `Mmu` trait.
  - Phase C3 — closure-based `Cpu::without_interrupts` HAL primitive for IRQ-masked critical sections.
- **T-002 opened** in `Draft` status for Milestone A3.

## What we learned

**Unplanned ADR insertions ripple through reserved numbers.** The ADR-0015 AI-integration decision landed between the original reservation pass and A3, forcing a +1 shift across Phase A–I. The fix was mechanical but touched 9 phase files. Future unplanned ADRs should either (a) take the *next* free number rather than displacing a reservation, or (b) accept the ripple up front. Phase-a.md already notes "Numbers may shift if unexpected decisions land in between" — that is the accurate mental model, and the ledger should be treated as intent, not a promise.

**The WOSR analysis format earned its keep.** Reading Philipp Oppermann's *Writing an OS in Rust* produced three concrete patterns (typed flush tokens, `without_interrupts` primitive, fixed-size-block arenas) that were worth naming in advance of the code that will use them. ADR-0016 adopts one of those patterns (fixed-size-block arenas) directly. A similar study of seL4 or Hubris before Phase B is likely worth doing, again kept local via `.gitignore`.

**Reviews caught semantic bugs that tests did not.** Two findings in T-001's review were not caught by the 29 kernel tests:
- `cap_drop` of an interior node orphaned its children — a correctness bug the tests happened not to trigger because every `drop` test was against a leaf. Fixed with `CapError::HasChildren` plus a new test.
- `CapRights::from_raw` accepted any `u32`, letting reserved bits smuggle themselves past subset checks. Fixed with a `KNOWN_BITS` mask.

Both bugs were present under good test coverage of the *typed-error* paths. The lesson: typed-error coverage is not semantic-invariant coverage. Future tasks should enumerate *invariants to uphold* as explicit acceptance criteria, not just "operations return the documented errors".

**Encapsulation-by-default saved us later.** The second review round flagged `CapObject(pub u64)` as too open even for a placeholder. Changing the field to private with `new`/`raw` accessors is trivial today and will be load-bearing when ADR-0016 replaces the placeholder with a typed enum — every construction site is already auditable. Same lesson applies to other placeholder types: keep fields private from day one.

**Two review rounds despite "measured pace".** Round 1 addressed correctness; round 2 addressed cross-file consistency (Rust module-path style `::` hyphens vs. underscores, "Pi 4" vs. "Pi 5", "markdown" vs. "Markdown", MMU-capable board filtering for the RISC-V BSP, untagged code fence). A pre-commit self-review pass on the diff — specifically comparing wording across all changed files — would likely catch most of these. Worth adopting as part of the commit workflow.

**Zero-`unsafe` target was achievable.** The capability table is 350 lines of safe Rust. The zero-`unsafe` goal from ADR-0014 held. This is a useful data point for ADR-0016: if the kernel-object arenas follow the same pattern, they should also be `unsafe`-free.

## Adjustments

- [ ] **Open a standards doc** — `docs/standards/kernel-api-conventions.md` — capturing the "encapsulation-by-default, typed non-exhaustive errors, `new`/`raw` accessor pattern" conventions the capability subsystem now embodies. Trigger: before T-002 implementation begins, so the same conventions are applied from the start. Execution: `propose-standard-change` skill.
- [ ] **Tighten task acceptance criteria** to enumerate semantic invariants, not just error-return paths. Trigger: when T-002 moves from `Draft` to `Ready`, revisit its acceptance criteria against this pattern. Execution: inline edit during the T-002 transition.
- [ ] **Repeat the WOSR-analysis exercise** for one of: seL4, Hubris, Theseus, NuttX. Trigger: before Phase B (userspace) starts — seL4 is the most relevant prior art for userspace / address-space / syscall design. Execution: local under `docs/analysis/technical-analysis/` (gitignored), with any extracted patterns surfaced into phase-b sub-breakdowns the same way WOSR was.
- [ ] **Add a pre-commit diff-scan step** to the maintainer's workflow: before staging, scan every changed file for cross-file wording/style consistency. Does not need to block; a five-minute re-read. Execution: informal; may grow into a checklist in `CONTRIBUTING.md` if the value becomes clear.
- [ ] **ADR-0016 → Accepted** before T-002 implementation code lands. Trigger: maintainer review of the Proposed ADR. Execution: status edit on [`ADR-0016`](../../../decisions/0016-kernel-object-storage.md) + a one-line commit.

## Next

- **Active phase:** A
- **Active milestone:** A3 — Kernel objects
- **Active task:** [T-002 — Kernel object storage foundation](../../tasks/phase-a/T-002-kernel-object-storage.md) (Draft → Ready after this review is committed, then → In Progress once ADR-0016 is Accepted)
- **Next review trigger:** code + security review of the T-002 implementation when it reaches `In Review`; business review waits for A6 per [phase-a.md closure](../../../roadmap/phases/phase-a.md).
