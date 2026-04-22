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
//! Per [ADR-0022], the BSP is responsible for registering a kernel idle
//! task via [`add_task`] during boot. The idle task's entry function loops
//! `Cpu::wait_for_interrupt` + `yield_now`; it is a regular ready-queue
//! resident and never leaves it. With idle registered, the ready queue is
//! structurally never empty, so [`SchedError::Deadlock`] is a defensive
//! return rather than an everyday path. Without idle, every `ipc_recv_and_yield`
//! that would have otherwise panicked now returns `Err(SchedError::Deadlock)`
//! with the scheduler state restored to its pre-call shape.
//!
//! [ADR-0022]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0022-idle-task-and-typed-scheduler-deadlock.md
//!
//! [T-004]: https://github.com/cemililik/UmbrixOS/blob/main/docs/analysis/tasks/phase-a/T-004-cooperative-scheduler.md
//! [ADR-0019]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0019-scheduler-shape.md
//! [ADR-0020]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0020-cpu-trait-v2-context-switch.md

use umbrix_hal::{ContextSwitch, Cpu, IrqGuard};

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
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SchedError {
    /// No task is currently running; the operation requires a current task.
    NoCurrentTask,
    /// The ready queue is full.
    QueueFull,
    /// IPC operation failed.
    Ipc(IpcError),
    /// The ready queue is empty while the current task has just blocked —
    /// every task in the system is waiting on IPC and no idle task is
    /// registered. Per [ADR-0022], registering a BSP idle task at boot
    /// makes this variant structurally unreachable in the v1 cooperative
    /// workload; it is preserved as a defensive return for preemption /
    /// SMP / "BSP forgot to register idle" so the kernel never panics on
    /// a userspace-reachable condition.
    ///
    /// When this is returned from [`ipc_recv_and_yield`], the caller's
    /// scheduler state is restored to what it was before the call — the
    /// current task is not left marked `Blocked`.
    ///
    /// [ADR-0022]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0022-idle-task-and-typed-scheduler-deadlock.md
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
// [ADR-0021]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0021-raw-pointer-scheduler-ipc-bridge.md

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
/// Panics if no tasks have been added (the ready queue is empty).
///
/// # Safety
///
/// See the "Shared safety contract" above. `sched` must satisfy *Pointer
/// validity* and must not alias any live `&mut Scheduler<C>`. Because the
/// bootstrap frame is abandoned, this function also honours the "no `&mut`
/// across the switch" rule — the throwaway context is constructed on a
/// stack frame that `cpu.context_switch` never returns to.
pub unsafe fn start<C: ContextSwitch + Cpu>(sched: *mut Scheduler<C>, cpu: &C) -> ! {
    let next_idx = {
        // SAFETY: caller contract — `sched` valid and exclusive for this
        // inner block; `&mut` does not cross the switch below. Audit:
        // UNSAFE-2026-0014.
        let s = unsafe { &mut *sched };

        #[allow(
            clippy::panic,
            reason = "empty ready queue is a kernel programming error"
        )]
        let Some(next_handle) = s.ready.dequeue() else {
            panic!("scheduler start called with empty ready queue");
        };
        let next_idx = next_handle.slot().index() as usize;
        s.task_states[next_idx] = TaskState::Ready;
        s.current = Some(next_handle);
        next_idx
    }; // `s: &mut Scheduler<C>` drops here.

    let mut throwaway = <C::TaskContext as Default>::default();
    let _guard = IrqGuard::new(cpu);
    // SAFETY: `next_idx` is in range (written by `add_task`); interrupts are
    // masked by `IrqGuard`; the throwaway current context lives on the
    // abandoned bootstrap stack frame and is never restored. No `&mut
    // Scheduler<C>` is live — the context pointer is derived from the raw
    // `sched` pointer. Audit: UNSAFE-2026-0008.
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
        // Audit: UNSAFE-2026-0014.
        let s = unsafe { &mut *sched };

        let current_handle = s.current.ok_or(SchedError::NoCurrentTask)?;
        let current_idx = current_handle.slot().index() as usize;

        // Re-enqueue current as ready. Cannot be full: the running task was
        // not in the ready queue (it was dequeued when it started running),
        // so at most TASK_ARENA_CAPACITY-1 other tasks are queued.
        s.task_states[current_idx] = TaskState::Ready;
        #[allow(
            clippy::panic,
            reason = "the running task is not in the ready queue, so at most \
                      TASK_ARENA_CAPACITY-1 tasks are enqueued; enqueue cannot fail"
        )]
        let Ok(()) = s.ready.enqueue(current_handle) else {
            panic!("scheduler invariant: ready queue full on yield re-enqueue");
        };

        // Dequeue the next task.
        let next_handle = match s.ready.dequeue() {
            Some(h) if h != current_handle => h,
            _ => {
                // Only one ready task exists. The queue is transiently empty
                // and `s.current` is unchanged. The next yield will re-enqueue
                // the current task; no switch is performed here.
                return Ok(());
            }
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
    // are disabled by `IrqGuard`. Audit: UNSAFE-2026-0008.
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
    // dropped before the `yield_now` call site. Audit: UNSAFE-2026-0014.
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
        // own momentary `&mut` without overlapping ours. Audit: UNSAFE-2026-0014.
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
///   registered). The scheduler state is restored before the return so
///   this variant leaves the caller's world unchanged. Per [ADR-0022],
///   registering an idle task at boot makes this variant unreachable in
///   the v1 cooperative workload.
/// - [`SchedError::Ipc`] — wraps [`IpcError`] failures from the underlying
///   [`ipc_recv`] calls. In particular,
///   [`IpcError::PendingAfterResume`] is returned when the resume-path
///   `ipc_recv` still returns `Pending` after a cooperative context switch
///   — a scheduler invariant violation that in debug builds also fires a
///   `debug_assert!`.
///
/// # Safety
///
/// See the "Shared safety contract" above. Every `*mut` parameter must meet
/// *Pointer validity*. The four pointers must not alias each other or any
/// live `&mut` in the caller's scope.
///
/// [ADR-0022]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0022-idle-task-and-typed-scheduler-deadlock.md
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
    // is dropped before the switch below. Audit: UNSAFE-2026-0014.
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
    // If the ready queue is empty after blocking the current task, every
    // task in the system is blocked on IPC and no idle task is registered
    // (ADR-0022). The pre-block scheduler state is restored before
    // returning `Err(SchedError::Deadlock)` so the caller observes the
    // same state it had before the bridge was called.
    let (current_idx, next_idx) = {
        // SAFETY: caller contract — `sched` valid and exclusive for this
        // block; `&mut` does not cross the switch below. Audit: UNSAFE-2026-0014.
        let s = unsafe { &mut *sched };
        let current_handle = s.current.ok_or(SchedError::NoCurrentTask)?;
        let current_idx = current_handle.slot().index() as usize;
        let prior_state = s.task_states[current_idx];
        s.task_states[current_idx] = TaskState::Blocked { on: ep_handle };
        s.current = None;

        let Some(next_handle) = s.ready.dequeue() else {
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
        // the ready queue before dequeue); both indices in range. Audit:
        // UNSAFE-2026-0008.
        unsafe {
            let ctx_ptr = (*sched).contexts.as_mut_ptr();
            let cur_ctx = &mut *ctx_ptr.add(current_idx);
            let nxt_ctx = &*ctx_ptr.add(next_idx);
            cpu.context_switch(cur_ctx, nxt_ctx);
        }
    }

    // Phase 3 — resumed; collect the delivered message.
    // SAFETY: caller contract — arenas/queues/table still valid; the
    // `&mut`s reacquired here did not exist during the switch. Audit:
    // UNSAFE-2026-0014.
    let result = unsafe {
        let arena_ref = &mut *ep_arena;
        let queues_ref = &mut *queues;
        let table_ref = &mut *caller_table;
        ipc_recv(arena_ref, queues_ref, ep_cap, table_ref)
    };

    // A second `Pending` result is a scheduler bug — the sender should have
    // queued a message before unblocking this task. The debug_assert fires
    // loudly in test/debug builds; release builds propagate the typed
    // IpcError::PendingAfterResume via SchedError::Ipc per ADR-0022 so a
    // stripped assert never silently decays into a caller-side panic.
    debug_assert!(
        !matches!(result, Ok(RecvOutcome::Pending)),
        "ipc_recv returned Pending after context-switch resume — \
         sender must deliver before unblocking receiver"
    );
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
        fn current_core_id(&self) -> umbrix_hal::CoreId {
            0
        }
        fn disable_irqs(&self) -> umbrix_hal::IrqState {
            umbrix_hal::IrqState(0)
        }
        fn restore_irq_state(&self, _: umbrix_hal::IrqState) {}
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
}
