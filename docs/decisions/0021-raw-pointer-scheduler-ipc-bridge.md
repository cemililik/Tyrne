# 0021 — Raw-pointer scheduler IPC-bridge API

- **Status:** Accepted
- **Date:** 2026-04-22
- **Deciders:** @cemililik

## Context

[`UNSAFE-2026-0012`](../audits/unsafe-log.md) records a `&mut` aliasing hazard in the cooperative scheduler's IPC bridge. The bodies of `task_a` and `task_b` in [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs) materialise `&mut` references to `SCHED`, `EP_ARENA`, `IPC_QUEUES`, and the per-task `CapabilityTable`, and hold those references live across the `cpu.context_switch` call inside [`Scheduler::ipc_send_and_yield`](../../kernel/src/sched/mod.rs) / [`Scheduler::ipc_recv_and_yield`](../../kernel/src/sched/mod.rs). When the *other* task resumes, it acquires its own `&mut` references to the same `UnsafeCell` interiors. Under Rust's strict aliasing model two live `&mut` references to the same referent is immediately undefined behaviour — the compiler is entitled to optimise as if each were uniquely aliased, regardless of whether the accesses occur simultaneously.

Tyrne v1's single-core cooperative execution model happens to shield the current compiled binary from observable miscompilation: no two tasks ever execute at once, and the `naked_asm!` context-switch barrier is opaque enough that today's LLVM does not rearrange loads or stores around it. But this is a compile-time accident, not a guarantee. A future LLVM version, a change in the inliner's budgets, or the introduction of preemption, SMP, or the MMU could all break the shield without code changes on our side. The 2026-04-21 [security review of Phase A exit](../analysis/reviews/security-reviews/2026-04-21-tyrne-to-phase-a.md) makes this the **#1 Phase-B blocker**, to be closed before any later Phase B milestone (EL drop, MMU activation, per-task address spaces, syscall boundary, userspace) compounds the hazard.

This ADR chooses the API shape that retires UNSAFE-2026-0012. It is the architectural decision that governs [`T-006`](../analysis/tasks/phase-b/T-006-raw-pointer-scheduler-api.md); implementation and audit-log updates follow as task work.

## Decision drivers

