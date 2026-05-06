//! Cooperative scheduler — Milestone A5, [T-004].
//!
//! Design settled in [ADR-0019] (queue structure, yield semantics,
//! blocked-task representation, IPC bridge ownership). Generic over
//! `C: ContextSwitch` per [ADR-0020].
//!
//! # Overview
//!
//! - [`SchedQueue<N>`] — bounded FIFO of [`TaskHandle`]s.
//! - [`TaskState`] — per-task state enum (`Idle / Ready / Blocked`).
//! - [`Scheduler<C>`] — ready queue, per-task state, saved contexts, and
//!   the identity of the currently running task.
//!
//! The IPC bridge (`ipc_send_and_yield` / `ipc_recv_and_yield`) is an
//! orchestration layer on top of [`crate::ipc`]; the IPC module itself
//! remains ignorant of scheduling concerns.
//!
//! # Idle task
//!
//! Per [ADR-0026] (which supersedes [ADR-0022]'s *idle-task-location* axis;
//! the typed-error axis stands), the BSP registers a kernel idle task via
//! [`register_idle`] during boot. Idle lives in [`Scheduler::idle`] — a
//! dedicated `Option<TaskHandle>` slot — and is **never enqueued in the
//! ready queue**. The dispatcher consults `idle` only when
//! `ready.dequeue()` returns `None`, so idle never displaces a real Ready
//! task. With idle registered, [`SchedError::Deadlock`] is a defensive
//! return for "BSP forgot to call `register_idle`" / future preemption /
//! SMP edge cases — the v1 cooperative workload cannot reach it. The idle
//! task's entry function still loops `Cpu::wait_for_interrupt` +
//! `yield_now`; idle's `yield_now` becomes a no-op when nothing else is
//! Ready (the dispatcher falls back to `idle` again, which equals
//! `current` → fast-path return).
//!
//! ADR-0022's earlier shape (idle as a regular ready-queue resident via
//! [`add_task`]) was superseded after the 2026-05-06 QEMU smoke surfaced an
//! idle-dispatch regression: round-robin FIFO selected idle ahead of a
//! just-unblocked receiver and the kernel hung in `WFI`. See
//! [`docs/analysis/reviews/business-reviews/2026-05-06-B1-smoke-regression.md`]
//! for the incident report and ADR-0026 for the structural fix.
//!
//! [ADR-0022]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0022-idle-task-and-typed-scheduler-deadlock.md
//! [ADR-0026]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0026-idle-dispatch-fallback.md
//! [`docs/analysis/reviews/business-reviews/2026-05-06-B1-smoke-regression.md`]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/reviews/business-reviews/2026-05-06-B1-smoke-regression.md
//!
//! [T-004]: https://github.com/cemililik/TyrneOS/blob/main/docs/analysis/tasks/phase-a/T-004-cooperative-scheduler.md
//! [ADR-0019]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0019-scheduler-shape.md
//! [ADR-0020]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0020-cpu-trait-v2-context-switch.md

use tyrne_hal::{ContextSwitch, Cpu, IrqGuard};

use crate::cap::{CapHandle, CapObject, CapabilityTable};
use crate::ipc::{ipc_recv, ipc_send, IpcError, IpcQueues, Message, RecvOutcome, SendOutcome};
use crate::obj::endpoint::EndpointArena;
use crate::obj::{EndpointHandle, TaskHandle, TASK_ARENA_CAPACITY};

// ─── SchedQueue ───────────────────────────────────────────────────────────────

/// Bounded FIFO queue of [`TaskHandle`]s, capacity `N`.
///
/// Capacity equals [`TASK_ARENA_CAPACITY`] so the queue can never be full
/// relative to the number of tasks that can exist.
pub struct SchedQueue<const N: usize> {
    buf: [Option<TaskHandle>; N],
    head: usize,
    len: usize,
}

impl<const N: usize> Default for SchedQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> SchedQueue<N> {
    /// Construct an empty queue.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buf: [None; N],
            head: 0,
            len: 0,
        }
    }

    /// Push `handle` to the back of the queue.
    ///
    /// # Errors
    ///
    /// Returns `Err(handle)` when the queue is full.
    pub fn enqueue(&mut self, handle: TaskHandle) -> Result<(), TaskHandle> {
        if self.len == N {
            return Err(handle);
        }
        // N is the queue capacity; head and len are both < N when not full.
        // wrapping_add followed by % N is safe: head < N, len < N, so their
        // sum fits in usize before the modulo.
        #[allow(
            clippy::arithmetic_side_effects,
            reason = "N > 0 enforced by caller; head < N and len < N"
        )]
        let tail = self.head.wrapping_add(self.len) % N;
        self.buf[tail] = Some(handle);
        self.len = self.len.wrapping_add(1);
        Ok(())
    }

    /// Pop the front handle from the queue, or `None` if empty.
    pub fn dequeue(&mut self) -> Option<TaskHandle> {
        if self.len == 0 {
            return None;
        }
        let handle = self.buf[self.head].take();
        // head wraps around N — N > 0 because len > 0.
        #[allow(
            clippy::arithmetic_side_effects,
            reason = "N > 0 because len > 0 (queue not empty)"
        )]
        {
            self.head = self.head.wrapping_add(1) % N;
        }
        self.len = self.len.wrapping_sub(1);
        handle
    }

    /// Number of handles currently in the queue.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when the queue contains no handles.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

// ─── TaskState ────────────────────────────────────────────────────────────────

/// Scheduling state of one task slot.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TaskState {
    /// Slot is not occupied by a live task.
    Idle,
    /// Task is in the ready queue or currently running.
    Ready,
    /// Task is waiting for a message on an endpoint.
    Blocked {
        /// The endpoint the task is blocked on.
        on: EndpointHandle,
    },
}

// ─── SchedError ──────────────────────────────────────────────────────────────

/// Errors returned by scheduler operations.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SchedError {
    /// No task is currently running; the operation requires a current task.
    NoCurrentTask,
    /// The ready queue is full.
    QueueFull,
    /// IPC operation failed.
    Ipc(IpcError),
    /// The ready queue is empty AND no idle task is registered while the
    /// current task has just blocked — every task in the system is waiting
    /// on IPC and the dispatcher's fallback slot ([`Scheduler::idle`]) is
    /// `None`. Per [ADR-0022] + [ADR-0026], registering a BSP idle task at
    /// boot via [`register_idle`] makes this variant structurally
    /// unreachable in the v1 cooperative workload; it is preserved as a
    /// defensive return for preemption / SMP / "BSP forgot to call
    /// `register_idle`" so the kernel never panics on a userspace-reachable
    /// condition.
    ///
    /// # Rollback scope
    ///
    /// When returned from [`ipc_recv_and_yield`], the **scheduler** state
    /// is restored to its pre-call shape: `s.current` is reset to the
    /// calling task, `s.task_states[current_idx]` is reset from
    /// `Blocked` back to `Ready`, and the ready queue is left untouched.
    /// The **endpoint** state, however, is **not** rolled back — the
    /// Phase 1 `ipc_recv` call has already transitioned the endpoint
    /// from `Idle` to `RecvWaiting` (recording a waiting receiver) before
    /// Phase 2's ready-queue check runs, and the Deadlock path does not
    /// reverse that transition. A subsequent `ipc_recv_and_yield` on the
    /// same endpoint from the same caller therefore observes `QueueFull`
    /// (a receiver is already registered), not `Pending`. In the v1
    /// cooperative workload this is acceptable because the variant is
    /// structurally unreachable; recovery semantics (endpoint rollback
    /// or an explicit `ipc_cancel_recv`) are out of scope and will be
    /// revisited if preemption / SMP ever exercises the path. Callers
    /// that want the endpoint reset must destroy and re-create it.
    ///
    /// [ADR-0022]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0022-idle-task-and-typed-scheduler-deadlock.md
    /// [ADR-0026]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0026-idle-dispatch-fallback.md
    Deadlock,
}

impl From<IpcError> for SchedError {
    fn from(e: IpcError) -> Self {
        Self::Ipc(e)
    }
}

// ─── Scheduler ───────────────────────────────────────────────────────────────

/// Cooperative, single-core scheduler.
///
/// Generic over `C: ContextSwitch + Cpu` — the BSP provides both the
/// interrupt-masking needed for safe context switches and the register-save
/// assembly.
pub struct Scheduler<C: ContextSwitch + Cpu> {
    ready: SchedQueue<TASK_ARENA_CAPACITY>,
    task_states: [TaskState; TASK_ARENA_CAPACITY],
    /// Stored handles, indexed by slot index, so the scheduler can find
    /// a `TaskHandle` when unblocking without re-querying the arena.
    task_handles: [Option<TaskHandle>; TASK_ARENA_CAPACITY],
    current: Option<TaskHandle>,
    /// Idle-task fallback slot per [ADR-0026]. Written exclusively by
    /// [`register_idle`]; read by the dispatch sites
    /// ([`start_prelude`], [`yield_now`], [`ipc_recv_and_yield`]) when
    /// `ready.dequeue()` returns `None`. Idle's [`TaskHandle`] is **not**
    /// stored in [`Self::ready`] — that is the structural property
    /// ADR-0026 added to prevent idle from displacing real Ready tasks.
    /// Idle's saved [`ContextSwitch::TaskContext`] still lives in
    /// [`Self::contexts`] at the slot index `idle.slot().index()`, so
    /// dispatching idle is mechanically identical to dispatching any
    /// other task once the handle is selected.
    ///
    /// [ADR-0026]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0026-idle-dispatch-fallback.md
    idle: Option<TaskHandle>,
    /// Saved register contexts, one per task arena slot.
    ///
    /// Invariant: `contexts[i]` is valid for every slot `i` that has
    /// `task_states[i] != Idle` — either zero-initialised by `Default` and
    /// then filled by `init_context`, or saved by a prior `context_switch`.
    contexts: [C::TaskContext; TASK_ARENA_CAPACITY],
}

