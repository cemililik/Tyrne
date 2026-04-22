//! # tyrne-bsp-qemu-virt
//!
//! Board Support Package for QEMU's aarch64 `virt` machine — the primary
//! development target per [ADR-0004][adr-0004] and the BSP that every
//! Tyrne feature is first exercised against.
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
//! [adr-0004]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0004-target-platforms.md
//! [adr-0012]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0012-boot-flow-qemu-virt.md
//! [boot-doc]: https://github.com/cemililik/TyrneOS/blob/main/docs/architecture/boot.md

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

use tyrne_hal::{Console, FmtWriter};
use tyrne_kernel::cap::{CapHandle, CapObject, CapRights, Capability, CapabilityTable};
use tyrne_kernel::ipc::{IpcQueues, Message, RecvOutcome};
use tyrne_kernel::obj::endpoint::{create_endpoint, Endpoint, EndpointArena};
use tyrne_kernel::obj::task::{create_task, Task, TaskArena};
use tyrne_kernel::sched::{ipc_recv_and_yield, ipc_send_and_yield, start, yield_now, Scheduler};

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
/// [adr-0012]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0012-boot-flow-qemu-virt.md
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

// SAFETY: Tyrne v1 is single-core and cooperative; no two tasks ever run
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

    /// Return a raw `*mut T` pointer to the cell's storage without
    /// materialising a `&mut` to the underlying `MaybeUninit<T>`.
    ///
    /// Used by the raw-pointer scheduler bridge per [ADR-0021]: the BSP
    /// hands `*mut T` to the kernel's `ipc_send_and_yield` /
    /// `ipc_recv_and_yield` / `yield_now` entry points so that no `&mut`
    /// reference to any shared kernel state is alive across
    /// `cpu.context_switch`.
    ///
    /// The implementation is a plain pointer cast (`UnsafeCell::get()`
    /// returns `*mut MaybeUninit<T>`, then `cast::<T>` is a zero-cost
    /// reinterpretation permitted because `MaybeUninit<T>` shares `T`'s
    /// layout), so no borrow of any kind is produced here.
    ///
    /// # Safety
    ///
    /// The caller must ensure the cell has been initialised via a prior
    /// `(*cell.0.get()).write(...)` before dereferencing the returned pointer,
    /// and must not use the pointer to create a `&mut T` that outlives a
    /// cooperative context switch (ADR-0021). Audit: UNSAFE-2026-0013.
    ///
    /// [ADR-0021]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0021-raw-pointer-scheduler-ipc-bridge.md
    #[inline]
    #[allow(
        clippy::mut_from_ref,
        reason = "returns a raw pointer, not a reference; aliasing discipline documented in ADR-0021"
    )]
    const fn as_mut_ptr(&self) -> *mut T {
        self.0.get().cast::<T>()
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
/// Stack for the kernel idle task (ADR-0022). Sized the same as application
/// task stacks — the idle loop's `wfi` + `yield_now` path uses far less than
/// 4 KiB, but keeping a uniform `TaskStack` avoids a second static type just
/// for the idle path.
static TASK_IDLE_STACK: TaskStack = TaskStack::new();

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

/// Task kernel-object arena — global per [ADR-0016]. Although the v1 demo
/// never reads this arena after `create_task` has returned the two
/// `TaskHandle`s, global storage is the uniform pattern established by
/// ADR-0016 for every kernel-object kind. Keeping `TaskArena` here (and
/// not on `kernel_entry`'s stack) avoids a second BSP static-cell churn
/// when task destruction / status-query APIs arrive in later Phase B work.
///
/// [ADR-0016]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0016-kernel-object-storage.md
static TASK_ARENA: StaticCell<TaskArena> = StaticCell::new();

// ─── Idle task ────────────────────────────────────────────────────────────────

/// Kernel idle task — runs when no application task is ready.
///
/// Per [ADR-0022], the BSP owns the idle entry. The loop is a
/// `spin_loop` + `yield_now` — **not** `wfi` + `yield_now` — until a
/// timer IRQ source lands (T-009). In the v1 workload there is no IRQ
/// configured; a `wfi` here would suspend the core indefinitely the first
/// time FIFO scheduling dispatches idle between two ready application
/// tasks, hanging the demo. The spin-yield form keeps the kernel live and
/// the demo's cooperative-round-trip trace intact, at the cost of one
/// extra context switch per inter-task yield when idle happens to sit at
/// the head of the ready queue. ADR-0022 *Revision notes* records this
/// adjustment.
///
/// When T-009 wires a timer IRQ (and a fallback wake source is guaranteed),
/// this loop's body becomes `cpu.wait_for_interrupt(); yield_now(...)`.
/// The shape of the function and its registration path stay the same.
///
/// Full IPC-graph deadlock (every task blocked + no IRQ source) is
/// visible today as "idle spin-yielding forever", not a panic — the
/// kernel stays live; typed `SchedError::Deadlock` is reachable only if
/// the BSP did not register idle at all.
///
/// [ADR-0022]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0022-idle-task-and-typed-scheduler-deadlock.md
fn idle_entry() -> ! {
    // SAFETY: CPU is fully initialised in `kernel_entry` before `start()`;
    // single-core cooperative scheduling prevents concurrent access.
    // Audit: UNSAFE-2026-0010.
    let cpu = unsafe { (*CPU.0.get()).assume_init_ref() };
    loop {
        core::hint::spin_loop();
        // SAFETY: per ADR-0021 — `SCHED.as_mut_ptr()` is a pure pointer
        // cast (UNSAFE-2026-0013); idle's stack frame holds no `&mut` to
        // any shared state across the cooperative switch. `yield_now`
        // can only return `Err(NoCurrentTask)`, which is impossible once
        // the scheduler has started. Audit: UNSAFE-2026-0014.
        unsafe {
            yield_now(SCHED.as_mut_ptr(), cpu).expect("idle: yield_now failed");
        }
    }
}

// ─── Task B ───────────────────────────────────────────────────────────────────

/// IPC demo — receiver side. Registers as receiver on the endpoint, waits for
/// Task A's message, then sends a reply and yields to Task A. Control does not
/// return from that final yield in the v1 single-round demo: Task A's
/// `ipc_recv_and_yield` picks up the reply without blocking and runs to its
/// own spin loop. The tail-end `loop { spin_loop() }` therefore satisfies the
/// `fn() -> !` return type but is structurally unreachable.
fn task_b() -> ! {
    // SAFETY: CONSOLE is fully initialised in `kernel_entry` before `start()`;
    // single-core cooperative scheduling prevents concurrent access.
    // Audit: UNSAFE-2026-0010.
    let console = unsafe { (*CONSOLE.0.get()).assume_init_ref() };
    let mut w = FmtWriter(console);
    let _ = writeln!(w, "tyrne: task B \u{2014} waiting for IPC");

    // Register as receiver on the endpoint. If no sender is ready, blocks and
    // yields to Task A. Resumes when Task A delivers a message.
    //
    // SAFETY: per ADR-0021 — every `*mut` here is produced by
    // `StaticCell::as_mut_ptr()`, which is a pure pointer cast and never
    // materialises a `&mut`. `ipc_recv_and_yield` itself takes raw pointers
    // and only creates momentary `&mut`s strictly outside its
    // `cpu.context_switch` window (per the scheduler module's shared safety
    // contract). The four statics (`SCHED`, `EP_ARENA`, `IPC_QUEUES`,
    // `TABLE_B`) refer to distinct referents. `CPU` is accessed via `&`, an
    // immutable borrow which is always aliasing-safe. No `&mut` in this
    // task's stack frame crosses the cooperative switch — this is the
    // pattern that retires UNSAFE-2026-0012. Audit: UNSAFE-2026-0014.
    let recv_outcome = unsafe {
        ipc_recv_and_yield(
            SCHED.as_mut_ptr(),
            (*CPU.0.get()).assume_init_ref(),
            EP_ARENA.as_mut_ptr(),
            IPC_QUEUES.as_mut_ptr(),
            TABLE_B.as_mut_ptr(),
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
        "tyrne: task B \u{2014} received IPC (label=0x{:x}); replying",
        msg.label
    );

    // Send reply. Since Task A is in the ready queue (not yet blocked on recv),
    // this transitions the endpoint to SendPending and returns Enqueued — no
    // auto-yield. An explicit yield_now follows so Task A can collect the reply.
    let reply = Message {
        label: 0xBBBB,
        params: [0; 3],
    };
    // SAFETY: per ADR-0021 — same raw-pointer discipline as the
    // `ipc_recv_and_yield` call above. `yield_now` follows the same shared
    // safety contract — caller-side never materialises a `&mut` across the
    // switch. Audit: UNSAFE-2026-0014.
    unsafe {
        ipc_send_and_yield(
            SCHED.as_mut_ptr(),
            (*CPU.0.get()).assume_init_ref(),
            EP_ARENA.as_mut_ptr(),
            IPC_QUEUES.as_mut_ptr(),
            TABLE_B.as_mut_ptr(),
            *(*EP_CAP_B.0.get()).assume_init_ref(),
            reply,
            None,
        )
        .expect("task B: ipc_send reply failed");

        // Yield explicitly so Task A can receive the reply that was just queued
        // as SendPending. Without this yield, A's ipc_recv_and_yield would never
        // run (cooperative scheduling; B never blocks again after the send).
        // `yield_now` only errors with `NoCurrentTask`, which cannot happen
        // once the scheduler has started.
        yield_now(SCHED.as_mut_ptr(), (*CPU.0.get()).assume_init_ref())
            .expect("task B: yield_now after reply failed");
    }

    // Unreachable in the v1 single-round demo — see the task_b doc comment.
    // The loop satisfies `fn() -> !`; Task A's `ipc_recv_and_yield` runs to
    // its own spin loop without yielding back, so no further Task B code
    // executes. A post-reply epilogue would require either a dedicated
    // rendezvous (e.g. a completion notification) or an extra yield from
    // Task A, both out of scope for A6.
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
    console.write_bytes(b"tyrne: task A -- sending IPC\n");

    let msg = Message {
        label: 0xAAAA,
        params: [1, 2, 3],
    };

    // Send to Task B. Because the scheduler adds B before A, B has already
    // called ipc_recv_and_yield and is in RecvWaiting state. The send delivers
    // immediately (Delivered) and ipc_send_and_yield yields to B.
    //
    // SAFETY: per ADR-0021 — same raw-pointer discipline as task_b.
    // Audit: UNSAFE-2026-0014.
    unsafe {
        ipc_send_and_yield(
            SCHED.as_mut_ptr(),
            (*CPU.0.get()).assume_init_ref(),
            EP_ARENA.as_mut_ptr(),
            IPC_QUEUES.as_mut_ptr(),
            TABLE_A.as_mut_ptr(),
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
    // SAFETY: per ADR-0021 — same raw-pointer discipline as task_b's
    // ipc_recv_and_yield call. Audit: UNSAFE-2026-0014.
    let reply_outcome = unsafe {
        ipc_recv_and_yield(
            SCHED.as_mut_ptr(),
            (*CPU.0.get()).assume_init_ref(),
            EP_ARENA.as_mut_ptr(),
            IPC_QUEUES.as_mut_ptr(),
            TABLE_A.as_mut_ptr(),
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
        "tyrne: task A \u{2014} received reply (label=0x{:x}); done",
        reply.label
    );
    console.write_bytes(b"tyrne: all tasks complete\n");

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

    console.write_bytes(b"tyrne: hello from kernel_main\n");

    // ── Kernel-object setup ───────────────────────────────────────────────────

    // Publish the Task arena before any `create_task` call — subsequent
    // access is via raw pointer per the ADR-0021 discipline, even though
    // the arena sees no post-setup use in the v1 demo.
    // SAFETY: single-core; no task is running yet. Audit: UNSAFE-2026-0001.
    unsafe {
        (*TASK_ARENA.0.get()).write(TaskArena::default());
    }
    // SAFETY: `TASK_ARENA` was just written above; momentary `&mut` is
    // scoped to these three `create_task` calls and drops before any task
    // runs. Audit: UNSAFE-2026-0014.
    let (handle_a, handle_b, handle_idle) = unsafe {
        let arena = &mut *TASK_ARENA.as_mut_ptr();
        let ha = create_task(arena, Task::new(0)).expect("create_task A failed");
        let hb = create_task(arena, Task::new(1)).expect("create_task B failed");
        let hi = create_task(arena, Task::new(2)).expect("create_task idle failed");
        (ha, hb, hi)
    };

    // ── IPC infrastructure ────────────────────────────────────────────────────

    let mut ep_arena = EndpointArena::default();
    let ep_handle =
        create_endpoint(&mut ep_arena, Endpoint::new(0)).expect("create_endpoint failed");

    // Least privilege: both tasks need both directions on the same endpoint —
    // A sends the initial message and receives the reply; B receives the
    // initial message and sends the reply. Neither task duplicates or
    // transfers the endpoint capability (every `ipc_*` call passes `None`),
    // so DUPLICATE and TRANSFER rights are deliberately omitted.
    let ep_rights = CapRights::SEND | CapRights::RECV;

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
    // The idle task (ADR-0022) is added LAST so it sits behind B and A in
    // the FIFO — it only ever runs when both application tasks are blocked.
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
        sched
            .add_task(cpu, handle_idle, idle_entry, TASK_IDLE_STACK.top())
            .expect("add_task idle failed: queue full or arena exhausted");
    }

    // Publish the scheduler before transferring control.
    // SAFETY: single-core; no task is running yet. Audit: UNSAFE-2026-0001.
    unsafe {
        (*SCHED.0.get()).write(sched);
    }

    console.write_bytes(b"tyrne: starting cooperative scheduler\n");

    // Transfer control to Task B (the first ready task). Does not return.
    // SAFETY: per ADR-0021 — `SCHED.as_mut_ptr()` is a pure pointer cast
    // (UNSAFE-2026-0013); `SCHED` was written above and no other code path
    // holds a `&mut Scheduler` at this point. `start` honours the raw-pointer
    // discipline: no `&mut` is live across the initial context switch.
    // Audit: UNSAFE-2026-0014.
    unsafe {
        start(SCHED.as_mut_ptr(), cpu);
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

    console.write_bytes(b"\n!! tyrne panic !!\n");
    let mut w = FmtWriter(&console);
    let _ = writeln!(w, "{info}");

    loop {
        core::hint::spin_loop();
    }
}