- **Rust aliasing model correctness.** The primary driver. A bridge API that requires the caller to hold a `&mut` across `cpu.context_switch` is unsound under the language contract; any solution must make that pattern unreachable from the BSP task bodies.
- **Minimum invasive change.** Phase A shipped with the current API; 75 kernel-crate tests exercise it; 34 test-hal tests back it. We are fixing a specific hazard, not redesigning the scheduler. A refactor that ripples through unrelated subsystems owes the project a justification it does not need today.
- **Forward compatibility with preemption and SMP.** The chosen shape must remain sound when the cooperative-cover invariant (no two tasks execute simultaneously) no longer holds. A preemption-capable scheduler wraps shared state in per-CPU locks; whatever API we pick must adapt to that.
- **Consistency with existing `Scheduler` internals.** [`Scheduler::yield_now`](../../kernel/src/sched/mod.rs#L310-L316) already uses a split-borrow via raw-pointer arithmetic on `self.contexts`, audited under UNSAFE-2026-0008. Any new aliasing discipline should extend this precedent rather than invent a separate idiom.
- **Zero-`unsafe` kernel crate preservation.** A3–A6 closed with every `unsafe` block in the BSP or in scheduler code where the unsafety was intrinsic to the context-switch call. New `unsafe` should not leak into `kernel::cap`, `kernel::obj`, or `kernel::ipc`.
- **BSP ergonomics.** Task bodies must stay auditable. A per-call nested closure or a type-erased continuation is more expensive to read than a direct sequence of `unsafe` blocks, and readability at the `unsafe` boundary is a security property, not a taste preference.
- **Testability.** The 11 existing scheduler tests construct `FakeCpu` + `AlignedStack` + stack-local arenas and drive the bridge directly. Whatever shape we pick must not force those tests to route through a new ownership layer.
- **Audit surface.** Each new `unsafe` must carry a `// SAFETY:` comment conforming to [`unsafe-policy.md §1`](../standards/unsafe-policy.md#1-every-unsafe-use-is-justified-in-a--safety-comment) and a corresponding `UNSAFE-2026-NNNN` entry in the audit log.

## Considered options

1. **Option A — Raw-pointer IPC-bridge (all-pointers, including `self`).** The IPC bridge is expressed as `unsafe fn`s that take `*mut Scheduler<C>` as the first parameter — **no `&mut self` receiver** — plus `*mut EndpointArena`, `*mut IpcQueues`, and `*mut CapabilityTable`. Callers construct every pointer via `UnsafeCell::get()` without materialising a `&mut`. Inside the bridge, each pointer is momentarily dereferenced to a `&mut` only within narrow scopes that **end strictly before** `cpu.context_switch` is called; the post-switch work re-acquires fresh `&mut`s after the call returns. No `&mut` to `Scheduler`, `EndpointArena`, `IpcQueues`, or any `CapabilityTable` is alive across the switch. The only `&mut` references that *do* cross the switch are the two `&mut Aarch64TaskContext` / `&Aarch64TaskContext` borrowed from `self.contexts` via raw-pointer split-borrow arithmetic — same as `Scheduler::yield_now` today; these are provably non-aliasing because the two task-context indices are distinct.
2. **Option B — Scheduler owns the shared arenas.** `Scheduler<C>` grows to own `EndpointArena`, `IpcQueues`, and the per-task `CapabilityTable`s. The IPC bridge takes no external state parameters; callers hand the scheduler a `TaskHandle` and a `CapHandle` and receive a result. The BSP hands ownership of every arena to the scheduler at bootstrap and never touches them again.
3. **Option C — Continuation-passing bridge.** Callers pass a closure describing the resume path; the scheduler suspends the caller, performs the switch, and on resume invokes the closure, which re-acquires fresh references inside its own scope. No `&mut` crosses the switch at the API level because nothing the compiler sees persists across it.
4. **Option D — Per-task `TaskContext` extensions.** Each task's `TaskContext` is extended to carry references (or raw pointers) to its own arenas. The scheduler accesses these per-task during dispatch, never as parameters to an IPC call — effectively a variant of Option B where the ownership is per-task rather than global.

## Decision outcome

**Chosen: Option A — Raw-pointer IPC-bridge (all-pointers, including `self`).**

Option A retires UNSAFE-2026-0012 **in full** — no residual aliasing window remains. By making the bridge entry points `unsafe fn`s over `*mut Scheduler<C>` rather than methods with `&mut self`, the call graph never produces a `&mut Scheduler` that is alive across `cpu.context_switch`. The same applies to every arena and capability table: the BSP task bodies construct `*mut T` directly from the `StaticCell`'s `UnsafeCell::get()`, so no `&mut` ever materialises at the outer call site. Inside the bridge, `&mut` references are materialised momentarily — before, or after, the switch, never spanning it — and each such materialisation is audited under a single new UNSAFE-2026-NNNN entry that covers the pattern once.

The alternative "keep `&mut self` and only the parameters are raw-pointer" shape, which an earlier draft of this ADR considered, does **not** close the hazard: whoever calls `Scheduler::ipc_send_and_yield(&mut self, …)` holds a `&mut Scheduler` for the call's entire duration, and the call's duration spans `cpu.context_switch`. When the second task resumes and acquires its own `&mut Scheduler` via `SCHED.assume_init_mut()`, two live `&mut`s to the same referent exist — exactly the hazard UNSAFE-2026-0012 describes, merely relocated from the arenas to the scheduler itself. The full fix requires `self` to be a raw pointer, not a reference. See *Revision notes* at the end of this ADR for how this was caught.

The `Scheduler<C>` struct keeps its current shape (ready queue + per-task state + saved contexts); `ipc_send` / `ipc_recv` / `cap_take` / `insert_root` keep their current signatures; the 11 scheduler host tests recompile essentially unchanged because `FakeCpu`-driven tests already pass stack-local arenas into the bridge and can pass raw pointers to them just as easily as `&mut`. The mechanical change at a test call-site is `&mut sched` → `core::ptr::from_mut(&mut sched)` (stable Rust) per argument. No ownership restructuring.

The decision also preserves `Scheduler::yield_now`'s existing raw-pointer idiom on `self.contexts`. The new IPC-bridge `unsafe` blocks look like the existing ones — the reader who has already audited UNSAFE-2026-0008 sees the same shape, reuses the same mental model, and the review cost stays approximately flat. This matters more than it might seem: an inconsistent `unsafe` idiom raises the review cost of every change that touches the scheduler, and the scheduler is on the critical path for the rest of Phase B.

Option B is rejected because — even before its layering-collapse cost — it is **not an alternative to Option A, only a superset of it**: as long as the scheduler is reached through a `&mut Scheduler` at any caller, the aliasing hazard reappears. Option B would still need Option A's raw-pointer-on-`self` fix to close UNSAFE-2026-0012, and would then *additionally* fold `EndpointArena`, `IpcQueues`, and per-task `CapabilityTable` ownership into the scheduler for no incremental aliasing benefit. Beyond that, Option B collapses the deliberate layering between `kernel::cap`, `kernel::obj`, `kernel::ipc`, and `kernel::sched` — each of which covers its own ADR (0014, 0016, 0017, 0019) — into a single scheduler-owned god-object, and fights Phase C's per-CPU locking strategy.

Option C is rejected because the closure solves the wrong problem: its captured `&mut` references are exactly the aliasing hazard we are trying to eliminate. Forcing captures to be by value or by raw pointer defeats the ergonomics argument that motivated the option. Additionally, closure erasure (`Box<dyn FnOnce>`) requires an allocator we do not have, and static dispatch (`fn ipc_send<F: FnOnce(…)>(f: F)`) multiplies the kernel's monomorphised code size for no measured benefit.

Option D is rejected because it relocates rather than eliminates the hazard. Adding references inside `TaskContext` means the scheduler, when iterating `task_handles[]` or dispatching, has to materialise `&mut`s to those per-task references — and those `&mut`s can cross the switch just as the external ones do today. It also violates the ADR-0020 contract that `TaskContext` is a register-save frame, not a Rust-level state bag.

## Consequences

### Positive

- **UNSAFE-2026-0012 retires fully.** Its status becomes `Removed — <T-006 commit SHA>`. After the refactor, *no* `&mut` reference — to the scheduler, to any arena, to any capability table, or to IPC queues — is alive across `cpu.context_switch`. The only cross-switch `&mut`s are the two `Aarch64TaskContext` borrows used by the context-switch assembly itself, which are provably non-aliasing (distinct task indices) and already covered by UNSAFE-2026-0008.
- **Forward-compatible with preemption and SMP.** Raw pointers are trivially wrappable by per-CPU locks: `(*CELL.0.get()).as_mut_ptr()` becomes `LOCK.lock().arenas_mut().as_mut_ptr()` when Phase C lands. `&mut` parameters, by contrast, force the lock to be held across the entire bridge call — which blocks the target task's reacquisition of the same lock during its own IPC.
- **Consistency with `Scheduler::yield_now`.** The same raw-pointer split-borrow idiom that already runs under UNSAFE-2026-0008 now covers the IPC bridge. One review model covers both.
- **Test harness unaffected.** 75 kernel + 34 test-hal host tests continue to pass without structural changes; the pointer arguments are constructed trivially from stack-local arenas in tests.
- **Scheduler interface remains layered.** `kernel::cap`, `kernel::obj`, and `kernel::ipc` stay independently importable. Future ADRs can evolve each subsystem without scheduler churn.

### Negative

- **IPC-bridge signatures are less safe-Rust-native.** Accepting `*mut X` instead of `&mut X`, and — critically — exposing the bridge as `unsafe fn` free functions rather than `&mut self` methods, loses the compile-time non-aliasing guarantee on the function signatures. *Mitigation:* the aliasing contract is stated in doc-comments on each pointer parameter, the scheduler module doc cites this ADR, every caller's `// SAFETY:` block references this ADR, and `cargo miri test` is added to the scheduler test suite so the host-level aliasing checks run in CI (once CI exists; K3-7). In practice the two in-tree callers (`task_a`, `task_b`) are the only ones that will ever exist at the BSP layer; the userspace task model introduced in Phase B's syscall boundary routes through a kernel dispatcher that calls the scheduler on behalf of the task, so the userspace-reachable attack surface of this API does not grow.
- **Loss of method syntax.** Because the bridge cannot take `&mut self`, the call-site becomes `sched_ipc_send_and_yield(sched_ptr, cpu, ep_ptr, queues_ptr, table_ptr, cap, msg, transfer)` rather than `sched.ipc_send_and_yield(cpu, ep, queues, table, cap, msg, transfer)`. Argument count is the same; the dot-call is gone. *Mitigation:* a thin BSP-level wrapper (`fn task_send(&Handles, msg) -> …`) packages the arguments once for each task body, keeping task bodies readable. The scheduler remains the owner of the logic; only the calling convention changes.
- **New `unsafe` surface in the BSP and the scheduler.** The BSP needs a `StaticCell::as_mut_ptr()` inherent helper to turn `&StaticCell<T>` into `*mut T` without materialising a `&mut`; the scheduler needs an internal helper that performs the momentary `*mut → &mut` deref strictly outside the switch window. *Mitigation:* two new audit entries (one per helper), each pointing to a single documented SAFETY comment the helper carries once, each used from one-line call sites that cite the helper's audit tag. This is a strict improvement over the current state: today four separate `// SAFETY (aliasing)` blocks in the BSP describe the same hazard; tomorrow two narrow helpers carry the discipline.
- **Documentation debt.** The scheduler gains a paragraph in its module doc-comment describing the aliasing contract. The future architecture doc [`docs/architecture/scheduler.md`](../architecture/scheduler.md) (T-008) formalises this. *Mitigation:* T-008 is already planned in B0; the ADR explicitly names the doc location.

### Neutral

- **`TaskArena` migration is bundled with T-006 implementation.** phase-b.md §B0 couples K3-11 (`TaskArena` local → global `StaticCell`) with this refactor because both touch the same BSP static-cell surface. The bundling is a task-level decision, not an ADR-level one; the API shape this ADR decides is independent of how `TaskArena` is stored.
- **`Scheduler<C>` struct layout is unchanged.** All state the scheduler currently holds stays where it is. The change is API-shape only.
- **`ContextSwitch` / `Cpu` v2 contracts (ADR-0020) are unchanged.** The context-switch primitive itself retains its `&mut TaskContext` / `&TaskContext` signature; that borrow lives inside `Scheduler` and does not touch external arenas.

## Pros and cons of the options

### Option A — Raw-pointer IPC-bridge, all-pointers including `self` (chosen)

- **Pro:** Retires UNSAFE-2026-0012 **in full** — no `&mut` to the scheduler, any arena, or any capability table is alive across `cpu.context_switch`. This is the critical distinction from the earlier "parameters-only" sketch of Option A (see *Revision notes*).
- **Pro:** Extends `Scheduler::yield_now`'s existing audited split-borrow pattern on `self.contexts` to the IPC bridge — one idiom across the whole scheduler.
- **Pro:** Forward-compatible with preemption and SMP — raw pointers wrap cleanly in per-CPU locks; `&mut` receivers do not.
- **Pro:** Audit surface is narrow and single-pointed: one helper in the BSP (`StaticCell::as_mut_ptr`), one helper inside the scheduler (momentary `*mut → &mut` with documented scope). Two new audit entries; UNSAFE-2026-0012 retires.
- **Pro:** Test harness is unaffected — tests pass raw pointers from stack-local data with `core::ptr::from_mut` (stable) just as easily as they pass `&mut` today.
- **Con:** Loses compile-time non-aliasing guarantees on the function signatures; the invariant becomes a doc-comment contract verified by review and `cargo miri test` on host tests.
- **Con:** Loses method (dot-call) syntax at the BSP — the bridge is a free `unsafe fn`, so callers write `sched_ipc_send_and_yield(sched_ptr, …)` rather than `sched.ipc_send_and_yield(…)`. Mitigated via a thin BSP-level wrapper that packages the argument set for each task body.
- **Con:** Each momentary `*mut → &mut` deref inside the scheduler needs a `SAFETY:` comment (mitigated via the private helper pattern described in *Consequences — Negative*).

### Option B — Scheduler owns the shared arenas

- **Pro (illusory):** Appears to give "most safe-Rust feeling" because the IPC bridge takes no external state parameters. But see the first Con below — this pro disappears under analysis.
- **Con — does not close the hazard alone.** If the scheduler still appears via `&mut self` in its own methods, `&mut Scheduler` crosses the switch via the running method's receiver, and the aliasing UB reappears as soon as a second task calls the same method. Option B therefore **requires Option A's raw-pointer-on-`self` fix** to actually close UNSAFE-2026-0012. It is a superset of Option A, not an alternative.
- **Con:** Invasive — `Scheduler<C>` becomes a god-object owning `EndpointArena`, `IpcQueues`, and per-task `CapabilityTable`s; the current deliberate layering across `kernel::cap` / `kernel::obj` / `kernel::ipc` / `kernel::sched` collapses.
- **Con:** Breaks 11 scheduler host tests that drive the bridge with stack-local arenas; every test would need a `Scheduler::new_with_arenas(...)` factory.
- **Con:** Breaks IPC-layer host tests that exercise `ipc_send` / `ipc_recv` directly against raw `IpcQueues` / `EndpointArena`; those tests would need to route through a dummy scheduler.
- **Con:** Conflicts with preemption and SMP — per-CPU locks on arenas want per-arena granularity; a single scheduler-held lock serialises every IPC across every CPU.
- **Con:** Ownership of per-task `CapabilityTable`s by the scheduler is architecturally wrong — the table is a property of the task, not the scheduler.

### Option C — Continuation-passing bridge

- **Pro:** In principle most flexible; the closure can hold any state the caller wants.
- **Pro:** Natural basis for a future async extension (Phase G+).
- **Con:** Closure-captured references still cross `cpu.context_switch` — the captured `&mut`s are exactly the aliasing hazard we are eliminating. Forcing captures to be by value or by raw pointer defeats the ergonomics argument.
- **Con:** Closure erasure either requires an allocator (`Box<dyn FnOnce>`) — incompatible with `no_std` + no-heap — or pays generic-instantiation code size (`fn<F: FnOnce()>`), which bloats the kernel binary without measured benefit.
- **Con:** Unfamiliar bare-metal idiom; higher review cost per change.
- **Con:** Panic / failure handling across the closure boundary is harder to reason about than direct error returns.

### Option D — Per-task `TaskContext` extensions

- **Pro:** No bridge parameters at all — each task's per-task state is carried internally.
- **Con:** Relocates the aliasing hazard rather than eliminating it. The scheduler, iterating `task_handles[]`, would materialise `&mut`s to per-task references that can still live across the switch.
- **Con:** Violates ADR-0020's `TaskContext` contract — `Aarch64TaskContext` is a register-save frame (`x19`–`x28`, `fp`, `lr`, `sp`, `d8`–`d15`), not a Rust-level state bag. Extending it with references changes its purpose.
- **Con:** Makes `ContextSwitch::init_context` significantly more complex; the `fn() -> !` entry-function contract no longer suffices.

## Revision notes

Entries are in chronological order.

- **2026-04-22 — initial draft (commit `6c2e7a0`).** Option A was described as "the IPC bridge takes `*mut` for `EndpointArena`, `IpcQueues`, and `CapabilityTable`; the scheduler keeps `&mut self`". The consequence bullet claimed the remaining `&mut self` across `cpu.context_switch` was sound because it was "a single intra-struct borrow the scheduler itself owns".
- **2026-04-22 — revision (commit `85581ab`, pre-Accept).** That reasoning was wrong. A `&mut self` on `Scheduler::ipc_send_and_yield` is produced by `SCHED.assume_init_mut()` at the call site and lives for the method's full duration, which spans `cpu.context_switch`. When the second task resumes and calls `SCHED.assume_init_mut()` again, a second `&mut Scheduler` to the same referent exists — the same aliasing UB as UNSAFE-2026-0012, merely relocated from the arenas to the scheduler. Retiring UNSAFE-2026-0012 requires the bridge to never produce a `&mut Scheduler` that crosses the switch; therefore the bridge cannot accept `&mut self`. The chosen Option A is therefore "all-pointers, including `self`" — bridge entry points are `unsafe fn`s over `*mut Scheduler<C>`, with momentary `&mut` materialisation strictly outside the switch window. This revision updates the *Decision outcome*, *Considered options* (Option A), *Consequences — Positive / Negative*, and *Pros and cons — Option A / Option B* sections accordingly; the hash levels, section order, and option numbering are preserved. The ADR was accepted with this shape in commit `3b8aa34`.
- **2026-04-22 — post-merge follow-up rider (commit `7eaa10a`).** Two refinements to the Option A discipline after the first implementation landed, captured here so the ADR continues to match the shipped code:
  1. `Scheduler::start` was inadvertently left as a `&mut self` method in the initial `f9b72f8` commit; although its bootstrap `&mut` lives on a frame that is never resumed, it re-introduces the exact pattern this ADR rules out. The follow-up commit reshapes `start` as an `unsafe fn` free function over `*mut Scheduler<C>`, matching `yield_now` / `ipc_send_and_yield` / `ipc_recv_and_yield`. Every scheduler entry point is now raw-pointer-typed; there is no `&mut self` surface.
  2. The shared safety contract in [`kernel/src/sched/mod.rs`](../../kernel/src/sched/mod.rs) (module doc at the raw-pointer-bridge section) is now explicit that the "no `&mut` across the switch" rule is a **global** invariant: no other kernel path may hold a `&mut` to `Scheduler<C>`, `EndpointArena`, `IpcQueues`, or `CapabilityTable` while a task is mid-bridge. This was implicit in the original text; the follow-up states it outright so a future reviewer does not need to reconstruct the invariant from per-call `SAFETY:` comments.

- **2026-04-28 — Amendment: aliasing discipline extends to the IRQ-handler frame (T-012, commit `28c5ce9`).** T-012 introduces a new kernel entry point — [`irq_entry`](../../bsp-qemu-virt/src/exceptions.rs) — that runs on the interrupted task's stack after the asm trampoline at `tyrne_vectors+0x280` saves the AAPCS64 caller-saved register frame. This Amendment makes explicit that the "no `&mut Scheduler<C>` / `&mut EndpointArena` / `&mut IpcQueues` / `&mut CapabilityTable` across `cpu.context_switch`" discipline is a **superset** of a stricter rule that also applies to IRQ entry: **no IRQ handler may hold a `&mut` to any of those statics across the boundary back to interrupted code.** The reasoning is symmetric to the original ADR — when `eret` resumes the interrupted task, that task's own `&mut` to the same referent (acquired via `assume_init_mut()` in the BSP task body) becomes live again, and Rust's aliasing model is violated regardless of whether the two `&mut`s ever observe each other through memory.

  v1's `irq_entry` is structured as **ack-and-ignore**: it dispatches only the EL1 virtual generic-timer IRQ (PPI 27) by masking `CNTV_CTL_EL0` and signalling EOI to the GIC; no scheduler state is read or mutated. This means v1 vacuously satisfies the IRQ-frame discipline — there is nothing to alias because nothing kernel-statics-related is borrowed. This is by design, not by accident: the alternative (signalling a wake to the scheduler from the timer ISR) would have been the first real test of this Amendment and was deferred until after this Amendment is in writing.

  Future arcs that require IRQ-driven scheduler mutation — preemption, `time_sleep_until` wake-on-deadline, asynchronous capability revocation, IPI-based cross-CPU wake — must follow Option A's existing momentary-`&mut` discipline:
  1. The IRQ handler takes nothing more than the trapped-frame pointer at the function signature level (`unsafe extern "C" fn(_frame: *mut TrapFrame)`); no `&mut Scheduler<C>` parameter, no method receiver on a scheduler. The `unsafe fn` qualifier is part of the contract — the function has caller-side preconditions (frame validity, AAPCS64 stack-frame discipline supplied by the asm trampoline) that are documented in the function's `# Safety` doc-section, and the `unsafe fn` makes those preconditions visible at the type level to any future Rust caller (see [`bsp-qemu-virt/src/exceptions.rs::irq_entry`](../../bsp-qemu-virt/src/exceptions.rs)).
  2. Inside the handler, any pointer to a kernel static (`SCHED.0.get()`, etc.) is dereferenced to a `&mut` only within a scope that ends strictly before `eret` (i.e., before the function returns to the trampoline). The scope must not span any function boundary that could itself perform a context switch.
  3. Each such momentary-`&mut` materialisation gets its own `// SAFETY:` comment citing this ADR and a corresponding entry or Amendment in [`docs/audits/unsafe-log.md`](../audits/unsafe-log.md).

  The new call site is audited in UNSAFE-2026-0020 (vector-table install + asm trampolines, including the `bl irq_entry` boundary). UNSAFE-2026-0014 — the canonical record of the momentary-`&mut`-from-`*mut`-Scheduler pattern — already names `irq_entry` as a future site of the same pattern in its 2026-04-28 Amendment (commit `28c5ce9`); v1's body has no live citation yet because `irq_entry` is *ack-and-ignore* and never materialises a `&mut Scheduler<C>`. When a future scheduler-touching IRQ handler activates this site, it should add a follow-up Amendment recording the activation alongside the introducing commit's SHA, **not** create a parallel audit entry. This keeps a single mental model: "every momentary `&mut Scheduler` materialisation in the kernel, regardless of caller (BSP task body or IRQ handler), is the same pattern with the same SAFETY contract."

  Rejected alternatives recorded for completeness: (a) Routing IRQ handlers through a `&mut Scheduler<C>` parameter would require the trampoline to construct that reference — re-introducing the very aliasing hazard this ADR closed. (b) Returning a `WakeRequest` enum from `irq_entry` for the post-`eret` task to act on would push the aliasing window into the task body, which is even worse because the task body is BSP code (less reviewed than the kernel). (c) A per-CPU "pending IRQ work" mailbox that the scheduler drains on the next `yield` would work but adds a queue layer the cooperative single-CPU v1 does not need; revisit during Phase C SMP work.

## References

- [ADR-0013 — Roadmap and planning process](0013-roadmap-and-planning.md) — the planning framework inside which this ADR is written.
- [ADR-0014 — Capability representation](0014-capability-representation.md) — defines `CapabilityTable`, one of the arenas the bridge API takes as a pointer.
- [ADR-0016 — Kernel object storage](0016-kernel-object-storage.md) — defines `EndpointArena`; motivates the companion `TaskArena` migration bundled with T-006.
- [ADR-0017 — IPC primitive set](0017-ipc-primitive-set.md) — defines `IpcQueues` and the rendezvous semantics the bridge preserves.
- [ADR-0019 — Scheduler shape](0019-scheduler-shape.md) — the scheduler shape this ADR preserves.
- [ADR-0020 — `ContextSwitch` trait and `Cpu` v2](0020-cpu-trait-v2-context-switch.md) — the context-switch abstraction around which the aliasing window opens.
- [UNSAFE-2026-0012 — audit entry](../audits/unsafe-log.md) — the hazard this ADR closes.
- [Security review of Phase A exit](../analysis/reviews/security-reviews/2026-04-21-tyrne-to-phase-a.md) — §1 and §3, where the blocker is enumerated.
- [Code review of Phase A exit](../analysis/reviews/code-reviews/2026-04-21-tyrne-to-phase-a.md) — §Correctness (Scheduler bullet 2) flags the same surface.
- [Phase B plan §B0](../roadmap/phases/phase-b.md) — the milestone this ADR opens.
- [T-006 — Raw-pointer scheduler API refactor](../analysis/tasks/phase-b/T-006-raw-pointer-scheduler-api.md) — the implementing task.
- [`unsafe-policy.md`](../standards/unsafe-policy.md) — the discipline every new `unsafe` block upholds.
- [`Scheduler::yield_now`'s split-borrow pattern](../../kernel/src/sched/mod.rs#L310-L316) — the in-tree precedent.
- Rust reference, ["Behaviour considered undefined"](https://doc.rust-lang.org/reference/behavior-considered-undefined.html) — specifies that two live `&mut` references to the same referent is UB regardless of access timing.
- [`miri` — the interpreter that catches aliasing UB in host tests](https://github.com/rust-lang/miri) — the verification path proposed in *Consequences — Negative*.
- seL4 kernel call structure — each task's TCB holds a capability-slot pointer and receives IPC through the TCB without cross-task aliasing; prior art for the ownership direction Option B proposed.