impl<C: ContextSwitch + Cpu> Default for Scheduler<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: ContextSwitch + Cpu> Scheduler<C> {
    /// Construct an empty scheduler with all contexts zero-initialised.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ready: SchedQueue::new(),
            task_states: [TaskState::Idle; TASK_ARENA_CAPACITY],
            task_handles: [None; TASK_ARENA_CAPACITY],
            current: None,
            idle: None,
            contexts: core::array::from_fn(|_| C::TaskContext::default()),
        }
    }

    /// Register a new task and enqueue it as ready.
    ///
    /// Initialises the task's context so that the first restore begins
    /// executing `entry` with `stack_top` as the initial stack pointer.
    ///
    /// # Errors
    ///
    /// [`SchedError::QueueFull`] if the ready queue is already at capacity
    /// (cannot happen when `TASK_ARENA_CAPACITY` slots exist and only one
    /// task occupies each slot).
    ///
    /// # Safety
    ///
    /// `stack_top` must satisfy [`ContextSwitch::init_context`]'s contract:
    /// 16-byte aligned, at least 512 bytes of backing memory, valid for the
    /// task's entire lifetime.
    pub unsafe fn add_task(
        &mut self,
        cpu: &C,
        handle: TaskHandle,
        entry: fn() -> !,
        stack_top: *mut u8,
    ) -> Result<(), SchedError> {
        let idx = handle.slot().index() as usize;
        // SAFETY: caller guarantees stack_top validity per the # Safety doc.
        // Forwarding to the BSP's init_context which writes lr and sp into
        // the context slot at `idx`. Audit: UNSAFE-2026-0009.
        unsafe {
            cpu.init_context(&mut self.contexts[idx], entry, stack_top);
        }
        // Enqueue before writing task_states / task_handles so that a
        // QueueFull error leaves no partial registration in those arrays.
        self.ready
            .enqueue(handle)
            .map_err(|_| SchedError::QueueFull)?;
        self.task_states[idx] = TaskState::Ready;
        self.task_handles[idx] = Some(handle);
        Ok(())
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Resolve a capability handle to an [`EndpointHandle`].
    fn resolve_ep_cap(
        caller_table: &CapabilityTable,
        ep_cap: CapHandle,
    ) -> Result<EndpointHandle, SchedError> {
        let cap = caller_table
            .lookup(ep_cap)
            .map_err(|_| SchedError::Ipc(IpcError::InvalidCapability))?;
        match cap.object() {
            CapObject::Endpoint(h) => Ok(h),
            _ => Err(SchedError::Ipc(IpcError::InvalidCapability)),
        }
    }

    /// Scan `task_states` for a task blocked on `ep` and re-enqueue it.
    ///
    /// **Single-waiter semantics.** Only the first blocked task found is
    /// woken; subsequent blocked tasks (if any) remain blocked. In A5 at
    /// most one task waits per endpoint at a time (ADR-0019), so this is
    /// correct. Multi-waiter wake-up is deferred to a future ADR.
    ///
    /// O(N) scan — acceptable at `TASK_ARENA_CAPACITY ≤ 16` (ADR-0019).
    fn unblock_receiver_on(&mut self, ep: EndpointHandle) {
        for idx in 0..TASK_ARENA_CAPACITY {
            if let TaskState::Blocked { on } = self.task_states[idx] {
                if on == ep {
                    if let Some(handle) = self.task_handles[idx] {
                        self.task_states[idx] = TaskState::Ready;
                        #[allow(
                            clippy::panic,
                            reason = "ready-queue capacity equals task-arena capacity; \
                                      the running task is not enqueued, so at least one \
                                      free slot always exists when unblocking a receiver"
                        )]
                        let Ok(()) = self.ready.enqueue(handle) else {
                            panic!("scheduler invariant: ready queue full on unblock");
                        };
                        return;
                    }
                }
            }
        }
    }
}

// ─── Raw-pointer bridge (ADR-0021) ────────────────────────────────────────────
//
// The three entry points below replace the former `&mut self` methods
// `Scheduler::yield_now`, `Scheduler::ipc_send_and_yield`, and
// `Scheduler::ipc_recv_and_yield`. They take `*mut Scheduler<C>` plus raw
// pointers to any external shared state (arenas, queues, capability tables)
// so that *no* `&mut` reference to any of those referents is alive across
// `cpu.context_switch`. Momentary `&mut` references are materialised only
// inside narrow inner `unsafe` blocks that end strictly before a switch or
// begin strictly after a switch returns. See [ADR-0021] for the full
// contract and the reasoning.
//
// # Shared safety contract
//
// Every function in this block has the same caller-facing contract. Stating
// it once here and referring to it from each function keeps each body focused
// on its own logic.
//
// - **Pointer validity.** Every `*mut T` pointer passed in must be non-null,
//   properly aligned for `T`, and point at a valid, exclusively-owned `T`.
//   "Exclusively-owned" here follows v1's single-core cooperative invariant:
//   no two tasks ever run simultaneously, so at any given instant only one
//   task is executing and thus only one dereference site is live. When a task
//   is suspended mid-bridge, its stack frame holds only raw pointers (never a
//   `&mut`), so the other task is free to re-derive its own raw pointers
//   from the same `UnsafeCell` interiors with no aliasing hazard.
// - **Non-aliasing across the switch.** No `&mut` reference to `Scheduler<C>`,
//   `EndpointArena`, `IpcQueues`, or `CapabilityTable` may be alive across
//   `cpu.context_switch`. Each bridge function honours this by confining
//   `&mut` materialisation to an inner `unsafe` block that ends before the
//   switch call site. This is a **global** invariant: when a task is
//   suspended mid-bridge, no other kernel path may hold a `&mut` to the
//   same referent. The single-core cooperative model satisfies this because
//   the only concurrent "path" is the task that just resumed via
//   `cpu.context_switch`, and its own bridge call re-derives raw pointers
//   from the same `UnsafeCell` interiors without ever materialising an
//   overlapping `&mut`.
// - **Distinct task indices.** The internal split-borrow on
//   `(*sched).contexts[current_idx]` vs `contexts[next_idx]` is sound because
//   the scheduler never enqueues the running task twice; the two indices are
//   therefore distinct, as already audited under UNSAFE-2026-0008.
// - **Interrupts disabled.** An [`IrqGuard`] is held for the duration of the
//   context-switch call; the guard is constructed before the switch and
//   dropped on return.
//
// # Rejected safer alternatives
//
// The `unsafe fn` + `*mut` signatures across every bridge entry point are
// load-bearing — not chosen for convenience. Each safer alternative was
// considered and rejected in [ADR-0021]; the short form is:
//
// - **`&mut self` + `&mut` parameters (the previous shape).** Reintroduces
//   the exact aliasing hazard [UNSAFE-2026-0012] describes: the `&mut
//   Scheduler` produced at the caller site (`SCHED.assume_init_mut()`)
//   lives for the full method call, which spans `cpu.context_switch`; when
//   the other task resumes and re-derives its own `&mut Scheduler` from
//   the same `UnsafeCell`, two live `&mut` references to the same
//   referent exist. This is UB under Rust's strict aliasing model
//   regardless of whether the accesses actually overlap at runtime.
// - **`Mutex<Scheduler>` / `RwLock` around the shared state.** Requires a
//   spin implementation that itself uses `unsafe` (so the unsafety
//   relocates rather than disappears); the lock must be held across
//   `cpu.context_switch`, which in preemptive / SMP futures blocks the
//   resuming task's reacquisition of the same lock and deadlocks. Per
//   ADR-0021 *Consequences*, raw pointers wrap cleanly in per-CPU locks
//   when Phase C lands; `&mut` receivers do not.
// - **Scheduler owns the arenas (ADR-0021 Option B).** Collapses the
//   `kernel::cap` / `kernel::obj` / `kernel::ipc` / `kernel::sched`
//   layering into a scheduler-owned god-object and still needs the
//   raw-pointer receiver fix on `self` to close the aliasing — strict
//   superset of this shape for no incremental benefit at v1 scale.
// - **Continuation-passing style (ADR-0021 Option C).** Closure captures
//   reintroduce the same `&mut` aliasing via capture environments;
//   `Box<dyn FnOnce>` requires an allocator we do not have.
//
// Every per-block `// SAFETY:` comment in this section relies on this
// shared rationale for the "why not safer Rust" half of its justification,
// alongside the block-local invariants it states inline.
//
// [ADR-0021]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0021-raw-pointer-scheduler-ipc-bridge.md
// [UNSAFE-2026-0012]: https://github.com/cemililik/TyrneOS/blob/main/docs/audits/unsafe-log.md

/// Register the BSP-owned idle task in the dispatcher's fallback slot.
///
/// Per [ADR-0026], idle lives in [`Scheduler::idle`] and is dispatched
/// **only** when [`Scheduler::ready`] is empty. The handle is *not*
/// enqueued — that is the structural property ADR-0026 introduced to
/// prevent idle from displacing real Ready tasks (the regression the
/// 2026-05-06 QEMU smoke surfaced).
///
/// Like [`Scheduler::add_task`], this initialises the task's saved
/// [`ContextSwitch::TaskContext`] so the first restore begins executing
/// `entry` with `stack_top` as the initial stack pointer. The slot's
/// [`TaskState`] is set to [`TaskState::Ready`] — when idle is the
/// dispatched task, `current` references it like any other; when idle
/// sits in the fallback slot waiting, the `Ready` state simply means
/// "available for dispatch" (idle is never `Blocked`, never goes back to
/// `Idle` once registered).
///
/// # Panics
///
/// Panics unconditionally (in **both** debug and release builds) if
/// [`Scheduler::idle`] was already set. Calling `register_idle` more than
/// once is a kernel programming error — silently overwriting the slot
/// would break the single-idle invariant ADR-0026 relies on (the
/// previously-registered idle's `TaskContext` is still in
/// [`Scheduler::contexts`] and could be re-entered if the dispatcher
/// happened to pick its slot index again). The `assert!` is unconditional
/// rather than `debug_assert!` so the violation surfaces loudly even in
/// production builds.
///
/// # Safety
///
/// `sched` must satisfy the Shared safety contract above (Pointer
/// validity, exclusive ownership, no aliased `&mut Scheduler<C>`). The
/// `&mut Scheduler<C>` materialised inside this function lives only for
/// the body and is dropped before return, so the caller can take a fresh
/// momentary `&mut` afterwards without violating the non-aliasing rule.
/// `stack_top` must satisfy [`ContextSwitch::init_context`]'s contract:
/// 16-byte aligned, at least 512 bytes of backing memory, valid for the
/// idle task's entire lifetime.
///
/// [ADR-0026]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0026-idle-dispatch-fallback.md
pub unsafe fn register_idle<C: ContextSwitch + Cpu>(
    sched: *mut Scheduler<C>,
    cpu: &C,
    handle: TaskHandle,
    entry: fn() -> !,
    stack_top: *mut u8,
) {
    // SAFETY: caller satisfies the Shared safety contract for `sched`; the
    // `&mut` is contained to this block and dropped before return.
    // Rejected alternatives match `add_task`'s shape (`&mut self` on the
    // bridge reintroduces UNSAFE-2026-0012; ADR-0021 Option B is a strict
    // superset). Audit: UNSAFE-2026-0014.
    let s = unsafe { &mut *sched };

    // Unconditional `assert!` (not `debug_assert!`) so a duplicate
    // registration cannot silently overwrite `s.idle` in release builds —
    // ADR-0026's single-idle invariant is load-bearing.
    #[allow(
        clippy::panic,
        reason = "duplicate register_idle is a kernel programming error \
                  (boot-time discipline; never recoverable at runtime)"
    )]
    {
        assert!(
            s.idle.is_none(),
            "register_idle called twice — idle slot is set-once by boot-time discipline"
        );
    }

    let idx = handle.slot().index() as usize;

    // SAFETY: caller guarantees `stack_top` validity per the # Safety doc.
    // Forwarding to the BSP's `init_context` which writes lr and sp into
    // the context slot at `idx`. Audit: UNSAFE-2026-0009.
    unsafe {
        cpu.init_context(&mut s.contexts[idx], entry, stack_top);
    }

    s.task_states[idx] = TaskState::Ready;
    s.task_handles[idx] = Some(handle);
    s.idle = Some(handle);
}

