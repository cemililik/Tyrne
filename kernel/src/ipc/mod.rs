//! IPC subsystem — Milestone A4 / [T-003][t003].
//!
//! Implements the three IPC primitives settled in [ADR-0017][adr-0017]:
//!
//! - [`ipc_send`] — synchronous rendezvous send on an [`Endpoint`].
//! - [`ipc_recv`] — synchronous rendezvous receive on an [`Endpoint`].
//! - [`ipc_notify`] — non-blocking bit-OR into a [`Notification`].
//!
//! ## Waiter-state design
//!
//! Each `Endpoint` can be in one of four states (see [`EndpointState`]):
//!
//! ```text
//! Idle ──send──► SendPending   (message + optional cap waiting for receiver)
//! Idle ──recv──► RecvWaiting   (receiver registered; no sender yet)
//! RecvWaiting ──send──► RecvComplete  (sender delivered to waiting receiver)
//! RecvComplete ──recv──► Idle         (receiver picks up the delivery)
//! SendPending  ──recv──► Idle         (receiver drains the pending send)
//! ```
//!
//! This state lives in [`IpcQueues`], not inside the [`Endpoint`] struct,
//! to avoid a circular module dependency: `cap` imports from `obj` for the
//! typed handles; putting `Capability` in `obj::Endpoint` would require `obj`
//! to import from `cap`, creating a cycle.
//!
//! ## Capability transfer
//!
//! When `ipc_send` is called with a non-`None` transfer handle, the capability
//! is extracted from the sender's table via [`CapabilityTable::cap_take`] and
//! stored in the endpoint's waiter state. On the matching `ipc_recv`, the
//! capability is installed into the receiver's table via
//! [`CapabilityTable::insert_root`]. Between these two calls, the capability is
//! owned by the endpoint state — not by any table.
//!
//! ## A4 scope note
//!
//! Phase A4 has no running scheduler. "Blocking" means recording the pending
//! state in the endpoint; the A5 scheduler will drain waiter queues when it
//! schedules tasks. `ipc_notify` sets bits on the notification word; waiter
//! wakeup is wired in A5.
//!
//! [t003]: https://github.com/cemililik/Tyrne/blob/main/docs/analysis/tasks/phase-a/T-003-ipc-primitives.md
//! [adr-0017]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0017-ipc-primitive-set.md

use crate::cap::{CapHandle, CapObject, CapRights, Capability, CapabilityTable};
use crate::obj::endpoint::{EndpointArena, EndpointHandle};
use crate::obj::notification::{NotificationArena, NotificationHandle};
use crate::obj::ENDPOINT_ARENA_CAPACITY;

// ── Public types ────────────────────────────────────────────────────────────

/// Fixed-size IPC message body. Passed by value — no heap, no pointers.
///
/// `label` is a caller-defined discriminator (opcode, tag, error code on
/// reply). `params` carries up to three arbitrary-width data words. Content
/// interpretation is entirely the caller's responsibility; the kernel does not
/// inspect or validate fields beyond delivering them.
///
/// Shape and rationale: [ADR-0017][adr-0017].
///
/// [adr-0017]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0017-ipc-primitive-set.md
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Message {
    /// Caller-defined discriminator. The kernel does not interpret this field.
    pub label: u64,
    /// Up to three general-purpose data words.
    pub params: [u64; 3],
}

/// Errors returned by IPC operations.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IpcError {
    /// The endpoint or notification capability is invalid, stale, or the
    /// caller lacks the required right (`SEND`, `RECV`, or `NOTIFY`).
    InvalidCapability,
    /// The endpoint's waiter queue is at capacity (depth 1 in v1): a second
    /// blocked sender arrived while the first is still pending, or a second
    /// receiver registered before the first was served.
    QueueFull,
    /// The capability nominated for transfer is invalid or stale.
    InvalidTransferCap,
    /// The receiver's capability table has no free slot; cap transfer aborted.
    /// The message itself is not delivered — retry after freeing a slot.
    ReceiverTableFull,
    /// The scheduler bridge's resume path observed an `ipc_recv` that still
    /// returned `Pending` after a cooperative context switch. Per
    /// [ADR-0022], this indicates a scheduler invariant violation: the
    /// sender that was supposed to deliver before unblocking the receiver
    /// either did not deliver or unblocked the wrong task. The bridge
    /// returns this variant (wrapped as `SchedError::Ipc(PendingAfterResume)`)
    /// instead of silently decoding as `Ok(Pending)` which the caller would
    /// turn into a downstream panic.
    ///
    /// [ADR-0022]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0022-idle-task-and-typed-scheduler-deadlock.md
    PendingAfterResume,
}

/// Outcome of a successful [`ipc_send`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SendOutcome {
    /// A receiver was waiting; the message was delivered immediately. The
    /// endpoint state advances to `RecvComplete`; the receiver must call
    /// [`ipc_recv`] to pick it up (A5 does this by scheduling the waiter).
    Delivered,
    /// No receiver was waiting; the message is stored in the endpoint queue.
    /// A subsequent [`ipc_recv`] will drain it.
    ///
    /// When used through the scheduler bridge (`ipc_send_and_yield`), this
    /// outcome means no receiver was unblocked and the bridge did **not**
    /// yield. The caller is responsible for an explicit `yield_now` if it
    /// wants another task to run before it continues (e.g. a reply-then-
    /// resume pattern). See the BSP's `task_b` reply path for an example.
    Enqueued,
}

