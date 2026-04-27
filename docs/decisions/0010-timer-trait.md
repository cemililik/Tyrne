# 0010 — `Timer` HAL trait signature (v1)

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

The fourth HAL trait in Phase 4b, after Console (ADR-0007), Cpu (ADR-0008), and Mmu (ADR-0009). The `Timer` provides monotonic time and the ability to schedule a one-shot deadline that arrives as an interrupt on the timer's hardware IRQ line. It is the foundation for:

- The scheduler's tick (an armed deadline that re-arms in its ISR handler).
- Deadline-based task wakeups (`time_sleep_until` syscall).
- Operation timeouts in kernel and userspace services.
- Any "how long ago" question the kernel needs to answer.

The v1 scope is, like Cpu v1, single-core: the trait describes the current core's timer view. Multi-core timer coordination, per-core timers, and alternate clock sources are deferred to later ADRs.

## Decision drivers

- **Scheduler compatibility.** The scheduler computes "the time at which the next task should wake" and asks the timer to fire then. Absolute deadlines compose better than relative delays for this use.
- **Direct hardware mapping.** On aarch64, the ARM generic timer's `CNTVCT_EL0` read gives current time and `CNTV_TVAL_EL0` / `CNTV_CVAL_EL0` set the deadline. Each trait method should map to a handful of system-register accesses.
- **Object-safe.** The kernel uses `&'static dyn Timer`; every method must dispatch through a vtable.
- **`Send + Sync`.** Enforced at the trait bound, consistent with the rest of the HAL.
- **No allocation.** Consistent with the rest of the HAL.
- **Unambiguous units.** One unit of time throughout the trait; no mixing of ticks and nanoseconds or of seconds and nanoseconds.
- **Honest "at most one deadline" semantics.** The underlying hardware generic timer has a single compare register; exposing a pretence of unlimited deadlines would require software multiplexing that the scheduler is the right owner of, not the HAL.

## Considered options

### Option A — absolute deadlines only

```rust
pub trait Timer: Send + Sync {
    fn now_ns(&self) -> u64;
    fn arm_deadline(&self, deadline_ns: u64);
    fn cancel_deadline(&self);
    fn resolution_ns(&self) -> u64;
}
```

### Option B — relative delays only

```rust
pub trait Timer: Send + Sync {
    fn now_ns(&self) -> u64;
    fn arm_delay(&self, delay_ns: u64);
    fn cancel(&self);
    fn resolution_ns(&self) -> u64;
}
```

### Option C — both absolute and relative

`arm_deadline` and `arm_delay` both on the trait.

### Option D — `Instant` / `Duration` newtypes

Define `pub struct Instant(u64); pub struct Duration(u64);` as the time unit, with arithmetic operators.

## Decision outcome

**Chosen: Option A — absolute deadlines only, with `u64` nanoseconds as the time unit, no newtype wrappers.**

```rust
pub trait Timer: Send + Sync {
    fn now_ns(&self) -> u64;
    fn arm_deadline(&self, deadline_ns: u64);
    fn cancel_deadline(&self);
    fn resolution_ns(&self) -> u64;
}
```

Semantics:

- `now_ns` is nanoseconds since boot, monotonic. Never goes backwards across suspend or core switches within a single-core v1. 64 bits give ~584 years of runtime before wrap — comfortably beyond any kernel instance.
- `arm_deadline` takes an absolute deadline. When `now_ns()` reaches or exceeds `deadline_ns`, the hardware timer IRQ fires. At most one deadline is armed at any time; a subsequent `arm_deadline` replaces the previous one. Arming a deadline in the past causes the IRQ to fire as soon as the hardware can deliver it — BSPs must not silently drop past-deadline arms.
- `cancel_deadline` clears any pending deadline. A no-op when none is armed. Does not change `now_ns`.
- `resolution_ns` returns the minimum meaningful deadline granularity for this implementation. Precision beyond the resolution is silently lost (rounded to the nearest multiple).

Relative delays (Option B) were rejected because the scheduler most naturally expresses wake-ups as absolute times. A relative interface would force `now + delta` at every call site, introducing drift when the arithmetic and the arming of the deadline are not atomic. Having both (Option C) adds surface without adding capability — `arm_delay(d)` is indistinguishable from `arm_deadline(now_ns() + d)`.

`Instant` / `Duration` newtypes (Option D) were rejected for v1 because they add type-level ceremony for little gain while the trait has only one time argument per method. The kernel layer above the HAL may introduce its own types; the HAL keeps the primitive interface simple.

## Consequences

### Positive

