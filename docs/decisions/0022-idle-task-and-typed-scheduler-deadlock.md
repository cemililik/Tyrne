# 0022 — Idle task and typed scheduler deadlock error

- **Status:** Accepted
- **Date:** 2026-04-22
- **Deciders:** @cemililik

## Context

The A5 scheduler ([ADR-0019](0019-scheduler-shape.md)) contains two hard-panic paths that survive into Phase B unchanged:

1. **`sched::ipc_recv_and_yield` deadlock.** When the caller has been marked `Blocked` and the ready queue is then dequeued, if the queue is empty every task in the system is blocked on IPC — the scheduler cannot pick a next task. The current code path at [`kernel/src/sched/mod.rs`](../../kernel/src/sched/mod.rs) (the `ipc_recv_and_yield` body) calls `panic!("deadlock: all tasks blocked on IPC and no idle task available")`. In Phase A this was acceptable because the A6 demo has exactly two tasks that always make progress together; in Phase B every subsequent milestone (EL drop, MMU on, address spaces, syscalls, userspace) adds paths where an ill-formed IPC graph can reach this state without a kernel bug, and panicking on a userspace-caused condition is a kernel-liveness failure.

2. **`sched::start` empty ready queue.** If `start` is called without any prior `add_task`, it panics. This is a programming error at boot — but by the same standard it is also a typed-error opportunity, and Phase B's integration boundaries (BSP init from a config, userspace root-task spawn) benefit from a uniform "init returned Err, shut down cleanly" shape rather than a panic.

The 2026-04-21 Phase-A-exit security review flagged path (1) as §4 (liveness). The code review's *Correctness* section flagged both paths and also flagged the resume-path `debug_assert!` in `ipc_recv_and_yield` that guards against `ipc_recv` returning `Pending` after a context-switch resume — a release build silently drops the assertion and returns `Ok(RecvOutcome::Pending)` to the caller, which the caller's `RecvOutcome::Received { … }` let-else turns into a panic one frame up. The `debug_assert` is correct as a test-time invariant check but the release fallback should be a typed error, not a downstream crash. Phase-B §B0 plan item 2 bundles all three hardenings into a single ADR so the typed-error taxonomy is designed once.

Two questions therefore need to be settled together before any code change:

- **Mechanism** — how does the scheduler avoid reaching "ready queue empty" in the cooperative single-core case? An idle task is the standard answer, but where it lives, how it is registered, and how it is dispatched all admit several shapes.
- **Surface** — what typed errors replace the three panics, and where do they appear in `SchedError`?

The two questions are coupled: with an idle task registered the liveness-bug panic in (1) becomes structurally unreachable, so the typed error for (1) is a defensive return (preemption, SMP, idle-not-registered) rather than the everyday path. Without an idle task the typed error *is* the everyday path.

**Constraints inherited from earlier ADRs.**

- All kernel state is statically bounded — no heap ([ADR-0016](0016-kernel-object-storage.md)).
- Task entry functions are `fn() -> !` — no environment capture ([ADR-0020](0020-cpu-trait-v2-context-switch.md)).
- The scheduler bridge is a set of `unsafe fn` free functions over `*mut Scheduler<C>` ([ADR-0021](0021-raw-pointer-scheduler-ipc-bridge.md)); any new API follows the same shape.
- `TaskArena` is a BSP-owned `StaticCell` (T-006 / K3-11); the scheduler cannot allocate a `TaskHandle` without pointer access to that arena.
- `Cpu::wait_for_interrupt` ([ADR-0008](0008-cpu-trait.md)) is already available — the idle loop has a zero-cost primitive.
- Tyrne v1 is single-core cooperative; no timer IRQ yet (the first timer wiring is T-009). An idle task today sleeps the CPU until an IRQ that never arrives in pure-IPC systems — which is still strictly better than busy-spinning in kernel context.

## Decision drivers

