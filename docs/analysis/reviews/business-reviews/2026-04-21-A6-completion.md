# Business review 2026-04-21 — A6 completion / Phase A retrospective

- **Trigger:** milestone-completion; Phase A exit bar met.
- **Scope:** Milestones A3–A6 (A2 was covered separately); full Phase A retrospective.
- **Period:** 2026-04-21 (T-002 landed) → 2026-04-21 (T-005 Done).
- **Participants:** @cemililik (+ Claude Sonnet 4.6 agent as scribe)

---

## What landed (A3–A6)

### Milestones closed

| Milestone | Task | Done | Description |
|-----------|------|------|-------------|
| A3 | T-002 | 2026-04-21 | Kernel object storage (`Task`, `Endpoint`, `Notification` arenas, generation-tagged typed handles) |
| A4 | T-003 | 2026-04-21 | IPC primitives (`ipc_send`, `ipc_recv`, `ipc_notify`; capability transfer; rendezvous state machine) |
| A5 | T-004 | 2026-04-21 | Cooperative scheduler and context switch (`ContextSwitch` trait, `#[unsafe(naked)]` asm, `yield_now`, IPC bridge) |
| A6 | T-005 | 2026-04-21 | Two-task IPC demo — Phase A exit bar |

### ADRs accepted in A3–A6

| ADR | Title | Status |
|-----|-------|--------|
| ADR-0016 | Kernel object storage | Accepted 2026-04-21 |
| ADR-0017 | IPC primitive set | Accepted 2026-04-21 |
| ADR-0018 | Badge scheme | Deferred (explicitly noted in ADR-0017) |
| ADR-0019 | Scheduler shape | Accepted 2026-04-21 |
| ADR-0020 | `Cpu` trait v2 / context-switch extension | Accepted 2026-04-21 |

### Test counts at Phase A closure

| Crate | Tests |
|-------|-------|
| `umbrix_kernel` (host) | 75 |
| `umbrix_hal` (host) | 34 |
| **Total** | **109** |

Zero failures. QEMU smoke test confirmed manually.

---

## Phase A exit bar — verdict

> *Two kernel tasks exchange IPC messages under capability control, scheduled cooperatively, running on QEMU `virt` aarch64.*

**Met.** The A6 QEMU trace shows:

```
umbrix: hello from kernel_main
umbrix: starting cooperative scheduler
umbrix: task B — waiting for IPC
umbrix: task A -- sending IPC
umbrix: task B — received IPC (label=0xaaaa); replying
umbrix: task A — received reply (label=0xbbbb); done
umbrix: all tasks complete
```

All five components of the exit bar are verified:
1. **Two kernel tasks** — Task A (initiator) and Task B (responder) both ran to completion.
2. **Capability control** — each task holds its own `CapabilityTable`; raw object pointers are never exposed.
3. **IPC** — `ipc_send_and_yield` and `ipc_recv_and_yield` drove the round trip; no bare `yield_now` was used for the IPC itself.
4. **Cooperative scheduling** — the scheduler's `SchedQueue`, `TaskState`, and `unblock_receiver_on` managed all task transitions without a timer or preemption.
5. **QEMU `virt` aarch64** — confirmed running on `qemu-system-aarch64` with `virt` machine type and Cortex-A72 CPU model.

---

## What went well

- **Structural consistency.** The pattern of bounded arenas indexed by generation-tagged handles, first established for the capability table (ADR-0014), carried cleanly through kernel objects (ADR-0016), IPC queues (`IpcQueues`), and scheduler state (`task_states`, `task_handles`, `contexts` parallel arrays). Adding a new kernel object type costs ≈ one new arena struct; the existing patterns need no change.
- **Zero-`unsafe` kernel crate.** The kernel crate (capability, IPC, scheduler logic) contains no `unsafe` code. All `unsafe` is isolated in the BSP — specifically in four categories: MMIO construction (`Pl011Uart::new`), system-register assembly (`QemuVirtCpu`), context-switch assembly (`context_switch_asm`), and static-cell access patterns. The audit log (`unsafe-log.md`) tracks all twelve entries with rationale.
- **ADR-first discipline.** Every significant decision was written before implementation. No code was written without an Accepted ADR covering its shape. This produced a design that was internally coherent and avoidable mistakes were caught at the ADR stage (e.g. the `ContextSwitch` trait separation from `Cpu` — settled in ADR-0020 before any asm was written).
- **Boot-checklist effectiveness.** The three boot bugs fixed in A5 (IrqGuard vtable corruption, CPACR_EL1.FPEN missing, context_switch_asm compiler prologue) are all covered by the [BSP boot checklist](../../../standards/bsp-boot-checklist.md). The checklist was written after the bugs; A6 brought up without encountering any of the listed failure modes.

