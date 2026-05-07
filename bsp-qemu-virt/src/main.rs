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
//! [adr-0004]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0004-target-platforms.md
//! [adr-0012]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0012-boot-flow-qemu-virt.md
//! [boot-doc]: https://github.com/cemililik/Tyrne/blob/main/docs/architecture/boot.md

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

use tyrne_hal::{Console, Cpu, FmtWriter, Timer};
use tyrne_kernel::cap::{CapHandle, CapObject, CapRights, Capability, CapabilityTable};
use tyrne_kernel::ipc::{IpcQueues, Message, RecvOutcome};
use tyrne_kernel::obj::endpoint::{create_endpoint, Endpoint, EndpointArena};
use tyrne_kernel::obj::task::{create_task, Task, TaskArena};
use tyrne_kernel::sched::{
    ipc_recv_and_yield, ipc_send_and_yield, register_idle, start, yield_now, Scheduler,
};

mod console;
mod cpu;
mod exceptions;
mod gic;

use console::Pl011Uart;
use cpu::QemuVirtCpu;
use gic::{QemuVirtGic, QEMU_VIRT_GIC_CPU_INTERFACE_BASE, QEMU_VIRT_GIC_DISTRIBUTOR_BASE};

/// MMIO base of the QEMU `virt` machine's PL011 UART.
///
/// Hardcoded per [ADR-0012][adr-0012]; each BSP carries its own
/// peripheral addresses. QEMU `virt` has exposed this address across
/// all versions the project targets.
///
/// [adr-0012]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0012-boot-flow-qemu-virt.md
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
    /// [ADR-0021]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0021-raw-pointer-scheduler-ipc-bridge.md
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
        // SAFETY: UnsafeCell deref is sound under the caller's
        // outlives-task contract (see `# Safety`) and single-core
        // cooperative scheduling; `add(4096)` is a one-past-end
        // sentinel, not an out-of-bounds dereference. Rejected
        // alternatives + full rationale live in UNSAFE-2026-0011's
        // 2026-04-23 Amendment (covers both the Sync marker and
        // `top()`'s pointer arithmetic under one audit entry).
        // Audit: UNSAFE-2026-0011.
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

/// The GIC v2 controller handle. Constructed once in `kernel_entry`
/// and accessed via `&` from `irq_entry` (the asm-trampoline-side
/// dispatcher in `src/exceptions.rs`). Pre-T-012 v1 had no IRQ source;
/// T-012 lights this up alongside the vector-table install.
static GIC: StaticCell<QemuVirtGic> = StaticCell::new();

/// The PL011 console — used by task functions for diagnostic output.
static CONSOLE: StaticCell<Pl011Uart> = StaticCell::new();

/// Boot-time `now_ns()` snapshot, written once by `kernel_entry` after the
/// CPU is constructed and read by `task_a` to compute the boot-to-end
/// elapsed time. T-009 measurement scaffold; replaced by a richer
/// instrumentation surface when the first hypothesis-driven performance
/// review is conducted.
static BOOT_NS: StaticCell<u64> = StaticCell::new();

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
/// [ADR-0016]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0016-kernel-object-storage.md
static TASK_ARENA: StaticCell<TaskArena> = StaticCell::new();

// ─── Idle task ────────────────────────────────────────────────────────────────