- **Kernel liveness.** A userspace-reachable condition must not panic the kernel. Deadlock is a userspace bug and belongs at the `Result` boundary, not at `panic!`.
- **Minimal invariant surface.** "The ready queue is never empty while `current.is_some()`" is one of the scheduler's cleanest invariants; preserving it without special cases is worth shape constraints elsewhere.
- **Consistency with ADR-0021.** Any new scheduler API takes `*mut Scheduler<C>` and honours the "no `&mut` across the switch" rule.
- **Single-core cooperative fit.** The v1 workload is two-task IPC; the idle task must not cost more than a handful of lines in `kernel::sched` and a few dozen bytes of idle stack in the BSP.
- **Forward compatibility.** Preemption (later Phase B or Phase C), SMP (Phase C), and a timer tick (T-009) all change the idle/deadlock analysis. The shape chosen now should extend, not require rewriting, when those arrive.
- **Audit surface.** New `unsafe` must be justified and minimal. The idle task's stack and entry function should not introduce a new `unsafe` pattern; reusing the `TaskStack` + `fn() -> !` pattern established by `task_a` / `task_b` means no new audit-log entries are needed for the idle task itself.

## Considered options

### Idle-task location

**Option A — Idle is a regular task in the ready queue.**
BSP calls `create_task` for idle the same way it does for `task_a` / `task_b`, provides an idle-entry `fn() -> !` (a BSP function that loops `cpu.wait_for_interrupt()` + `yield_now`), and calls `add_task` to register it. No new scheduler API. The scheduler's ready queue never goes empty because idle is always ready or currently running.

**Option B — Idle is a dedicated slot on `Scheduler<C>`.**
`Scheduler` gains a field `idle: Option<TaskHandle>` and a `register_idle` free function; `dequeue`-with-fallback picks idle when the ready queue is empty. Idle never enters the ready queue — it is dispatched only as the fallback.

**Option C — No idle task; inline WFI loop in the scheduler.**
When the ready queue would be empty, the scheduler enters a kernel-context `loop { cpu.wait_for_interrupt(); recheck_queue(); }` without a context switch. No task, no stack, no handle.

**Option D — Idle owned by kernel, allocated in `TaskArena` via a new kernel API.**
`kernel::sched::register_idle(sched, task_arena, cpu, stack_top)` internally calls `create_task`, produces a kernel-owned idle entry via a generic helper `idle_loop::<C>`, and adds it to the ready queue. BSP only supplies the `TaskArena` pointer, the `Cpu`, and the stack.

### Typed error shape

**Option E — Single `SchedError::Deadlock` variant.**
One variant covers all three current panics; context is carried by the call site (caller knows whether it was `start`-no-tasks, `ipc_recv_and_yield`-all-blocked, or resume-path-Pending).

**Option F — Three distinct variants.**
`SchedError::QueueEmpty` (from `start`), `SchedError::Deadlock` (from `ipc_recv_and_yield`), and `SchedError::IpcPendingAfterResume` (from the debug_assert converted to Err). Each is self-describing.

**Option G — Two variants + keep `start`'s panic.**
`SchedError::Deadlock` and `SchedError::Ipc(IpcError::PendingAfterResume)` (folded into the existing `Ipc` variant). `start`'s empty-queue stays a panic because it is strictly a boot-time programming error, not a runtime condition.

## Decision outcome

