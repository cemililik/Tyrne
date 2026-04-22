# `unsafe` audit log

This log tracks every `unsafe` block, `unsafe fn` declaration, `unsafe impl`, and `unsafe trait` introduced into Tyrne. See [unsafe-policy.md](../standards/unsafe-policy.md) for the policy this log implements and [security-review.md](../standards/security-review.md) for the review pass that signs each entry off.

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
- **Location:** [`bsp-qemu-virt/src/cpu.rs`](../../bsp-qemu-virt/src/cpu.rs) — `context_switch_asm` and `QemuVirtCpu::context_switch`; [`kernel/src/sched/mod.rs`](../../kernel/src/sched/mod.rs) — `start`, `yield_now`, `ipc_recv_and_yield` (all three are raw-pointer free functions per ADR-0021).
- **Operation:** Saves `x19`–`x28`, `x29` (fp), `x30` (lr), `sp` to `*current` and restores from `*next` via `STP`/`LDP`/`STR`/`LDR` instructions; returns via `RET` which jumps to the loaded `lr`.
- **Invariants relied on:**
  - `current` and `next` are distinct (different task indices) wherever the split-borrow pattern is used in `Scheduler`.
  - Both pointers are 8-byte aligned — `Aarch64TaskContext` is `#[repr(C)]` with all `u64` fields.
  - Interrupts are disabled by `IrqGuard` before `context_switch` is called. An IRQ mid-switch would observe partially saved registers.
  - `next` was either written by a prior `context_switch_asm` call or fully initialised by `init_context` (UNSAFE-2026-0009).
  - The `ret` instruction will jump to `next.lr`; for a task's first run, `lr` is the entry function address set by `init_context`. The entry function is `fn() -> !` and truly never returns.
- **Known gaps (intentional, v1):** `TPIDR_EL0` and `TPIDRRO_EL0` (aarch64 TLS registers) are *not* saved or restored — v1 has no TLS users. If Phase B or later introduces TLS at EL1, the save set in `context_switch_asm` and the `Aarch64TaskContext` layout must be extended in the same commit as the TLS introduction; otherwise the first TLS-using task to context-switch will silently corrupt another task's TLS pointer.
- **Rejected alternatives:** Context switching requires register-level manipulation that cannot be expressed in safe Rust. The assembly is minimal (13 saves + 13 restores + ret).
- **Reviewed by:** @cemililik; security-reviewed 2026-04-21 (see `docs/analysis/reviews/security-reviews/2026-04-21-tyrne-to-phase-a.md` §3).
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
  - Tyrne v1 is single-core and cooperative: no two tasks ever run simultaneously, so no two threads can reach a `StaticCell` concurrently.
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

### UNSAFE-2026-0012 — `&mut` aliasing on shared kernel state across cooperative yields

- **Introduced:** 2026-04-21, T-004 / A5 BSP bootstrap. Extended in T-005 / A6 to cover IPC statics.
- **Location:** [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs) — `task_a`, `task_b` — every `assume_init_mut()` call on `SCHED`, `EP_ARENA`, `IPC_QUEUES`, `TABLE_A`, and `TABLE_B`.
- **Operation:** In A5, `(*SCHED.0.get()).assume_init_mut()` creates a `&mut Scheduler` that is technically alive across the cooperative context switch inside `yield_now` and `ipc_recv_and_yield`. In A6, the pattern extends to `EP_ARENA`, `IPC_QUEUES`, and the per-task capability tables: `ipc_recv_and_yield` holds `&mut` references to all three across the `cpu.context_switch` call. When the suspended task later resumes, the same statics are accessed again from the same stack frame. When the other task runs concurrently (within the context switch), it derives its own `&mut` references to `SCHED`, `EP_ARENA`, and `IPC_QUEUES` from the same `UnsafeCell`s — technically creating aliased mutable references, which is undefined behaviour under Rust's strict aliasing rules.
- **Invariants relied on:**
  - Single-core cooperative model: no two tasks execute simultaneously; there is no concurrent memory access to any of these statics.
  - The references on a suspended task's stack frame are not accessed while that task is suspended (the context-switch `naked_asm!` barrier prevents the compiler from observing or reordering accesses across the switch point).
  - `ipc_recv_and_yield` does not access `ep_arena`, `queues`, or `caller_table` between the `cpu.context_switch` call (suspend) and the second `ipc_recv` call (resume) — the only intervening code is the `IrqGuard` drop and some `TaskState` manipulation on `self`, all stack-local or on the `Scheduler` struct which is owned by `self`.
  - The per-task `TABLE_A` / `TABLE_B` statics are disjoint: tasks never access each other's tables, so those never alias between concurrent frames.