/// Outcome of a successful [`ipc_recv`].
#[derive(Debug)]
pub enum RecvOutcome {
    /// A message was available — either a waiting sender or a prior delivery
    /// from a sender that found a registered receiver. Returns the message
    /// and an optional `CapHandle` in the **receiver's** table (if the sender
    /// transferred a capability).
    Received {
        /// The delivered message body.
        msg: Message,
        /// Present when the sender transferred a capability with the message.
        cap: Option<CapHandle>,
    },
    /// No sender was ready; this endpoint now records that a receiver is
    /// waiting. Call [`ipc_recv`] again after [`ipc_send`] delivers to pick
    /// up the message. In A5, the scheduler resumes the waiting task.
    Pending,
}

// ── Internal waiter state ───────────────────────────────────────────────────

/// State machine for one endpoint's IPC waiter queue (v1: depth 1).
///
/// Not `Copy` because `SendPending` and `RecvComplete` hold an optional
/// [`Capability`] which is deliberately non-`Copy`.
#[derive(Default)]
enum EndpointState {
    #[default]
    Idle,
    SendPending {
        msg: Message,
        /// Capability extracted from the sender's table via `cap_take`;
        /// held here until the receiver installs it via `insert_root`.
        cap: Option<Capability>,
    },
    RecvWaiting,
    RecvComplete {
        msg: Message,
        /// Capability waiting for the receiver to install via `insert_root`.
        cap: Option<Capability>,
    },
}

/// IPC waiter state for all endpoint slots.
///
/// Indexed by the raw slot index of an [`EndpointHandle`]. Callers must
/// validate the handle against the [`EndpointArena`] before using it to index
/// here — the arena's generation check ensures the slot is still live.
///
/// ## Generation tracking
///
/// Each slot also stores the generation of the endpoint that last wrote to it.
/// If a new endpoint is allocated in the same slot (after the old one was
/// destroyed), [`state_of`][Self::state_of] and [`peek_state`][Self::peek_state]
/// detect the mismatch and reset the slot to `Idle`, preventing the new
/// endpoint from inheriting stale waiter state (e.g. `RecvWaiting`) left by
/// its predecessor.
pub struct IpcQueues {
    states: [EndpointState; ENDPOINT_ARENA_CAPACITY],
    /// Generation of the endpoint that last occupied each slot.
    slot_generations: [u32; ENDPOINT_ARENA_CAPACITY],
}

impl Default for IpcQueues {
    fn default() -> Self {
        Self {
            states: core::array::from_fn(|_| EndpointState::Idle),
            slot_generations: [0; ENDPOINT_ARENA_CAPACITY],
        }
    }
}

impl IpcQueues {
    /// Construct a new set of queues with every endpoint in the `Idle` state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset the slot to `Idle` if the handle's generation has advanced past
    /// the recorded generation. Returns the slot index for callers to use.
    ///
    /// # Destruction invariant
    ///
    /// When the recorded generation is stale, any in-flight `Capability`
    /// carried by the old state would be silently dropped here — the
    /// function has no capability table in which to return it. A blanket
    /// drain-before-destroy invariant is what future endpoint destruction
    /// paths must uphold (Phase B); until such a path exists the mismatch
    /// branch is exercised only by `RecvWaiting` stale state, which carries
    /// no capability and is therefore safe to reset. The `debug_assert!`
    /// below catches the moment a destroy path introduces a `Some(cap)`
    /// leak: if the prior state was `SendPending { cap: Some(_), .. }` or
    /// `RecvComplete { cap: Some(_), .. }`, destruction forgot to drain.
    fn reset_if_stale_generation(&mut self, handle: EndpointHandle) -> usize {
        let idx = handle.slot().index() as usize;
        let gen = handle.slot().generation();
        if self.slot_generations[idx] != gen {
            debug_assert!(
                !matches!(
                    self.states[idx],
                    EndpointState::SendPending { cap: Some(_), .. }
                        | EndpointState::RecvComplete { cap: Some(_), .. }
                ),
                "endpoint slot must be drained before its generation is bumped: \
                 a SendPending/RecvComplete with Some(cap) would be silently dropped"
            );
            self.states[idx] = EndpointState::Idle;
            self.slot_generations[idx] = gen;
        }
        idx
    }

    fn state_of(&mut self, handle: EndpointHandle) -> &mut EndpointState {
        let idx = self.reset_if_stale_generation(handle);
        &mut self.states[idx]
    }

    fn peek_state(&mut self, handle: EndpointHandle) -> &EndpointState {
        let idx = self.reset_if_stale_generation(handle);
        &self.states[idx]
    }
}

// ── Public IPC operations ───────────────────────────────────────────────────