/// Pre-switch dequeue + state-mutation half of [`start`].
///
/// Extracted from `start`'s inner block so the dequeue/state half can be
/// covered by a host test — the post-prelude context switch is ABI
/// assembly that no host harness can run, but everything else can be
/// asserted on directly. After the call, the scheduler reflects:
///
/// - the head of `(*sched).ready` has been dequeued,
/// - `(*sched).task_states[<returned index>]` is `TaskState::Ready`,
/// - `(*sched).current` is `Some(<dequeued handle>)`.
///
/// The returned `usize` is the slot index of the dispatched task; `start`
/// passes it straight to `cpu.context_switch` to address the saved
/// context array. Refs: T-011.
///
/// # Panics
///
/// Panics if the ready queue is empty **and** [`Scheduler::idle`] is `None` —
/// i.e. the BSP called `start` without registering any task (via `add_task`)
/// or any idle (via [`register_idle`]). Per ADR-0026, the dispatch chain is
/// `s.ready.dequeue().or(s.idle)`; the panic fires only when both halves
/// resolve to `None`. Registering only an idle task is sufficient to avoid
/// the panic, and is the post-T-014 "minimum viable boot" shape for any
/// future BSP that wants to come up without an application task.
///
/// # Safety
///
/// `sched` must satisfy the Shared safety contract above (Pointer
/// validity, exclusive ownership, no aliased `&mut Scheduler<C>`). The
/// `&mut Scheduler<C>` materialised inside this function lives only for
/// the body and is dropped before return, so `start` (and any future
/// callers) can take a fresh momentary `&mut` afterwards without
/// violating the non-aliasing rule.
unsafe fn start_prelude<C: ContextSwitch + Cpu>(sched: *mut Scheduler<C>) -> usize {
    // SAFETY: caller satisfies the Shared safety contract; the `&mut` is
    // contained to this block. Rejected alternatives match `start`'s
    // original block (`&mut self` reintroduces UNSAFE-2026-0012; ADR-0021
    // Option B is a strict superset; a mutex relocates the unsafety).
    // Audit: UNSAFE-2026-0014.
    let s = unsafe { &mut *sched };

    // Per ADR-0026: dispatch the head of the ready queue, or fall back to
    // the idle slot when ready is empty. Panicking on "neither half has a
    // task" preserves ADR-0022's "boot-time programming error → panic"
    // semantics — the only way to reach this branch is for the BSP to call
    // `start` without registering any task and without registering idle.
    #[allow(
        clippy::panic,
        reason = "empty ready queue and no idle task is a kernel programming error \
                  (BSP failed to register any task at boot)"
    )]
    let next_handle = match s.ready.dequeue() {
        Some(h) => h,
        None => match s.idle {
            Some(idle_h) => idle_h,
            None => panic!("scheduler start called with empty ready queue and no idle task"),
        },
    };
    let next_idx = next_handle.slot().index() as usize;
    s.task_states[next_idx] = TaskState::Ready;
    s.current = Some(next_handle);
    next_idx
}

/// Start the scheduler by switching to the first ready task.
///
/// Dequeues the head of the ready queue and restores its saved context,
/// abandoning the bootstrap stack frame. Intended to be called exactly once
/// from `kernel_main` after tasks have been added. Does not return.
///
/// # IRQ state on task entry
///
/// An `IrqGuard` is constructed on the bootstrap frame immediately before
/// the context switch. Because that frame is never resumed, the guard's
/// `Drop` never runs; tasks therefore begin executing with interrupts
/// **masked** (DAIF = 0xF). In A5 this is acceptable because no interrupt
/// sources are configured; a task that needs interrupts enabled must call
/// `cpu.restore_irq_state(IrqState(0))` explicitly. Revisited when Phase B
/// introduces a timer or other interrupt source.
///
/// # Panics
///
/// Panics if no task **and** no idle has been registered — see
/// [`start_prelude`]'s `# Panics` for the exact contract. Per ADR-0026 the
/// dispatcher's `s.ready.dequeue().or(s.idle)` chain falls back to idle when
/// the ready queue is empty, so registering only [`register_idle`] (and no
/// `add_task`) is enough to avoid the panic.
///
/// # Safety
///
/// See the "Shared safety contract" above. `sched` must satisfy *Pointer
/// validity* and must not alias any live `&mut Scheduler<C>`. Because the
/// bootstrap frame is abandoned, this function also honours the "no `&mut`
/// across the switch" rule — the throwaway context is constructed on a
/// stack frame that `cpu.context_switch` never returns to.
pub unsafe fn start<C: ContextSwitch + Cpu>(sched: *mut Scheduler<C>, cpu: &C) -> ! {
    // SAFETY: caller satisfies the Shared safety contract for `sched`.
    // `start_prelude` materialises a momentary `&mut Scheduler<C>` inside
    // its own block and drops it before returning, so no `&mut` is alive
    // across the `cpu.context_switch` below. Audit: UNSAFE-2026-0014.
    let next_idx = unsafe { start_prelude(sched) };

    let mut throwaway = <C::TaskContext as Default>::default();
    let _guard = IrqGuard::new(cpu);
    // SAFETY: `next_idx` is in range (written by `add_task`); interrupts are
    // masked by `IrqGuard`; the throwaway current context lives on the
    // abandoned bootstrap stack frame and is never restored. No `&mut
    // Scheduler<C>` is live — the context pointer is derived from the raw
    // `sched` pointer. Rejected alternatives: the context-switch primitive
    // is register-save assembly; no safe-Rust abstraction can express it
    // (see UNSAFE-2026-0008's audit entry for the full enumeration), and
    // `&mut` to `contexts[next_idx]` is avoided by using raw-pointer
    // arithmetic per the Shared safety contract above. Audit: UNSAFE-2026-0008.
    unsafe {
        let ctx_ptr = (*sched).contexts.as_ptr();
        cpu.context_switch(&mut throwaway, &*ctx_ptr.add(next_idx));
    }

    // `cpu.context_switch` does not return on this path — the bootstrap
    // frame is abandoned. The loop satisfies `-> !` defensively.
    #[allow(
        clippy::empty_loop,
        reason = "unreachable: context_switch abandons this frame"
    )]
    loop {
        core::hint::spin_loop();
    }
}