- **Rejected alternatives:** A raw-pointer API eliminates the aliasing entirely — see [ADR-0021](../decisions/0021-raw-pointer-scheduler-ipc-bridge.md) (Accepted 2026-04-22). A `Mutex<Scheduler>` would introduce lock overhead and a blocking primitive before the kernel has blocking support; Option B ("scheduler owns the arenas") and Options C/D are weighed in ADR-0021.
- **Reviewed by:** @cemililik.
- **Status:** Removed — 2026-04-22, commit `f9b72f8`. T-006 / ADR-0021 reshaped the scheduler's IPC bridge (`Scheduler::ipc_send_and_yield`, `Scheduler::ipc_recv_and_yield`, `Scheduler::yield_now`) from `&mut self` methods into `unsafe fn` free functions over `*mut Scheduler<C>` with `*mut` parameters for every arena and capability table. The BSP's `task_a` / `task_b` now produce each pointer via `StaticCell::as_mut_ptr()` (a plain `UnsafeCell::get().cast()` — see UNSAFE-2026-0013) and never materialise a `&mut` at the call site. Inside the kernel, momentary `&mut` references to `Scheduler`, arenas, queues, and tables live only inside narrow inner blocks that end strictly before `cpu.context_switch` and are reacquired strictly after it returns (see UNSAFE-2026-0014). No `&mut` is alive across the switch. Commit `f9b72f8` lands the refactor; commit `1746bc8` completes the related `TaskArena` migration to a `StaticCell` global. QEMU smoke trace matches the A6 baseline; all 109 host tests remain green.
- **Post-review rider (2026-04-22, follow-up commit):** The original `f9b72f8` left one `&mut self` path in place — `Scheduler::start` — relying on the fact that its bootstrap `&mut Scheduler` lives on a frame that is never resumed. Although technically sound, this was the same pattern UNSAFE-2026-0012 describes, merely one that happens to be reachable only once. The follow-up commit reshapes `start` into a raw-pointer free function (`sched::start(*mut Scheduler<C>, &C)`), bringing the full scheduler API under the ADR-0021 discipline and eliminating the exception noted here. UNSAFE-2026-0012 now retires without residue.

### UNSAFE-2026-0013 — `StaticCell::as_mut_ptr` BSP helper

- **Introduced:** 2026-04-22, commit `f9b72f8` (T-006 / ADR-0021).
- **Location:** [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs) — inherent `impl<T> StaticCell<T>` method `as_mut_ptr`.
- **Operation:** Returns a `*mut T` pointer to the cell's storage via `self.0.get().cast::<T>()`. `UnsafeCell::get` returns `*mut MaybeUninit<T>`, which shares layout with `T`; the `.cast::<T>()` is a zero-cost reinterpretation. No borrow (`&` or `&mut`) is materialised at any point.
- **Invariants relied on:**
  - `MaybeUninit<T>` and `T` share layout (guaranteed by the standard library).
  - The cell has been initialised by a prior `(*cell.0.get()).write(...)` before any caller dereferences the returned pointer. Every BSP call site runs `write` in `kernel_entry` before `Scheduler::start` — see `kernel_entry`'s publish blocks for each static cell.
  - The caller does not use the returned pointer to create a `&mut T` that outlives a cooperative context switch. This is the ADR-0021 contract that governs every caller and is re-stated at each caller's `// SAFETY:` comment.
- **Rejected alternatives:** Returning a `&mut T` is the bug UNSAFE-2026-0012 describes. Returning an `Option<*mut T>` with a runtime-checked initialised flag would add a per-access branch and violate the "zero-runtime-cost" framing of `StaticCell`'s use as a `static` construct. Exposing `self.0.get()` raw and letting callers `cast::<T>()` themselves would scatter the cast across every call site; centralising the cast makes the aliasing contract a single documented helper.
- **Reviewed by:** @cemililik (+ Claude Opus 4.7 agent).
- **Status:** Active — this helper is the foundation on which UNSAFE-2026-0012's retirement rests.

### UNSAFE-2026-0014 — Scheduler free-function momentary `&mut` pattern

- **Introduced:** 2026-04-22, commit `f9b72f8` (T-006 / ADR-0021).
- **Location:** [`kernel/src/sched/mod.rs`](../../kernel/src/sched/mod.rs) — inside the `unsafe fn` free functions `yield_now`, `ipc_send_and_yield`, `ipc_recv_and_yield` (the raw-pointer bridge introduced by ADR-0021). Also at the `create_task` call site in [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs) `kernel_entry` where `TASK_ARENA` is accessed via a momentary `&mut`.
- **Operation:** Dereferences a `*mut T` into a `&mut T` inside a narrow inner block. The block is structured so that the resulting `&mut` is dropped strictly before any `cpu.context_switch` call and is reacquired strictly after the switch returns. The pattern looks like:

  ```rust
  // SAFETY: caller contract — pointers valid, distinct, and exclusive
  // for this block; the &muts do not cross cpu.context_switch.
  let (…) = unsafe {
      let s: &mut Scheduler<C> = &mut *sched;
      let arena_ref: &mut EndpointArena = &mut *ep_arena;
      // … pre-switch work …
      (…)
  }; // all &muts drop here
  unsafe { /* context_switch uses raw pointer arithmetic on s.contexts */ }
  // Phase 3: new &muts may be acquired in another inner block.
  ```

- **Invariants relied on:**
  - Every `*mut` pointer passed in is valid, non-null, properly aligned, and refers to an exclusively-owned object for the block's duration (caller's responsibility, established by the shared safety contract at the top of the raw-pointer-bridge section).
  - No `&mut` produced by this pattern is live across `cpu.context_switch`; the block's lexical scope ends before the switch site.
  - The two cross-switch `&mut` borrows that **do** exist (`&mut (*sched).contexts[current_idx]` and `&(*sched).contexts[next_idx]`) are at provably distinct indices and therefore non-aliasing — already covered by UNSAFE-2026-0008.
  - Interrupts are masked by `IrqGuard` for the duration of the switch.
- **Rejected alternatives:** Extending the `&mut self` signatures is the hazard UNSAFE-2026-0012 describes; see ADR-0021 §Decision outcome. Using `NonNull<T>` instead of `*mut T` throughout would add a `NonNull::as_mut` call at every site and does not strengthen the aliasing contract (the caller already guarantees non-null via the shared safety contract). `core::ptr::addr_of_mut!` can avoid constructing an intermediate `&mut` to the parent scheduler when walking into `self.contexts`, but the context-switch call still uses the split-borrow idiom documented under UNSAFE-2026-0008; no net change.
- **Reviewed by:** @cemililik (+ Claude Opus 4.7 agent).
- **Status:** Active.
