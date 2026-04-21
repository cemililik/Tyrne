# `unsafe` audit log

This log tracks every `unsafe` block, `unsafe fn` declaration, `unsafe impl`, and `unsafe trait` introduced into Umbrix. See [unsafe-policy.md](../standards/unsafe-policy.md) for the policy this log implements and [security-review.md](../standards/security-review.md) for the review pass that signs each entry off.

Entries are **append-only**. When an `unsafe` region is removed, its entry gains a `Removed` status with date and commit; the entry itself is not deleted — the historical reasoning stays on record.

## Entries

### UNSAFE-2026-0001 — construct PL011 `Console` from kernel entry

- **Introduced:** 2026-04-20, Phase 4c bring-up commit.
- **Location:** [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs) — `kernel_entry`.
- **Operation:** `Pl011Uart::new(PL011_UART_BASE)` — wraps the MMIO base of the QEMU `virt` PL011 in the BSP's concrete `Console` type.
- **Invariants relied on:**
  - `0x0900_0000` is the QEMU `virt` PL011 MMIO base across all targeted QEMU versions.
  - The kernel is single-core in v1 and no other subsystem owns this MMIO window.
  - The window is mapped and addressable at the moment the constructor runs (identity-mapped by QEMU before kernel entry).
- **Rejected alternatives:** None viable — the kernel must have an early diagnostic console, and constructing the `Pl011Uart` is the only safe-wrapper entry point.
- **Reviewed by:** @cemililik (self-review per solo-phase discipline; see [security-review.md](../standards/security-review.md)).
- **Status:** Active.

### UNSAFE-2026-0002 — construct PL011 `Console` inside the panic handler

- **Introduced:** 2026-04-20, Phase 4c bring-up commit.
- **Location:** [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs) — `panic` handler.
- **Operation:** `Pl011Uart::new(PL011_UART_BASE)` — reconstructs the UART in the panic path.
- **Invariants relied on:** Same as UNSAFE-2026-0001.
- **Rejected alternatives:** Reusing the original `Console` reference would require smuggling it into the panic handler via a `static` slot, which adds lifetime and initialization complexity. Constructing a fresh `Pl011Uart` is acceptable because `Console` writes are best-effort (ADR-0007): any concurrent writer at panic time may interleave, which is the intended failure mode.
- **Reviewed by:** @cemililik.
- **Status:** Active.

### UNSAFE-2026-0003 — `unsafe impl Send for Pl011Uart`

- **Introduced:** 2026-04-20, Phase 4c bring-up commit.
- **Location:** [`bsp-qemu-virt/src/console.rs`](../../bsp-qemu-virt/src/console.rs).
- **Operation:** Asserts that a `Pl011Uart` value can be transferred between threads.
- **Invariants relied on:** The only state inside `Pl011Uart` is a base address (a `usize`). The PL011 hardware itself is the synchronization domain; its TX FIFO serializes writes.
- **Rejected alternatives:** A wrapping type (e.g. `AtomicUsize`) buys nothing; the base address never changes and a simple `Send` bound is what callers need.
- **Reviewed by:** @cemililik.
- **Status:** Active.

### UNSAFE-2026-0004 — `unsafe impl Sync for Pl011Uart`

- **Introduced:** 2026-04-20, Phase 4c bring-up commit.
- **Location:** [`bsp-qemu-virt/src/console.rs`](../../bsp-qemu-virt/src/console.rs).
- **Operation:** Asserts that `&Pl011Uart` is safe to share across threads.
- **Invariants relied on:** Same as UNSAFE-2026-0003. Concurrent writes from multiple cores may interleave at the byte level, which the [`Console`](../../hal/src/console.rs) contract (see [ADR-0007](../decisions/0007-console-trait.md)) accepts as best-effort behaviour.
- **Rejected alternatives:** Interior-mutable synchronization (a spinlock around writes) would be safer but is overkill for a console whose contract explicitly permits interleaving. If the contract changes, revisit.
- **Reviewed by:** @cemililik.
- **Status:** Active.

### UNSAFE-2026-0005 — MMIO read/write in `Pl011Uart::write_bytes`

- **Introduced:** 2026-04-20, Phase 4c bring-up commit.
- **Location:** [`bsp-qemu-virt/src/console.rs`](../../bsp-qemu-virt/src/console.rs) — `Pl011Uart::write_bytes`.
- **Operation:** `read_volatile((base + UARTFR) as *const u32)` and `write_volatile((base + UARTDR) as *mut u32, byte_as_u32)` to drive the PL011 TX path.
- **Invariants relied on:**
  - `base` is the MMIO base of a PL011 window, as established by `Pl011Uart::new`'s safety contract (see UNSAFE-2026-0001).
  - `UARTFR` (offset `0x18`) and `UARTDR` (offset `0x00`) are 4-byte-aligned and within the window.
  - Volatile accesses prevent the compiler from reordering or eliding the reads and writes.