/// Yield the current task cooperatively.
///
/// Re-enqueues the running task as `Ready` and switches to the head of the
/// ready queue. If the queue contains no other task, returns without a
/// switch.
///
/// # Errors
///
/// Returns [`SchedError::NoCurrentTask`] if called before [`start`].
///
/// # Panics
///
/// Panics if the ready queue is somehow full when re-enqueueing the current
/// task — a scheduler-invariant violation that cannot occur in correct code
/// (the running task is not in the queue, so at most `TASK_ARENA_CAPACITY-1`
/// other tasks are enqueued).
///
/// # Safety
///
/// See the "Shared safety contract" above this function's definition.
/// `sched` must satisfy *Pointer validity* and must not alias any live
/// `&mut Scheduler<C>` in the caller's scope.
pub unsafe fn yield_now<C: ContextSwitch + Cpu>(
    sched: *mut Scheduler<C>,
    cpu: &C,
) -> Result<(), SchedError> {
    // Pre-switch work — momentary &mut Scheduler, dropped before the switch.
    let (current_idx, next_idx) = {
        // SAFETY: caller contract — `sched` is valid, exclusively-owned for
        // the duration of this inner block, and this `&mut` does not cross
        // the `cpu.context_switch` call below because the block ends first.
        // Rejected alternatives: see §Rejected safer alternatives in the
        // Shared safety contract above — `&mut self` on the bridge
        // reintroduces UNSAFE-2026-0012; a lock relocates the unsafety
        // and deadlocks under preemption. Audit: UNSAFE-2026-0014.
        let s = unsafe { &mut *sched };

        let current_handle = s.current.ok_or(SchedError::NoCurrentTask)?;
        let current_idx = current_handle.slot().index() as usize;
        let current_is_idle = s.idle == Some(current_handle);

        // Mark current Ready (idempotent: idle and any regular running task
        // are already Ready by invariant; the assignment matches what
        // `add_task` and `register_idle` initialise to).
        s.task_states[current_idx] = TaskState::Ready;

        // Re-enqueue current ONLY if it is a regular task. Idle is never
        // in the ready queue per ADR-0026 — when idle is currently
        // running, it returns to the fallback slot via `s.idle`, not
        // through `ready.enqueue`. Cannot be full: the running task was
        // not in the ready queue (it was dequeued when it started running),
        // so at most TASK_ARENA_CAPACITY-1 other tasks are queued.
        if !current_is_idle {
            #[allow(
                clippy::panic,
                reason = "the running task is not in the ready queue, so at most \
                          TASK_ARENA_CAPACITY-1 tasks are enqueued; enqueue cannot fail"
            )]
            let Ok(()) = s.ready.enqueue(current_handle) else {
                panic!("scheduler invariant: ready queue full on yield re-enqueue");
            };
        }

        // Pick next: head of ready queue, or fall back to idle when empty.
        // Per ADR-0026, idle is the dispatcher's fallback — never a queue
        // resident. The fast-path early-return covers two cases:
        //   (a) current was a regular task and was just re-enqueued as the
        //       sole queue entry (single-task self-yield, no idle): dequeue
        //       returns current; no switch.
        //   (b) current is idle and queue is empty (idle yields to itself
        //       when no real task is ready): fallback resolves to current;
        //       no switch.
        let next_handle = match s.ready.dequeue() {
            Some(h) if h != current_handle => h,
            Some(_) => {
                // Dequeue returned current itself (case a). No other ready
                // task. Stay on current — nothing to switch to. Idle is
                // not a useful fallback here because there is no event
                // pending that idle's WFI could service.
                return Ok(());
            }
            None => match s.idle {
                Some(idle_h) if idle_h != current_handle => idle_h,
                _ => {
                    // Either: queue empty and idle is None (no fallback
                    // exists; nothing to switch to); or idle is current
                    // (case b). Either way, no switch.
                    return Ok(());
                }
            },
        };

        let next_idx = next_handle.slot().index() as usize;
        s.task_states[next_idx] = TaskState::Ready;
        s.current = Some(next_handle);

        (current_idx, next_idx)
    }; // `s: &mut Scheduler<C>` drops here

    // Switch window — no `&mut Scheduler<C>` is live.
    debug_assert_ne!(
        current_idx, next_idx,
        "split-borrow invariant: current and next task indices must differ"
    );
    let _guard = IrqGuard::new(cpu);
    // SAFETY: `current_idx != next_idx` by construction (the running task is
    // never in the ready queue, so `next_handle != current_handle` → distinct
    // indices). Both indices are within [0, TASK_ARENA_CAPACITY). Interrupts
    // are disabled by `IrqGuard`. Rejected alternatives: context-switch is
    // register-save assembly with no safe-Rust equivalent (see
    // UNSAFE-2026-0008); the split borrow uses raw-pointer arithmetic
    // because `&mut Scheduler` spanning the switch would violate the
    // Shared safety contract's non-aliasing rule. Audit: UNSAFE-2026-0008.
    unsafe {
        let ctx_ptr = (*sched).contexts.as_mut_ptr();
        let cur_ctx = &mut *ctx_ptr.add(current_idx);
        let nxt_ctx = &*ctx_ptr.add(next_idx);
        cpu.context_switch(cur_ctx, nxt_ctx);
    }

    Ok(())
}

/// Send a message; if the send delivers to a waiting receiver, unblock that
/// receiver and yield the current task so the receiver can run.
///
/// # Errors
///
/// Propagates [`IpcError`] as [`SchedError::Ipc`]; returns
/// [`SchedError::NoCurrentTask`] if the caller-yield path has no current
/// task (cannot happen after `Scheduler::start`).
///
/// # Safety
///
/// See the "Shared safety contract" above. Every `*mut` parameter must meet
/// *Pointer validity*. The four pointers must not alias each other or any
/// live `&mut` in the caller's scope.
#[allow(
    clippy::too_many_arguments,
    reason = "IPC bridge must forward all parameters that ipc_send requires"
)]
pub unsafe fn ipc_send_and_yield<C: ContextSwitch + Cpu>(
    sched: *mut Scheduler<C>,
    cpu: &C,
    ep_arena: *mut EndpointArena,
    queues: *mut IpcQueues,
    caller_table: *mut CapabilityTable,
    ep_cap: CapHandle,
    msg: Message,
    transfer: Option<CapHandle>,
) -> Result<SendOutcome, SchedError> {
    // Pre-switch work — momentary &muts, dropped before the switch.
    // SAFETY: caller contract — all four pointers are valid, distinct, and
    // exclusively-owned for the duration of this inner block. Each `&mut`
    // materialised in the tuple below lives only inside this block and is
    // dropped before the `yield_now` call site. Rejected alternatives: see
    // §Rejected safer alternatives in the Shared safety contract above —
    // `&mut` parameter receivers would pin the borrow across the switch
    // (reproducing UNSAFE-2026-0012); ADR-0021 §Decision outcome enumerates
    // the full alternative set. Audit: UNSAFE-2026-0014.
    let (outcome, needs_yield) = unsafe {
        let s: &mut Scheduler<C> = &mut *sched;
        let arena_ref: &mut EndpointArena = &mut *ep_arena;
        let queues_ref: &mut IpcQueues = &mut *queues;
        let table_ref: &mut CapabilityTable = &mut *caller_table;

        // Resolve the endpoint handle up-front so it remains valid even after
        // `ipc_send` mutates the endpoint state.
        let ep_handle = Scheduler::<C>::resolve_ep_cap(table_ref, ep_cap)?;

        let outcome = ipc_send(arena_ref, queues_ref, ep_cap, table_ref, msg, transfer)?;

        let needs_yield = if outcome == SendOutcome::Delivered {
            s.unblock_receiver_on(ep_handle);
            true
        } else {
            false
        };

        (outcome, needs_yield)
    }; // All `&mut`s drop here.

    // Switch window — no `&mut` to any shared state is alive.
    if needs_yield {
        // SAFETY: `sched` still satisfies the caller contract; we have just
        // released our `&mut` so the re-entrant `yield_now` can acquire its
        // own momentary `&mut` without overlapping ours. Rejected
        // alternatives: passing the already-materialised `&mut Scheduler`
        // would violate the non-aliasing rule in the Shared safety contract
        // (the callee's switch spans this frame); see §Rejected safer
        // alternatives above. Audit: UNSAFE-2026-0014.
        unsafe {
            yield_now(sched, cpu)?;
        }
    }

    Ok(outcome)
}

