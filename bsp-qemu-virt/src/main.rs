//! # umbrix-bsp-qemu-virt
//!
//! Board Support Package for QEMU's aarch64 `virt` machine — the primary
//! development target per [ADR-0004][adr-0004] and the BSP that every
//! Umbrix feature is first exercised against.
//!
//! This crate is the bootable binary: it provides the reset vector
//! (`_start`, assembled from `boot.s` via [`core::arch::global_asm!`]),
//! the Rust entry `kernel_entry`, a panic handler, and the hardware
//! implementations of the HAL traits. The A6 milestone demonstrates an
//! end-to-end IPC round trip: Task B registers as receiver on a capability-
//! gated endpoint, Task A sends a message, B replies, and A receives the
//! reply — proving the Phase A exit bar.
//!
//! The boot flow is documented in [`docs/architecture/boot.md`][boot-doc]
//! and the memory-layout decisions in [ADR-0012][adr-0012].
//!
//! [adr-0004]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0004-target-platforms.md
//! [adr-0012]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0012-boot-flow-qemu-virt.md
//! [boot-doc]: https://github.com/cemililik/UmbrixOS/blob/main/docs/architecture/boot.md

#![no_std]
#![no_main]
// Binary crate: `pub` items serve the linker (`#[no_mangle]`) rather than
// external consumers; `unreachable_pub` is therefore expected throughout.
#![allow(unreachable_pub, reason = "binary crate; pub items are for the linker")]

use core::arch::global_asm;
use core::cell::UnsafeCell;
use core::fmt::Write;
use core::mem::MaybeUninit;
use core::panic::PanicInfo;

use umbrix_hal::{Console, FmtWriter};
use umbrix_kernel::cap::{CapHandle, CapObject, CapRights, Capability, CapabilityTable};
use umbrix_kernel::ipc::{IpcQueues, Message, RecvOutcome};
use umbrix_kernel::obj::endpoint::{create_endpoint, Endpoint, EndpointArena};
use umbrix_kernel::obj::task::{create_task, Task, TaskArena};
use umbrix_kernel::sched::Scheduler;

mod console;
mod cpu;

use console::Pl011Uart;
use cpu::QemuVirtCpu;

/// MMIO base of the QEMU `virt` machine's PL011 UART.
///
/// Hardcoded per [ADR-0012][adr-0012]; each BSP carries its own
/// peripheral addresses. QEMU `virt` has exposed this address across
/// all versions the project targets.
///
/// [adr-0012]: https://github.com/cemililik/UmbrixOS/blob/main/docs/decisions/0012-boot-flow-qemu-virt.md
const PL011_UART_BASE: usize = 0x0900_0000;

// ─── StaticCell ───────────────────────────────────────────────────────────────
//
// Task entry functions are `fn() -> !` — they cannot capture environment.
// The scheduler, CPU, console, and IPC infrastructure are stored as immutable
// statics wrapping `UnsafeCell<MaybeUninit<T>>` so all tasks can reach them
// without `static mut`. All accesses remain `unsafe`; safety is ensured by the
// single-core, cooperative execution model (no two tasks run simultaneously).

/// `Sync` wrapper around `UnsafeCell<MaybeUninit<T>>` for write-once globals.
///
/// Written exactly once from `kernel_entry` (before `start()` is called).
/// Tasks then access the value through `assume_init_ref` / `assume_init_mut`.
/// All accesses are `unsafe`; this type only satisfies the `Sync` bound that
/// `static` requires.
struct StaticCell<T>(UnsafeCell<MaybeUninit<T>>);

// SAFETY: Umbrix v1 is single-core and cooperative; no two tasks ever run
// simultaneously, so there are no data races on `StaticCell` contents.
// Rejected alternatives: `Mutex` / `RwLock` require a runtime (heap, OS) or
// a spin implementation that itself relies on `unsafe` and adds overhead
// inappropriate for a bare-metal `static`. `OnceCell` / `LazyCell` from
// `core` are not available in `no_std` without an allocator in A5/A6.
// Audit: UNSAFE-2026-0010.
unsafe impl<T> Sync for StaticCell<T> {}

impl<T> StaticCell<T> {
    const fn new() -> Self {
        Self(UnsafeCell::new(MaybeUninit::uninit()))
    }
}

