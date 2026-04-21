//! # umbrix-bsp-qemu-virt
//!
//! Board Support Package for QEMU's aarch64 `virt` machine — the primary
//! development target per [ADR-0004][adr-0004] and the BSP that every
//! Umbrix feature is first exercised against.
//!
//! This crate is the bootable binary: it provides the reset vector
//! (`_start`, assembled from `boot.s` via [`core::arch::global_asm!`]),
//! the Rust entry `kernel_entry`, a panic handler, and the hardware
//! implementations of the HAL traits. The A5 milestone adds the
//! [`cpu::QemuVirtCpu`] implementation of [`umbrix_hal::Cpu`] and
//! [`umbrix_hal::ContextSwitch`], and a two-task cooperative scheduler
//! smoke test.
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
// The scheduler, CPU, and console are stored as immutable statics wrapping
// `UnsafeCell<MaybeUninit<T>>` so all tasks can reach them without `static mut`.
// All accesses remain `unsafe`; safety is ensured by the single-core,
// cooperative execution model (no two tasks run simultaneously).

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
// `core` are not available in `no_std` without an allocator in A5.
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

/// The cooperative scheduler, concrete over the QEMU BSP CPU type.
///
/// Written once in `kernel_entry` before `start()`. Tasks access it via
/// `SCHED` to yield control cooperatively.
static SCHED: StaticCell<Scheduler<QemuVirtCpu>> = StaticCell::new();

/// The CPU handle — needed by `yield_now` to mask IRQs during the switch.
static CPU: StaticCell<QemuVirtCpu> = StaticCell::new();

/// The PL011 console — used by task functions for diagnostic output.
static CONSOLE: StaticCell<Pl011Uart> = StaticCell::new();

// ─── Task A ───────────────────────────────────────────────────────────────────

/// First smoke-test task. Prints its iteration index and yields three times,
/// then spins. The alternating output with task B confirms context switching.
fn task_a() -> ! {
    for i in 0u32..3 {
        // SAFETY: CONSOLE is fully initialised in `kernel_entry` before
        // `start()` transfers control; no other task writes concurrently
        // (cooperative scheduling). Audit: UNSAFE-2026-0010.
        let console = unsafe { (*CONSOLE.0.get()).assume_init_ref() };
        let mut w = FmtWriter(console);
        let _ = writeln!(w, "umbrix: task A — iteration {i}");

        // SAFETY: SCHED and CPU are both fully initialised before `start()`.
        //
        // Aliasing note (Audit: UNSAFE-2026-0012): `assume_init_mut` creates
        // a `&mut Scheduler` that is technically alive when `yield_now`
        // suspends this task and another task creates its own `&mut Scheduler`.
        // This relaxes Rust's strict aliasing rules. It is safe under the
        // single-core cooperative model because:
        //   (a) no two tasks execute simultaneously — there is no concurrent
        //       memory access;
        //   (b) `yield_now` does not observe `self` after the context switch
        //       returns (the only post-switch code is the `IrqGuard` drop and
        //       the `Ok(())` return, both of which operate on stack locals
        //       within yield_now's own frame, not on `self`).
        // The `&mut` is not bound to a named variable so its scope is limited
        // to the duration of the `yield_now` call expression.
        // A raw-pointer API would eliminate the aliasing entirely; that refactor
        // is deferred to a future ADR. Audit: UNSAFE-2026-0010.
        //
        // yield_now returns Err only when current == None, which cannot happen
        // once the scheduler has started.
        unsafe {
            let _ = (*SCHED.0.get())
                .assume_init_mut()
                .yield_now((*CPU.0.get()).assume_init_ref());
        }
    }

    // SAFETY: CONSOLE is fully initialised; no concurrent access.
    // Audit: UNSAFE-2026-0010.
    let console = unsafe { (*CONSOLE.0.get()).assume_init_ref() };
    console.write_bytes(b"umbrix: task A done; spinning\n");
    loop {
        core::hint::spin_loop();
    }
}

// ─── Task B ───────────────────────────────────────────────────────────────────

/// Second smoke-test task. Symmetric to task A.
fn task_b() -> ! {
    for i in 0u32..3 {
        // SAFETY: same invariants as task_a. Audit: UNSAFE-2026-0010.
        let console = unsafe { (*CONSOLE.0.get()).assume_init_ref() };
        let mut w = FmtWriter(console);
        let _ = writeln!(w, "umbrix: task B — iteration {i}");

        // SAFETY: same aliasing invariants as task_a. Audit: UNSAFE-2026-0012.
        unsafe {
            let _ = (*SCHED.0.get())
                .assume_init_mut()
                .yield_now((*CPU.0.get()).assume_init_ref());
        }
    }

    // SAFETY: CONSOLE is fully initialised; no concurrent access.
    // Audit: UNSAFE-2026-0010.
    let console = unsafe { (*CONSOLE.0.get()).assume_init_ref() };
    console.write_bytes(b"umbrix: task B done; spinning\n");
    loop {
        core::hint::spin_loop();
    }
}