/// Receive a message; if none is ready, mark the current task `Blocked`,
/// switch to another ready task, and on resume collect the now-delivered
/// message.
///
/// # Errors
///
/// - [`SchedError::NoCurrentTask`] — no running task when blocking is required.
/// - [`SchedError::Deadlock`] — the ready queue is empty after blocking
///   the current task (every task is waiting on IPC and no idle task is
///   registered). The **scheduler** state is restored before the return
///   (see [`SchedError::Deadlock`]'s *Rollback scope*); the **endpoint**
///   state was moved from `Idle` to `RecvWaiting` during Phase 1 and
///   stays there — a subsequent `ipc_recv_and_yield` on the same
///   endpoint from the same caller therefore observes `QueueFull`. Per
///   [ADR-0022], registering an idle task at boot makes this variant
///   unreachable in the v1 cooperative workload.
/// - [`SchedError::Ipc`] — wraps [`IpcError`] failures from the underlying
///   [`ipc_recv`] calls. In particular,
///   [`IpcError::PendingAfterResume`] is returned when the resume-path
///   `ipc_recv` still returns `Pending` after a cooperative context switch
///   — a scheduler invariant violation. Per ADR-0022 *Revision notes*
///   second rider, the typed `Err` is the sole signal; no companion
///   `debug_assert!` fires (it was dropped as redundant with the typed
///   return and blocking the test that exercises this path).
///
/// # Safety
///
/// See the "Shared safety contract" above. Every `*mut` parameter must meet
/// *Pointer validity*. The four pointers must not alias each other or any
/// live `&mut` in the caller's scope.
///
/// [ADR-0022]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0022-idle-task-and-typed-scheduler-deadlock.md
pub unsafe fn ipc_recv_and_yield<C: ContextSwitch + Cpu>(
    sched: *mut Scheduler<C>,
    cpu: &C,
    ep_arena: *mut EndpointArena,
    queues: *mut IpcQueues,
    caller_table: *mut CapabilityTable,
    ep_cap: CapHandle,
) -> Result<RecvOutcome, SchedError> {
    // Phase 1 — try non-blocking recv, momentary &muts.
    // SAFETY: caller contract — all pointers valid, distinct, and
    // exclusively-owned for this inner block. Each `&mut` materialised here
    // is dropped before the switch below. Rejected alternatives: see
    // §Rejected safer alternatives in the Shared safety contract above —
    // a `&mut`-parameter signature pins the borrow across the Phase 2
    // switch; Option B (scheduler owns the arenas) collapses the
    // kernel/cap / kernel/obj / kernel/ipc layering for no incremental
    // benefit. Audit: UNSAFE-2026-0014.
    let (ep_handle, outcome) = unsafe {
        let arena_ref: &mut EndpointArena = &mut *ep_arena;
        let queues_ref: &mut IpcQueues = &mut *queues;
        let table_ref: &mut CapabilityTable = &mut *caller_table;

        let ep_handle = Scheduler::<C>::resolve_ep_cap(table_ref, ep_cap)?;
        let outcome = ipc_recv(arena_ref, queues_ref, ep_cap, table_ref)?;
        (ep_handle, outcome)
    };

    if !matches!(outcome, RecvOutcome::Pending) {
        return Ok(outcome);
    }

    // Phase 2 — block current, dequeue next, switch. Momentary &mut to
    // scheduler only, dropped before the switch.
    //
    // Per ADR-0026, the dispatch chain is `ready.dequeue().or_else(|| idle)`
    // — idle is consulted only when the ready queue is empty. If both
    // halves are `None` (no other Ready task AND no idle registered),
    // every task in the system is blocked on IPC and the kernel cannot
    // make progress. The pre-block **scheduler** state is restored before
    // returning `Err(SchedError::Deadlock)` so the caller observes the
    // same scheduler state it had before the bridge was called. Note that
    // the **endpoint** state was already transitioned from Idle to
    // RecvWaiting by the Phase 1 ipc_recv call above; that transition is
    // NOT rolled back here (see SchedError::Deadlock's doc-comment for
    // the rollback-scope rationale). With idle registered (the v1
    // expected configuration), Deadlock is structurally unreachable.
    let (current_idx, next_idx) = {
        // SAFETY: caller contract — `sched` valid and exclusive for this
        // block; `&mut` does not cross the switch below. Rejected
        // alternatives as above (Shared safety contract §Rejected safer
        // alternatives). Audit: UNSAFE-2026-0014.
        let s = unsafe { &mut *sched };
        let current_handle = s.current.ok_or(SchedError::NoCurrentTask)?;
        let current_idx = current_handle.slot().index() as usize;
        let prior_state = s.task_states[current_idx];
        debug_assert_eq!(
            prior_state,
            TaskState::Ready,
            "scheduler invariant: the running task's slot must be marked Ready"
        );
        s.task_states[current_idx] = TaskState::Blocked { on: ep_handle };
        s.current = None;

        let Some(next_handle) = s.ready.dequeue().or(s.idle) else {
            // Restore so Err(Deadlock) leaves the scheduler unchanged.
            s.task_states[current_idx] = prior_state;
            s.current = Some(current_handle);
            return Err(SchedError::Deadlock);
        };
        let next_idx = next_handle.slot().index() as usize;
        s.task_states[next_idx] = TaskState::Ready;
        s.current = Some(next_handle);
        (current_idx, next_idx)
    }; // `s: &mut Scheduler<C>` drops here.

    // Switch window — no `&mut` is alive.
    debug_assert_ne!(
        current_idx, next_idx,
        "split-borrow invariant: current and next task indices must differ"
    );
    {
        let _guard = IrqGuard::new(cpu);
        // SAFETY: `current_idx != next_idx` (running task was removed from
        // the ready queue before dequeue); both indices in range. Rejected
        // alternatives: context-switch is register-save assembly, no safe
        // Rust equivalent (UNSAFE-2026-0008); the split borrow uses
        // raw-pointer arithmetic to avoid two `&mut` into `contexts` per
        // the Shared safety contract. Audit: UNSAFE-2026-0008.
        unsafe {
            let ctx_ptr = (*sched).contexts.as_mut_ptr();
            let cur_ctx = &mut *ctx_ptr.add(current_idx);
            let nxt_ctx = &*ctx_ptr.add(next_idx);
            cpu.context_switch(cur_ctx, nxt_ctx);
        }
    }

    // Phase 3 — resumed; collect the delivered message.
    // SAFETY: caller contract — arenas/queues/table still valid; the
    // `&mut`s reacquired here did not exist during the switch. Rejected
    // alternatives: reusing the Phase 1 `&mut`s would carry them across
    // the switch (UNSAFE-2026-0012 hazard); the re-acquisition discipline
    // here is the per-phase pattern that Shared safety contract §Rejected
    // safer alternatives argues for. Audit: UNSAFE-2026-0014.
    let result = unsafe {
        let arena_ref = &mut *ep_arena;
        let queues_ref = &mut *queues;
        let table_ref = &mut *caller_table;
        ipc_recv(arena_ref, queues_ref, ep_cap, table_ref)
    };

    // A second `Pending` result is a scheduler bug — the sender should
    // have delivered before unblocking this task. Per ADR-0022 the bridge
    // returns `Err(SchedError::Ipc(IpcError::PendingAfterResume))` in this
    // case; the typed return *is* the loud signal (unhandled, it surfaces
    // at the caller's error path and carries the bridge's context), so a
    // redundant `debug_assert!` that made the condition untestable is not
    // kept.
    match result {
        Ok(RecvOutcome::Pending) => Err(SchedError::Ipc(IpcError::PendingAfterResume)),
        Ok(outcome) => Ok(outcome),
        Err(e) => Err(SchedError::Ipc(e)),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    reason = "test pragmas not permitted in production kernel code"
)]
mod tests {
    use super::*;
    use crate::obj::arena::SlotId;
    use crate::obj::endpoint::EndpointHandle;

    // ── FakeCpu ───────────────────────────────────────────────────────────────

    struct FakeCpu;

    #[derive(Default, Debug, PartialEq)]
    struct FakeCtx {
        switched: bool,
    }

    // SAFETY: FakeCpu is a zero-size marker with no interior mutability
    // and no shared mutable state. Send + Sync are safe.
    unsafe impl Send for FakeCpu {}
    // SAFETY: same reasoning as Send impl above.
    unsafe impl Sync for FakeCpu {}

    impl Cpu for FakeCpu {
        fn current_core_id(&self) -> tyrne_hal::CoreId {
            0
        }
        fn disable_irqs(&self) -> tyrne_hal::IrqState {
            tyrne_hal::IrqState(0)
        }
        fn restore_irq_state(&self, _: tyrne_hal::IrqState) {}
        fn wait_for_interrupt(&self) {}
        fn instruction_barrier(&self) {}
    }

    impl ContextSwitch for FakeCpu {
        type TaskContext = FakeCtx;

        unsafe fn context_switch(
            &self,
            current: &mut Self::TaskContext,
            _next: &Self::TaskContext,
        ) {
            current.switched = true;
        }