// ─── Task-stack storage ───────────────────────────────────────────────────────

/// Aligned storage for one task's call stack.
///
/// `#[repr(C, align(16))]` guarantees the 16-byte sp alignment required by
/// AAPCS64 at every function-call boundary. The inner array is wrapped in
/// `UnsafeCell` so the static need not be `mut`; all access is still `unsafe`.
#[repr(C, align(16))]
struct TaskStack(UnsafeCell<[u8; 4096]>);

// SAFETY: single-core cooperative kernel; only one task touches each stack at
// a time, and no task can interrupt another (cooperative scheduling).
// Rejected alternatives: wrapping in `Mutex` would add lock overhead and
// require a runtime or spin implementation. Making the static `mut` would
// expose the interior to safe code via `static mut` aliasing, which is
// worse. `UnsafeCell` with manual discipline is the standard bare-metal
// pattern and is the minimal wrapper that satisfies the `Sync` bound.
// Audit: UNSAFE-2026-0011.
unsafe impl Sync for TaskStack {}

impl TaskStack {
    const fn new() -> Self {
        Self(UnsafeCell::new([0u8; 4096]))
    }

    /// Return a pointer one past the end of the stack (the initial sp value).
    ///
    /// # Safety
    ///
    /// The caller must ensure this `TaskStack` outlives every task that uses it.
    unsafe fn top(&self) -> *mut u8 {
        // SAFETY: add(4096) is the one-past-end sentinel, not a dereference.
        // Caller guarantees the stack's lifetime exceeds the task.
        unsafe { (*self.0.get()).as_mut_ptr().add(4096) }
    }
}

/// Stack for task A.
static TASK_A_STACK: TaskStack = TaskStack::new();
/// Stack for task B.
static TASK_B_STACK: TaskStack = TaskStack::new();

// ─── Global kernel state ──────────────────────────────────────────────────────

/// The cooperative scheduler, concrete over the QEMU BSP CPU type.
static SCHED: StaticCell<Scheduler<QemuVirtCpu>> = StaticCell::new();

/// The CPU handle — needed by `yield_now` and IPC bridge to mask IRQs.
static CPU: StaticCell<QemuVirtCpu> = StaticCell::new();

/// The PL011 console — used by task functions for diagnostic output.
static CONSOLE: StaticCell<Pl011Uart> = StaticCell::new();

// ─── IPC infrastructure ───────────────────────────────────────────────────────

/// Endpoint arena — the kernel-object pool backing the IPC demo endpoint.
static EP_ARENA: StaticCell<EndpointArena> = StaticCell::new();

/// IPC queue state for all endpoint slots.
static IPC_QUEUES: StaticCell<IpcQueues> = StaticCell::new();

/// Task A's capability table — contains Task A's cap on the demo endpoint.
static TABLE_A: StaticCell<CapabilityTable> = StaticCell::new();

/// Task B's capability table — contains Task B's cap on the demo endpoint.
static TABLE_B: StaticCell<CapabilityTable> = StaticCell::new();

/// Task A's endpoint capability handle (index into `TABLE_A`).
static EP_CAP_A: StaticCell<CapHandle> = StaticCell::new();

/// Task B's endpoint capability handle (index into `TABLE_B`).
static EP_CAP_B: StaticCell<CapHandle> = StaticCell::new();

// ─── Task B ───────────────────────────────────────────────────────────────────

