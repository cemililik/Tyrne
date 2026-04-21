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

    /// Start the scheduler by switching to the first ready task.
    ///
    /// Saves throwaway state for the bootstrap context (which is never
    /// resumed) and restores the first task. Intended to be called exactly
    /// once from `kernel_main` after tasks have been added.
    ///
    /// # IRQ state on task entry
    ///
    /// This method creates an `IrqGuard` immediately before the context switch.
    /// The guard is on the bootstrap stack frame, which is abandoned (never
    /// resumed). Tasks therefore begin executing with interrupts **masked**
    /// (DAIF = 0xF). In A5 this is acceptable because no interrupt sources
    /// are configured; a task that needs interrupts enabled must call
    /// `cpu.restore_irq_state(IrqState(0))` explicitly. This will be
    /// revisited when Phase B introduces a timer or other interrupt source.
    ///
    /// # Panics
    ///
    /// Panics if no tasks have been added (the ready queue is empty).
    pub fn start(&mut self, cpu: &C) {
        #[allow(
            clippy::panic,
            reason = "empty ready queue is a kernel programming error"
        )]
        let Some(next_handle) = self.ready.dequeue() else {
            panic!("Scheduler::start called with empty ready queue");
        };
        let next_idx = next_handle.slot().index() as usize;
        self.task_states[next_idx] = TaskState::Ready;
        self.current = Some(next_handle);

        let mut throwaway = C::TaskContext::default();
        let _guard = IrqGuard::new(cpu);
        // SAFETY: `next` was written by `add_task` → `init_context`.
        // Interrupts are disabled by the IrqGuard for the duration of the
        // switch. The throwaway current context is never restored.
        // Audit: UNSAFE-2026-0008.
        unsafe {
            cpu.context_switch(&mut throwaway, &self.contexts[next_idx]);
        }
    }

    /// Yield the current task: re-enqueue it as ready and switch to the
    /// next task at the head of the ready queue.
    ///
    /// If only one task exists (the queue is empty after re-enqueue), the
    /// function returns without switching — the single task keeps running.
    ///
    /// # Errors
    ///
    /// [`SchedError::NoCurrentTask`] if called before [`Scheduler::start`].
    pub fn yield_now(&mut self, cpu: &C) -> Result<(), SchedError> {
        let current_handle = self.current.ok_or(SchedError::NoCurrentTask)?;
        let current_idx = current_handle.slot().index() as usize;

        // Re-enqueue current as ready. Cannot be full: the running task was
        // not in the ready queue (it was dequeued when it started running),
        // so at most TASK_ARENA_CAPACITY-1 other tasks are queued.
        self.task_states[current_idx] = TaskState::Ready;
        let _ = self.ready.enqueue(current_handle);

        // Dequeue the next task.
        let next_handle = match self.ready.dequeue() {
            Some(h) if h != current_handle => h,
            _ => {
                // Only one ready task exists. The queue is transiently empty
                // and self.current is unchanged. The next yield will re-enqueue
                // the current task; no switch is performed here.
                return Ok(());
            }
        };

        let next_idx = next_handle.slot().index() as usize;
        self.task_states[next_idx] = TaskState::Ready;
        self.current = Some(next_handle);

        let _guard = IrqGuard::new(cpu);
        // SAFETY: current_idx != next_idx — they are two distinct tasks;
        // current was the running task (not in the queue) while next was
        // dequeued from the ready queue. Both indices are within
        // [0, TASK_ARENA_CAPACITY). Interrupts disabled by IrqGuard.
        // Audit: UNSAFE-2026-0008.
        unsafe {
            let ctx_ptr = self.contexts.as_mut_ptr();
            // Split-borrow: current and next are at distinct indices,
            // so the two resulting references do not alias.
            let cur_ctx = &mut *ctx_ptr.add(current_idx);
            let nxt_ctx = &*ctx_ptr.add(next_idx);
            cpu.context_switch(cur_ctx, nxt_ctx);
        }

        Ok(())
    }

    // ── IPC bridge ────────────────────────────────────────────────────────────

    /// Send a message; if it was delivered to a waiting receiver, unblock
    /// that receiver and yield the current task.
    ///
    /// # Errors
    ///
    /// Propagates [`IpcError`] as [`SchedError::Ipc`].
    #[allow(
        clippy::too_many_arguments,
        reason = "IPC bridge must forward all parameters that ipc_send requires"
    )]
    pub fn ipc_send_and_yield(
        &mut self,
        cpu: &C,
        ep_arena: &mut EndpointArena,
        queues: &mut IpcQueues,
        caller_table: &mut CapabilityTable,
        ep_cap: CapHandle,
        msg: Message,
        transfer: Option<CapHandle>,
    ) -> Result<SendOutcome, SchedError> {
        // Resolve the endpoint handle before calling ipc_send so we can
        // identify the blocked receiver even after state has changed.
        let ep_handle = Self::resolve_ep_cap(caller_table, ep_cap)?;

        let outcome = ipc_send(ep_arena, queues, ep_cap, caller_table, msg, transfer)?;

        if outcome == SendOutcome::Delivered {
            self.unblock_receiver_on(ep_handle);
            self.yield_now(cpu)?;
        }

        Ok(outcome)
    }

    /// Receive a message; if none is ready, block and yield to another task.
    ///
    /// When the blocked task is resumed (after a sender delivers), calls
    /// `ipc_recv` again to collect the delivered message.
    ///
    /// # Panics
    ///
    /// Panics when all tasks (including this one) are blocked on IPC
    /// simultaneously, producing a deadlock with no idle task to run.
    ///
    /// # Errors
    ///
    /// Propagates [`IpcError`] as [`SchedError::Ipc`].
    pub fn ipc_recv_and_yield(
        &mut self,
        cpu: &C,
        ep_arena: &mut EndpointArena,
        queues: &mut IpcQueues,
        caller_table: &mut CapabilityTable,
        ep_cap: CapHandle,
    ) -> Result<RecvOutcome, SchedError> {
        let ep_handle = Self::resolve_ep_cap(caller_table, ep_cap)?;

        let outcome = ipc_recv(ep_arena, queues, ep_cap, caller_table)?;

        if matches!(outcome, RecvOutcome::Pending) {
            let current_handle = self.current.ok_or(SchedError::NoCurrentTask)?;
            let current_idx = current_handle.slot().index() as usize;
            self.task_states[current_idx] = TaskState::Blocked { on: ep_handle };
            self.current = None;

            #[allow(
                clippy::panic,
                reason = "deadlock with no idle task is a fatal A5 condition; \
                          an idle task is Phase B work (ADR-0019 open questions)"
            )]
            let Some(next_handle) = self.ready.dequeue() else {
                panic!("deadlock: all tasks blocked on IPC and no idle task available");
            };
            let next_idx = next_handle.slot().index() as usize;
            self.task_states[next_idx] = TaskState::Ready;
            self.current = Some(next_handle);

            let _guard = IrqGuard::new(cpu);
            // SAFETY: current_idx != next_idx; both valid indices; IRQs
            // disabled. When this task is later resumed (by another task
            // calling ipc_send_and_yield → unblock_receiver_on → yield_now),
            // execution resumes after this context_switch call.
            // Audit: UNSAFE-2026-0008.
            unsafe {
                let ctx_ptr = self.contexts.as_mut_ptr();
                let cur_ctx = &mut *ctx_ptr.add(current_idx);
                let nxt_ctx = &*ctx_ptr.add(next_idx);
                cpu.context_switch(cur_ctx, nxt_ctx);
            }

            // Resumed here: the sender has delivered; collect the message.
            // A second Pending result would be a scheduler bug — the sender
            // should have queued a message before unblocking this task.
            let result = ipc_recv(ep_arena, queues, ep_cap, caller_table);
            debug_assert!(
                !matches!(result, Ok(RecvOutcome::Pending)),
                "ipc_recv returned Pending after context-switch resume — \
                 sender must deliver before unblocking receiver"
            );
            return result.map_err(SchedError::Ipc);
        }

        Ok(outcome)
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
                        let _ = self.ready.enqueue(handle);
                        return;
                    }
                }
            }
        }
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
    /// helper is reusable if a real init_context is ever wired into tests.
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
            sched
                .add_task(&cpu, h0, spin_entry(), s0.top())
                .unwrap();
            sched
                .add_task(&cpu, h1, spin_entry(), s1.top())
                .unwrap();
        }
        // Simulate h0 running: it was dequeued when it started running.
        sched.ready.dequeue(); // removes h0 (head of queue)
        sched.current = Some(h0);
        // h1 is still in the queue.
        assert_eq!(sched.ready.len(), 1);

        sched.yield_now(&cpu).unwrap();

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
        assert_eq!(sched.yield_now(&cpu), Err(SchedError::NoCurrentTask));
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