        unsafe fn init_context(
            &self,
            _ctx: &mut Self::TaskContext,
            _entry: fn() -> !,
            _stack_top: *mut u8,
        ) {
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn task_handle(index: u16) -> TaskHandle {
        TaskHandle::from_slot(SlotId::from_parts(index, 0))
    }

    fn ep_handle(index: u16) -> EndpointHandle {
        EndpointHandle::from_slot(SlotId::from_parts(index, 0))
    }

    fn spin_entry() -> fn() -> ! {
        || loop {
            core::hint::spin_loop();
        }
    }

    /// Test-only stack with guaranteed 16-byte alignment, satisfying the
    /// contract of [`ContextSwitch::init_context`]. `FakeCpu::init_context`
    /// is a no-op, so the alignment is not strictly required by the tests,
    /// but it is stated here so the SAFETY comment is accurate and so the
    /// helper is reusable if a real `init_context` is ever wired into tests.
    #[repr(C, align(16))]
    struct AlignedStack<const N: usize>([u8; N]);

    impl<const N: usize> AlignedStack<N> {
        fn new() -> Self {
            Self([0u8; N])
        }
        fn top(&mut self) -> *mut u8 {
            self.0.as_mut_ptr_range().end
        }
    }

    // ── SchedQueue tests ──────────────────────────────────────────────────────

    #[test]
    fn queue_enqueue_dequeue_fifo_order() {
        let mut q: SchedQueue<4> = SchedQueue::new();
        let h0 = task_handle(0);
        let h1 = task_handle(1);
        q.enqueue(h0).unwrap();
        q.enqueue(h1).unwrap();
        assert_eq!(q.dequeue(), Some(h0));
        assert_eq!(q.dequeue(), Some(h1));
        assert_eq!(q.dequeue(), None);
    }

    #[test]
    fn queue_full_returns_error() {
        let mut q: SchedQueue<2> = SchedQueue::new();
        q.enqueue(task_handle(0)).unwrap();
        q.enqueue(task_handle(1)).unwrap();
        assert!(q.enqueue(task_handle(2)).is_err());
    }

    #[test]
    fn queue_empty_dequeue_is_none() {
        let mut q: SchedQueue<4> = SchedQueue::new();
        assert!(q.dequeue().is_none());
    }

    #[test]
    fn queue_wraps_around() {
        let mut q: SchedQueue<2> = SchedQueue::new();
        q.enqueue(task_handle(0)).unwrap();
        q.dequeue();
        q.enqueue(task_handle(1)).unwrap();
        assert_eq!(q.dequeue(), Some(task_handle(1)));
    }

    #[test]
    fn queue_len_and_is_empty() {
        let mut q: SchedQueue<4> = SchedQueue::new();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
        q.enqueue(task_handle(0)).unwrap();
        assert!(!q.is_empty());
        assert_eq!(q.len(), 1);
    }

    // ── Scheduler state-transition tests ─────────────────────────────────────

    #[test]
    fn add_task_sets_ready_state_and_stores_handle() {
        let cpu = FakeCpu;
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        let h = task_handle(0);
        let mut stack = AlignedStack::<512>::new();
        // SAFETY: stack is 512 bytes and 16-byte aligned (AlignedStack repr).
        // FakeCpu::init_context is a no-op so the stack is never actually used.
        unsafe { sched.add_task(&cpu, h, spin_entry(), stack.top()).unwrap() };
        assert_eq!(sched.task_states[0], TaskState::Ready);
        assert_eq!(sched.task_handles[0], Some(h));
        assert_eq!(sched.ready.len(), 1);
    }

    #[test]
    fn yield_now_switches_context_and_updates_current() {
        let cpu = FakeCpu;
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        let h0 = task_handle(0);
        let h1 = task_handle(1);
        let mut s0 = AlignedStack::<512>::new();
        let mut s1 = AlignedStack::<512>::new();
        // SAFETY: stacks are 512 bytes and 16-byte aligned (AlignedStack repr).
        // FakeCpu::init_context is a no-op so the stacks are never actually used.
        unsafe {
            sched.add_task(&cpu, h0, spin_entry(), s0.top()).unwrap();
            sched.add_task(&cpu, h1, spin_entry(), s1.top()).unwrap();
        }
        // Simulate h0 running: it was dequeued when it started running.
        sched.ready.dequeue(); // removes h0 (head of queue)
        sched.current = Some(h0);
        // h1 is still in the queue.
        assert_eq!(sched.ready.len(), 1);

        // SAFETY: `sched` is a stack-local `Scheduler<FakeCpu>`; no aliasing
        // with any other task because the test harness is single-threaded.
        // `FakeCpu::context_switch` is a marker-only no-op, so the switch
        // never actually runs — the aliasing invariant is trivially satisfied.
        unsafe {
            yield_now(core::ptr::from_mut(&mut sched), &cpu).unwrap();
        }

        assert_eq!(sched.current, Some(h1));
        assert_eq!(sched.task_states[0], TaskState::Ready);
        assert_eq!(sched.task_states[1], TaskState::Ready);
        // FakeCpu::context_switch marks the saved (h0) context as switched.
        assert!(sched.contexts[0].switched);
    }

    #[test]
    fn yield_now_with_no_current_returns_error() {
        let cpu = FakeCpu;
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        // SAFETY: same reasoning as the test above — `sched` is stack-local,
        // single-threaded test; no aliasing.
        let result = unsafe { yield_now(core::ptr::from_mut(&mut sched), &cpu) };
        assert_eq!(result, Err(SchedError::NoCurrentTask));
    }

    #[test]
    fn unblock_receiver_on_moves_task_to_ready() {
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        let h0 = task_handle(0);
        let ep = ep_handle(0);
        sched.task_states[0] = TaskState::Blocked { on: ep };
        sched.task_handles[0] = Some(h0);

        sched.unblock_receiver_on(ep);

        assert_eq!(sched.task_states[0], TaskState::Ready);
        assert_eq!(sched.ready.len(), 1);
    }

    #[test]
    fn unblock_receiver_on_wrong_ep_is_noop() {
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        let h0 = task_handle(0);
        let ep0 = ep_handle(0);
        let ep1 = ep_handle(1);
        sched.task_states[0] = TaskState::Blocked { on: ep0 };
        sched.task_handles[0] = Some(h0);

        sched.unblock_receiver_on(ep1);

        assert_eq!(sched.task_states[0], TaskState::Blocked { on: ep0 });
        assert!(sched.ready.is_empty());
    }

    #[test]
    fn task_state_variants_are_distinct() {
        let ep = ep_handle(0);
        assert_ne!(TaskState::Idle, TaskState::Ready);
        assert_ne!(TaskState::Ready, TaskState::Blocked { on: ep });
        assert_ne!(TaskState::Idle, TaskState::Blocked { on: ep });
        assert_eq!(TaskState::Blocked { on: ep }, TaskState::Blocked { on: ep });
    }

    // ── ADR-0022 typed-error tests (T-007) ───────────────────────────────────

    use crate::cap::{CapRights, Capability};
    use crate::obj::endpoint::{create_endpoint, Endpoint};

    /// Helpers shared by the two ADR-0022 tests.
    fn setup_single_task_with_recv_cap(
        sched: &mut Scheduler<FakeCpu>,
        ep_arena: &mut EndpointArena,
        table: &mut CapabilityTable,
        task: TaskHandle,
        stack: &mut AlignedStack<512>,
    ) -> CapHandle {
        let cpu = FakeCpu;
        // SAFETY: 16-byte aligned, 512-byte stack; FakeCpu::init_context is
        // a no-op — stack is never actually used.
        unsafe {
            sched
                .add_task(&cpu, task, spin_entry(), stack.top())
                .unwrap();
        }
        // Simulate `start` having dispatched `task`: it was dequeued and is
        // now the running task.
        sched.ready.dequeue();
        sched.current = Some(task);

        let ep = create_endpoint(ep_arena, Endpoint::new(0)).unwrap();
        let cap = Capability::new(CapRights::RECV, CapObject::Endpoint(ep));
        table.insert_root(cap).unwrap()
    }

    #[test]
    fn ipc_recv_and_yield_returns_deadlock_when_ready_queue_empty() {
        // T-007 / ADR-0022: without an idle task, blocking the sole ready
        // task on IPC must return Err(SchedError::Deadlock) — not panic —
        // and the scheduler state must be restored to its pre-call shape.
        let cpu = FakeCpu;
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let mut table = CapabilityTable::new();
        let mut stack = AlignedStack::<512>::new();
        let task = task_handle(0);

        let ep_cap = setup_single_task_with_recv_cap(
            &mut sched,
            &mut ep_arena,
            &mut table,
            task,
            &mut stack,
        );

        // Snapshot pre-call state.
        let prior_current = sched.current;
        let prior_state = sched.task_states[0];
        assert_eq!(prior_current, Some(task));
        assert_eq!(prior_state, TaskState::Ready);

        // SAFETY: all four pointers refer to stack-local test state owned
        // exclusively by this thread; no aliasing. FakeCpu::context_switch
        // is a no-op marker; the deadlock path returns before reaching it.
        let result = unsafe {
            ipc_recv_and_yield(
                core::ptr::from_mut(&mut sched),
                &cpu,
                core::ptr::from_mut(&mut ep_arena),
                core::ptr::from_mut(&mut queues),
                core::ptr::from_mut(&mut table),
                ep_cap,
            )
        };

        assert!(
            matches!(result, Err(SchedError::Deadlock)),
            "expected Err(Deadlock), got {result:?}"
        );
        // Scheduler state must be restored.
        assert_eq!(sched.current, prior_current);
        assert_eq!(sched.task_states[0], prior_state);
        assert!(sched.ready.is_empty());
    }

    /// `FakeCpu` variant that resets the `IpcQueues` state to `Idle` during
    /// `context_switch`, simulating the pathological "resumed without a
    /// delivery" scenario that `SchedError::Ipc(IpcError::PendingAfterResume)`
    /// is designed to catch.
    struct ResetQueuesCpu {
        queues: *mut IpcQueues,
    }
    // SAFETY: test-only; the pointer refers to a stack-local IpcQueues the
    // test thread exclusively owns. No cross-thread sharing.
    unsafe impl Send for ResetQueuesCpu {}
    // SAFETY: same reasoning as Send.
    unsafe impl Sync for ResetQueuesCpu {}

    impl Cpu for ResetQueuesCpu {
        fn current_core_id(&self) -> tyrne_hal::CoreId {
            0
        }
        fn disable_irqs(&self) -> tyrne_hal::IrqState {
            tyrne_hal::IrqState(0)
        }
        fn restore_irq_state(&self, _: tyrne_hal::IrqState) {}
        fn wait_for_interrupt(&self) {}
        fn instruction_barrier(&self) {}
    }

    impl ContextSwitch for ResetQueuesCpu {
        type TaskContext = FakeCtx;
        unsafe fn context_switch(
            &self,
            current: &mut Self::TaskContext,
            _next: &Self::TaskContext,
        ) {
            current.switched = true;
            // Reset all endpoint states to Idle so the resume-path
            // ipc_recv observes Pending (RecvWaiting would yield QueueFull
            // instead, which is covered by the existing IPC tests).
            // SAFETY: `queues` is valid per the test's construction and
            // not concurrently accessed.
            unsafe {
                let q = &mut *self.queues;
                *q = IpcQueues::new();
            }
        }

        unsafe fn init_context(
            &self,
            _ctx: &mut Self::TaskContext,
            _entry: fn() -> !,
            _stack_top: *mut u8,
        ) {
        }
    }

    #[test]
    fn ipc_recv_and_yield_resume_pending_returns_typed_err() {
        // T-007 / ADR-0022: if the resume-path ipc_recv observes Pending
        // after the cooperative switch, the bridge must return
        // Err(SchedError::Ipc(IpcError::PendingAfterResume)) instead of
        // letting Ok(Pending) propagate — where the caller's
        // `let RecvOutcome::Received { … } else panic!` would turn it into
        // a downstream panic. `ResetQueuesCpu` forces the pathological
        // state by zeroing the queues during context_switch.
        let mut sched: Scheduler<ResetQueuesCpu> = Scheduler::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let mut table = CapabilityTable::new();
        let mut stack0 = AlignedStack::<512>::new();
        let mut stack1 = AlignedStack::<512>::new();
        let h0 = task_handle(0);
        let h1 = task_handle(1);

        // Set up the endpoint + cap before taking any raw pointers — the
        // `&mut ep_arena` and `&mut table` borrows below complete and
        // drop before we start building the raw-pointer configuration.
        let ep = create_endpoint(&mut ep_arena, Endpoint::new(0)).unwrap();
        // Guard: test correctness depends on the resume-path ipc_recv
        // seeing state == Idle (so it returns Pending). ResetQueuesCpu
        // rewrites to IpcQueues::new() whose slot_generations are 0; the
        // endpoint's generation must match so reset_if_stale_generation
        // does not bump the state.
        assert_eq!(ep.slot().generation(), 0);
        let cap = Capability::new(CapRights::RECV, CapObject::Endpoint(ep));
        let ep_cap = table.insert_root(cap).unwrap();

        // Derive each raw pointer exactly once and reuse. Deriving a
        // fresh `&mut X` twice on the same referent invalidates the
        // earlier pointer's Stacked-Borrows tag per Rust's aliasing
        // model (miri catches this). This is the same discipline
        // ADR-0021 enforces on the production side — the test has to
        // honour it too, because `ResetQueuesCpu` caches a pointer to
        // `queues` that the bridge would otherwise re-borrow.
        let sched_ptr = core::ptr::from_mut(&mut sched);
        let ep_arena_ptr = core::ptr::from_mut(&mut ep_arena);
        let queues_ptr = core::ptr::from_mut(&mut queues);
        let table_ptr = core::ptr::from_mut(&mut table);

        let cpu = ResetQueuesCpu { queues: queues_ptr };
        // SAFETY: 16-byte aligned 512-byte stacks; init_context is a no-op.
        // add_task uses `&mut self` on Scheduler, which temporarily
        // reborrows through `sched_ptr` — fine because the reborrow
        // ends before the bridge call below.
        unsafe {
            let s = &mut *sched_ptr;
            s.add_task(&cpu, h0, spin_entry(), stack0.top()).unwrap();
            s.add_task(&cpu, h1, spin_entry(), stack1.top()).unwrap();
            s.ready.dequeue();
            s.current = Some(h0);
        }

        // SAFETY: all four pointers refer to stack-local test state owned
        // exclusively by this thread; each was derived exactly once
        // above, so no tag-invalidating re-borrow happens here.
        let result = unsafe {
            ipc_recv_and_yield(sched_ptr, &cpu, ep_arena_ptr, queues_ptr, table_ptr, ep_cap)
        };

        assert!(
            matches!(result, Err(SchedError::Ipc(IpcError::PendingAfterResume))),
            "expected Err(Ipc(PendingAfterResume)), got {result:?}"
        );
    }

    // ── start_prelude (T-011) ─────────────────────────────────────────────────
    //
    // `start_prelude` is the dequeue + state-mutation half of `start`,
    // extracted in T-011 so a host test can assert on the post-call
    // scheduler state without invoking the assembly context-switch the
    // host harness cannot run. The two tests below cover the happy path
    // (one or more tasks queued) and the failure mode (no tasks queued —
    // a kernel programming error per ADR-0019). Refs: ADR-0019, T-011.

    #[test]
    fn start_prelude_dispatches_head_and_marks_ready() {
        let cpu = FakeCpu;
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        let h0 = task_handle(0);
        let h1 = task_handle(1);
        let mut s0 = AlignedStack::<512>::new();
        let mut s1 = AlignedStack::<512>::new();
        // SAFETY: 16-byte aligned 512-byte stacks; FakeCpu init_context is
        // a no-op so the stack memory is never read by the test path.
        unsafe {
            sched.add_task(&cpu, h0, spin_entry(), s0.top()).unwrap();
            sched.add_task(&cpu, h1, spin_entry(), s1.top()).unwrap();
        }

        // Sanity: pre-call shape.
        assert_eq!(sched.ready.len(), 2);
        assert_eq!(sched.current, None);

        // SAFETY: stack-local single-threaded scheduler; no aliasing.
        let next_idx = unsafe { start_prelude(core::ptr::from_mut(&mut sched)) };

        assert_eq!(next_idx, 0, "FIFO head (h0) is dispatched first");
        assert_eq!(sched.task_states[next_idx], TaskState::Ready);
        assert_eq!(sched.current, Some(h0));
        assert_eq!(sched.ready.len(), 1, "h1 remains in the queue");
    }

    #[test]
    #[should_panic(expected = "empty ready queue")]
    fn start_prelude_panics_on_empty_ready_queue() {
        // No tasks added AND no idle registered — `start_prelude` must fire
        // the named panic (a kernel programming error: `start` cannot be
        // called before any task is registered, with or without idle).
        // ADR-0026 / T-014 tightened the panic message to mention idle, but
        // the substring "empty ready queue" remains for `should_panic`'s
        // expected-message matcher.
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        // SAFETY: stack-local scheduler; the panic is the assertion.
        let _ = unsafe { start_prelude(core::ptr::from_mut(&mut sched)) };
    }

    // ── ADR-0026 / T-014 idle-dispatch tests ─────────────────────────────────
    //
    // Three tests cover the structural fix that ADR-0026 introduced for the
    // 2026-05-06 B1 smoke regression. The first asserts `register_idle`'s
    // shape (handle in `Scheduler::idle`, not in `ready`); the second asserts
    // the dispatcher consults `idle` only when `ready` is empty; the third
    // reproduces the exact demo flow that hung the kernel under ADR-0022's
    // Option A and verifies the post-fix dispatcher routes correctly.
    // Refs: ADR-0026, T-014, [B1 smoke regression mini-retro].
    //
    // [B1 smoke regression mini-retro]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/reviews/business-reviews/2026-05-06-B1-smoke-regression.md

    #[test]
    fn register_idle_stores_handle_in_idle_slot_and_not_in_ready_queue() {
        let cpu = FakeCpu;
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        let h_idle = task_handle(7);
        let mut s_idle = AlignedStack::<512>::new();

        // Pre-call shape: empty queue, idle slot None.
        assert!(sched.ready.is_empty());
        assert_eq!(sched.idle, None);

        // SAFETY: stack-local scheduler; FakeCpu::init_context is a no-op so
        // the stack memory is never actually read.
        unsafe {
            register_idle(
                core::ptr::from_mut(&mut sched),
                &cpu,
                h_idle,
                spin_entry(),
                s_idle.top(),
            );
        }

        // Post-call shape: idle slot set, ready queue still empty (idle is
        // *never* enqueued).
        assert_eq!(sched.idle, Some(h_idle));
        assert!(
            sched.ready.is_empty(),
            "idle must not be enqueued — it lives in the fallback slot per ADR-0026"
        );
        // Idle's TaskState slot is Ready (available for dispatch); its
        // task_handles slot is set (so unblock_receiver_on's scan can find
        // the handle if it ever needs to — though idle is never Blocked).
        assert_eq!(sched.task_states[7], TaskState::Ready);
        assert_eq!(sched.task_handles[7], Some(h_idle));
    }

    #[test]
    fn dispatcher_picks_idle_only_when_ready_queue_empty() {
        // Setup: one regular task A (slot 0) registered + idle (slot 1)
        // registered. start_prelude must select A first; after A blocks,
        // the dispatcher (via ipc_recv_and_yield's fallback chain) must
        // select idle.
        let cpu = FakeCpu;
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let mut table = CapabilityTable::new();
        let mut s_a = AlignedStack::<512>::new();
        let mut s_idle = AlignedStack::<512>::new();
        let h_a = task_handle(0);
        let h_idle = task_handle(1);

        // SAFETY: stack-local scheduler + 16-byte-aligned stacks; FakeCpu's
        // init_context / context_switch are no-ops.
        unsafe {
            sched.add_task(&cpu, h_a, spin_entry(), s_a.top()).unwrap();
            register_idle(
                core::ptr::from_mut(&mut sched),
                &cpu,
                h_idle,
                spin_entry(),
                s_idle.top(),
            );
        }

        // start_prelude must pick A (head of ready queue), not idle.
        // SAFETY: stack-local scheduler.
        let next_idx = unsafe { start_prelude(core::ptr::from_mut(&mut sched)) };
        assert_eq!(next_idx, 0, "start_prelude picked idle ahead of regular A");
        assert_eq!(sched.current, Some(h_a));

        // Now block A on a freshly-created endpoint and call
        // ipc_recv_and_yield — the dispatcher's `ready.or(idle)` chain must
        // fall back to idle (queue is empty since A is the sole regular
        // task and is now blocking).
        let ep = create_endpoint(&mut ep_arena, Endpoint::new(0)).unwrap();
        let cap = Capability::new(CapRights::RECV, CapObject::Endpoint(ep));
        let ep_cap = table.insert_root(cap).unwrap();

        // SAFETY: pointer-derivation discipline as elsewhere; FakeCpu's
        // context_switch is a no-op marker. The path of interest is the
        // pre-switch dispatcher: select idle as fallback.
        let result = unsafe {
            ipc_recv_and_yield(
                core::ptr::from_mut(&mut sched),
                &cpu,
                core::ptr::from_mut(&mut ep_arena),
                core::ptr::from_mut(&mut queues),
                core::ptr::from_mut(&mut table),
                ep_cap,
            )
        };

        // The bridge's Phase 3 re-runs ipc_recv after the (no-op) switch.
        // Because no sender has delivered, ipc_recv returns Pending again,
        // which the bridge maps to PendingAfterResume. We don't care about
        // the terminal Result here — the assertion of interest is that the
        // dispatcher *did* select idle as the next task during Phase 2.
        let _ = result;
        assert_eq!(
            sched.current,
            Some(h_idle),
            "dispatcher should fall back to idle when ready queue is empty"
        );
    }

    #[test]
    fn unblock_after_yield_dispatches_unblocked_receiver_not_idle() {
        // Reproduce the exact demo flow that hung under ADR-0022 Option A:
        //   1. B is Blocked on ep (waiting for IPC).
        //   2. A is current, queue is empty (B was the queue's only entry
        //      before it blocked; it was dequeued and removed when blocking).
        //   3. A calls ipc_send_and_yield: ipc_send delivers to B,
        //      unblock_receiver_on enqueues B → queue [B].
        //   4. yield_now re-enqueues A → queue [B, A] and dequeues head → B.
        //   5. The dispatcher MUST switch to B (not idle).
        //
        // Under ADR-0022 Option A, idle was at the FIFO head (added third by
        // kernel_entry's `add_task(B, A, idle)` ordering and rotated through
        // the ready queue), so step 5 selected idle and the kernel hung.
        // ADR-0026's fix removes idle from the FIFO entirely, so even with
        // idle registered, step 5 picks the just-unblocked B.
        let cpu = FakeCpu;
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let mut table = CapabilityTable::new();
        let mut s_a = AlignedStack::<512>::new();
        let mut s_b = AlignedStack::<512>::new();
        let mut s_idle = AlignedStack::<512>::new();
        let h_a = task_handle(0);
        let h_b = task_handle(1);
        let h_idle = task_handle(2);

        // Endpoint with full SEND+RECV rights (simulating cap_a / cap_b
        // installed in distinct tables in the BSP would be more realistic;
        // a single shared table is fine for this dispatch-flow test).
        let ep = create_endpoint(&mut ep_arena, Endpoint::new(0)).unwrap();
        let cap = Capability::new(CapRights::SEND | CapRights::RECV, CapObject::Endpoint(ep));
        let ep_cap = table.insert_root(cap).unwrap();

        // Move endpoint into RecvWaiting via a real ipc_recv call — this
        // models B having already executed `ipc_recv_and_yield`'s Phase 1.
        let recv_outcome = ipc_recv(&mut ep_arena, &mut queues, ep_cap, &mut table).unwrap();
        assert!(matches!(recv_outcome, RecvOutcome::Pending));

        // SAFETY: stack-local scheduler + 16-byte-aligned stacks.
        unsafe {
            sched.add_task(&cpu, h_b, spin_entry(), s_b.top()).unwrap();
            sched.add_task(&cpu, h_a, spin_entry(), s_a.top()).unwrap();
            register_idle(
                core::ptr::from_mut(&mut sched),
                &cpu,
                h_idle,
                spin_entry(),
                s_idle.top(),
            );
        }

        // Wire up the running shape: A is current; B is blocked on ep;
        // queue is empty (B was dequeued + blocked; A was dequeued).
        sched.ready.dequeue(); // h_b head → would have started running
        sched.ready.dequeue(); // h_a → about to be current
        sched.current = Some(h_a);
        sched.task_states[h_b.slot().index() as usize] = TaskState::Blocked { on: ep };
        assert!(
            sched.ready.is_empty(),
            "demo-flow precondition: queue is empty before A's send"
        );
        assert_eq!(
            sched.idle,
            Some(h_idle),
            "demo-flow precondition: idle is registered as fallback"
        );

        // A sends to B → ipc_send delivers (B was RecvWaiting); the bridge
        // unblocks B (queue becomes [B]); yield_now re-enqueues A (queue
        // becomes [B, A]); dispatcher dequeues B.
        // SAFETY: all four pointers refer to stack-local test state owned
        // exclusively by this single-threaded test. Each pointer is derived
        // exactly once from `core::ptr::from_mut`; no aliasing re-borrow.
        // FakeCpu's context_switch is a no-op marker; the path of interest
        // is the pre-switch dispatcher selecting B (not idle).
        let result = unsafe {
            ipc_send_and_yield(
                core::ptr::from_mut(&mut sched),
                &cpu,
                core::ptr::from_mut(&mut ep_arena),
                core::ptr::from_mut(&mut queues),
                core::ptr::from_mut(&mut table),
                ep_cap,
                send_msg(0xAAAA),
                None,
            )
        };

        assert_eq!(result.unwrap(), SendOutcome::Delivered);
        assert_eq!(
            sched.current,
            Some(h_b),
            "REGRESSION GUARD: dispatcher selected idle instead of just-unblocked B \
             — this is the exact 2026-05-06 smoke-regression failure mode"
        );
        assert_ne!(
            sched.current,
            Some(h_idle),
            "idle must not be dispatched while a real Ready task exists (ADR-0026)"
        );
        assert_eq!(
            sched.task_states[h_a.slot().index() as usize],
            TaskState::Ready,
            "A re-enqueued as Ready by yield_now"
        );
        assert_eq!(
            sched.task_states[h_b.slot().index() as usize],
            TaskState::Ready,
            "B unblocked"
        );
        assert_eq!(
            sched.ready.len(),
            1,
            "queue holds A after the switch (idle is in the fallback slot, not the queue)"
        );
        // Idle's slot was never touched by the dispatch chain.
        assert_eq!(sched.idle, Some(h_idle));
    }

    // ── ipc_send_and_yield three-case bundle (T-011) ──────────────────────────
    //
    // T-007 covered the `Deadlock` and `PendingAfterResume` paths of
    // `ipc_recv_and_yield`. T-011 closes the symmetric gap on the send
    // side: the bridge has three terminal shapes — Delivered (deliver +
    // unblock + yield), Enqueued (no receiver yet, no yield), and Err
    // (ipc_send failed, scheduler state must not have moved). All three
    // are asserted below. Refs: ADR-0017, ADR-0019, ADR-0021.

    /// Build a `Capability` of given rights on a fresh endpoint, install
    /// it into `table`, and return both handles.
    fn setup_send_endpoint(
        ep_arena: &mut EndpointArena,
        table: &mut CapabilityTable,
        rights: CapRights,
    ) -> (EndpointHandle, CapHandle) {
        let ep = create_endpoint(ep_arena, Endpoint::new(0)).unwrap();
        let cap = Capability::new(rights, CapObject::Endpoint(ep));
        let cap_h = table.insert_root(cap).unwrap();
        (ep, cap_h)
    }

    fn send_msg(label: u64) -> Message {
        Message {
            label,
            params: [label, label, label],
        }
    }

    #[test]
    fn ipc_send_and_yield_delivered_unblocks_receiver_and_yields() {
        // Setup: h0 (sender, current), h1 (receiver, Blocked on ep).
        // The endpoint's IPC queue is in RecvWaiting because h1 already
        // ran ipc_recv before being context-switched out.
        let cpu = FakeCpu;
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let mut table = CapabilityTable::new();
        let mut s0 = AlignedStack::<512>::new();
        let mut s1 = AlignedStack::<512>::new();
        let h0 = task_handle(0);
        let h1 = task_handle(1);

        let (ep, ep_cap) =
            setup_send_endpoint(&mut ep_arena, &mut table, CapRights::SEND | CapRights::RECV);

        // Move the endpoint into RecvWaiting via a real ipc_recv call.
        let recv_outcome = ipc_recv(&mut ep_arena, &mut queues, ep_cap, &mut table).unwrap();
        assert!(matches!(recv_outcome, RecvOutcome::Pending));

        // SAFETY: stacks are 16-byte aligned 512-byte buffers; FakeCpu
        // init_context is a no-op so the stack memory is never used.
        unsafe {
            sched.add_task(&cpu, h0, spin_entry(), s0.top()).unwrap();
            sched.add_task(&cpu, h1, spin_entry(), s1.top()).unwrap();
        }

        // Arrange the running shape: h0 is the current task; h1 is
        // blocked on `ep`. Both have already been popped from the ready
        // queue by their respective scheduler events.
        sched.ready.dequeue(); // h0 head → current
        sched.ready.dequeue(); // h1 → about to be marked Blocked
        sched.current = Some(h0);
        sched.task_states[1] = TaskState::Blocked { on: ep };

        let sched_ptr = core::ptr::from_mut(&mut sched);
        let arena_ptr = core::ptr::from_mut(&mut ep_arena);
        let queues_ptr = core::ptr::from_mut(&mut queues);
        let table_ptr = core::ptr::from_mut(&mut table);

        // SAFETY: all four pointers refer to stack-local test state
        // owned exclusively by this single-threaded test. Each pointer
        // was derived exactly once above; no aliasing re-borrow.
        let result = unsafe {
            ipc_send_and_yield(
                sched_ptr,
                &cpu,
                arena_ptr,
                queues_ptr,
                table_ptr,
                ep_cap,
                send_msg(42),
                None,
            )
        };

        assert_eq!(result.unwrap(), SendOutcome::Delivered);
        assert_eq!(sched.current, Some(h1), "yield switched to unblocked h1");
        assert_eq!(
            sched.task_states[0],
            TaskState::Ready,
            "h0 re-enqueued as Ready"
        );
        assert_eq!(sched.task_states[1], TaskState::Ready, "h1 unblocked");
        assert_eq!(
            sched.ready.len(),
            1,
            "ready queue holds h0 after the switch"
        );
        assert!(
            sched.contexts[0].switched,
            "FakeCpu records that h0's context was saved",
        );
    }

    #[test]
    fn ipc_send_and_yield_enqueued_does_not_yield() {
        // Setup: h0 alone, no receiver. ipc_send returns Enqueued; the
        // bridge must NOT yield (needs_yield = false), so the scheduler
        // is structurally unchanged after the call.
        let cpu = FakeCpu;
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let mut table = CapabilityTable::new();
        let mut s0 = AlignedStack::<512>::new();
        let h0 = task_handle(0);

        let (_, ep_cap) =
            setup_send_endpoint(&mut ep_arena, &mut table, CapRights::SEND | CapRights::RECV);

        // SAFETY: see prior tests.
        unsafe {
            sched.add_task(&cpu, h0, spin_entry(), s0.top()).unwrap();
        }
        sched.ready.dequeue();
        sched.current = Some(h0);

        let sched_ptr = core::ptr::from_mut(&mut sched);
        let arena_ptr = core::ptr::from_mut(&mut ep_arena);
        let queues_ptr = core::ptr::from_mut(&mut queues);
        let table_ptr = core::ptr::from_mut(&mut table);

        // SAFETY: pointer-derivation discipline as above.
        let result = unsafe {
            ipc_send_and_yield(
                sched_ptr,
                &cpu,
                arena_ptr,
                queues_ptr,
                table_ptr,
                ep_cap,
                send_msg(99),
                None,
            )
        };

        assert_eq!(result.unwrap(), SendOutcome::Enqueued);
        assert_eq!(sched.current, Some(h0), "no switch on Enqueued");
        assert_eq!(sched.task_states[0], TaskState::Ready);
        assert_eq!(sched.ready.len(), 0, "ready queue still empty");
        assert!(
            !sched.contexts[0].switched,
            "FakeCpu did not record a save (no switch happened)",
        );
    }

    #[test]
    fn ipc_send_and_yield_send_error_preserves_scheduler_state() {
        // Setup: h0 (current), h1 (Ready in queue). The endpoint cap
        // grants RECV but not SEND, so `ipc_send` fails its rights check
        // with `IpcError::InvalidCapability`. The bridge must surface
        // this typed error and leave the scheduler exactly as it was.
        // Symmetric to T-007's `ipc_recv_and_yield_returns_deadlock_…`
        // state-restore guarantee.
        let cpu = FakeCpu;
        let mut sched: Scheduler<FakeCpu> = Scheduler::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let mut table = CapabilityTable::new();
        let mut s0 = AlignedStack::<512>::new();
        let mut s1 = AlignedStack::<512>::new();
        let h0 = task_handle(0);
        let h1 = task_handle(1);

        // RECV-only cap → ipc_send's SEND check fails.
        let (_, ep_cap) = setup_send_endpoint(&mut ep_arena, &mut table, CapRights::RECV);

        // SAFETY: see prior tests.
        unsafe {
            sched.add_task(&cpu, h0, spin_entry(), s0.top()).unwrap();
            sched.add_task(&cpu, h1, spin_entry(), s1.top()).unwrap();
        }
        sched.ready.dequeue(); // h0 → current
        sched.current = Some(h0);
        // h1 stays in the ready queue.

        // Snapshot pre-call scheduler shape.
        let prior_current = sched.current;
        let prior_state_0 = sched.task_states[0];
        let prior_state_1 = sched.task_states[1];
        let prior_ready_len = sched.ready.len();

        let sched_ptr = core::ptr::from_mut(&mut sched);
        let arena_ptr = core::ptr::from_mut(&mut ep_arena);
        let queues_ptr = core::ptr::from_mut(&mut queues);
        let table_ptr = core::ptr::from_mut(&mut table);

        // SAFETY: pointer-derivation discipline as above.
        let result = unsafe {
            ipc_send_and_yield(
                sched_ptr,
                &cpu,
                arena_ptr,
                queues_ptr,
                table_ptr,
                ep_cap,
                send_msg(0),
                None,
            )
        };

        assert!(
            matches!(result, Err(SchedError::Ipc(IpcError::InvalidCapability))),
            "expected Err(Ipc(InvalidCapability)), got {result:?}"
        );
        // Scheduler state must be untouched.
        assert_eq!(sched.current, prior_current);
        assert_eq!(sched.task_states[0], prior_state_0);
        assert_eq!(sched.task_states[1], prior_state_1);
        assert_eq!(sched.ready.len(), prior_ready_len);
        assert!(
            !sched.contexts[0].switched,
            "no switch — error path returned before yield",
        );
    }
}