/// IPC demo — receiver side. Registers as receiver on the endpoint, waits for
/// Task A's message, then sends a reply. Yields cooperatively before spinning.
fn task_b() -> ! {
    // SAFETY: CONSOLE is fully initialised in `kernel_entry` before `start()`;
    // single-core cooperative scheduling prevents concurrent access.
    // Audit: UNSAFE-2026-0010.
    let console = unsafe { (*CONSOLE.0.get()).assume_init_ref() };
    let mut w = FmtWriter(console);
    let _ = writeln!(w, "umbrix: task B \u{2014} waiting for IPC");

    // Register as receiver on the endpoint. If no sender is ready, blocks and
    // yields to Task A. Resumes when Task A delivers a message.
    //
    // SAFETY (aliasing): `assume_init_mut` on SCHED, EP_ARENA, IPC_QUEUES, and
    // TABLE_B creates `&mut` references that are alive across the cooperative
    // context switch inside `ipc_recv_and_yield`. When Task A runs during the
    // suspension, it creates its own `&mut` references to SCHED, EP_ARENA, and
    // IPC_QUEUES from the same UnsafeCells — technically aliasing mutable
    // references. This is safe under the single-core cooperative invariant
    // (no two tasks execute simultaneously) and the same reasoning documented
    // in UNSAFE-2026-0012: the suspended task's stack frame does not access any
    // of these references after the context switch returns, and the compiler
    // cannot observe the aliasing across the assembly context-switch barrier.
    // SAFETY: see aliasing note above. Audit: UNSAFE-2026-0012.
    let recv_outcome = unsafe {
        (*SCHED.0.get())
            .assume_init_mut()
            .ipc_recv_and_yield(
                (*CPU.0.get()).assume_init_ref(),
                (*EP_ARENA.0.get()).assume_init_mut(),
                (*IPC_QUEUES.0.get()).assume_init_mut(),
                (*TABLE_B.0.get()).assume_init_mut(),
                *(*EP_CAP_B.0.get()).assume_init_ref(),
            )
            .expect("task B: ipc_recv failed")
    };

    let RecvOutcome::Received { msg, .. } = recv_outcome else {
        panic!("task B: expected Received outcome from ipc_recv_and_yield")
    };

    // SAFETY: CONSOLE initialised in kernel_entry; single-core cooperative. Audit: UNSAFE-2026-0010.
    let console = unsafe { (*CONSOLE.0.get()).assume_init_ref() };
    let mut w = FmtWriter(console);
    let _ = writeln!(
        w,
        "umbrix: task B \u{2014} received IPC (label=0x{:x}); replying",
        msg.label
    );

    // Send reply. Since Task A is in the ready queue (not yet blocked on recv),
    // this transitions the endpoint to SendPending and returns Enqueued — no
    // auto-yield. An explicit yield_now follows so Task A can collect the reply.
    let reply = Message {
        label: 0xBBBB,
        params: [0; 3],
    };
    // SAFETY: same aliasing invariants as the ipc_recv_and_yield call above.
    // Audit: UNSAFE-2026-0012.
    unsafe {
        (*SCHED.0.get())
            .assume_init_mut()
            .ipc_send_and_yield(
                (*CPU.0.get()).assume_init_ref(),
                (*EP_ARENA.0.get()).assume_init_mut(),
                (*IPC_QUEUES.0.get()).assume_init_mut(),
                (*TABLE_B.0.get()).assume_init_mut(),
                *(*EP_CAP_B.0.get()).assume_init_ref(),
                reply,
                None,
            )
            .expect("task B: ipc_send reply failed");

        // Yield explicitly so Task A can receive the reply that was just queued
        // as SendPending. Without this yield, A's ipc_recv_and_yield would never
        // run (cooperative scheduling; B never blocks again after the send).
        // yield_now returns Err only when current == None, which cannot happen
        // once the scheduler has started.
        let _ = (*SCHED.0.get())
            .assume_init_mut()
            .yield_now((*CPU.0.get()).assume_init_ref());
    }

    // SAFETY: CONSOLE initialised in kernel_entry; single-core cooperative. Audit: UNSAFE-2026-0010.
    let console = unsafe { (*CONSOLE.0.get()).assume_init_ref() };
    console.write_bytes(b"umbrix: task B -- done; spinning\n");
    loop {
        core::hint::spin_loop();
    }
}

// ─── Task A ───────────────────────────────────────────────────────────────────

