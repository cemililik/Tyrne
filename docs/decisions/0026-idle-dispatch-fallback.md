# 0026 — Idle dispatch via separate fallback slot (supersedes ADR-0022 Option A)

- **Status:** Accepted
- **Date:** 2026-05-06
- **Deciders:** @cemililik

## Context

This ADR supersedes [ADR-0022 — Idle task and typed scheduler deadlock error](0022-idle-task-and-typed-scheduler-deadlock.md) on the *idle-task location* axis only. ADR-0022's *typed-error* axis (Option G — `SchedError::Deadlock` + `IpcError::PendingAfterResume` + `start`'s panic preserved) is unaffected and stands. The earlier decision picked **Option A** (idle as a regular task in the FIFO ready queue) over Option B (idle in a dedicated `Option<TaskHandle>` slot, dispatched only when the ready queue is otherwise empty); the rationale in [ADR-0022 §Decision outcome](0022-idle-task-and-typed-scheduler-deadlock.md#decision-outcome) was that *yield_now*'s "only one ready task" early-return path would collapse the solo-idle case to an inline WFI loop, making Option A's ergonomic simplicity costless.

That reasoning was correct only when idle is the **sole** Ready task. The cooperative IPC demo's flow puts all three of `task_b`, `task_a`, and `idle_entry` into Ready state simultaneously (the moment after `task_a`'s `ipc_send` calls `unblock_receiver_on(ep)` to mark `task_b` Ready, and before `yield_now` re-enqueues `task_a`). At that point the round-robin FIFO dispatches whatever is at the head — and the head, by construction of `kernel_entry`'s `add_task` order (B, A, idle), is **idle**. Idle's body issues `WFI` and the kernel sleeps indefinitely because v1's cooperative IPC demo never arms a deadline; no IRQ ever fires.

The first end-to-end QEMU smoke at HEAD `214052d` (2026-05-06) surfaced the regression — six prior review/closure artefacts (Phase A code review, B0 closure trio, B1 closure trio) had cleared the code path on the strength of host tests + miri + static analysis, none of which model "WFI without an IRQ source hangs". The defect entered the tree at the same instant ADR-0022 was Accepted (2026-04-22, T-007); B0 and B1 inherited it unmodified. See [`2026-05-06-B1-smoke-regression.md`](../analysis/reviews/business-reviews/2026-05-06-B1-smoke-regression.md) for the full mini-retro.

Two observations from that mini-retro inform this ADR:

- **ADR-0022 did not run a queue-state simulation.** The choice between Option A and Option B was justified in prose; the four-row table walking `(queue-state-pre, action, queue-state-post)` through `unblock_receiver_on` + `yield_now` was never written. That table — which §"Decision outcome" below presents — surfaces the exact bug. Code review and security review both inherited the same blind spot.
- **The "yield_now's one-ready fast path" claim assumed idle would only run when alone.** The fast path collapses solo-idle to a no-switch return, but it does *not* prevent idle from being dispatched when other tasks are Ready. Option A's ergonomic argument therefore collapses for any workload with two or more application tasks.