- **Rejected alternatives:** Using a `volatile_register` crate would wrap these in typed abstractions at some ergonomic cost; the plain-MMIO form is small enough and easy enough to audit here. Revisit if more registers join the picture.
- **Reviewed by:** @cemililik.
- **Status:** Active.

### UNSAFE-2026-0006 — `Send` + `Sync` for `QemuVirtCpu`

- **Introduced:** 2026-04-21, T-004 / A5 context-switch implementation.
- **Location:** [`bsp-qemu-virt/src/cpu.rs`](../../bsp-qemu-virt/src/cpu.rs) — `unsafe impl Send for QemuVirtCpu` and `unsafe impl Sync for QemuVirtCpu`.
- **Operation:** Declares that `QemuVirtCpu` can be transferred between threads and that shared references to it are safe to use concurrently.
- **Invariants relied on:**
  - `QemuVirtCpu` is a zero-size type with no fields, no heap allocation, and no interior mutability.
  - The hardware resources it accesses (DAIF interrupt-mask register, MPIDR) are per-core system registers — inherently core-local in a single-core v1 system.
  - In a multi-core system, each core would construct its own `QemuVirtCpu`; a future ADR will revisit this.
- **Rejected alternatives:** The compiler cannot derive `Send`/`Sync` for structs containing raw pointers; since `QemuVirtCpu` uses inline assembly to access system registers rather than storing raw pointers, this is a marker assertion rather than a pointer-safety claim.
- **Reviewed by:** @cemililik (self-review, solo phase).
- **Status:** Active.

### UNSAFE-2026-0007 — inline assembly in `QemuVirtCpu::Cpu` methods

- **Introduced:** 2026-04-21, T-004 / A5 context-switch implementation.
- **Location:** [`bsp-qemu-virt/src/cpu.rs`](../../bsp-qemu-virt/src/cpu.rs) — `current_core_id`, `disable_irqs`, `restore_irq_state`, `wait_for_interrupt`, `instruction_barrier`.
- **Operation:** `MRS`/`MSR` DAIF and MPIDR_EL1 register accesses, `WFI`, and `ISB` via `core::arch::asm!`.
- **Invariants relied on:**
  - All instructions are EL1-privileged; the kernel runs at EL1 on QEMU `virt`.
  - `MRS` reads are non-destructive; `MSR DAIFSET` masks interrupts atomically.
  - `MSR DAIF, x` in `restore_irq_state` writes exactly the value returned by a prior `disable_irqs` call — the caller is contractually bound to pass the value unmodified.
  - `WFI` and `ISB` do not modify registers or memory; `options(nostack, nomem)` is correct.
- **Rejected alternatives:** No safe Rust abstraction exists for EL1 system-register access; the HAL trait is the safe abstraction wrapping these blocks.
- **Reviewed by:** @cemililik.
- **Status:** Active.

### UNSAFE-2026-0008 — context-switch assembly in `context_switch_asm` and callers

- **Introduced:** 2026-04-21, T-004 / A5 context-switch implementation.
- **Location:** [`bsp-qemu-virt/src/cpu.rs`](../../bsp-qemu-virt/src/cpu.rs) — `context_switch_asm` and `QemuVirtCpu::context_switch`; [`kernel/src/sched/mod.rs`](../../kernel/src/sched/mod.rs) — `Scheduler::start`, `yield_now`, `ipc_recv_and_yield`.
- **Operation:** Saves `x19`–`x28`, `x29` (fp), `x30` (lr), `sp` to `*current` and restores from `*next` via `STP`/`LDP`/`STR`/`LDR` instructions; returns via `RET` which jumps to the loaded `lr`.
- **Invariants relied on:**
  - `current` and `next` are distinct (different task indices) wherever the split-borrow pattern is used in `Scheduler`.
  - Both pointers are 8-byte aligned — `Aarch64TaskContext` is `#[repr(C)]` with all `u64` fields.
  - Interrupts are disabled by `IrqGuard` before `context_switch` is called. An IRQ mid-switch would observe partially saved registers.
  - `next` was either written by a prior `context_switch_asm` call or fully initialised by `init_context` (UNSAFE-2026-0009).
  - The `ret` instruction will jump to `next.lr`; for a task's first run, `lr` is the entry function address set by `init_context`. The entry function is `fn() -> !` and truly never returns.
- **Rejected alternatives:** Context switching requires register-level manipulation that cannot be expressed in safe Rust. The assembly is minimal (13 saves + 13 restores + ret).
- **Reviewed by:** @cemililik.
- **Status:** Active.

### UNSAFE-2026-0009 — context initialisation in `QemuVirtCpu::init_context` and callers