/// Send a message to an `Endpoint`, optionally transferring a capability.
///
/// The caller must hold a capability on the target endpoint with the
/// [`CapRights::SEND`] right (`ep_cap` in `caller_table`).
///
/// If `transfer` is `Some(h)`, the capability at `h` is atomically removed
/// from `caller_table` and stored in the endpoint's in-flight state until
/// the receiver delivers it to their own table via [`ipc_recv`].
///
/// # Errors
///
/// - [`IpcError::InvalidCapability`] — `ep_cap` is stale or lacks `SEND`.
/// - [`IpcError::InvalidTransferCap`] — `transfer` handle is stale.
/// - [`IpcError::QueueFull`] — a previous send is still pending (or a
///   delivery for a waiting receiver is uncollected).
pub fn ipc_send(
    ep_arena: &mut EndpointArena,
    queues: &mut IpcQueues,
    ep_cap: CapHandle,
    caller_table: &mut CapabilityTable,
    msg: Message,
    transfer: Option<CapHandle>,
) -> Result<SendOutcome, IpcError> {
    let ep_handle = validate_ep_cap(caller_table, ep_cap, CapRights::SEND)?;

    // Pre-flight: validate the transfer cap before touching endpoint state.
    // Also enforce the TRANSFER right: the caller must hold this right to
    // include the capability in an IPC message.
    if let Some(xfer) = transfer {
        let xfer_cap = caller_table
            .lookup(xfer)
            .map_err(|_| IpcError::InvalidTransferCap)?;
        if !xfer_cap.rights().contains(CapRights::TRANSFER) {
            return Err(IpcError::InvalidTransferCap);
        }
    }

    // Confirm the endpoint handle is still live in the arena.
    ep_arena
        .get(ep_handle.slot())
        .ok_or(IpcError::InvalidCapability)?;

    // Pre-flight: queue-full check. Peek state non-destructively before any
    // cap manipulation so that a QueueFull return leaves both the endpoint
    // state and caller_table unchanged.
    if matches!(
        queues.peek_state(ep_handle),
        EndpointState::SendPending { .. } | EndpointState::RecvComplete { .. }
    ) {
        return Err(IpcError::QueueFull);
    }

    // Take the cap before mutating endpoint state. If cap_take fails (e.g.
    // HasChildren), endpoint state is left unchanged — in particular,
    // RecvWaiting is preserved so the registered receiver is not lost.
    let owned = take_cap_if_some(caller_table, transfer)?;

    // Commit state transition. SendPending / RecvComplete branches are
    // excluded by the pre-check above.
    let state = queues.state_of(ep_handle);
    match core::mem::replace(state, EndpointState::Idle) {
        EndpointState::RecvWaiting => {
            *state = EndpointState::RecvComplete { msg, cap: owned };
            Ok(SendOutcome::Delivered)
        }
        EndpointState::Idle => {
            *state = EndpointState::SendPending { msg, cap: owned };
            Ok(SendOutcome::Enqueued)
        }
        EndpointState::SendPending { .. } | EndpointState::RecvComplete { .. } => {
            // Excluded by the pre-check above; unreachable in correct code.
            unreachable!()
        }
    }
}

/// Receive a message from an `Endpoint`.
///
/// The caller must hold a capability on the target endpoint with the
/// [`CapRights::RECV`] right.
///
/// - If a sender is already waiting (or a prior [`ipc_send`] delivered to a
///   registered receiver), the message is returned immediately.
/// - If no sender is present, the endpoint records that a receiver is waiting
///   and returns [`RecvOutcome::Pending`]. Call [`ipc_recv`] again after a
///   sender delivers to collect the message. In A5, the scheduler replaces
///   this second call by resuming the blocked receiver task.
///
/// # Errors
///
/// - [`IpcError::InvalidCapability`] — `ep_cap` is stale or lacks `RECV`.
/// - [`IpcError::ReceiverTableFull`] — the receiver's table has no free slot
///   for the capability carried with the pending message. Free a slot first.
/// - [`IpcError::QueueFull`] — a receiver is already registered on this endpoint.
pub fn ipc_recv(
    ep_arena: &mut EndpointArena,
    queues: &mut IpcQueues,
    ep_cap: CapHandle,
    caller_table: &mut CapabilityTable,
) -> Result<RecvOutcome, IpcError> {
    let ep_handle = validate_ep_cap(caller_table, ep_cap, CapRights::RECV)?;

    ep_arena
        .get(ep_handle.slot())
        .ok_or(IpcError::InvalidCapability)?;

    // Pre-flight: if the pending state carries a capability, ensure the
    // receiver's table has room before committing the state transition. This
    // guarantees that install_cap_if_some(caller_table, cap) cannot fail after
    // core::mem::replace moves the state to Idle — without this check a full
    // table would cause us to drop the in-flight capability. If
    // install_cap_if_some's error conditions or caller_table's capacity
    // semantics change, this invariant must be revisited.
    let pending_has_cap = matches!(
        queues.peek_state(ep_handle),
        EndpointState::SendPending { cap: Some(_), .. }
            | EndpointState::RecvComplete { cap: Some(_), .. }
    );
    if pending_has_cap && caller_table.is_full() {
        return Err(IpcError::ReceiverTableFull);
    }

    let state = queues.state_of(ep_handle);
    let old = core::mem::replace(state, EndpointState::Idle);
    match old {
        EndpointState::SendPending { msg, cap } | EndpointState::RecvComplete { msg, cap } => {
            // Deliver the message. Install cap (if any) into the receiver's table.
            let xfer = install_cap_if_some(caller_table, cap)?;
            Ok(RecvOutcome::Received { msg, cap: xfer })
        }
        EndpointState::Idle => {
            *state = EndpointState::RecvWaiting;
            Ok(RecvOutcome::Pending)
        }
        EndpointState::RecvWaiting => {
            *state = EndpointState::RecvWaiting;
            Err(IpcError::QueueFull)
        }
    }
}