**Chosen: Option A (idle as a regular task) + Option G (two new error surfaces, keep `start`'s panic).**

**Idle-as-regular-task** is the simplest shape that satisfies every driver. The scheduler keeps its current FIFO dispatch with no fallback branch; the ready queue never goes empty because idle is always enqueued or currently running; and the only new code is a BSP function `fn idle_entry() -> !` that calls `cpu.wait_for_interrupt()` then `sched::yield_now(...)` in a loop. No new scheduler API, no new `Scheduler<C>` field, no new `unsafe`. Per-yield cost is whatever `yield_now` costs today plus one `wfi` — entirely negligible at the v1 scale and transparently correct: when idle yields to itself (only task ready), `yield_now`'s existing "only one ready task" early-return path handles the case without a context switch, so the idle loop degrades to exactly the WFI spin of Option C when idle is genuinely alone.

Option B's dedicated-slot design is rejected because it introduces a scheduler-state duality — idle lives in `Option<TaskHandle>` but "regular" tasks live in the ready queue — and that duality shows up as a new branch in every dispatch decision (`dequeue_ready_or_idle`), a new invariant (idle never double-enqueued), and new audit surface for the dispatch branch. The ergonomic gain is nil at v1 scale; the invariant cost compounds in Phase C's preemption work. Option C (inline WFI) is tempting because it is "zero tasks, zero stack" but it foregoes a `TaskContext` for the idle path, which makes preemption impossible to add later without a rewrite (the preempting IRQ handler must have a kernel task to return to; inline-WFI-in-the-scheduler has none). Option D (kernel-allocated idle) is rejected because it forces a generic `idle_loop<C: Cpu>` helper and a new scheduler API for no ergonomic benefit the BSP cannot already provide with Option A; it also forces every BSP to hand over a raw `*mut TaskArena` into a kernel function, widening the ADR-0021 surface without need. Keeping idle-task allocation in the BSP preserves ADR-0016's "BSP owns the arenas" framing.

**Two typed error surfaces** (Option G) captures the runtime conditions without inflating `SchedError`'s variant count for a condition that should not fire in the cooperative v1 workload. `SchedError::Deadlock` covers the "ready queue empty after blocking current" path inside `ipc_recv_and_yield`; with idle registered this is structurally unreachable, so the variant is a defensive return for future preemption / SMP / "caller forgot to register idle". The resume-path condition becomes `IpcError::PendingAfterResume` (extending the existing `IpcError` enum rather than inflating `SchedError`) and propagates through `SchedError::Ipc(…)` — keeping IPC's fault taxonomy inside IPC and the scheduler's fault taxonomy scheduler-scoped. `start`'s empty-queue panic **is kept** because the only way to reach it is a boot-time kernel-entry bug: the BSP forgot to register any task before calling `start`. There is no caller that can recover from this; converting it to `Err` means kernel_entry has to panic one frame up instead, which is strictly more code and less informative. Panicking exactly where the invariant is violated is the right call for a boot-time programming error.

Option E (single `Deadlock` variant) is rejected because it conflates the IPC resume-path condition with scheduler queue exhaustion — two orthogonal faults. Option F is rejected because `SchedError::QueueEmpty` adds a runtime variant for a boot-time-only condition; inflating the enum encourages future code to conflate "boot setup failed" with "runtime fault", which is exactly the conflation the security review flagged.

## Consequences

### Positive

- **Kernel-liveness panics become typed returns.** The `ipc_recv_and_yield` deadlock path and the resume-path `debug_assert` both become propagated errors; no userspace-reachable path can panic the kernel through the scheduler after T-007.
- **Invariant-preserving shape.** "Ready queue never empty when `current.is_some()`" becomes a structural property of the scheduler rather than a hoped-for outcome. The `debug_assert_ne!(current_idx, next_idx)` introduced by T-006's post-review fix continues to hold trivially.
- **Zero new `unsafe`.** Idle task uses the BSP's existing `TaskStack` pattern, existing `add_task` surface, and existing `yield_now` / `wfi` primitives. No new audit-log entries.
- **Forward compatibility with preemption.** When a timer tick lands (T-009 groundwork, full preemption later), idle already has a `TaskContext` and can be preempted like any other task — no special case. Option C would have required a rewrite here.
- **Consistency with ADR-0021.** Idle registration uses `sched::add_task` which is already raw-pointer-safe; no new `&mut Scheduler` crosses any switch.
- **Audit-log status for the retired panics.** UNSAFE-2026-0012's "§ Post-review rider" pattern (panic → typed error documented in-place) extends naturally to the two retired panics.

### Negative

- **One extra task slot consumed for idle.** `TASK_ARENA_CAPACITY` is 16 in v1; idle consuming one slot reduces the workload cap to 15. *Mitigation:* none needed at v1 scale; when `TASK_ARENA_CAPACITY` becomes a real constraint (Phase C userspace), bumping the arena capacity is a one-line ADR-level change that has been anticipated in ADR-0016.
- **Per-yield cost gains one `wfi` instruction when idle is scheduled.** Measurable only once T-009 wires `CNTPCT_EL0`; expected to be unobservable at v1 cadence. *Mitigation:* none needed — `wfi` is explicitly designed to be cheap and this is exactly the workload it targets.
- **BSP now owns an idle entry function per board.** Every future BSP (the rpi4 BSP is next; more follow in later phases) must provide `fn idle_entry() -> !`. *Mitigation:* the entry is ~3 lines (loop over `cpu.wait_for_interrupt()` + `yield_now`); it is part of the BSP contract and belongs next to the other BSP task entries. If the pattern duplicates enough to justify a helper, ADR-0020 can grow a `Cpu::idle_loop` default-method; this is deferred until the second BSP demonstrates the duplication.
- **`SchedError::Deadlock` is a variant nobody reaches in v1.** Dead-variant drift is a known code smell; a defensive return that can't be exercised by any test invites rot. *Mitigation:* T-011's missing-tests bundle adds a unit test that directly constructs a scheduler, skips idle registration, blocks the sole task, and asserts `Err(SchedError::Deadlock)` — the variant is covered even if the integration path is unreachable by construction.

### Neutral

- **`start`'s empty-queue panic survives.** Intentional; see *Decision outcome*. The ADR documents the choice so a future reviewer does not revisit it.
- **`SchedError` variant count grows by one** (`Deadlock`); `IpcError` grows by one (`PendingAfterResume`). Both small, both flagged `#[non_exhaustive]` already per ADR-0017 / ADR-0019 conventions.
- **Idle stack size is a BSP-level parameter**, not a scheduler decision. v1 uses the same 4 KiB `TaskStack` as other tasks; tuning is a BSP concern when the memory budget tightens.

## Pros and cons of the options

### Option A — Idle as a regular task (chosen)

- Pro: zero new scheduler API; idle uses `add_task` + `yield_now` unchanged.
- Pro: zero new `unsafe`; zero new audit entries.
- Pro: `Scheduler<C>` struct unchanged; no dual-dispatch branch.
- Pro: `yield_now`'s existing "only one ready task" fast path collapses the solo-idle case to an inline WFI loop — matching Option C's efficiency without Option C's preemption cost.
- Pro: forward-compatible with preemption — idle has a normal `TaskContext`.
- Con: consumes one `TaskArena` slot.
- Con: the idle-priority concept is absent — if Phase B grows priorities, idle becoming "lowest priority" is a follow-up ADR, not an intrinsic property. Acceptable because priorities are not on the B0/B1 roadmap.

### Option B — Dedicated idle slot on `Scheduler<C>`

- Pro: idle cannot accidentally be treated as a regular task (e.g. cannot appear in any "iterate ready tasks" loop a future developer writes).
- Con: introduces a dual-dispatch branch in every `dequeue` call site; new invariant "idle never double-enqueued" must be maintained.
- Con: new scheduler API (`register_idle`, `dequeue_ready_or_idle`) → new audit surface for the dispatch path.
- Con: loses Option A's "yield_now's one-ready fast path" — the idle slot needs its own fast path.

### Option C — Inline WFI loop in the scheduler

- Pro: zero tasks, zero stack, zero arena slot consumed.
- Con: no `TaskContext` for idle → preemption requires a rewrite (preempting IRQ must return to a task, and there is none).
- Con: entering the WFI loop happens in kernel context with no saved registers; debugging a hang at this point is harder than debugging a hang in a normal task.
- Con: the `ready queue is never empty` invariant becomes `ready queue is never empty OR we are in the idle loop` — a weaker invariant that every future scheduler change must account for.

### Option D — Kernel-allocated idle

- Pro: the BSP does not need to write an idle entry function.
- Con: forces a generic `idle_loop<C: Cpu>` helper and a new raw-pointer scheduler API that takes `*mut TaskArena`; widens ADR-0021's surface.
- Con: violates ADR-0016's "BSP owns the arenas" framing; pushes arena allocation into kernel code for no ergonomic benefit over Option A.
- Con: the idle entry cannot easily reach the BSP's CPU singleton (CPU is BSP-typed, not kernel-typed), so the helper either takes a `cpu: *const C` parameter (breaking the `fn() -> !` constraint) or relies on a BSP-side thread-local.

### Option E — Single `SchedError::Deadlock` variant

- Pro: minimal enum churn.
- Con: conflates two orthogonal runtime conditions (ready-queue exhaustion vs. IPC resume-path Pending), making the error's meaning context-dependent. Callers must parse the call site to know what happened.

### Option F — Three distinct variants

- Pro: each error is self-describing.
- Con: `SchedError::QueueEmpty` adds a runtime variant for a boot-time-only condition; future code may accidentally treat them as peers and handle a programming error as if it were a runtime condition.
- Con: `IpcPendingAfterResume` belongs inside the IPC fault taxonomy, not the scheduler's.

### Option G — Two new error surfaces (chosen)

- Pro: scheduler errors and IPC errors stay in their respective enums; `SchedError::Ipc(IpcError::PendingAfterResume)` is the natural propagation shape.
- Pro: `start`'s empty-queue panic is kept where the invariant is violated — a boot-time kernel-entry programming error has a panic, a runtime fault has an `Err`.
- Con: `SchedError::Deadlock` is dead-variant-shaped at v1 because idle registration makes it unreachable; T-011 adds a direct unit test to keep the variant live.

## Revision notes

- **2026-04-22 — T-007 implementation rider.** Option A's *Decision outcome* paragraph described idle's body as `cpu.wait_for_interrupt()` + `yield_now`. During T-007 implementation, QEMU smoke confirmed that this hangs the v1 demo: FIFO scheduling dispatches idle between two ready application tasks (e.g. after Task A's `ipc_send_and_yield` unblocks Task B and yields, the ready queue head becomes idle before B), and with **no IRQ source configured in v1** (timer wiring is T-009), `wfi` suspends the core indefinitely. The hang is a trace truncation at "Task A — sending IPC" — the demo never reaches B's reply.
  The fix lands in T-007: idle's body is `core::hint::spin_loop()` + `yield_now` for now. Shape and registration path are unchanged; only the suspend primitive is deferred. When T-009 wires a timer IRQ (and guarantees a periodic wake source), idle's body becomes `wait_for_interrupt` + `yield_now` and no other call site moves. Option A's "yield_now's single-ready fast path collapses solo-idle to a tight WFI loop" claim is therefore **accurate only once a wake source exists**; until then idle spin-yields.
  This rider does not change the chosen option. It corrects the *Decision outcome*'s implied workload claim (WFI is cheap only when there is something to wake you up) and it records the T-006-retro's "trace the call graph" lesson applied in hindsight: the ADR assumed idle would run rarely and briefly, but FIFO places idle at the head of the ready queue on a regular cadence, making WFI's IRQ-source dependency load-bearing even for the two-task demo. **Cost of the adjustment:** one extra context switch per inter-task yield when idle happens to sit at the head — acceptable for v1; revisited when performance measurements are wired in T-009.

  **Second rider — resume-path `debug_assert!` dropped.** The original *Decision outcome* kept the `debug_assert!` in `ipc_recv_and_yield`'s resume path as a "loud test-mode complement" to the typed `IpcError::PendingAfterResume` return. During T-007 the two assertions were found to be fundamentally in tension: the test that verifies the release-mode typed-error path has to produce the pathological `Ok(Pending)` resume on purpose, and the `debug_assert!` fires first — making the typed path untestable without a `cfg(not(test))` guard or a `should_panic` hack. The debug_assert is dropped; the typed `Err(SchedError::Ipc(IpcError::PendingAfterResume))` is the observable contract and, when unhandled, surfaces at the caller's error path carrying full bridge context — which is strictly more informative than a stripped-in-release `debug_assert` message. This is consistent with the "invariant assertions must be testable" lesson from the T-006 retro: an untestable assertion rots.

## References

- [ADR-0013 — Roadmap and planning process](0013-roadmap-and-planning.md) — the planning framework this ADR fits under.
- [ADR-0016 — Kernel object storage](0016-kernel-object-storage.md) — defines `TaskArena` and the BSP-owns-arenas framing the idle-task decision honours.
- [ADR-0017 — IPC primitive set](0017-ipc-primitive-set.md) — defines `IpcError`; this ADR extends it with `PendingAfterResume`.
- [ADR-0019 — Scheduler shape](0019-scheduler-shape.md) — defines `SchedError`; this ADR extends it with `Deadlock`.
- [ADR-0020 — `ContextSwitch` trait and `Cpu` v2](0020-cpu-trait-v2-context-switch.md) — defines `Cpu::wait_for_interrupt`, the idle loop's core primitive.
- [ADR-0021 — Raw-pointer scheduler IPC-bridge API](0021-raw-pointer-scheduler-ipc-bridge.md) — the raw-pointer convention the idle-registration path inherits.
- [Security review — Tyrne → Phase A exit](../analysis/reviews/security-reviews/2026-04-21-tyrne-to-phase-a.md) — §4 (liveness) flags the deadlock panic.
- [Code review — Tyrne → Phase A exit](../analysis/reviews/code-reviews/2026-04-21-tyrne-to-phase-a.md) — *Correctness (Scheduler bullets 2, 4)* flags the three panics and the resume-path assertion.
- [Phase B plan](../roadmap/phases/phase-b.md) — §B0 item 2 bundles the three hardenings into this ADR; T-007 is the implementation task.
- [T-006 mini-retro](../analysis/reviews/business-reviews/2026-04-22-T-006-mini-retro.md) — the "trace who holds &mut across switch/lock" lesson applied here to the idle-registration path (idle's `add_task` call is a single-shot init, no cross-switch borrow).
