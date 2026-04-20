# Phase C — Multi-core

**Exit bar:** Two or more cores running concurrently, scheduled preemptively, with working cross-core IPC.

**Scope:** Secondary core start via PSCI, per-core state, preemptive scheduling with timer tick, cross-core IPC, multi-core TLB shootdown. Still on QEMU virt; Pi 4 is Phase D.

**Out of scope:** Real hardware, userspace drivers, filesystem, network.

---

## Milestone C1 — Secondary core start

Bring secondary cores online via PSCI `CPU_ON`. Each core arrives at a kernel entry point and waits until the primary core hands it work.

### Sub-breakdown

1. **ADR-0027 — Secondary core start protocol.** PSCI vs. spin-table. Entry point for secondaries (shared with primary or separate). Rendezvous semantics (when the primary considers a secondary "up").
2. **`Cpu` trait v3 extension** — adds `start_core(core_id, entry, context)` and `core_count()`; probably as a sibling `MultiCore` trait to keep `Cpu` v2 stable.
3. **Secondary-core asm entry** in `bsp-qemu-virt` — minimal per-core stack setup before Rust.
4. **Per-core state struct** introduced here (fully fleshed out in C2).
5. **Tests** — QEMU run with `-smp 4` brings all four cores to a known checkpoint; serial shows each core announcing itself.

### Acceptance criteria

- ADR-0027 Accepted.
- `Cpu::start_core` (or sibling trait) lands in `umbrix-hal`.
- All configured cores reach the Rust-level rendezvous point on QEMU.

---

## Milestone C2 — Per-core state

Every online core needs its own current-task pointer, IRQ-mask shadow, and scheduler queue (if per-core queues are chosen).

### Sub-breakdown

1. **ADR-0028 — Per-core state access pattern.** `TPIDR_EL1` pointer vs. indexed lookup. Thread-local-like access to the current core's state.
2. **`PerCore<T>` abstraction** — kernel-provided primitive for per-core state with interior synchronization.
3. **Current-task pointer** moved to per-core state.
4. **Tests** — each core sees its own state; no accidental cross-core access.

### Acceptance criteria

- ADR-0028 Accepted.
- Per-core state accessible from any core via the chosen pattern.
- Tests cover the access invariants.

---

## Milestone C3 — Preemptive scheduler (with timer tick)

Replace the cooperative scheduler from A5 with a preemptive one driven by the timer tick. Per-core scheduling queues (probably; ADR decides).

### Sub-breakdown

1. **ADR-0029 — Scheduler topology.** Per-core queues with work stealing, vs. a single global queue with locking, vs. hybrid. Real-time guarantees (or the lack thereof).
2. **Timer tick wiring** — [`Timer`](../../../hal/src/timer.rs) arm-deadline fires an IRQ; [`IrqController`](../../../hal/src/irq_controller.rs) delivers it; ISR triggers the scheduler's tick handler.
3. **Preemption points** — when and how a running task can be interrupted and the scheduler invoked.
4. **Time slice** — configurable per-task or global for v1.
5. **Idle-core behaviour** — WFI until IRQ, wake on timer or work-steal signal.
6. **Interrupt-masked critical-section primitive on [`umbrix-hal::Cpu`](../../../hal/src/cpu.rs).** Introduce a closure-based `Cpu::without_interrupts(|| { ... })` (equivalent of `x86_64::instructions::interrupts::without_interrupts`) backed by aarch64 `DAIF` manipulation. Every spin-locked kernel resource that an IRQ handler can touch must be acquired inside this closure to avoid handler-vs.-main-path deadlock. Discipline is mandatory, not optional; C3 makes it real because this is the phase where IRQs can interrupt kernel code.
6. **Tests** — two userspace tasks (from B6) time-slice; tick frequency observable; tasks that never yield still get preempted.

### Acceptance criteria

- ADR-0029 Accepted.
- Preemption works: a CPU-bound userspace task is preempted by the tick and another runnable task gets CPU time.
- Idle cores enter low-power WFI.
- No scheduling-related deadlocks or priority inversions (v1 is single priority, so this is mostly vacuous; real-time concerns deferred).

---

## Milestone C4 — Cross-core IPC

A sender on core 0 sending to a receiver on core 1 works. The receiver wakes on the right core; migration is not in scope.

### Sub-breakdown

1. **ADR-0030 — Cross-core wakeup.** IPI-based (inter-processor interrupt) vs. polling. Latency expectations.
2. **IPI support** — new primitive on `IrqController` (or a sibling trait) to send an IPI to another core.
3. **Endpoint rendezvous across cores** — the wait/wake path handles the cross-core case correctly.
4. **Tests** — cross-core IPC round trip; behaviour when the receiver's core is idle (WFI'd); behaviour when both cores are busy.

### Acceptance criteria

- ADR-0030 Accepted.
- IPI primitive implemented for QEMU virt (GICv3 SGI).
- Cross-core IPC has the same correctness guarantees as same-core IPC (atomic cap transfer, etc.).

---

## Milestone C5 — Multi-core TLB shootdown

When an address space is modified on one core, other cores with that address space active must invalidate their TLBs.

### Sub-breakdown

1. **ADR-0031 — TLB shootdown protocol.** Broadcast IPI vs. per-address targeted; whether to extend `Mmu` trait or add a sibling.
2. **`invalidate_tlb_cross_core` primitive** — probably on a sibling trait, since `Mmu` v1 is single-core.
3. **Integration with address-space unmap paths.**
4. **Tests** — cross-core unmap visibility is immediate; stale TLB entries never observed after shootdown.

### Acceptance criteria

- ADR-0031 Accepted.
- Cross-core unmap is safely observable on all cores before the next memory access.

### Phase C closure

Business review. Phase D (Pi 4) or Phase D + E overlap becomes active.

---

## ADR ledger for Phase C

| ADR | Purpose | Expected state |
|-----|---------|----------------|
| ADR-0027 | Secondary core start protocol | C1 |
| ADR-0028 | Per-core state access pattern | C2 |
| ADR-0029 | Scheduler topology (preemptive) | C3 |
| ADR-0030 | Cross-core wakeup (IPI) | C4 |
| ADR-0031 | TLB shootdown protocol | C5 |

## Open questions carried into Phase C

- Whether preemption is tick-driven or also includes manual preemption points (e.g., long-running kernel operations yielding).
- Whether per-core-queues with work-stealing justify their complexity in v1 (global queue with locking may suffice).
- Real-time guarantees for the scheduler (probably "none beyond priority" in v1; richer RT is a later ADR).