// ─── Boot entry ───────────────────────────────────────────────────────────────

// Reset entry (`_start`). See `boot.s` and `docs/architecture/boot.md`.
global_asm!(include_str!("boot.s"));

/// First Rust entry after the assembly stub.
///
/// Initialises the console, CPU, and cooperative scheduler, registers two
/// smoke-test tasks, then transfers control to the scheduler. This function
/// never returns — the BSP reset stub halts defensively if it somehow does.
///
/// # Panics
///
/// Panics if the `TaskArena` cannot accommodate two tasks. The arena capacity
/// is 16, so in practice this branch is unreachable.
#[unsafe(no_mangle)]
pub extern "C" fn kernel_entry() -> ! {
    // ── Hardware setup ────────────────────────────────────────────────────────

    // SAFETY: 0x0900_0000 is the well-known QEMU virt PL011 UART MMIO
    // base, exclusively owned by this kernel in v1 (single-core, no
    // concurrent drivers). Audit: UNSAFE-2026-0001.
    let console = unsafe { Pl011Uart::new(PL011_UART_BASE) };
    // SAFETY: constructed exactly once in kernel_entry; single-core v1.
    // See QemuVirtCpu::new # Safety. Audit: UNSAFE-2026-0006.
    let cpu = unsafe { QemuVirtCpu::new() };

    // Publish the console and CPU before any task can run.
    // SAFETY: single-core; no concurrent writer exists before `start()`.
    // Audit: UNSAFE-2026-0001.
    unsafe {
        (*CONSOLE.0.get()).write(console);
        (*CPU.0.get()).write(cpu);
    }

    // SAFETY: both cells were initialised in the block above.
    // Audit: UNSAFE-2026-0001.
    let console = unsafe { (*CONSOLE.0.get()).assume_init_ref() };
    // SAFETY: CPU cell was initialised above. Audit: UNSAFE-2026-0001.
    let cpu = unsafe { (*CPU.0.get()).assume_init_ref() };

    console.write_bytes(b"umbrix: hello from kernel_main\n");

    // ── Kernel-object setup ───────────────────────────────────────────────────

    let mut arena = TaskArena::default();
    // Infallible: arena capacity is 16 and we allocate 2 tasks.
    let handle_a = create_task(&mut arena, Task::new(0)).ok().unwrap();
    let handle_b = create_task(&mut arena, Task::new(1)).ok().unwrap();

    // ── Scheduler setup ───────────────────────────────────────────────────────

    let mut sched = Scheduler::<QemuVirtCpu>::new();

    // SAFETY: add_task calls init_context; the stack tops are 16-byte aligned
    // (guaranteed by TaskStack's repr) and remain valid for the process
    // lifetime. Entry functions are `fn() -> !`. Audit: UNSAFE-2026-0009.
    //
    // Stack tops are computed inline to avoid introducing two local bindings
    // with similar names that would trigger the `similar_names` lint.
    unsafe {
        sched
            .add_task(cpu, handle_a, task_a, TASK_A_STACK.top())
            .expect("add_task A failed: queue full or arena exhausted");
        sched
            .add_task(cpu, handle_b, task_b, TASK_B_STACK.top())
            .expect("add_task B failed: queue full or arena exhausted");
    }

    // Publish the scheduler before transferring control.
    // SAFETY: single-core; no task is running yet. Audit: UNSAFE-2026-0001.
    unsafe {
        (*SCHED.0.get()).write(sched);
    }

    console.write_bytes(b"umbrix: starting cooperative scheduler\n");

    // Transfer control to the first ready task. Does not return.
    // SAFETY: SCHED is fully initialised; the first task's context was set
    // up by add_task above. Audit: UNSAFE-2026-0008.
    unsafe { (*SCHED.0.get()).assume_init_mut() }.start(cpu);

    // Unreachable — start() switches away and the bootstrap context is never
    // restored. The BSP reset stub halts the core if this line is somehow
    // reached, which is a kernel bug.
    loop {
        core::hint::spin_loop();
    }
}

// ─── Panic handler ────────────────────────────────────────────────────────────

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // SAFETY: constructing a fresh Pl011Uart in the panic path is
    // best-effort diagnostic output. If the original instance is still
    // reachable in some caller, writes may interleave at the FIFO —
    // acceptable per the Console contract (ADR-0007). The UART MMIO
    // window itself is the same one kernel_entry uses.
    // Audit: UNSAFE-2026-0002.
    let console = unsafe { Pl011Uart::new(PL011_UART_BASE) };

    console.write_bytes(b"\n!! umbrix panic !!\n");
    let mut w = FmtWriter(&console);
    let _ = writeln!(w, "{info}");

    loop {
        core::hint::spin_loop();
    }
}