/// IPC demo — initiator side. Sends a message to Task B, then waits for
/// the reply. On receiving the reply, prints the Phase A completion banner.
fn task_a() -> ! {
    // SAFETY: CONSOLE initialised in kernel_entry; single-core cooperative.
    // Audit: UNSAFE-2026-0010.
    let console = unsafe { (*CONSOLE.0.get()).assume_init_ref() };
    console.write_bytes(b"umbrix: task A -- sending IPC\n");

    let msg = Message {
        label: 0xAAAA,
        params: [1, 2, 3],
    };

    // Send to Task B. Because the scheduler adds B before A, B has already
    // called ipc_recv_and_yield and is in RecvWaiting state. The send delivers
    // immediately (Delivered) and ipc_send_and_yield yields to B.
    //
    // SAFETY: same aliasing invariants as task_b. Audit: UNSAFE-2026-0012.
    unsafe {
        (*SCHED.0.get())
            .assume_init_mut()
            .ipc_send_and_yield(
                (*CPU.0.get()).assume_init_ref(),
                (*EP_ARENA.0.get()).assume_init_mut(),
                (*IPC_QUEUES.0.get()).assume_init_mut(),
                (*TABLE_A.0.get()).assume_init_mut(),
                *(*EP_CAP_A.0.get()).assume_init_ref(),
                msg,
                None,
            )
            .expect("task A: ipc_send failed");
    }

    // Task A resumes here after B delivered the reply. The endpoint is now in
    // SendPending (B's reply). Calling ipc_recv_and_yield collects it immediately
    // without blocking (SendPending → Received → Idle).
    //
    // SAFETY: same aliasing invariants as task_b's ipc_recv_and_yield call. Audit: UNSAFE-2026-0012.
    let reply_outcome = unsafe {
        (*SCHED.0.get())
            .assume_init_mut()
            .ipc_recv_and_yield(
                (*CPU.0.get()).assume_init_ref(),
                (*EP_ARENA.0.get()).assume_init_mut(),
                (*IPC_QUEUES.0.get()).assume_init_mut(),
                (*TABLE_A.0.get()).assume_init_mut(),
                *(*EP_CAP_A.0.get()).assume_init_ref(),
            )
            .expect("task A: ipc_recv (reply) failed")
    };

    let RecvOutcome::Received { msg: reply, .. } = reply_outcome else {
        panic!("task A: expected Received outcome from reply ipc_recv_and_yield")
    };

    // SAFETY: CONSOLE initialised in kernel_entry; single-core cooperative. Audit: UNSAFE-2026-0010.
    let console = unsafe { (*CONSOLE.0.get()).assume_init_ref() };
    let mut w = FmtWriter(console);
    let _ = writeln!(
        w,
        "umbrix: task A \u{2014} received reply (label=0x{:x}); done",
        reply.label
    );
    console.write_bytes(b"umbrix: all tasks complete\n");

    loop {
        core::hint::spin_loop();
    }
}

// ─── Boot entry ───────────────────────────────────────────────────────────────

// Reset entry (`_start`). See `boot.s` and `docs/architecture/boot.md`.
global_asm!(include_str!("boot.s"));