- **Introduced:** 2026-04-21, T-004 / A5 context-switch implementation.
- **Location:** [`bsp-qemu-virt/src/cpu.rs`](../../bsp-qemu-virt/src/cpu.rs) — `QemuVirtCpu::init_context`; [`kernel/src/sched/mod.rs`](../../kernel/src/sched/mod.rs) — `Scheduler::add_task`.
- **Operation:** Writes `entry` (cast to `u64`) into `ctx.lr` and `stack_top` (cast to `u64`) into `ctx.sp`. The first restore of this context will begin executing `entry` with `stack_top` as the stack pointer.
- **Invariants relied on:**
  - `stack_top` must be 16-byte aligned and point one byte past the top of at least 512 bytes of stack memory that remains valid for the task's lifetime. Callers are contractually bound by the `# Safety` doc.
  - Function pointers are always valid addresses in Rust — casting `fn() -> !` to `usize` then `u64` is safe.
  - The entry function truly never returns; if it did, the `ret` in `context_switch_asm` would jump to garbage.
  - `ctx` is at a valid, exclusively-owned index within `Scheduler::contexts`.
- **Rejected alternatives:** Initialising a context requires writing raw register values; no safe abstraction exists.
- **Reviewed by:** @cemililik.
- **Status:** Active.

### UNSAFE-2026-0010 — `unsafe impl Sync for StaticCell<T>`

- **Introduced:** 2026-04-21, T-004 / A5 BSP bootstrap.
- **Location:** [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs) — `unsafe impl<T> Sync for StaticCell<T>`.
- **Operation:** Declares that `&StaticCell<T>` can be shared across threads, allowing `StaticCell` to appear in `static` position.
- **Invariants relied on:**
  - Umbrix v1 is single-core and cooperative: no two tasks ever run simultaneously, so no two threads can reach a `StaticCell` concurrently.
  - Each cell is written exactly once from `kernel_entry` before `start()` is called; subsequent accesses are read-only (via `assume_init_ref`) or guarded by the cooperative schedule.
- **Rejected alternatives:** `Mutex` / `RwLock` require a runtime or a spin implementation that itself uses `unsafe`; using them would defer rather than eliminate the unsafety. `OnceCell` / `LazyLock` are not available without `std` in A5. `static mut` would expose the interior to safe code via aliasing.
- **Reviewed by:** @cemililik.
- **Status:** Active.

### UNSAFE-2026-0011 — `unsafe impl Sync for TaskStack`

- **Introduced:** 2026-04-21, T-004 / A5 BSP bootstrap.
- **Location:** [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs) — `unsafe impl Sync for TaskStack`.
- **Operation:** Declares that `&TaskStack` can be shared across threads, allowing `static TASK_A_STACK` / `TASK_B_STACK` to satisfy the `Sync` bound on `static`.
- **Invariants relied on:**
  - Single-core cooperative kernel: only one task uses each stack at a time.
  - The inner `UnsafeCell<[u8; 4096]>` is only accessed via `TaskStack::top`, which returns a raw pointer; no safe reference to the interior is ever materialised.
  - Stack lifetimes exceed the tasks that use them (static storage).
- **Rejected alternatives:** Wrapping in `Mutex` adds lock overhead inappropriate for a bare-metal stack. `static mut` exposes the interior unsafely and makes aliasing analysis harder. `UnsafeCell` with manual discipline is the minimal and standard pattern for bare-metal static storage.
- **Reviewed by:** @cemililik.
- **Status:** Active.

### UNSAFE-2026-0012 — `&mut Scheduler` aliasing across cooperative yield

- **Introduced:** 2026-04-21, T-004 / A5 BSP bootstrap.
- **Location:** [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs) — `task_a`, `task_b` — `assume_init_mut().yield_now(...)` call.
- **Operation:** `(*SCHED.0.get()).assume_init_mut()` creates a `&mut Scheduler` that is technically alive across the cooperative context switch inside `yield_now`. When another task calls the same expression, a second `&mut Scheduler` is derived from the same `UnsafeCell`, creating aliased mutable references — undefined behaviour under Rust's strict aliasing rules.
- **Invariants relied on:**
  - Single-core cooperative model: no two tasks execute simultaneously; there is no concurrent access to the Scheduler's memory.
  - The `&mut` is not bound to a named variable; its scope is limited to the single `yield_now` call expression. After `yield_now` suspends, the scheduler's data is not modified by the suspended frame's stack.
  - `yield_now` does not read `self` after the `cpu.context_switch` call within its body (only the `IrqGuard` drop and `Ok(())` return occur, both stack-local).
  - LLVM's context switch (`naked_asm!` with `ret`) acts as a full memory barrier, preventing the compiler from caching or reordering accesses across the switch point.
- **Rejected alternatives:** A raw-pointer API (`yield_raw(*mut Scheduler, &C)`) would eliminate the aliasing entirely by ensuring no `&mut Scheduler` is live across the context switch. This refactor is the correct long-term fix but requires restructuring the BSP task functions and potentially the Scheduler API; it is deferred to a future ADR. A `Mutex<Scheduler>` would introduce lock overhead and a blocking primitive before the kernel has blocking support.
- **Reviewed by:** @cemililik.
- **Status:** Active — to be resolved by raw-pointer API refactor (future ADR).