## What was harder than expected

- **A5 debugging time.** Three boot bugs were uncovered only by observing a silent QEMU hang and working backward. Each fix required diagnosing from QEMU's `-d int` exception log. The debugging took longer than the implementation. The root causes are now preventative checklist items (BSP boot checklist §1, §2, §6) and standard rules (`unsafe-policy.md` §5a).
- **`&mut` aliasing across context switches.** The single-core cooperative model is sound, but Rust's strict aliasing rules make it formally UB to have two `&mut` references to the same data alive across a context switch — even if no actual concurrent access occurs. This is documented as UNSAFE-2026-0012 and deferred to a raw-pointer API refactor. It is the most significant known UB-adjacent pattern in the codebase and must be resolved before Phase B adds preemption or SMP.
- **`ipc_send_and_yield` yield semantics.** The scheduler only auto-yields on `Delivered`; a sender that arrives before a receiver gets `Enqueued` and returns without yielding. This required careful task ordering in `kernel_entry` (B added first, so B registers as receiver before A sends) and an explicit `yield_now` in Task B after sending the reply. This is a known limitation documented in [ADR-0019's open questions](../../../decisions/0019-scheduler-shape.md#open-questions).

## Technical debt entering Phase B

| Item | Severity | Owner ADR |
|------|----------|-----------|
| `&mut` aliasing on shared statics (UNSAFE-2026-0012) | Medium — formally UB, safe under single-core cooperative invariant | Future ADR (raw-pointer API) |
| `reply_recv` fastpath absent — server tasks pay a scheduler round-trip on every reply | Low — latency cost only, no correctness issue | ADR-0018 (deferred) |
| Stack watermark probes absent — no visibility into per-task stack usage | Low — Phase A stacks are 4 KiB each and demo tasks are shallow | Phase B observability task |
| No free-running timer — IPC latency unmeasured | Medium — blocks performance tuning | Phase B (timer init task) |
| BSS zero loop not optimized — `STR XZR` word at a time | Negligible for 17.5 KiB | Not a priority |

## Phase B readiness

Phase B requires the following to be true before starting. All items are met:

- [x] Kernel boots on QEMU `virt` aarch64.
- [x] Cooperative scheduler proven correct via QEMU smoke test.
- [x] IPC round-trip demonstrated end-to-end with capability discipline.
- [x] All `unsafe` blocks audited; audit log complete and current.
- [x] 109 host tests passing with zero failures.
- [x] No known correctness bugs. UNSAFE-2026-0012 is a formal-aliasing concern, not a runtime bug under the v1 single-core cooperative invariant.

Phase B's first milestone should address, in roughly this order:
1. Timer initialisation and `CNTPCT_EL0`-based latency measurement.
2. Raw-pointer API refactor to eliminate UNSAFE-2026-0012.
3. Page-table setup (MMU-on) — the largest Phase B risk.

---

## References

- [Phase A plan](../../../roadmap/phases/phase-a.md)
- [T-005: Two-task IPC demo](../../tasks/phase-a/T-005-two-task-ipc-demo.md)
- [Two-task demo guide](../../../guides/two-task-demo.md)
- [Baseline performance review](../performance-optimization-reviews/2026-04-21-A6-baseline.md)
- [UNSAFE-2026-0012](../../audits/unsafe-log.md)