/// First Rust entry after the assembly stub.
///
/// Sets up the console, CPU, kernel objects, capability tables, IPC
/// infrastructure, and cooperative scheduler. Registers Task B before Task A
/// so that B runs first and registers as IPC receiver before A sends.
/// Transfers control to the scheduler. This function never returns.
///
/// # Panics
///
/// Panics if any kernel-object allocation or capability-table operation fails.
/// All capacities are statically bounded and the demo uses far fewer objects
/// than the limits, so in practice none of these branches are reachable.
#[unsafe(no_mangle)]
pub extern "C" fn kernel_entry() -> ! {
    // ── Hardware setup ────────────────────────────────────────────────────────

    // SAFETY: 0x0900_0000 is the well-known QEMU virt PL011 UART MMIO
    // base, exclusively owned by this kernel in v1. Audit: UNSAFE-2026-0001.
    let console = unsafe { Pl011Uart::new(PL011_UART_BASE) };
    // SAFETY: constructed exactly once in kernel_entry; single-core v1.
    // See QemuVirtCpu::new # Safety. Audit: UNSAFE-2026-0006.
    let cpu = unsafe { QemuVirtCpu::new() };

    // SAFETY: single-core; no concurrent writer exists before `start()`.
    // Audit: UNSAFE-2026-0001.
    unsafe {
        (*CONSOLE.0.get()).write(console);
        (*CPU.0.get()).write(cpu);
    }

    // SAFETY: CONSOLE was written in the block above. Audit: UNSAFE-2026-0001.
    let console = unsafe { (*CONSOLE.0.get()).assume_init_ref() };
    // SAFETY: CPU was written in the block above. Audit: UNSAFE-2026-0001.
    let cpu = unsafe { (*CPU.0.get()).assume_init_ref() };

    console.write_bytes(b"umbrix: hello from kernel_main\n");

    // ── Kernel-object setup ───────────────────────────────────────────────────

    let mut arena = TaskArena::default();
    let handle_a = create_task(&mut arena, Task::new(0)).expect("create_task A failed");
    let handle_b = create_task(&mut arena, Task::new(1)).expect("create_task B failed");

    // ── IPC infrastructure ────────────────────────────────────────────────────

    let mut ep_arena = EndpointArena::default();
    let ep_handle =
        create_endpoint(&mut ep_arena, Endpoint::new(0)).expect("create_endpoint failed");

    // Both tasks get SEND | RECV so each can both send and receive on the
    // same endpoint — Task A sends the initial message and receives the reply;
    // Task B receives the initial message and sends the reply.
    let ep_rights = CapRights::SEND | CapRights::RECV | CapRights::DUPLICATE;

    let mut table_a = CapabilityTable::new();
    let mut table_b = CapabilityTable::new();

    let cap_a = Capability::new(ep_rights, CapObject::Endpoint(ep_handle));
    let cap_b = Capability::new(ep_rights, CapObject::Endpoint(ep_handle));

    let ep_cap_a = table_a
        .insert_root(cap_a)
        .expect("table A: insert_root failed");
    let ep_cap_b = table_b
        .insert_root(cap_b)
        .expect("table B: insert_root failed");

    // Publish IPC state before the scheduler starts.
    // SAFETY: single-core; no task is running yet. Audit: UNSAFE-2026-0001.
    unsafe {
        (*EP_ARENA.0.get()).write(ep_arena);
        (*IPC_QUEUES.0.get()).write(IpcQueues::new());
        (*TABLE_A.0.get()).write(table_a);
        (*TABLE_B.0.get()).write(table_b);
        (*EP_CAP_A.0.get()).write(ep_cap_a);
        (*EP_CAP_B.0.get()).write(ep_cap_b);
    }

    // ── Scheduler setup ───────────────────────────────────────────────────────

    let mut sched = Scheduler::<QemuVirtCpu>::new();

    // Task B is added FIRST so the scheduler runs B before A. B calls
    // ipc_recv_and_yield and enters RecvWaiting; only then does A call
    // ipc_send_and_yield, ensuring Delivered (not Enqueued) on the first send.
    //
    // SAFETY: add_task calls init_context; stack tops are 16-byte aligned
    // (guaranteed by TaskStack's repr) and remain valid for the process
    // lifetime. Entry functions are `fn() -> !`. Audit: UNSAFE-2026-0009.
    unsafe {
        sched
            .add_task(cpu, handle_b, task_b, TASK_B_STACK.top())
            .expect("add_task B failed: queue full or arena exhausted");
        sched
            .add_task(cpu, handle_a, task_a, TASK_A_STACK.top())
            .expect("add_task A failed: queue full or arena exhausted");
    }

    // Publish the scheduler before transferring control.
    // SAFETY: single-core; no task is running yet. Audit: UNSAFE-2026-0001.
    unsafe {
        (*SCHED.0.get()).write(sched);
    }

    console.write_bytes(b"umbrix: starting cooperative scheduler\n");

    // Transfer control to Task B (the first ready task). Does not return.
    // SAFETY: SCHED is fully initialised; contexts set up by add_task.
    // Audit: UNSAFE-2026-0008.
    unsafe { (*SCHED.0.get()).assume_init_mut() }.start(cpu);

    loop {
        core::hint::spin_loop();
    }
}

// ─── Panic handler ────────────────────────────────────────────────────────────

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // SAFETY: constructing a fresh Pl011Uart in the panic path is
    // best-effort diagnostic output. Writes may interleave if the original
    // instance is still reachable — acceptable per the Console contract
    // (ADR-0007). Audit: UNSAFE-2026-0002.
    let console = unsafe { Pl011Uart::new(PL011_UART_BASE) };

    console.write_bytes(b"\n!! umbrix panic !!\n");
    let mut w = FmtWriter(&console);
    let _ = writeln!(w, "{info}");

    loop {
        core::hint::spin_loop();
    }
}