The constraints inherited from prior ADRs in [ADR-0022 §Context](0022-idle-task-and-typed-scheduler-deadlock.md#context) all still apply (no heap, `fn() -> !` task entries, raw-pointer scheduler bridge, BSP-owned arenas, `Cpu::wait_for_interrupt` available, single-core cooperative). Two are now load-bearing in a way ADR-0022 did not weight:

- **The ready queue's "only one ready task" fast path is not a safety net for idle competing with real tasks.** It only kicks in when exactly one Ready task exists. With idle plus one real task plus an unblocked receiver, three are Ready and the fast path does not apply.
- **`Cpu::wait_for_interrupt` is power-cheap exactly when there is something to wake the core.** v1 has no userspace and no IRQ-driven workload; the timer is wired (T-009 / T-012) but the demo never arms a deadline. Idle's WFI is a *liveness sink* in v1, not a power optimisation. The implication is the inverse of what ADR-0022's first rider's Sub-rider read: WFI activation requires not just the *infrastructure* (T-009 + T-012) but also a *caller* that arms a deadline — and in v1's demo, none exists.

## Decision drivers

- **Kernel liveness.** The same driver as ADR-0022, now with a stronger reading: liveness is not just "no panic on a userspace-reachable condition", it is also "no silent hang on a startup-by-default kernel-internal state". The smoke regression is exactly a silent-hang failure mode; the dispatch shape must rule it out structurally.
- **Invariant preservation.** ADR-0022's "ready queue never empty when `current.is_some()`" invariant was preserved at the cost of putting idle in the queue alongside real tasks. The right shape is to weaken the invariant slightly — "the dispatcher always finds a next task, but idle is only one of two sources" — and gain the stronger property that idle never displaces a real Ready task.
- **Forward compatibility with preemption.** ADR-0022 argued that Option A keeps idle compatible with preemption. Option B preserves that property: idle still has a `TaskContext` and can be preempted; the only change is *when* the dispatcher selects it.
- **Simulation discipline.** The corrective lesson from the smoke regression is that multi-step state-machine ADRs need an explicit queue-state walkthrough. This ADR includes one (in §Decision outcome below) so the choice is verifiable.
- **Audit surface.** ADR-0022 boasted "zero new `unsafe`". Option B introduces no `unsafe` either — `register_idle` is a raw-pointer free function in the same shape as `add_task` (already covered by [UNSAFE-2026-0014](../audits/unsafe-log.md#unsafe-2026-0014--scheduler-free-function-momentary-mut-pattern)); idle's `TaskContext` lives in the existing `contexts` array. The audit-log delta is one Amendment to UNSAFE-2026-0014 noting the new `register_idle` site.
- **Single-core cooperative fit.** Same as ADR-0022. Option B costs ~30-50 LOC across the kernel + 1 line in the BSP `kernel_entry`; the cost is bounded.
- **Honest assessment of ADR-0022's rejected-Option-B costs.** ADR-0022 listed three costs for Option B: (a) "scheduler-state duality", (b) "new dispatch branch", (c) "new invariant idle-never-double-enqueued". The smoke regression demonstrates that the *inverse* costs apply to Option A — it has *implicit* duality (idle vs real tasks share a queue but have different semantics), an *implicit* dispatch branch (the fast path that was supposed to handle solo-idle), and an *implicit* invariant that did not hold (idle would only run when alone). Making these explicit in Option B is now the right call: explicit branches and invariants are auditable; implicit ones cause silent hangs.

## Considered options

The same options ADR-0022 considered, re-evaluated with the simulation discipline this ADR adds.

1. **Option A — Idle in the FIFO ready queue (status quo, superseded).** BSP calls `add_task(idle_entry, ...)` like any other task. Demonstrated to hang in the cooperative IPC demo per the §Context smoke trace.
2. **Option B — Dedicated `idle: Option<TaskHandle>` slot on `Scheduler<C>`.** New `register_idle` raw-pointer free function; the dispatcher consults `idle` only when the ready queue is empty. Idle never enters the FIFO.
3. **Option C — Inline WFI loop in the scheduler.** Same shape ADR-0022 rejected; rejected again here for the same forward-compatibility-with-preemption reason. Re-listed because the smoke regression caused some reviewers to ask whether "idle is too complicated, simplify it" was the right reading. It is not — Option C abandons preemption compatibility, and Phase B is on track to need preemption within two milestones.
4. **Option D — Kernel-allocated idle.** Same as ADR-0022; rejected for the same ADR-0016 / ADR-0021 alignment reasons.
5. **Option E — Patch Option A's FIFO ordering** (e.g. "always put idle at the tail; bump unblocked receivers to head"). This is the "minimum-diff" option that papers over the symptom without fixing the structural duality. Rejected — see Pros/cons below.

## Decision outcome

Chosen option: **Option B — dedicated `idle: Option<TaskHandle>` slot on `Scheduler<C>`.**

### Queue-state simulation (the discipline ADR-0022 missed)

The demo's instruction-level flow under Option B:

| Step | Event | Ready queue | Idle slot | Current | Switch target |
|------|-------|-------------|-----------|---------|---------------|
| 0 | Boot. `add_task(B, A)`; `register_idle(idle_entry)` | `[B, A]` | `Some(idle_h)` | `None` | — |
| 1 | `start()` dequeues head | `[A]` | `Some(idle_h)` | `Some(B)` | **B** |
| 2 | B → `ipc_recv` (Pending) → blocks; yield dequeues head | `[]` | `Some(idle_h)` | `Some(A)` | **A** |
| 3 | A → `ipc_send` delivers; `unblock_receiver_on(ep)` enqueues B | `[B]` | `Some(idle_h)` | `Some(A)` | — |
| 4 | A → `yield_now` re-enqueues A; dequeues head | `[A]` | `Some(idle_h)` | `Some(B)` | **B** |
| 5 | B → reply; `ipc_send` enqueues nobody (A already Ready); `yield_now` | `[]` | `Some(idle_h)` | `Some(A)` | **A** |
| 6 | A → `ipc_recv` collects reply; prints "all tasks complete"; `spin_loop` | — | — | `Some(A)` | (loops) |

At no step is idle dispatched. The same flow under Option A (current state at HEAD `214052d`) puts idle at the head of the queue at step 4 and the kernel hangs in `WFI`. The simulation table is the proof that Option B fixes the bug structurally — not as a heuristic, but by removing idle from the dispatch contention entirely.

### Why Option B and not E (patch the FIFO ordering)

The "minimum-diff" alternative is to leave idle in the FIFO and either (a) bump unblocked receivers to the queue *head*, or (b) require idle to always sit at the *tail* and never rotate. Both are local hacks that paper over the structural problem: the FIFO is being asked to express two different priority classes (idle = lowest, real tasks = equal) using a single queue with no priority field. Any future workload that introduces a third priority class — explicit per-task priorities (Phase B5+), preemption (Phase B5+), or simple "this task should run before that one" hints — will rediscover the same ad-hoc-priority-via-queue-position problem.

Option E also fails to express idle's intrinsic property: *idle is the dispatcher's fallback, not a participant*. Putting idle in the FIFO conflates two concepts the rest of the kernel needs to keep separate (especially Phase C's SMP world, where idle-per-core is a natural data layout that Option E cannot represent without further hacks). Option B's `idle: Option<TaskHandle>` field is the correct data type for the actual semantics, and the dispatch-with-fallback branch is one *if* statement — measured in lines of code, smaller than Option E's ordering invariant maintenance.

### Why this choice does not conflict with ADR-0022's other axis

ADR-0022 chose Option A for *idle location* AND Option G for *typed errors*. This ADR overrides only the *idle location* axis. The typed-error surface (`SchedError::Deadlock` + `IpcError::PendingAfterResume` + `start`'s panic kept) is unchanged. `SchedError::Deadlock` remains a defensive return for the case "idle is not registered AND every task is Blocked AND the ready queue is empty"; with Option B's `idle: Option<TaskHandle>` slot, the dispatcher's fallback is *literally* `self.ready.dequeue().or_else(|| self.idle)`, and Deadlock fires only when both halves are `None`. ADR-0022's Pro "T-011 has a unit test that asserts `Err(SchedError::Deadlock)` by skipping idle registration" applies unchanged — the test will simply omit the new `register_idle` call instead of omitting the `add_task(idle_entry, ...)` call. T-011's test set survives intact.

### Dependency chain

For this decision to be fully in effect:

```text
1. Refactor `Scheduler<C>` to add `idle: Option<TaskHandle>` field          — T-014 (Draft, opens with this ADR)
2. Add `unsafe fn register_idle(sched: *mut Scheduler<C>, ...)` raw-pointer
   free function in the ADR-0021 shape                                       — T-014 (same task)
3. Modify dispatch sites (`start_prelude`, `yield_now`, `ipc_recv_and_yield`)
   to consult `idle` only when `ready.dequeue()` returns `None`              — T-014 (same task)
4. Update `bsp-qemu-virt::kernel_entry` to call `register_idle(idle_entry)`
   instead of `add_task(idle_entry, ...)`                                    — T-014 (same task)
5. Append Amendment to UNSAFE-2026-0014 noting the new `register_idle` site  — T-014 (same task)
6. Re-run QEMU smoke; expect full demo trace + `tyrne: all tasks complete`   — T-014 (DoD)
7. Append final Amendment to UNSAFE-2026-0019 / 0020 lifting the partial-
   verification notes inserted 2026-05-06 (the smoke now reaches the same
   sites *and* completes the demo)                                           — T-014 (DoD)
```

T-014 is opened as `Draft` in the same commit as this ADR's Propose commit, per [ADR-0025 §Rule 1](0025-adr-governance-amendments.md). All seven steps fall inside T-014's scope; no separate task is needed.

UNSAFE-2026-0021's `Pending QEMU smoke verification` clearance is **not** in T-014's DoD — v1's demo never arms a deadline, so the timer-write site this entry audits is unreachable in T-014's smoke trace. That clearance is gated on a future B-phase task that introduces a real `arm_deadline` caller (per the 2026-05-06 Amendment to UNSAFE-2026-0021).

## Consequences

### Positive

- **Idle never displaces a real Ready task.** Structural fix; no silent-hang regression of the form the smoke surfaced is reachable from any cooperative or preemptive workload that registers exactly one idle task.
- **The `Scheduler<C>::idle` field is the data type matching the semantics.** "Idle is the dispatcher's fallback" is now expressed in the type system, not encoded as a queue-position convention.
- **Compatible with future per-CPU idle (Phase C SMP).** When SMP arrives, `idle: Option<TaskHandle>` becomes `idle: [Option<TaskHandle>; NCPU]` or moves into a per-CPU struct; the dispatcher branch generalises trivially. Option A would have required reworking the FIFO to a per-CPU FIFO + idle-rotation scheme.
- **The `SchedError::Deadlock` defensive return becomes more meaningful.** Under Option A, `Deadlock` fires only when *every* task including idle is Blocked — pragmatically impossible if idle is registered. Under Option B, `Deadlock` fires when the ready queue is empty AND `idle` is `None` AND `current.is_some()` is false in the dispatch path — still rare, but a real signal that "the BSP forgot to call `register_idle`" or "the kernel reached a path that should have a wake source but does not". The defensive return is now an actual diagnostic.
- **Auditable dispatch branch.** The `if let Some(idle) = self.idle { ... } else { panic-or-Deadlock }` branch is one *if* statement in three call sites (`start_prelude`, `yield_now`, `ipc_recv_and_yield`). Each can be inline-commented and unit-tested.

### Negative

- **One additional field on `Scheduler<C>` (`idle: Option<TaskHandle>`).** Eight bytes of struct overhead on aarch64. *Mitigation:* none needed; the scheduler struct is already kernel-static and 8 bytes is invisible.
- **One additional dispatch branch in three sites.** The scheduler's dispatch logic gains an `or_else(|| self.idle)`-shaped fallback. This is the cost ADR-0022 explicitly cited as a Con of Option B. *Mitigation:* the branch is well-typed (`Option<TaskHandle>`), is exercised by both unit tests and the smoke, and is in three call sites all of which already have inner-block discipline per ADR-0021. The "audit surface" growth ADR-0022 worried about is real but small.
- **One new invariant: idle is never enqueued in the ready queue.** The `register_idle` API does not call `self.ready.enqueue`, and `unblock_receiver_on` / `yield_now` / `start_prelude` never enqueue an idle handle. *Mitigation:* a `debug_assert!` at the top of `register_idle` checks the slot was not already a regular task; a unit test asserts the FIFO never contains the idle handle through an exhaustive demo simulation.
- **`fn idle_entry() -> !`'s body can simplify slightly** — it no longer needs to call `yield_now` after `wfi`, because there is no FIFO turn to yield. *Mitigation:* keeping the `wait_for_interrupt() + yield_now` shape uniform with the v1 implementation is fine; `yield_now` from inside idle becomes a no-op (ready queue empty + `current` is idle → `idle.dequeue()` returns idle again → fast path early-returns). This costs nothing and preserves the option to grow idle's body later (e.g. periodic housekeeping between WFI calls).
- **Existing host tests that referenced "idle is in the ready queue" need to be re-read.** The `start_prelude_panics_on_empty_ready_queue` test ([`kernel/src/sched/mod.rs:1317`](../../kernel/src/sched/mod.rs)) is now subtly stale: with idle in a separate slot, the *boot-time programming error* it tests is "BSP called `start` without registering any task AND without registering idle". The test's assertion still holds — empty ready queue + `idle.is_none()` panics — but its commentary needs to update.

### Neutral

- **Idle's `TaskContext` still lives in `Scheduler::contexts[idle_handle.slot().index()]`.** No memory-layout change; the `register_idle` path uses `init_context` exactly as `add_task` does today. The only difference is that the resulting `TaskHandle` is stored in `self.idle` instead of being routed through `self.ready.enqueue`.
- **`TASK_ARENA_CAPACITY` impact unchanged.** Idle still consumes one slot in the BSP-side `TaskArena` (it has a `TaskHandle` regardless of where the handle is stored). v1's 16-slot capacity drops effective workload to 15 either way.
- **Per-yield cost.** Identical or slightly lower than Option A: the dispatcher's `ready.dequeue().or_else(|| self.idle)` chain is one `Option::or_else` call when the queue is empty, identical to direct dequeue when it is not. No measurable delta at v1 scale.
- **ADR-0022 §Revision notes' first rider's *Sub-rider*** (which closed 2026-04-28 with T-012's WFI activation) remains accurate as a description of *what idle's body does*; T-014's idle body shape is unchanged from T-012's `wait_for_interrupt + yield_now`. The only change is *when the dispatcher selects idle*. The 2026-04-28 Sub-rider closure stands.

## Pros and cons of the options

### Option A — Idle as a regular task (status quo, superseded)

- Pro: zero new fields on `Scheduler<C>`; idle uses `add_task` like any other task.
- Pro: zero new dispatch branches; FIFO logic unchanged.
- Pro: zero new `unsafe`; zero new audit-log entries.
- **Con (load-bearing):** dispatches idle when other tasks are Ready (the smoke regression). Demonstrated to hang the v1 cooperative IPC demo at HEAD `214052d`.
- Con: implicit duality (idle vs real tasks share a queue but have different semantics); the "yield_now's one-ready fast path collapses solo-idle" claim is only true when idle is alone, which is not the demo's flow.
- Con: per-CPU idle (Phase C SMP) requires reworking the FIFO into per-CPU FIFOs + idle-rotation logic; Option B already has the right data type.

### Option B — Dedicated idle slot (chosen)

- Pro: structurally rules out the smoke regression; idle never displaces a real Ready task.
- Pro: the data type matches the semantics — `idle: Option<TaskHandle>` says exactly "the dispatcher's fallback" and nothing else.
- Pro: forward-compatible with per-CPU idle in Phase C SMP.
- Pro: makes `SchedError::Deadlock` a meaningful diagnostic ("BSP forgot to register idle") rather than a defensive-only return.
- Pro: ~30-50 LOC kernel diff + 1 BSP `kernel_entry` line; bounded cost.
- Con: one extra dispatch branch per yield (`Option::or_else`); negligible at v1 scale, mechanically auditable.
- Con: one new invariant (idle is never in the ready queue); cheap to enforce via `debug_assert!` and unit-testable.
- Con: ADR-0022's Pro "zero new code" no longer applies — Option B is more code than Option A by ~30-50 LOC. Acceptable because the alternative is silent hang.

### Option C — Inline WFI loop in the scheduler

- Pro: zero tasks, zero stack, zero `TaskHandle` for idle.
- Con: no `TaskContext` for idle → preemption (Phase B5+) requires a rewrite (preempting IRQ must return *to* a task, and there is none).
- Con: kernel-context WFI is harder to debug than task-context WFI.
- Con: weakens the "ready queue never empty when `current.is_some()`" invariant to "ready queue never empty OR we are in the idle loop"; every future scheduler change must account for the second clause.

### Option D — Kernel-allocated idle

- Pro: the BSP does not need to write an idle entry function.
- Con: forces a generic `idle_loop<C: Cpu>` helper and a new raw-pointer scheduler API that takes `*mut TaskArena`; widens ADR-0021's surface.
- Con: violates ADR-0016's "BSP owns the arenas" framing.
- Con: the idle entry cannot easily reach the BSP's CPU singleton; either takes a `cpu: *const C` parameter (breaking the `fn() -> !` constraint) or relies on a BSP-side thread-local.

### Option E — Patch Option A's FIFO ordering

- Pro: minimum diff; preserves "zero new struct field" property.
- Con: papers over the symptom; the structural duality (idle vs real tasks in same queue) remains.
- Con: any future workload with a third priority class will rediscover the ad-hoc-priority-via-queue-position problem.
- Con: "always put idle at tail" requires every `enqueue` site to know about idle; "bump unblocked receivers to head" introduces a per-message-class queue-position rule that will not generalise to preemption.
- Con: Phase C SMP's per-CPU idle is harder to express as queue-ordering than as per-CPU `idle` slots.

## References

- [ADR-0022 — Idle task and typed scheduler deadlock error](0022-idle-task-and-typed-scheduler-deadlock.md) — the superseded decision; this ADR replaces only its idle-task-location axis.
- [ADR-0019 — Scheduler shape](0019-scheduler-shape.md) — defines the FIFO ready queue and `SchedError`.
- [ADR-0021 — Raw-pointer scheduler IPC-bridge API](0021-raw-pointer-scheduler-ipc-bridge.md) — the raw-pointer convention `register_idle` follows.
- [ADR-0025 — ADR governance amendments](0025-adr-governance-amendments.md) — §Rule 1 (forward-reference contract) governs T-014's opening alongside this ADR's Propose commit.
- [B1 smoke-regression mini-retro (2026-05-06)](../analysis/reviews/business-reviews/2026-05-06-B1-smoke-regression.md) — the empirical event motivating the supersession.
- [Comprehensive code review (2026-05-06)](../analysis/reviews/code-reviews/2026-05-06-full-tree-comprehensive.md) — the multi-agent review that ran earlier the same day; its Track A (Kernel correctness) approved the dispatch path on static analysis, illustrating the simulation gap this ADR's §Decision outcome closes.
- [`kernel/src/sched/mod.rs`](../../kernel/src/sched/mod.rs) — the dispatch sites T-014 will modify.
- [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs) — the `kernel_entry` call site that switches from `add_task(idle_entry, ...)` to `register_idle(idle_entry)`.