/// OR `bits` into a `Notification`'s saturating word.
///
/// The caller must hold a capability on the target notification with the
/// [`CapRights::NOTIFY`] right. The operation is non-blocking: bits are set
/// immediately.
///
/// # Waiter wake-up is not yet wired
///
/// `Notification` has **no blocking-wait API** in Phases A4 and A5, so the
/// "wake a waiter" half of notify/wait is intentionally absent. If a future
/// path allows a task to block on a notification (e.g. a `wait_notify_and_yield`
/// scheduler bridge), this function must grow a corresponding
/// `unblock_waiter_on` step — otherwise any waiter would sleep forever
/// (silent deadlock). Tracked for Phase B alongside the scheduler/IPC
/// wait-set design.
///
/// # Errors
///
/// [`IpcError::InvalidCapability`] — `notif_cap` is stale or lacks `NOTIFY`.
pub fn ipc_notify(
    notif_arena: &mut NotificationArena,
    notif_cap: CapHandle,
    caller_table: &CapabilityTable,
    bits: u64,
) -> Result<(), IpcError> {
    let notif_handle = validate_notif_cap(caller_table, notif_cap)?;
    let notif = notif_arena
        .get_mut(notif_handle.slot())
        .ok_or(IpcError::InvalidCapability)?;
    notif.set(bits);
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn validate_ep_cap(
    table: &CapabilityTable,
    ep_cap: CapHandle,
    required: CapRights,
) -> Result<EndpointHandle, IpcError> {
    let cap = table
        .lookup(ep_cap)
        .map_err(|_| IpcError::InvalidCapability)?;
    if !cap.rights().contains(required) {
        return Err(IpcError::InvalidCapability);
    }
    match cap.object() {
        CapObject::Endpoint(h) => Ok(h),
        _ => Err(IpcError::InvalidCapability),
    }
}

fn validate_notif_cap(
    table: &CapabilityTable,
    notif_cap: CapHandle,
) -> Result<NotificationHandle, IpcError> {
    let cap = table
        .lookup(notif_cap)
        .map_err(|_| IpcError::InvalidCapability)?;
    if !cap.rights().contains(CapRights::NOTIFY) {
        return Err(IpcError::InvalidCapability);
    }
    match cap.object() {
        CapObject::Notification(h) => Ok(h),
        _ => Err(IpcError::InvalidCapability),
    }
}

fn take_cap_if_some(
    table: &mut CapabilityTable,
    handle: Option<CapHandle>,
) -> Result<Option<Capability>, IpcError> {
    match handle {
        Some(h) => table
            .cap_take(h)
            .map(Some)
            .map_err(|_| IpcError::InvalidTransferCap),
        None => Ok(None),
    }
}

fn install_cap_if_some(
    table: &mut CapabilityTable,
    cap: Option<Capability>,
) -> Result<Option<CapHandle>, IpcError> {
    match cap {
        Some(c) => table
            .insert_root(c)
            .map(Some)
            .map_err(|_| IpcError::ReceiverTableFull),
        None => Ok(None),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::manual_let_else,
    reason = "tests may use pragmas forbidden in production kernel code"
)]
mod tests {
    use super::{
        ipc_notify, ipc_recv, ipc_send, EndpointState, IpcError, IpcQueues, Message, RecvOutcome,
        SendOutcome,
    };
    use crate::cap::{CapHandle, CapObject, CapRights, Capability, CapabilityTable};
    use crate::obj::arena::SlotId;
    use crate::obj::endpoint::{create_endpoint, Endpoint, EndpointArena, EndpointHandle};
    use crate::obj::notification::{create_notification, Notification, NotificationArena};
    use crate::obj::TaskHandle;

    // ── Setup helpers ────────────────────────────────────────────────────────

    fn all_ep_rights() -> CapRights {
        CapRights::SEND
            | CapRights::RECV
            | CapRights::DUPLICATE
            | CapRights::DERIVE
            | CapRights::REVOKE
            | CapRights::TRANSFER
    }

    fn all_task_rights() -> CapRights {
        CapRights::DUPLICATE | CapRights::DERIVE | CapRights::REVOKE | CapRights::TRANSFER
    }

    /// Create an endpoint in the arena and install a capability in `table`.
    fn setup_ep(
        table: &mut CapabilityTable,
        ep_arena: &mut EndpointArena,
        rights: CapRights,
    ) -> (EndpointHandle, CapHandle) {
        let ep_handle = create_endpoint(ep_arena, Endpoint::new(0)).unwrap();
        let cap = Capability::new(rights, CapObject::Endpoint(ep_handle));
        let cap_handle = table.insert_root(cap).unwrap();
        (ep_handle, cap_handle)
    }

    /// Install a notification capability in `table`.
    fn setup_notif(table: &mut CapabilityTable, notif_arena: &mut NotificationArena) -> CapHandle {
        let notif_handle = create_notification(notif_arena, Notification::new(0)).unwrap();
        let cap = Capability::new(
            CapRights::NOTIFY | CapRights::DUPLICATE,
            CapObject::Notification(notif_handle),
        );
        table.insert_root(cap).unwrap()
    }

    fn all_rights() -> CapRights {
        CapRights::DUPLICATE | CapRights::DERIVE | CapRights::REVOKE | CapRights::TRANSFER
    }

    fn task_object(tag: u16) -> CapObject {
        CapObject::Task(TaskHandle::test_handle(tag, 0))
    }

    fn root_cap() -> Capability {
        Capability::new(all_rights(), task_object(0xAA))
    }

    fn test_msg(label: u64) -> Message {
        Message {
            label,
            params: [label, label, label],
        }
    }

    // ── send + recv (sender first) ────────────────────────────────────────────

    #[test]
    fn sender_first_delivers_on_recv() {
        let mut sender_table = CapabilityTable::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let (_, ep_cap) = setup_ep(&mut sender_table, &mut ep_arena, all_ep_rights());

        let outcome = ipc_send(
            &mut ep_arena,
            &mut queues,
            ep_cap,
            &mut sender_table,
            test_msg(42),
            None,
        )
        .unwrap();
        assert_eq!(outcome, SendOutcome::Enqueued);

        // Receiver with its own table picks up the message.
        let mut recv_table = CapabilityTable::new();
        let recv_ep_cap = {
            let cap = Capability::new(
                all_ep_rights(),
                CapObject::Endpoint(
                    // extract the handle by looking through the sender's cap
                    match sender_table.lookup(ep_cap).unwrap().object() {
                        CapObject::Endpoint(h) => h,
                        _ => panic!("wrong kind"),
                    },
                ),
            );
            recv_table.insert_root(cap).unwrap()
        };

        let recv_outcome =
            ipc_recv(&mut ep_arena, &mut queues, recv_ep_cap, &mut recv_table).unwrap();
        let RecvOutcome::Received { msg, cap: None } = recv_outcome else {
            panic!("expected Received, got {recv_outcome:?}");
        };
        assert_eq!(msg, test_msg(42));
    }

    // ── recv + send (receiver first) ─────────────────────────────────────────

    #[test]
    fn receiver_first_delivers_on_send() {
        let mut recv_table = CapabilityTable::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let (ep_handle, recv_ep_cap) = setup_ep(&mut recv_table, &mut ep_arena, all_ep_rights());

        // Receiver registers first — no sender yet.
        let outcome1 = ipc_recv(&mut ep_arena, &mut queues, recv_ep_cap, &mut recv_table).unwrap();
        assert!(matches!(outcome1, RecvOutcome::Pending));

        // Sender delivers.
        let mut sender_table = CapabilityTable::new();
        let sender_ep_cap = {
            let cap = Capability::new(all_ep_rights(), CapObject::Endpoint(ep_handle));
            sender_table.insert_root(cap).unwrap()
        };
        let send_outcome = ipc_send(
            &mut ep_arena,
            &mut queues,
            sender_ep_cap,
            &mut sender_table,
            test_msg(99),
            None,
        )
        .unwrap();
        assert_eq!(send_outcome, SendOutcome::Delivered);

        // Receiver picks up the delivery with a second recv call.
        let outcome2 = ipc_recv(&mut ep_arena, &mut queues, recv_ep_cap, &mut recv_table).unwrap();
        let RecvOutcome::Received { msg, cap: None } = outcome2 else {
            panic!("expected Received, got {outcome2:?}");
        };
        assert_eq!(msg, test_msg(99));
    }

    // ── capability transfer (sender first) ────────────────────────────────────

    #[test]
    fn send_transfers_cap_atomically() {
        let mut sender_table = CapabilityTable::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let (ep_handle, ep_cap) = setup_ep(&mut sender_table, &mut ep_arena, all_ep_rights());

        // Give sender a second endpoint cap to transfer.
        let xfer_ep_handle = create_endpoint(&mut ep_arena, Endpoint::new(1)).unwrap();
        let xfer_cap_h = {
            let c = Capability::new(all_task_rights(), CapObject::Endpoint(xfer_ep_handle));
            sender_table.insert_root(c).unwrap()
        };

        ipc_send(
            &mut ep_arena,
            &mut queues,
            ep_cap,
            &mut sender_table,
            test_msg(1),
            Some(xfer_cap_h),
        )
        .unwrap();

        // The cap must no longer be in the sender's table.
        assert!(sender_table.lookup(xfer_cap_h).is_err());

        // Receiver collects the message and the cap.
        let mut recv_table = CapabilityTable::new();
        let recv_ep_cap = {
            let c = Capability::new(all_ep_rights(), CapObject::Endpoint(ep_handle));
            recv_table.insert_root(c).unwrap()
        };
        let outcome = ipc_recv(&mut ep_arena, &mut queues, recv_ep_cap, &mut recv_table).unwrap();
        let RecvOutcome::Received {
            msg,
            cap: Some(recv_cap_h),
        } = outcome
        else {
            panic!("expected Received with cap, got {outcome:?}");
        };
        assert_eq!(msg, test_msg(1));
        // The transferred cap should now exist in the receiver's table.
        assert!(recv_table.lookup(recv_cap_h).is_ok());
    }

    // ── capability transfer (receiver first) ──────────────────────────────────

    #[test]
    fn receiver_first_then_send_with_cap() {
        let mut recv_table = CapabilityTable::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let (ep_handle, recv_ep_cap) = setup_ep(&mut recv_table, &mut ep_arena, all_ep_rights());

        ipc_recv(&mut ep_arena, &mut queues, recv_ep_cap, &mut recv_table).unwrap();

        // Sender with a cap to transfer.
        let mut sender_table = CapabilityTable::new();
        let task_ep_handle = create_endpoint(&mut ep_arena, Endpoint::new(2)).unwrap();
        let xfer_cap_h = {
            let c = Capability::new(all_task_rights(), CapObject::Endpoint(task_ep_handle));
            sender_table.insert_root(c).unwrap()
        };
        let sender_ep_cap = {
            let c = Capability::new(all_ep_rights(), CapObject::Endpoint(ep_handle));
            sender_table.insert_root(c).unwrap()
        };

        let send_out = ipc_send(
            &mut ep_arena,
            &mut queues,
            sender_ep_cap,
            &mut sender_table,
            test_msg(77),
            Some(xfer_cap_h),
        )
        .unwrap();
        assert_eq!(send_out, SendOutcome::Delivered);

        // Sender's table no longer has the xfer cap.
        assert!(sender_table.lookup(xfer_cap_h).is_err());

        // Receiver picks up.
        let outcome = ipc_recv(&mut ep_arena, &mut queues, recv_ep_cap, &mut recv_table).unwrap();
        let RecvOutcome::Received {
            msg,
            cap: Some(recv_cap_h),
        } = outcome
        else {
            panic!("expected Received with cap, got {outcome:?}");
        };
        assert_eq!(msg, test_msg(77));
        assert!(recv_table.lookup(recv_cap_h).is_ok());
    }

    // ── rights enforcement ───────────────────────────────────────────────────

    #[test]
    fn send_without_send_right_fails() {
        let mut table = CapabilityTable::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        // Cap with RECV but not SEND.
        let (_, ep_cap) = setup_ep(&mut table, &mut ep_arena, CapRights::RECV);
        assert_eq!(
            ipc_send(
                &mut ep_arena,
                &mut queues,
                ep_cap,
                &mut table,
                test_msg(0),
                None
            )
            .unwrap_err(),
            IpcError::InvalidCapability
        );
    }

    #[test]
    fn recv_without_recv_right_fails() {
        let mut table = CapabilityTable::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let (_, ep_cap) = setup_ep(&mut table, &mut ep_arena, CapRights::SEND);
        assert_eq!(
            ipc_recv(&mut ep_arena, &mut queues, ep_cap, &mut table).unwrap_err(),
            IpcError::InvalidCapability
        );
    }

    // ── queue-full paths ─────────────────────────────────────────────────────

    #[test]
    fn second_send_when_pending_fails() {
        let mut table = CapabilityTable::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let (_, ep_cap) = setup_ep(&mut table, &mut ep_arena, all_ep_rights());

        ipc_send(
            &mut ep_arena,
            &mut queues,
            ep_cap,
            &mut table,
            test_msg(1),
            None,
        )
        .unwrap();
        assert_eq!(
            ipc_send(
                &mut ep_arena,
                &mut queues,
                ep_cap,
                &mut table,
                test_msg(2),
                None
            )
            .unwrap_err(),
            IpcError::QueueFull
        );
    }

    #[test]
    fn second_recv_when_waiting_fails() {
        let mut table = CapabilityTable::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let (_, ep_cap) = setup_ep(&mut table, &mut ep_arena, all_ep_rights());

        ipc_recv(&mut ep_arena, &mut queues, ep_cap, &mut table).unwrap();
        assert_eq!(
            ipc_recv(&mut ep_arena, &mut queues, ep_cap, &mut table).unwrap_err(),
            IpcError::QueueFull
        );
    }

    // ── state-preservation on failed send ────────────────────────────────────

    #[test]
    fn send_with_bad_transfer_cap_preserves_recv_waiting() {
        // Regression: if cap_take fails (e.g. HasChildren), ipc_send must
        // leave a RecvWaiting endpoint in RecvWaiting — not silently reset it
        // to Idle, which would lose the registered receiver.
        let mut recv_table = CapabilityTable::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let (ep_handle, recv_ep_cap) = setup_ep(&mut recv_table, &mut ep_arena, all_ep_rights());

        // Receiver registers — endpoint transitions to RecvWaiting.
        ipc_recv(&mut ep_arena, &mut queues, recv_ep_cap, &mut recv_table).unwrap();

        // Build a transfer cap that has a child (cap_take must fail HasChildren).
        let mut sender_table = CapabilityTable::new();
        let sender_ep_cap = {
            let c = Capability::new(all_ep_rights(), CapObject::Endpoint(ep_handle));
            sender_table.insert_root(c).unwrap()
        };
        let parent_h = sender_table.insert_root(root_cap()).unwrap();
        let _child_h = sender_table
            .cap_derive(parent_h, all_rights(), task_object(1))
            .unwrap();
        // parent_h has a child → cap_take will return HasChildren.
        let err = ipc_send(
            &mut ep_arena,
            &mut queues,
            sender_ep_cap,
            &mut sender_table,
            test_msg(0),
            Some(parent_h),
        )
        .unwrap_err();
        assert_eq!(err, IpcError::InvalidTransferCap);

        // RecvWaiting must still be intact: a second recv attempt returns
        // QueueFull (one receiver already registered), not Pending.
        let err2 = ipc_recv(&mut ep_arena, &mut queues, recv_ep_cap, &mut recv_table).unwrap_err();
        assert_eq!(err2, IpcError::QueueFull);
    }

    // ── TRANSFER right enforcement ────────────────────────────────────────────

    #[test]
    fn send_without_transfer_right_on_xfer_cap_fails() {
        let mut table = CapabilityTable::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let (_, ep_cap) = setup_ep(&mut table, &mut ep_arena, all_ep_rights());

        // A cap without TRANSFER right.
        let no_transfer_h = {
            let c = Capability::new(CapRights::DUPLICATE | CapRights::DERIVE, task_object(1));
            table.insert_root(c).unwrap()
        };
        assert_eq!(
            ipc_send(
                &mut ep_arena,
                &mut queues,
                ep_cap,
                &mut table,
                test_msg(0),
                Some(no_transfer_h),
            )
            .unwrap_err(),
            IpcError::InvalidTransferCap
        );
    }

    // ── stale IpcQueues state reset on endpoint slot reuse ────────────────────

    #[test]
    fn stale_queue_state_reset_on_slot_reuse() {
        use crate::obj::endpoint::destroy_endpoint;
        let mut table = CapabilityTable::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let (ep_handle, ep_cap) = setup_ep(&mut table, &mut ep_arena, all_ep_rights());

        // Put the endpoint into RecvWaiting.
        ipc_recv(&mut ep_arena, &mut queues, ep_cap, &mut table).unwrap();

        // Destroy the endpoint (bumps slot generation).
        table.cap_drop(ep_cap).unwrap();
        destroy_endpoint(&mut ep_arena, ep_handle).unwrap();

        // Allocate a fresh endpoint in what may be the same slot.
        let (new_ep_handle, new_ep_cap) = setup_ep(&mut table, &mut ep_arena, all_ep_rights());
        let _ = new_ep_handle; // may or may not reuse the slot

        // The new endpoint must start in Idle, not inherit RecvWaiting.
        // Verify by sending — if state were RecvWaiting it would return
        // Delivered; if Idle it returns Enqueued.
        let outcome = ipc_send(
            &mut ep_arena,
            &mut queues,
            new_ep_cap,
            &mut table,
            test_msg(7),
            None,
        )
        .unwrap();
        assert_eq!(outcome, SendOutcome::Enqueued);
    }

    // ── notify ───────────────────────────────────────────────────────────────

    #[test]
    fn notify_sets_bits() {
        let mut table = CapabilityTable::new();
        let mut notif_arena = NotificationArena::default();
        let notif_cap = setup_notif(&mut table, &mut notif_arena);

        ipc_notify(&mut notif_arena, notif_cap, &table, 0b0101).unwrap();
        ipc_notify(&mut notif_arena, notif_cap, &table, 0b1010).unwrap();

        // The notification word should have all four bits set (OR semantics).
        let notif_handle = match table.lookup(notif_cap).unwrap().object() {
            CapObject::Notification(h) => h,
            _ => panic!("wrong kind"),
        };
        let word = notif_arena.get(notif_handle.slot()).unwrap().word();
        assert_eq!(word, 0b1111);
    }

    #[test]
    fn notify_without_notify_right_fails() {
        let mut table = CapabilityTable::new();
        let mut notif_arena = NotificationArena::default();
        let notif_handle = create_notification(&mut notif_arena, Notification::new(0)).unwrap();
        // Cap with DUPLICATE but not NOTIFY.
        let cap = Capability::new(CapRights::DUPLICATE, CapObject::Notification(notif_handle));
        let cap_h = table.insert_root(cap).unwrap();
        assert_eq!(
            ipc_notify(&mut notif_arena, cap_h, &table, 0xFF).unwrap_err(),
            IpcError::InvalidCapability
        );
    }

    // ── blocked-sender wake (sender-first round-trip) ─────────────────────────

    #[test]
    fn blocked_sender_delivered_on_subsequent_recv() {
        let mut sender_table = CapabilityTable::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let (ep_handle, ep_cap) = setup_ep(&mut sender_table, &mut ep_arena, all_ep_rights());

        // Sender blocks (no receiver).
        assert_eq!(
            ipc_send(
                &mut ep_arena,
                &mut queues,
                ep_cap,
                &mut sender_table,
                test_msg(55),
                None
            )
            .unwrap(),
            SendOutcome::Enqueued
        );

        // Receiver arrives and drains the queue.
        let mut recv_table = CapabilityTable::new();
        let recv_ep_cap = {
            let c = Capability::new(all_ep_rights(), CapObject::Endpoint(ep_handle));
            recv_table.insert_root(c).unwrap()
        };
        let outcome = ipc_recv(&mut ep_arena, &mut queues, recv_ep_cap, &mut recv_table).unwrap();
        let RecvOutcome::Received { msg, cap: None } = outcome else {
            panic!("expected Received");
        };
        assert_eq!(msg, test_msg(55));
    }

    // ── ReceiverTableFull pre-flight protects an in-flight cap (T-011) ────────
    //
    // The receiver-side `ipc_recv` performs a pre-flight check
    // (`pending_has_cap && caller_table.is_full()`) before mutating endpoint
    // state. The contract is: if the receiver's table cannot accept the
    // in-flight cap, the operation must fail *without* moving the
    // endpoint out of `SendPending` — the cap stays parked there until
    // some other recv attempt has room. This test pairs the failure-mode
    // with a successful retry to prove that step 1's failure preserved the
    // cap rather than silently dropping it. Refs: ADR-0017.

    #[test]
    fn recv_with_full_table_preserves_pending_cap() {
        // Arrange: sender holds an extra (transferable) endpoint cap; the
        // first ipc_send moves that cap into the endpoint's SendPending
        // state.
        let mut sender_table = CapabilityTable::new();
        let mut ep_arena = EndpointArena::default();
        let mut queues = IpcQueues::new();
        let (ep_handle, send_ep_cap) = setup_ep(&mut sender_table, &mut ep_arena, all_ep_rights());
        let xfer_target = create_endpoint(&mut ep_arena, Endpoint::new(0xBB)).unwrap();
        let xfer_cap_h = sender_table
            .insert_root(Capability::new(
                all_task_rights(),
                CapObject::Endpoint(xfer_target),
            ))
            .unwrap();

        let send_outcome = ipc_send(
            &mut ep_arena,
            &mut queues,
            send_ep_cap,
            &mut sender_table,
            test_msg(7),
            Some(xfer_cap_h),
        )
        .unwrap();
        assert_eq!(send_outcome, SendOutcome::Enqueued);

        // Receiver holds the recv cap; we then fill the rest of the table
        // with dummy task caps until is_full() returns true.
        let mut recv_table = CapabilityTable::new();
        let recv_ep_cap = recv_table
            .insert_root(Capability::new(
                CapRights::RECV,
                CapObject::Endpoint(ep_handle),
            ))
            .unwrap();

        // Track the first dummy so the recovery step has a slot to free.
        let mut first_dummy: Option<CapHandle> = None;
        let mut tag: u16 = 0;
        while !recv_table.is_full() {
            let h = recv_table
                .insert_root(Capability::new(all_rights(), task_object(tag)))
                .unwrap();
            if first_dummy.is_none() {
                first_dummy = Some(h);
            }
            tag = tag.wrapping_add(1);
        }
        assert!(recv_table.is_full(), "test setup: recv_table must be full");

        // Act 1: recv on a full table with a pending cap returns the typed
        // error and does *not* mutate the endpoint state.
        let err = ipc_recv(&mut ep_arena, &mut queues, recv_ep_cap, &mut recv_table).unwrap_err();
        assert_eq!(err, IpcError::ReceiverTableFull);

        // Act 2: free one slot in recv_table and retry. If the cap had been
        // silently dropped in Act 1, the second ipc_recv would either
        // succeed with `cap: None` (silent loss) or return Pending. The
        // assertion below — `cap: Some(_)` — proves the cap survived.
        let dummy = first_dummy.expect("test setup: at least one dummy was inserted");
        recv_table.cap_drop(dummy).unwrap();
        assert!(!recv_table.is_full());

        let outcome = ipc_recv(&mut ep_arena, &mut queues, recv_ep_cap, &mut recv_table).unwrap();
        let RecvOutcome::Received {
            msg,
            cap: Some(recv_cap_h),
        } = outcome
        else {
            panic!("expected Received with Some(cap), got {outcome:?}");
        };
        assert_eq!(msg, test_msg(7));
        // The recovered cap is live in recv_table.
        assert!(recv_table.lookup(recv_cap_h).is_ok());
    }

    // ── reset_if_stale_generation guard tests (T-011) ─────────────────────────
    //
    // `IpcQueues::reset_if_stale_generation` (used by `state_of` /
    // `peek_state`) protects the slot-reuse path. When a new endpoint is
    // allocated in a slot whose previous occupant has been destroyed, the
    // queue's recorded `slot_generations[idx]` lags behind the new
    // handle's generation. The function detects the mismatch, asserts —
    // in debug — that the prior state did not leave a `Some(cap)` behind
    // (which would be silently dropped here, since the function holds no
    // capability table), and resets the slot to `Idle`.
    //
    // The pair below covers both halves of the contract:
    //   1. SendPending { cap: Some(_) } with a stale generation must
    //      panic so the leak is loud.
    //   2. RecvWaiting (no cap) with a stale generation must reset
    //      silently — proving the assert's predicate is not over-broad.
    // Both tests reach reset_if_stale_generation via `peek_state` (a
    // public-to-the-module method that calls it as the first step), so
    // the production path is exercised, not a local copy.

    /// Build an `EndpointHandle` directly for tests that need to inject a
    /// specific (slot, generation) without going through the arena. Mirrors
    /// the test helper pattern used in `sched/mod.rs`.
    fn handle_at(slot_idx: u16, generation: u32) -> EndpointHandle {
        EndpointHandle::from_slot(SlotId::from_parts(slot_idx, generation))
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "endpoint slot must be drained")]
    fn stale_send_pending_with_some_cap_panics_in_debug() {
        // Inject a SendPending { cap: Some(_) } at slot 0 with the queue's
        // generation pinned at 0; then peek with generation 1. The
        // mismatch path is the only branch reset_if_stale_generation
        // takes, and the debug_assert fires before any state mutation.
        let mut queues = IpcQueues::new();
        let leaked_cap = Capability::new(all_rights(), task_object(0xDE));
        queues.states[0] = EndpointState::SendPending {
            msg: test_msg(0xCAFE),
            cap: Some(leaked_cap),
        };
        queues.slot_generations[0] = 0;

        // peek_state internally calls reset_if_stale_generation.
        let _ = queues.peek_state(handle_at(0, 1));
    }

    #[test]
    fn stale_recv_waiting_resets_silently() {
        // RecvWaiting carries no cap; the assert's predicate must NOT
        // fire here. The state must be reset to Idle and slot_generations
        // updated to the new generation.
        let mut queues = IpcQueues::new();
        queues.states[0] = EndpointState::RecvWaiting;
        queues.slot_generations[0] = 0;

        let _ = queues.peek_state(handle_at(0, 1));

        assert!(matches!(queues.states[0], EndpointState::Idle));
        assert_eq!(queues.slot_generations[0], 1);
    }

    #[test]
    fn stale_send_pending_without_cap_resets_silently() {
        // Symmetric to the test above but for SendPending { cap: None }
        // — the assert's predicate explicitly excludes this case
        // (only Some(_) is loud).
        let mut queues = IpcQueues::new();
        queues.states[0] = EndpointState::SendPending {
            msg: test_msg(0xBEEF),
            cap: None,
        };
        queues.slot_generations[0] = 0;

        let _ = queues.peek_state(handle_at(0, 1));

        assert!(matches!(queues.states[0], EndpointState::Idle));
        assert_eq!(queues.slot_generations[0], 1);
    }
}