/// Kernel idle task — runs when no application task is ready.
///
/// Per [ADR-0022] (idle-task ownership) + [ADR-0026] (idle dispatch via
/// fallback slot, supersedes ADR-0022's *idle-task-location* axis), the
/// BSP owns the idle entry function and registers it via [`register_idle`]
/// rather than [`Scheduler::add_task`]. The loop body is
/// `cpu.wait_for_interrupt()` + `yield_now`: `WFI` suspends the core
/// until any unmasked IRQ raises a wake, then the cooperative `yield_now`
/// returns control to the dispatcher. The dispatcher consults the idle
/// fallback slot only when [`Scheduler::ready`] is empty, so idle never
/// displaces a real Ready task — the structural fix for the 2026-05-06
/// QEMU smoke regression where round-robin FIFO had selected idle ahead
/// of a just-unblocked receiver.
///
/// In v1's cooperative IPC demo, idle's loop body executes only when both
/// application tasks are simultaneously blocked on IPC. The demo's flow
/// keeps at least one of `task_a` / `task_b` Ready at every step, so idle
/// is structurally unreachable in the demo path — but if a future caller
/// arms a deadline via `arm_deadline` and both application tasks happen
/// to block at the same moment, idle's `WFI` becomes observable.
///
/// Full IPC-graph deadlock (every task blocked + no wake source ever
/// fires) is visible today as "idle waiting forever", not a panic — the
/// kernel stays live; typed `SchedError::Deadlock` is reachable only if
/// the BSP did not register idle at all (i.e. `register_idle` was not
/// called before `start`).
///
/// [ADR-0022]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0022-idle-task-and-typed-scheduler-deadlock.md
/// [ADR-0026]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0026-idle-dispatch-fallback.md
fn idle_entry() -> ! {
    // SAFETY: CPU is fully initialised in `kernel_entry` before `start()`;
    // single-core cooperative scheduling prevents concurrent access.
    // Audit: UNSAFE-2026-0010.
    let cpu = unsafe { (*CPU.0.get()).assume_init_ref() };
    loop {
        // T-012 step 5: idle waits for an interrupt instead of busy-
        // spinning. `wait_for_interrupt` issues a `WFI` instruction
        // which suspends the core until any unmasked IRQ raises a
        // wake. Closes ADR-0022 first rider's *Sub-rider* gate ("WFI
        // activation requires *two* tasks, not one") — the time-source
        // half landed with T-009; the IRQ-delivery half landed with
        // T-012's GIC + vector-table install (commit `a043079`).
        //
        // T-014: idle is now in the dispatcher's fallback slot per
        // ADR-0026, not the FIFO. The dispatcher routes here only when
        // both `task_a` and `task_b` are Blocked simultaneously and no
        // other task is Ready; the v1 cooperative IPC demo's flow
        // keeps at least one of them Ready at every step, so this
        // `WFI` remains structurally unreachable in the demo path.
        cpu.wait_for_interrupt();
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

    // T-009 measurement: print boot-to-end elapsed time. Uses `now_ns` on
    // the live Timer impl and the BOOT_NS snapshot taken in `kernel_entry`.
    // `saturating_sub` is defensive — the hardware counter is monotonic so
    // `now >= boot_ns` always holds, but the saturating form makes the
    // subtraction's correctness obvious to a reader scanning for overflow
    // hazards.
    //
    // SAFETY: CPU and BOOT_NS were both initialised in `kernel_entry`
    // before `start()`; single-core cooperative scheduling prevents any
    // concurrent writer. Audit: UNSAFE-2026-0010.
    let elapsed_ns = unsafe {
        let cpu = (*CPU.0.get()).assume_init_ref();
        let boot_ns = *(*BOOT_NS.0.get()).assume_init_ref();
        cpu.now_ns().saturating_sub(boot_ns)
    };
    let _ = writeln!(w, "tyrne: boot-to-end elapsed = {elapsed_ns} ns",);

    loop {
        core::hint::spin_loop();
    }
}

// ─── Boot entry ───────────────────────────────────────────────────────────────

// Reset entry (`_start`). See `boot.s` and `docs/architecture/boot.md`.
global_asm!(include_str!("boot.s"));

// EL1 exception vector table (`tyrne_vectors`). See `vectors.s` and
// `docs/architecture/exceptions.md`. Audit: UNSAFE-2026-0020.
global_asm!(include_str!("vectors.s"));

extern "C" {
    /// Symbol exported by `vectors.s`; resolves to the 2 KiB-aligned
    /// base of the EL1 vector table. Written to `VBAR_EL1` once at
    /// boot.
    static tyrne_vectors: u8;
}

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

    // ── Exception vector table install + GIC init (T-012) ────────────────────
    //
    // Sequence (per docs/architecture/exceptions.md §"Implementation map"):
    //   1. Install VBAR_EL1 with the 2 KiB-aligned `tyrne_vectors` base
    //      (assembled in src/vectors.s). After this, any exception taken
    //      while DAIF is still masked still gets caught — the panic-class
    //      trampolines fire instead of fetching from VBAR's reset value.
    //   2. Construct + init the GIC v2 controller. `init` disables every
    //      SPI, sets default priorities, routes SPIs to CPU 0, then
    //      enables the distributor + CPU interface. No IRQ source is
    //      enabled at this point — `enable(IrqNumber)` is a separate call.
    //   3. Unmask DAIF.I (clear the I bit only — D, A, F stay masked).
    //      With nothing enabled at the GIC, this is a no-op for IRQ
    //      delivery; future `enable` calls will deliver IRQs through the
    //      now-installed vector table.
    //
    // Order matters: vector table → GIC init → DAIF unmask. If GIC init
    // were first, a fault in init() would jump to the un-installed vector
    // and silently hang. Installing the vector table first guarantees
    // visible failure for any later boot bug.

    // SAFETY: `tyrne_vectors` is exported by src/vectors.s as the
    // 2 KiB-aligned base of the EL1 vector table. `MSR VBAR_EL1, x`
    // is privileged at EL1 (always available); the write does not
    // mutate any other state and `options(nostack, nomem)` is correct
    // (the asm reads no memory, writes no stack). Audit: UNSAFE-2026-0020.
    unsafe {
        let vbar_addr = core::ptr::addr_of!(tyrne_vectors) as u64;
        core::arch::asm!(
            "msr vbar_el1, {0}",
            "isb",
            in(reg) vbar_addr,
            options(nostack, nomem),
        );
    }

    // SAFETY: QEMU virt's GICv2 distributor + CPU interface live at
    // their well-known MMIO bases (per ADR-0011 references and the
    // QemuVirtGic module-level docs); single-core v1 means no
    // concurrent writer exists. The construction itself does no MMIO.
    // Audit: UNSAFE-2026-0019.
    let gic = unsafe {
        QemuVirtGic::new(
            QEMU_VIRT_GIC_DISTRIBUTOR_BASE,
            QEMU_VIRT_GIC_CPU_INTERFACE_BASE,
        )
    };
    // SAFETY: single-core; no concurrent writer to GIC static yet.
    // Audit: UNSAFE-2026-0001 (StaticCell pattern) + UNSAFE-2026-0019.
    unsafe { (*GIC.0.get()).write(gic) };

    // SAFETY: GIC was just published. `init` performs the boot-time
    // MMIO programming sequence per its doc; DAIF still masked, so no
    // IRQ can fire mid-init. Audit: UNSAFE-2026-0019.
    unsafe {
        let gic_ref = (*GIC.0.get()).assume_init_ref();
        gic_ref.init();
    }

    // SAFETY: With the vector table installed and the GIC initialised
    // (but no IRQ enabled at the GIC), unmasking DAIF.I is safe — no
    // IRQ source can fire until a later `gic.enable(...)` call. The
    // other DAIF bits (D, A, F) stay masked.
    //
    // `MSR DAIFClr, #0x2` clears the I bit specifically (bit value
    // matches PSTATE.DAIF[1]; cf. ARM ARM §C5.2.7). `options(nostack,
    // nomem)` is correct.
    // Audit: UNSAFE-2026-0020.
    unsafe {
        core::arch::asm!("msr daifclr, #0x2", options(nostack, nomem),);
    }

    // ── Timer banner + boot timestamp (T-009) ───────────────────────────────
    //
    // The CPU's Timer impl is live the moment `QemuVirtCpu::new` returned
    // (it sampled CNTFRQ_EL0 and cached the resolution). Print the timer
    // parameters so QEMU output makes the measurement visible, then
    // snapshot `now_ns()` so `task_a`'s "all tasks complete" path can
    // print boot-to-end elapsed nanoseconds.
    {
        let mut w = FmtWriter(console);
        let _ = writeln!(
            w,
            "tyrne: timer ready ({} Hz, resolution {} ns)",
            cpu.frequency_hz(),
            cpu.resolution_ns()
        );
    }
    let boot_ns = cpu.now_ns();
    // SAFETY: single-core; no concurrent writer exists before `start()`.
    // Audit: UNSAFE-2026-0001.
    unsafe {
        (*BOOT_NS.0.get()).write(boot_ns);
    }

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
    // The idle task is registered via `register_idle` (NOT `add_task`) per
    // [ADR-0026]: idle lives in the dispatcher's fallback slot
    // (`Scheduler::idle`) and is dispatched only when the ready queue is
    // empty. This is the structural fix for the 2026-05-06 smoke regression
    // — idle is no longer a FIFO resident that round-robin can dispatch
    // ahead of a just-unblocked receiver.
    //
    // SAFETY: add_task / register_idle call init_context; stack tops are
    // 16-byte aligned (guaranteed by TaskStack's repr) and remain valid
    // for the process lifetime. Entry functions are `fn() -> !`. Audit:
    // UNSAFE-2026-0009 (init_context site) + UNSAFE-2026-0014
    // (register_idle's momentary `&mut Scheduler<C>` discipline).
    //
    // [ADR-0026]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0026-idle-dispatch-fallback.md
    unsafe {
        sched
            .add_task(cpu, handle_b, task_b, TASK_B_STACK.top())
            .expect("add_task B failed: queue full or arena exhausted");
        sched
            .add_task(cpu, handle_a, task_a, TASK_A_STACK.top())
            .expect("add_task A failed: queue full or arena exhausted");
        register_idle(
            core::ptr::from_mut(&mut sched),
            cpu,
            handle_idle,
            idle_entry,
            TASK_IDLE_STACK.top(),
        );
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
    //
    // No defensive `loop {}` follows: `start` is `-> !`, so the type
    // system proves nothing after this call is reachable. Adding a
    // belt-and-braces parking loop would be flagged as
    // `unreachable_code` and `clippy::too_many_lines` — for the
    // `-> !` case the type signature is already the belt-and-braces
    // (any future refactor that drops `-> !` becomes a hard build
    // error in callers' return-type analysis). Comprehensive review
    // 2026-05-06 Track G's "defensive loop" suggestion is therefore
    // declined for `kernel_entry` specifically; it remains valid for
    // sites where the called function does not have `-> !`.
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
