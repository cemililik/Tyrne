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
//! [t003]: https://github.com/cemililik/UmbrixOS/blob/main/docs/analysis/tasks/phase-a/T-003-ipc-primitives.md
//! [adr-0017]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0017-ipc-primitive-set.md

use crate::cap::{CapHandle, CapObject, CapRights, Capability, CapabilityTable};
use crate::obj::endpoint::{EndpointArena, EndpointHandle};
use crate::obj::notification::NotificationArena;
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
/// [adr-0017]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0017-ipc-primitive-set.md
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
pub struct IpcQueues {
    states: [EndpointState; ENDPOINT_ARENA_CAPACITY],
}

impl Default for IpcQueues {
    fn default() -> Self {
        Self {
            states: core::array::from_fn(|_| EndpointState::Idle),
        }
    }
}

impl IpcQueues {
    /// Construct a new set of queues with every endpoint in the `Idle` state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn state_of(&mut self, handle: EndpointHandle) -> &mut EndpointState {
        &mut self.states[handle.slot().index() as usize]
    }

    fn peek_state(&self, handle: EndpointHandle) -> &EndpointState {
        &self.states[handle.slot().index() as usize]
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
    if let Some(xfer) = transfer {
        caller_table
            .lookup(xfer)
            .map_err(|_| IpcError::InvalidTransferCap)?;
    }

    // Confirm the endpoint handle is still live in the arena.
    ep_arena
        .get(ep_handle.slot())
        .ok_or(IpcError::InvalidCapability)?;

    let state = queues.state_of(ep_handle);
    let old = core::mem::replace(state, EndpointState::Idle);
    match old {
        EndpointState::RecvWaiting => {
            // A receiver is waiting. Extract the cap and transition to
            // RecvComplete so the receiver can pick up the result.
            let owned = take_cap_if_some(caller_table, transfer)?;
            *state = EndpointState::RecvComplete { msg, cap: owned };
            Ok(SendOutcome::Delivered)
        }
        EndpointState::Idle => {
            // No receiver. Store the message in the endpoint queue.
            let owned = take_cap_if_some(caller_table, transfer)?;
            *state = EndpointState::SendPending { msg, cap: owned };
            Ok(SendOutcome::Enqueued)
        }
        occupied @ (EndpointState::SendPending { .. } | EndpointState::RecvComplete { .. }) => {
            // Restore the original state unchanged.
            *state = occupied;
            Err(IpcError::QueueFull)
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

    // Pre-flight: if there is a pending cap to transfer, ensure the receiver's
    // table has room before committing the state transition.
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
/// immediately and any registered waiter is recorded for A5's scheduler to
/// wake. In Phase A4 (no scheduler), only the bit-set is performed.
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
    let notif_handle = {
        let cap = caller_table
            .lookup(notif_cap)
            .map_err(|_| IpcError::InvalidCapability)?;
        if !cap.rights().contains(CapRights::NOTIFY) {
            return Err(IpcError::InvalidCapability);
        }
        match cap.object() {
            CapObject::Notification(h) => h,
            _ => return Err(IpcError::InvalidCapability),
        }
    };

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
        ipc_notify, ipc_recv, ipc_send, IpcError, IpcQueues, Message, RecvOutcome, SendOutcome,
    };
    use crate::cap::{CapHandle, CapObject, CapRights, Capability, CapabilityTable};
    use crate::obj::endpoint::{create_endpoint, Endpoint, EndpointArena, EndpointHandle};
    use crate::obj::notification::{create_notification, Notification, NotificationArena};

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
        let (_, xfer_ep_handle) = {
            let h = create_endpoint(&mut ep_arena, Endpoint::new(1)).unwrap();
            let c = Capability::new(all_ep_rights(), CapObject::Endpoint(h));
            let ch = sender_table.insert_root(c).unwrap();
            (ch, h)
        };
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
        let (_, task_ep_handle) = {
            let h = create_endpoint(&mut ep_arena, Endpoint::new(2)).unwrap();
            let c = Capability::new(all_ep_rights(), CapObject::Endpoint(h));
            let ch = sender_table.insert_root(c).unwrap();
            (ch, h)
        };
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
}
