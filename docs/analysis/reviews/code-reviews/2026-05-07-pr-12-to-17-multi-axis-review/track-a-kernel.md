# Track A — Kernel correctness (PR #12–#17 multi-axis review)

- **Agent:** Claude general-purpose, 2026-05-07
- **Scope:** Kernel correctness across PR #12 / #13 / #14 / #15 / #16 / #17 merges.
- **Merge SHA range:** 298b5d2a..8dc433ee on main.

## Executive summary

The 14-day window across PR #12–#17 takes the kernel from the
B1-smoke-regression hotfix arc through the T-015 endpoint-rollback
implementation in a clean, well-staged sequence. PR #12 (T-014 / ADR-0026)
restructures idle dispatch via a dedicated `Scheduler::idle: Option<TaskHandle>`
slot consulted only when `ready.dequeue()` returns `None`; PR #15 polishes
the kernel with a `const { assert!(N > 0, ...) }` migration on `SchedQueue`,
a `register_idle` `assert!`-not-`debug_assert!` upgrade, and a comment-locking
discipline on `kernel/src/lib.rs`'s denylist; PR #17 (T-015 / ADR-0032) lands
the symmetric Deadlock-rollback `ipc_cancel_recv` recovery primitive plus six
new tests. PR #13 / #14 / #16 are documentation / URL / closure-trio sweeps;
their kernel touch is rustdoc-comment URL renames (PR #14) — no semantic
change. The new code holds the Phase A "error path leaves observable state
unchanged" invariant for both *scheduler* and *endpoint* state; UNSAFE-2026-0014
gains its 3rd and 4th amendments in lockstep with the new sites; the
`// SAFETY:` blocks at every momentary-`&mut` block name the audit tag
correctly.

The kernel-correctness verdict is **Approve-with-2-followups**: zero
blockers, zero majors, two minor findings (a redundant `&mut EndpointArena`
parameter on `ipc_cancel_recv`, and a doc-comment claim about
`SendPending/RecvComplete` no-op behaviour that overstates what the implementation
checks). The 2026-05-06 baseline's open non-blockers either close in this
window (the Deadlock-asymmetry one, by ADR-0032/T-015, is the load-bearing
example), advance with explicit ADR-tracked deferral (`IpcError` variant
split → ADR-0030), or carry over unchanged with no new conflations.

## Findings

### Blocker

- *(none)*

### Major

- *(none)*

### Minor

- **MIN-1 (PR #17).** [`kernel/src/ipc/mod.rs::ipc_cancel_recv`](../../../../../kernel/src/ipc/mod.rs#L464) takes `ep_arena: &mut EndpointArena` but only calls the immutable `Arena::get(&self, id)` — the `&mut` is stricter than the function actually needs. Symmetric with `ipc_recv` / `ipc_send`'s shape (which also use `&mut EndpointArena` with only `.get(...)` for the live-handle check), so the over-strictness is consistent rather than novel; not a correctness issue. Suggested follow-up: when ADR-0030 (syscall ABI) lands and finalises the IPC entry-point signatures, sweep all four primitives' arena parameters to `&EndpointArena` if no current use mutates the arena. **Severity rationale: minor — symmetric with siblings; cosmetic; calling sites already pass `&mut`-borrowed arenas.**

- **MIN-2 (PR #17).** [`kernel/src/ipc/mod.rs:439-445`](../../../../../kernel/src/ipc/mod.rs#L439) `ipc_cancel_recv`'s doc-comment says **"`SendPending { .. }` or `RecvComplete { .. }`: no-op; returns `Ok(())`. Phase 1 has already advanced past the recv-only state, so the cancel has nothing to undo."** The implementation only checks `matches!(state, EndpointState::RecvWaiting)` and falls through for everything else — so it is *behaviourally* a no-op for `Idle / SendPending / RecvComplete`. Correct, but the wording elides one subtlety: the call still goes through `state_of`, which calls `reset_if_stale_generation`, which `debug_assert!`s against `SendPending { cap: Some(_) }` or `RecvComplete { cap: Some(_) }` *with stale generation*. The branch is unreachable from the v1 Deadlock site (Phase 1 just made the slot live with current generation), but a future caller (post-B2 userspace destroy drain, per ADR-0032 §Consequences) could plausibly hit it. The doc-comment's "harmless" framing is true today; it is worth a one-line rider clarifying that the no-op is *behavioural* and that the `debug_assert!` in `reset_if_stale_generation` still fires under stale-generation cap-leak conditions — so future drain callers know to drain caps before calling cancel. **Severity rationale: minor — not load-bearing in v1 (the only caller is the Deadlock site which cannot trigger the stale path); a doc precision issue, not a correctness issue.**

### Nit

- **NIT-1 (PR #17).** [`kernel/src/sched/mod.rs:986`](../../../../../kernel/src/sched/mod.rs#L986) the cancel block's `let table_ref: &CapabilityTable = &*caller_table;` correctly takes the immutable view (matching `ipc_cancel_recv`'s `&CapabilityTable` parameter). The `// SAFETY:` block reads "*ep_arena*, *queues*, *caller_table* valid + distinct + exclusive" — the "exclusive" word is technically over-stated for `caller_table` because we take an immutable shared borrow, but the audit-log Amendment ([UNSAFE-2026-0014 4th](../../../../docs/audits/unsafe-log.md)) gets the wording right ("`&CapabilityTable` (immutable), reflecting that recovery does not mutate caller-table state"). Suggested resolution: tighten the on-site `// SAFETY:` block to mirror the audit Amendment's word choice. Cosmetic; the safety argument is correct either way.

- **NIT-2 (PR #15).** [`kernel/src/sched/mod.rs:89`](../../../../../kernel/src/sched/mod.rs#L89) `SchedQueue::new`'s `const { assert!(N > 0, "SchedQueue requires N > 0") }` is correctly placed at the top of the body, but the doc-comment says "**`# Panics (compile-time)`**". `const { assert!(...) }` is a build-time hard error rather than a runtime panic; the `# Panics` rustdoc convention typically refers to runtime panics. Suggested resolution: rename the section to `# Compile-time errors` or `# Build errors`; rustdoc renders both fine. The current form is unambiguous in context (the parenthetical "compile-time" disambiguates) but is a slight bend of the rustdoc convention. Cosmetic.

### Praise

- **PRAISE-1 (PR #15).** [`kernel/src/sched/mod.rs:508`](../../../../../kernel/src/sched/mod.rs#L508) `register_idle`'s `assert!` (not `debug_assert!`) guard against `s.idle.is_some()` is exactly the right call — the single-idle invariant is load-bearing for ADR-0026's structural property ("idle never displaces a real Ready task"); a release-build silent overwrite would lose the previously-installed idle's `TaskContext` pointer in the `contexts` array and the dispatcher could re-enter a stale context. The accompanying doc-comment cites the rationale explicitly ("silently overwriting the slot would break the single-idle invariant ADR-0026 relies on"). Pattern-of-record: when violating an invariant means *re-entering a stale code page*, the assertion should be release-firing.

- **PRAISE-2 (PR #17).** [`kernel/src/sched/mod.rs:939-995`](../../../../../kernel/src/sched/mod.rs#L939) the Deadlock-rollback path's two-stage borrow split is well-disciplined: the dispatch block (`let dispatch = { let s = unsafe { &mut *sched }; ... }`) drops its `&mut Scheduler<C>` at the closing brace, then the conditional `let Some(...) = dispatch else { ... }` binds the cancel block in a separate `unsafe { let arena_ref = &mut *ep_arena; ... }` scope. Visually + lexically + textually proves the non-aliasing rule holds without depending on a comment. The inline comment block ("the scheduler `&mut` was dropped at the end of the dispatch block above; no cross-referent alias is alive here") is icing. This is the cleanest application of the ADR-0021 momentary-`&mut` discipline so far, and exactly the shape the audit-log Amendment ([UNSAFE-2026-0014 4th](../../../../docs/audits/unsafe-log.md)) calls out as the intended pattern.

- **PRAISE-3 (PR #17).** [`kernel/src/sched/mod.rs:989-993`](../../../../../kernel/src/sched/mod.rs#L989) the `debug_assert!(cancel_result.is_ok(), "...")` guard on the cancel-call result is the right shape: the only error mode `ipc_cancel_recv` can surface is `IpcError::InvalidCapability`, which would mean the same `ep_cap` that just passed Phase 1's RECV-validation suddenly fails a second pass — structurally impossible under v1's single-thread cooperative invariant. The `debug_assert!` keeps the contract visible without paying release-build cost; the underlying `Ok(())` happy path is asserted in the host test (`ipc_recv_and_yield_deadlock_rolls_back_endpoint_state`). The wording ("cannot fail under v1's cooperative single-thread invariant") flags the assumption explicitly so a future preemptive-scheduler change knows where to revisit.

- **PRAISE-4 (PR #17).** [`kernel/src/ipc/mod.rs:464-481`](../../../../../kernel/src/ipc/mod.rs#L464) `ipc_cancel_recv`'s body is exemplarily small — it shares `validate_ep_cap` with `ipc_send` / `ipc_recv` (with `CapRights::RECV`), peeks the slot via `state_of`, conditionally writes `Idle`. No new helper functions, no new state machine entries, no new audit-log entries — the recovery primitive lives entirely under the existing UNSAFE-2026-0014 umbrella exactly as ADR-0032 *Consequences* §Neutral promised ("zero new audit-log entry"). The implementation matches its ADR's promise byte-for-byte.

- **PRAISE-5 (PR #15).** [`kernel/src/sched/mod.rs:88-94`](../../../../../kernel/src/sched/mod.rs#L88) `SchedQueue::new`'s migration to `const { assert!(N > 0, ...) }` is the correct closure of Phase A's outstanding non-blocker. The wrap arithmetic in `enqueue` / `dequeue` was already correct under N>0 (with `clippy::arithmetic_side_effects` `#[allow]` annotated against the same invariant), and the `const`-block makes the type-level contract an unconditional build-time hard error rather than implicit through-code semantics. Mirrors the discipline `Arena::new` already had.

- **PRAISE-6 (PR #15).** [`kernel/src/lib.rs:36-43`](../../../../../kernel/src/lib.rs#L36) the denylist comment now explicitly says "do NOT re-state workspace denies here". Reads like a discipline-locking sentinel — a future PR cannot silently re-introduce the duplication Phase A caught earlier. This is the right shape for "harmless-but-watch" issues: turn them into self-explaining comments rather than letting the discipline drift.

## Cross-PR observations

- **The 2026-05-06 baseline non-blocker on Deadlock asymmetric rollback is genuinely closed by PR #17, not papered over.** The baseline's [track-a-kernel.md L19](../../2026-05-06-full-tree/track-a-kernel.md) said *"the doc-comment on `SchedError::Deadlock` is honest about this gap [...] queue an ADR for endpoint rollback / `ipc_cancel_recv` before Phase B2"*. ADR-0032 lands; T-015 implements it; the doc-comment now records both halves of the rollback symmetrically; two host tests assert the endpoint-state recovery (one new, one upgraded). The closure is structural, not cosmetic.

- **The simulation-table discipline ADR-0026 added (queue-state walk through every demo step) is now applied to ADR-0032 too.** ADR-0032 §Decision outcome includes a five-row table walking Phase 2 Deadlock under each option (A inline, B primitive, C/D no-op). This is the second ADR to use the discipline post-2026-05-06 smoke-regression mini-retro; the pattern is settling cleanly. Cross-track signal: this is also why PR #16 (closure trio) was able to land the ADR-0023 placeholder + ADR-0032 Propose without burning re-review effort — the simulation table makes the design auditable on first read.

- **`register_idle`'s `assert!`-not-`debug_assert!` upgrade in PR #15 is the correct closure of the comprehensive review's flagged item.** The reviewer flagged it as "release-build silent overwrite hazard if `register_idle` is ever called twice"; the upgrade closes it. The accompanying allow-block (`#[allow(clippy::panic, reason = "duplicate register_idle is a kernel programming error ...")]`) is the correct discipline for `clippy::panic`-denied kernel code that legitimately must panic.

- **PR #15's γ.6 revert is correctly expressed as a comment-only rationale block.** The "defensive parking loop after `start()`" was rejected because (a) `start: -> !` already type-proves no fall-through, (b) `clippy::unreachable_code` + `clippy::too_many_lines` make the loop a hard build error, (c) the only refactor regression the loop would protect against is `start` losing `-> !` — which itself becomes a build error in every caller's return-type analysis. The 14-line comment block in [`bsp-qemu-virt/src/main.rs::kernel_entry`](../../../../../bsp-qemu-virt/src/main.rs)'s tail records this rationale so a future reviewer does not re-litigate the suggestion. (BSP-side, not strictly Track A — flagging for awareness because the rationale is kernel-correctness-adjacent.)

- **No new `unsafe` is introduced by any of the six PRs.** PR #17 reuses the existing UNSAFE-2026-0014 umbrella; PR #12 reuses it via `register_idle` (3rd Amendment); PRs #13/#14/#15/#16 add zero `unsafe`. The audit-log surface gained two Amendments (3rd + 4th on UNSAFE-2026-0014) — both correctly back-pointer to ADR-0026 / ADR-0032. ADR-0032 *Consequences* §Neutral's "no new audit-log entry" claim is therefore literally true.

- **The doc-comment on `SchedError::Deadlock` ([sched/mod.rs:179-211](../../../../../kernel/src/sched/mod.rs#L179)) now records the symmetric rollback in detail.** It calls out *both* the scheduler-side rollback (existing) and the endpoint-side rollback (new, via `ipc_cancel_recv`), names ADR-0017/0022/0026/0032 as the bibliographic chain, and explicitly states that the variant is structurally unreachable in v1 with idle registered. This is exactly the doc shape the 2026-05-06 baseline asked for ("worth re-visiting under an ADR before preemption / SMP land").

## Cross-track notes (route to other agents)

- **→ Track C (security):** the new `ipc_cancel_recv` requires `CapRights::RECV` (same as `ipc_recv`) — symmetric authorisation. No new way to reach the cancel-vs-recv asymmetry that would let a non-RECV holder leak a registration. Worth a security-axis confirmation that the cap-rights discipline matches the threat model's expectation; the kernel-correctness axis is satisfied.

- **→ Track C (security):** the audit-log 4th Amendment for UNSAFE-2026-0014 reads at the right level of detail and names the file:line site (Deadlock-branch block) explicitly, satisfying the "additional locations" field the audit-log standard requires. Cross-check that the security-review axis confirms the `// SAFETY:` block's wording matches the Amendment's.

- **→ Track D (perf):** `unblock_receiver_on` is still O(N) over `TASK_ARENA_CAPACITY=16` — unchanged from baseline. The new `ipc_cancel_recv` is O(1) per call (one `state_of` + one `matches!` + one optional write). No new hot loops introduced.

- **→ Track E (docs):** [`docs/architecture/ipc.md`](../../../../docs/architecture/ipc.md) §State machine gained the `RecvWaiting → Idle: ipc_cancel_recv (recovery)` arc — visible at L41. The accompanying paragraph at L63 cites ADR-0032 and explains the kernel-internal scope ("no userspace caller exists"). Bidirectional cross-reference with ADR-0017's §Revision notes rider (which records the additive recovery primitive). Doc surface looks consistent with the implementation.

- **→ Track F (tests):** six new tests land in PR #17 (5 IPC + 1 sched). The IPC tests cover the four state-transition corners (RecvWaiting→Idle, Idle no-op, SendPending no-op preserves message, missing-RECV-right fails) plus idempotency. The sched test (`ipc_recv_and_yield_deadlock_rolls_back_endpoint_state`) is the named-on-record regression guard for the symmetric rollback. The existing T-007 test (`ipc_recv_and_yield_returns_deadlock_when_ready_queue_empty`) gained an inline endpoint-state assertion — modest scope creep, justifiable because the test was already exercising the Deadlock branch. Test surface is well-balanced.

- **→ Track G (BSP):** [`bsp-qemu-virt/src/main.rs::idle_entry`](../../../../../bsp-qemu-virt/src/main.rs) calls `yield_now(SCHED.as_mut_ptr(), cpu).expect(...)` — the `expect` is allowed because the BSP crate is not under the kernel's `clippy::expect_used` denylist. Confirmed (kernel denylist is `kernel/src/lib.rs`-scoped). The fallback discipline "current is idle and queue empty → no switch, return Ok(())" inside `yield_now` is correctly implemented at [sched/mod.rs:738-746](../../../../../kernel/src/sched/mod.rs#L738) — idle's WFI loop won't re-enter the dispatcher in a broken way.

- **→ Track J (hygiene):** PR #14's URL rename (`cemililik/TyrneOS` → `cemililik/Tyrne`) touched `kernel/src/sched/mod.rs` rustdoc footers (16 changes per the merge stat) and `kernel/src/ipc/mod.rs` (8 changes). All are pure URL replacements within rustdoc `[link]: https://...` definitions; no source-behaviour or doc-semantics change. Cross-checked: every cross-reference in the new ADR-0032 / `ipc_cancel_recv` rustdoc uses the new `cemililik/Tyrne` URL (e.g., L46 `[adr-0017]: https://github.com/cemililik/Tyrne/...`). No stale URL drift introduced by the new T-015 code.

## Suggested follow-up actions

- **Minor (close in next polish PR):**
  - MIN-1: sweep `ipc_cancel_recv`'s `ep_arena: &mut EndpointArena` to `&EndpointArena` (consistent with the recovery primitive's read-only-arena usage); align with whatever ADR-0030 settles for the syscall ABI's IPC-entry signatures. Optional and aesthetic.
  - MIN-2: add a one-line clarification to `ipc_cancel_recv`'s "`SendPending` / `RecvComplete`: no-op" doc bullet noting that *behavioural* no-op coexists with `reset_if_stale_generation`'s `debug_assert!` against stale-generation cap leaks, so future drain-on-destroy callers (B2+) understand the cap-drain discipline.

- **Nit (sweep when convenient):**
  - NIT-1: tighten the cancel-block `// SAFETY:` wording in `ipc_recv_and_yield` to mirror the audit-log Amendment's phrasing (`&CapabilityTable` is immutable, not "exclusive").
  - NIT-2: rename `SchedQueue::new`'s `# Panics (compile-time)` doc-section to `# Compile-time errors` for rustdoc-convention consistency.

- **Tracked, no action:**
  - The Deadlock-asymmetry baseline non-blocker is closed by PR #17. The `IpcError::WrongObjectKind` / `MissingRight` split deferred to ADR-0030 (B5 syscall ABI). `unblock_receiver_on` O(N) and double-validation are still cross-track perf items rather than kernel-correctness items.

## Sub-verdict

**Approve-with-2-followups (MIN-1, MIN-2)**