- **Four-method object-safe surface**, directly mappable to ARM generic timer operations.
- **Scheduler-friendly.** Absolute deadlines compose with the scheduler's "next wake at" computation.
- **No drift from unit mixing.** Nanoseconds throughout; no seconds/nanos pair, no tick/ns crossover.
- **Hardware-honest.** The single-deadline model matches the single compare register; software multiplexing of many deadlines is the scheduler's job, not the HAL's.
- **Simple test fake.** `FakeTimer` exposes `set_now` / `advance` / `armed_deadline` / `cancel_count`.

### Negative

- **`u64` nanoseconds is not distinguishable from other `u64`s at the type level.** A caller could pass a cycle count or a microsecond count by mistake. Mitigation: strict rustdoc; kernel-level `Instant` / `Duration` types in a later abstraction layer.
- **No per-core timer in v1.** When multi-core arrives, the Timer trait will either gain a `core: CoreId` parameter (object-breaking for BSPs) or a sibling trait for per-core operations. We expect the sibling-trait pattern, the same as Cpu.
- **Arming in the past is BSP-dependent in exact firing latency.** Specified as "fire as soon as possible" but the exact delay depends on the hardware timer's compare logic. Documented, not specified-to-the-nanosecond.

### Neutral

- No periodic / tick API. A tick is implemented as "fire a deadline, handler re-arms for now + period." The HAL does not need to know it is a tick; the scheduler's handler does.
- No `Result` returns. None of the four operations can fail in a way the caller can do anything useful about: `now` always answers, `arm` always succeeds (even if the deadline is past), `cancel` is a no-op when idle, `resolution` is a constant.

## Pros and cons of the options

### Option A — absolute deadlines only (chosen)

- Pro: composable; scheduler-natural.
- Pro: small surface, clear semantics.
- Con: callers who want "N ns from now" write `now() + N` — one extra call.

### Option B — relative delays only

- Pro: ergonomic for timeout-style code.
- Con: drift when the caller computes delay but arms later.
- Con: awkward for scheduler-style code that manages a queue of absolute wake times.

### Option C — both absolute and relative

- Pro: two-idiom support.
- Con: more surface; one idiom is expressible in the other; redundant.

### Option D — `Instant` / `Duration` newtypes

- Pro: type-safe units.
- Con: ceremony; one-argument functions benefit less than multi-argument ones.
- Con: commits the HAL to a particular time representation; changing later is a bigger break.

## Open questions

Each is a future ADR.

- **Per-core timers.** When multi-core lands, do we extend `Timer` with `CoreId`, or introduce a sibling `PerCoreTimer` trait?
- **Periodic / tick API.** Useful primitive or scheduler's job? Lean: scheduler's job, but revisit if several kernel subsystems reimplement the same "re-arm in handler" pattern.
- **Higher-resolution clocks / alternate sources.** HPET, APIC-timer, SoC-specific high-resolution timers. Likely a separate `HighResTimer` trait selected via build config, not a replacement for the baseline `Timer`.
- **Suspend / low-power semantics.** Does `now_ns` freeze across a suspend? Today the answer is "no suspend yet, so the question doesn't arise"; to be pinned when power management arrives.
- **`core::time::Duration` interop.** At what layer of the kernel (above the HAL) do we introduce `Instant` / `Duration` types? Possibly never — maybe the `u64` interface is enough forever. Revisit when scheduler grows.
- **Deadline cancellation handle.** Should `arm_deadline` return a handle that `cancel_deadline_handle(h)` takes, for "I want to cancel *my* deadline, not whatever the scheduler armed last?" v1 assumes a single scheduler owner of the timer; multi-owner needs handles.

## Revision notes

- **2026-04-27 — pointer to architecture doc.** [T-008](../analysis/tasks/phase-b/T-008-architecture-docs.md) added a Timer subsection in [`docs/architecture/hal.md`](../architecture/hal.md) that synthesises this ADR with the post-T-009 implementation details (CNTVCT register-family choice, `tyrne_hal::timer` helper functions, IRQ-armed half deferred to T-012). The ADR body is unchanged; this rider provides the bidirectional cross-reference T-008's DoD asks for ("ADRs cited from architecture docs are the same ADRs whose §References sections cite the new architecture docs").

## References

- [ADR-0006: Workspace layout](0006-workspace-layout.md).
- [ADR-0007: Console trait](0007-console-trait.md).
- [ADR-0008: Cpu trait](0008-cpu-trait.md).
- [ADR-0009: Mmu trait](0009-mmu-trait.md).
- [`docs/architecture/hal.md`](../architecture/hal.md).
- ARM *Architecture Reference Manual* — generic timer (`CNTVCT_EL0`, `CNTV_CVAL_EL0`, `CNTV_TVAL_EL0`, `CNTV_CTL_EL0`).
- Tock kernel `kernel::hil::time::Alarm` — prior art for a comparable abstraction.
- Hubris timer primitives — https://hubris.oxide.computer/
