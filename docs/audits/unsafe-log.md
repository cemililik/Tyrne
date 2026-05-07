# `unsafe` audit log

This log tracks every `unsafe` block, `unsafe fn` declaration, `unsafe impl`, and `unsafe trait` introduced into Tyrne. See [unsafe-policy.md](../standards/unsafe-policy.md) for the policy this log implements and [security-review.md](../standards/security-review.md) for the review pass that signs each entry off.

Entries are **append-only**. The original body of an entry — fields written when the entry was introduced — must not be rewritten once committed. Two forms of post-hoc update are permitted because they preserve the historical record rather than overwriting it:

1. **Status change.** When an `unsafe` region is removed, the `Status:` field flips to `Removed` with a date and commit SHA. The original body stays on record. An explanatory paragraph (e.g. UNSAFE-2026-0012's *Post-review rider*) may follow the `Status:` line in the same entry.
2. **Amendment.** When an entry's scope expands — a new call site, an additional operation that falls under the same safety argument — an **`Amendment (YYYY-MM-DD, commit SHA): <short title>.`** block is appended to the entry's end. The block restates the additional location / operation / invariants / rejected alternatives explicitly; the original fields are not edited. See UNSAFE-2026-0011 for the canonical example.

Both forms are time-stamped so a reader can reconstruct the entry's state at any past commit. In-place editing of the original body is disallowed and counts as a policy violation (`docs/standards/unsafe-policy.md §3`).

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
- **Amendment (2026-04-23, commit `39fb66c`): scope extended to cover the post-T-009 struct shape.** T-009 added two `u64` fields to `QemuVirtCpu` — `frequency_hz` and `resolution_ns` — populated once in `new()` from `MRS CNTFRQ_EL0` and the derived round-to-nearest resolution. The original body's *Invariants relied on* line "`QemuVirtCpu` is a zero-size type with no fields" no longer holds; this Amendment records the new invariants explicitly without rewriting the original entry (per unsafe-policy §3 append-only).
  - **Updated invariant:** `QemuVirtCpu` holds two `u64` fields written exactly once at construction and never mutated thereafter; it has no interior mutability and no pointers. The Send/Sync claim now rests on "fields are immutable after `new()`" rather than on "fields do not exist".
  - **Updated invariant:** The hardware resources accessed via per-core system registers now include the ARM Generic Timer (`CNTVCT_EL0`, `CNTFRQ_EL0`) in addition to DAIF and MPIDR. All three are per-core registers, so the same single-core thread-locality argument applies.
  - **Rejected alternatives unchanged:** wrapping the cached fields in `AtomicU64` would add overhead with no benefit — the values are written once and read many times, with no concurrent writer in the v1 single-core cooperative model.

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
- **Amendment (2026-04-23, commit `d25a185`): scope extended to `TaskStack::top` inner `unsafe` block.** R1 retrospective audit found that `TaskStack::top` materialises `(*self.0.get()).as_mut_ptr().add(4096)` in an `unsafe { }` block that, while part of the same TaskStack pattern this entry covers, was not explicitly cited. The coverage is now made explicit without opening a new audit entry because the operation is the dereferenced-and-offset form of the same `UnsafeCell<[u8; 4096]>` access this entry audits.
  - **Additional location:** `TaskStack::top` in the same file.
  - **Additional operation:** `(*self.0.get()).as_mut_ptr().add(4096)` — one-past-end pointer arithmetic to produce the initial stack-pointer value for [`ContextSwitch::init_context`]; the returned pointer is never dereferenced by `top()` itself.
  - **Additional invariant:** `add(4096)` on a `[u8; 4096]` produces a one-past-end pointer, which is defined behaviour; out-of-bounds dereference responsibility lives with `init_context`'s `# Safety` contract.
  - **Additional rejected alternative:** safe slice indexing (`&self.0[4096..]`) cannot produce a one-past-end raw pointer without materialising a `&mut [u8]`, which would violate ADR-0021 by carrying a live `&mut` into task setup.
  - Third task stack (`TASK_IDLE_STACK`) added by T-007 (2026-04-22, commit `25cfaf4`) is also covered by this entry.

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

  **Amendment (2026-04-27, T-011 commit `761af95`): scope extended to `start_prelude` and retroactively named `start`.**
  T-011 extracted `start_prelude(sched: *mut Scheduler<C>) -> usize` from `start` so the dequeue + state-mutation half of the boot dispatch is host-testable in isolation (the post-prelude `cpu.context_switch` is ABI assembly the host harness cannot run). The momentary `&mut Scheduler<C>` discipline inside `start_prelude` is identical to this entry's safety argument — the inner block ends before any caller reaches `cpu.context_switch`. The `start` function itself was always covered by this entry's safety argument (its inner block uses the same pattern and cites this audit tag in its `// SAFETY:` comment, since the original raw-pointer refactor in commit `f9b72f8` / T-006) but was never named in the original *Location* field. Both are recorded here in one Amendment so the audit-log surface matches the source it audits.

  - **Additional locations:** `kernel/src/sched/mod.rs::start_prelude` (T-011, commit `761af95`) and `kernel/src/sched/mod.rs::start` (T-006, commit `f9b72f8` — pre-existing site whose `// SAFETY:` comment cited this audit tag from its introduction; never named in the original *Location* field above).
  - **Additional invariant:** the `&mut Scheduler<C>` materialised inside `start_prelude`'s body is dropped before its caller (`start`) acquires any further reference into `*sched`. `start` calls `start_prelude(sched)` first (consuming the prelude's own block), then enters its own `unsafe { … cpu.context_switch(…) }` block which derives a raw-pointer-arithmetic context pointer from `*sched` rather than re-borrowing — so no `&mut` from `start_prelude` is alive when the throwaway-context switch runs.
  - **Additional rejected alternative:** keeping the prelude inlined inside `start` would duplicate the dequeue + state-mutation logic if any future caller (e.g. an SMP `start_secondary`) needs the same boot dispatch — the helper centralises the discipline, and exposing it module-private (not `pub`) keeps the caller surface unchanged.

  **Amendment (2026-04-28, T-012 commit `28c5ce9`): IRQ-handler frame is now a sanctioned site for the same momentary-`&mut` pattern; v1 vacuously satisfies the discipline.** T-012 introduces [`irq_entry`](../../bsp-qemu-virt/src/exceptions.rs) — a new kernel entry point reached from the asm trampoline at `tyrne_vectors+0x280` (audited under UNSAFE-2026-0020). This Amendment names `irq_entry` as a future site of the momentary-`&mut`-from-`*mut`-Scheduler pattern this entry covers, and records that **v1's `irq_entry` does not yet exercise it** — the timer-IRQ dispatch is *ack-and-ignore* (mask `CNTV_CTL_EL0`, signal EOI to the GIC, return) and never borrows any kernel-static `&mut`. The Amendment is recorded now so future readers do not interpret the absence of an IRQ-side `&mut Scheduler` as an oversight; it is a deliberate design property of v1.

  See [ADR-0021's 2026-04-27 Amendment](../decisions/0021-raw-pointer-scheduler-ipc-bridge.md#revision-notes) for the architectural rule this Amendment enforces. The discipline carried over to IRQ entry is identical to the BSP-side discipline this entry already audits:

  - `irq_entry` takes only `*mut TrapFrame` at the function signature — no `&mut Scheduler<C>` parameter, no method receiver.
  - Any future scheduler-touching IRQ handler (preemption, `time_sleep_until` wake-on-deadline, IPI-driven cross-CPU wake) materialises `&mut Scheduler<C>` only inside an inner block that ends strictly before `eret` returns to the interrupted task.
  - The interrupted task — when resumed by `eret` — re-acquires its own `&mut`s via `assume_init_mut()` exactly as it does today; if a `&mut` from `irq_entry` were alive at that point, the same UNSAFE-2026-0012-class hazard would re-emerge, merely relocated from the BSP-task-pair frame to the IRQ-vs-task frame.

  - **Additional locations (placeholder):** `bsp-qemu-virt/src/exceptions.rs::irq_entry` is the new sanctioned site; v1's body has no momentary-`&mut` materialisation today, so no `// SAFETY:` block in `irq_entry` cites this tag yet. The first scheduler-touching IRQ handler (likely the wake-on-deadline arc when B-series tasks reach that milestone) will add the citation; a follow-up Amendment will record the activation alongside the introducing commit's SHA.
  - **Additional rejected alternative:** structuring IRQ handlers to take a `&mut Scheduler<C>` parameter — re-introduces the exact hazard ADR-0021 closed (the trampoline would have to construct the reference, and the reference would be live across `eret`). Returning a `WakeRequest` enum from `irq_entry` for the post-`eret` task to consume — defers the `&mut` materialisation into BSP code, where it is harder to audit. A per-CPU pending-IRQ-work mailbox drained on the next `yield` — works, but adds a queue layer the cooperative single-CPU v1 does not need; revisit during Phase C SMP work.

  **Amendment (2026-05-06, T-014 / ADR-0026): scope extended to `register_idle`.** [T-014](../analysis/tasks/phase-b/T-014-idle-dispatch-fallback.md) introduces a fourth raw-pointer entry point on the scheduler — [`register_idle`](../../kernel/src/sched/mod.rs) — that follows the exact same momentary-`&mut Scheduler<C>` pattern as `add_task`, `start_prelude`, and the IPC bridge functions this entry already covers. Per [ADR-0026](../decisions/0026-idle-dispatch-fallback.md)'s *Decision outcome*, idle is registered into a dedicated `Scheduler::idle: Option<TaskHandle>` slot rather than the FIFO ready queue; the writer is `register_idle` (set-once at boot), the readers are the three dispatch sites (`start_prelude` / `yield_now` / `ipc_recv_and_yield`) that consult `s.idle` only when `s.ready.dequeue()` returns `None`. The `// SAFETY:` block in `register_idle`'s body cites this audit tag (UNSAFE-2026-0014) for the same reason `add_task` does: the inner `&mut *sched` materialisation is contained to a block that ends before any caller acquires another momentary `&mut`. Rejected alternatives mirror `add_task`'s — `&mut self` reintroduces UNSAFE-2026-0012; ADR-0021 Option B is a strict superset; a mutex relocates the unsafety. v1's `register_idle` site is exercised by the post-T-014 QEMU smoke (full demo trace through `tyrne: all tasks complete`); no aliasing violation observable. The 2026-05-06 *partial-verification* notes on UNSAFE-2026-0019 / 0020 record the post-smoke status of the IRQ surface; this Amendment names the *scheduler-side* extension introduced by the same task arc.

  - **Additional locations (T-014 / ADR-0026, commit on PR #12):** [`kernel/src/sched/mod.rs::register_idle`](../../kernel/src/sched/mod.rs) — set-once writer to `Scheduler::idle`. The function's `// SAFETY:` block cites this audit tag.
  - **Additional invariant:** `register_idle` is set-once by boot-time discipline — a duplicate call is a kernel programming error. The function asserts `s.idle.is_none()` unconditionally (release builds included) so the single-idle invariant cannot be silently violated.

### UNSAFE-2026-0015 — generic-timer system-register reads (`CNTPCT_EL0`, `CNTFRQ_EL0`)

- **Introduced:** 2026-04-23, T-009 — Timer trait implementation for QEMU virt.
- **Location:** [`bsp-qemu-virt/src/cpu.rs`](../../bsp-qemu-virt/src/cpu.rs) — `QemuVirtCpu::new` (one `MRS CNTFRQ_EL0`) and `<QemuVirtCpu as Timer>::now_ns` (one `MRS CNTPCT_EL0` per call).
- **Operation:** Two read-only inline-asm `MRS` instructions:
  - `MRS xN, CNTFRQ_EL0` — reads the firmware-set generic-timer frequency in Hz. Sampled exactly once per `QemuVirtCpu` instance, at construction.
  - `MRS xN, CNTPCT_EL0` — reads the 64-bit free-running system counter. Sampled on every call to `now_ns`. The architecture guarantees monotonic non-decreasing reads.
- **Invariants relied on:**
  - Both registers are non-privileged reads at EL1; QEMU virt and any aarch64 hardware Tyrne targets run the kernel at EL1 by construction.
  - `MRS` does not modify any state; `options(nostack, nomem)` is correct (no stack pointer touch, no memory access from the asm itself).
  - `CNTFRQ_EL0` is set by firmware before kernel entry. `QemuVirtCpu::new` asserts it is non-zero; a zero value would cause a divide-by-zero in the resolution computation, so failing loudly at boot is preferable to a silent infinite resolution.
  - `CNTPCT_EL0` is monotonic per ARM ARM §D11 — successive reads on the same core return non-decreasing values without an explicit barrier. No `ISB` is issued before the read; the trait contract permits sub-resolution drift across reads.
  - Reordering: the inline-asm block carries no `clobber_abi` and does not declare `memory`, so the compiler may reorder it relative to surrounding non-asm code. For latency measurement this is acceptable; correctness of the kernel does not depend on the precise placement of the read.
- **Rejected alternatives:**
  - **Safe Rust intrinsic.** None exists; system-register access is intrinsically `unsafe` at the asm level. The `Timer` trait is the safe wrapper.
  - **A higher-level crate** (e.g. `cortex-a` or `aarch64-cpu`). Would add a dependency for two `MRS` reads. Per the project's dependency policy (`docs/standards/infrastructure.md`), pulling a crate for a six-line operation is out of proportion. Revisit if a third or fourth system-register surface joins the picture.
  - **Reading `CNTPCT_EL0` only and computing `freq` from a known-clock calibration.** Would couple the BSP to a specific platform; QEMU virt and Pi 4 differ. Reading firmware-set `CNTFRQ_EL0` is the portable choice.
  - **Caching `now_ns` results.** Would defeat the trait's monotonic-time guarantee. Not considered.
- **Reviewed by:** @cemililik (+ Claude Opus 4.7 agent).
- **Status:** Active. **Note for casual readers:** the original *Operation* / *Invariants* fields above describe `CNTPCT_EL0` and an "EL1 unconditional" claim; both are superseded by the 2026-04-23 Amendment below — current implementation reads `CNTVCT_EL0` and the EL precondition is documented against ADR-0012's non-VHE EL1 path. Read the Amendment first when assessing the current state of this audit entry.
- **Amendment (2026-04-23, commit `39fb66c`): switched read register from `CNTPCT_EL0` (physical) to `CNTVCT_EL0` (virtual); EL precondition language tightened.** Two corrections caught in the T-009 second-read review (commit `1df3641` + earlier `beb0963`). The original entry body is left intact per unsafe-policy §3; this Amendment records the change explicitly.
  - **Register family corrected.** ADR-0010's *References* list and ADR-0022's first-rider sub-rider both name the **virtual** family (`CNTVCT_EL0`, `CNTV_CVAL_EL0`, `CNTV_TVAL_EL0`, `CNTV_CTL_EL0`). The original implementation read `CNTPCT_EL0` (physical), which on QEMU virt with `CNTVOFF_EL2 = 0` coincides with `CNTVCT_EL0` but would silently mismatch the deferred deadline-arming side once a non-zero offset was set. Read site is now `MRS xN, CNTVCT_EL0`; the `MRS xN, CNTFRQ_EL0` half is unchanged because `CNTFRQ_EL0` is shared between physical and virtual families.
  - **EL precondition tightened.** The original *Invariants relied on* line "QEMU virt and any aarch64 hardware Tyrne targets run the kernel at EL1 by construction" was overconfident. ARM ARM §D11 documents `CNTHCTL_EL2.EL1{V,P}CTEN` gating that applies to EL1 access in VHE mode (`HCR_EL2.{E2H, TGE} = {1, 0}`). Tyrne enters `kernel_entry` at EL1 in non-VHE mode per [ADR-0012](../decisions/0012-boot-flow-qemu-virt.md), where the gating bits do not apply, so `CNTVCT_EL0` and `CNTFRQ_EL0` remain unconditionally readable — but the precondition now cites ADR-0012 explicitly rather than asserting "by construction" without backup.
  - **Saturating arithmetic.** The conversion path `count → ns` was extracted to `tyrne_hal::timer::ticks_to_ns`, which uses 128-bit intermediate arithmetic and a saturating cast back to `u64`. `ticks_to_ns` returns `u64::MAX` if the elapsed nanoseconds would overflow `u64` — preserving `Timer::now_ns`'s monotonic-time contract instead of silently wrapping. The original entry's "monotonic per ARM ARM §D11" invariant is now backed by a software-side guarantee at the conversion boundary, not only a hardware-side guarantee at the counter.

### UNSAFE-2026-0016 — boot-time `CurrentEL` self-check in `QemuVirtCpu::new`

- **Introduced:** 2026-04-27, T-009 second-read review follow-up. Closes the runtime-check half of Review 1's High #1 finding (the documentation half landed in commit `39fb66c`).
- **Location:** [`bsp-qemu-virt/src/cpu.rs`](../../bsp-qemu-virt/src/cpu.rs) — `QemuVirtCpu::new`, prior to the generic-timer reads it audits.
- **Operation:** One read-only inline-asm `MRS xN, CurrentEL` instruction. The two-bit Exception-Level field (bits [3:2] of `CurrentEL`) is shifted into the low bits and asserted equal to `1` (EL1). A mismatch panics with the observed EL — turning a future boot-flow regression into a loud, named boot-time error rather than letting subsequent `MRS CNTVCT_EL0` / `MRS CNTFRQ_EL0` calls trap or read undefined values at EL2 / EL3.
- **Invariants relied on:**
  - `CurrentEL` is readable at every Exception Level. ARM ARM §D11.2 specifies the register layout; bits [3:2] hold the current EL.
  - The MRS does not modify any state; `options(nostack, nomem)` is correct (no stack pointer touch, no memory access from the asm itself).
  - The shift `(raw >> 2) & 0b11` extracts the EL field exactly; the implementation does not depend on RES0 bits being zero.
- **Rejected alternatives:**
  - **Skip the check, document only.** This is what commit `39fb66c` did; the second-read review's High #1 explicitly asked for the runtime check on top of documentation. Skipping leaves a boot-flow regression silently producing wrong timer values until much later behaviour falls out of spec.
  - **Move the check into `boot.s`.** The boot stub's job is to set up the C-ABI environment, not to validate Exception Level (boot.s already presumes EL1 via `MSR cpacr_el1`). Putting the check at the head of `QemuVirtCpu::new` means it runs at the latest possible moment before the assumption is load-bearing — narrow scope, narrow audit.
  - **A higher-level crate** (`aarch64-cpu` or similar). Same dependency-policy argument as UNSAFE-2026-0015: pulling a crate for one MRS is disproportionate.
  - **Defer the check until preemption / SMP work lands.** v1 has no caller other than `kernel_entry`, but the cost of the check (one MRS + one compare) is negligible and the defensive value compounds the moment a second BSP or a future EL-drop sequence ships.
- **Reviewed by:** @cemililik (+ Claude Opus 4.7 agent + two independent review agents).
- **Status:** Active.

  **Amendment (2026-04-27, T-013): assertion is now load-bearing; reads via `tyrne_hal::cpu::current_el` helper.**
  Before T-013, the `CurrentEL == 1` assertion guarded against a hypothetical future regression — `boot.s` performed no EL transition and relied on QEMU virt's default of delivering at EL1. T-013 lands the actual EL2 → EL1 drop in `boot.s` per [ADR-0024](../decisions/0024-el-drop-policy.md); see UNSAFE-2026-0017 for the new boot-time MSR sequence. The assertion's behaviour is unchanged but its role has shifted: it is now the post-condition of T-013's `eret` rather than a safety net for an absent transition. A failure here would indicate a regression in `boot.s`'s EL drop logic (or a future EL3-entry hardware target the v1 boot path does not handle), not a missing transition. T-013's tests do not remove this assertion — per the task's acceptance criteria the assertion remains in place to catch any regression in the new transition's correctness.

  As part of the same T-013 arc, the inline-asm `MRS x, CurrentEL` block this entry originally documented has been replaced by a call to `tyrne_hal::cpu::current_el()` — the safe-Rust wrapper introduced by T-013 and audited under [UNSAFE-2026-0018](#unsafe-2026-0018--tyrne_halcpucurrent_el-safe-wrapper-around-mrs-currentel). The MRS instruction is unchanged; the `unsafe` block now lives in one auditable helper rather than at the call site, and any future caller (e.g. `kernel_entry` validating the EL drop's outcome before constructing `QemuVirtCpu`) reuses the same audited path. The assertion's panic message remains identical, so external behaviour is unchanged.

### UNSAFE-2026-0017 — `boot.s` reset-vector DAIF mask + EL2 → EL1 transition

- **Introduced:** 2026-04-27, [T-013 — EL drop to EL1 in boot](../analysis/tasks/phase-b/T-013-el-drop-to-el1.md). Implements [ADR-0024](../decisions/0024-el-drop-policy.md)'s "always drop to EL1" policy at the BSP reset vector.
- **Location:** [`bsp-qemu-virt/src/boot.s`](../../bsp-qemu-virt/src/boot.s) — pure aarch64 assembly; no surrounding Rust. The audit covers two contiguous code blocks at `_start`:
  1. **K3-12 DAIF mask.** A single `msr daifset, #0xf` at the very head of `_start`, before any other code. Sets D, A, I, F mask bits so the reset vector cannot accidentally take an interrupt before the kernel installs an exception vector table.
  2. **EL drop sequence.** `mrs x0, CurrentEL` to read the current Exception Level, masked to bits [3:2]. Branches: EL1 → fall through to the existing boot path; EL2 → configure `HCR_EL2` (RW=1, E2H=0, TGE=0; non-VHE explicit per ADR-0024), `SPSR_EL2` (mode = EL1h, DAIF masked), `ELR_EL2` (= post-`eret` label), then `eret`; any other EL (notably EL3) → halt via `wfe; b -1b` (no Rust panic infrastructure available pre-`kernel_entry`).
- **Invariants relied on:**
  - `MSR DAIFSet, #imm` is a write-only-to-DAIF-mask-bits instruction; available at every EL ≥ 1. The reset vector runs at the entry EL (EL1 or EL2 in v1 targets), so the instruction is always permitted.
  - `MRS xN, CurrentEL` is a non-privileged, read-only access to a read-only system register. ARM ARM §D11.2 specifies the layout; bits [3:2] hold the EL.
  - **Non-VHE configuration.** `HCR_EL2.E2H = 0` and `HCR_EL2.TGE = 0` together produce non-VHE EL1, the configuration the rest of the kernel assumes (see UNSAFE-2026-0015's first Amendment "non-VHE EL1" precondition for `CNTVCT_EL0` / `CNTFRQ_EL0` access). `RW = 1` ensures EL1 runs aarch64 rather than aarch32; without it `eret` would land in aarch32 and crash on the first Rust instruction.
  - **`SPSR_EL2 = 0x3c5` propagates DAIF to EL1.** The DAIF bits in `SPSR_EL2` become PSTATE.DAIF after `eret`, so the K3-12 mask carried in via the EL2 path remains in effect at EL1 — no second `msr daifset` is needed at the post-`eret` label.
  - **`ELR_EL2`'s adrp + add :lo12: addressing.** PC-relative resolution to a label in the same `.text.boot` section; works regardless of where the linker lays out the kernel image. `__stack_top`, `__bss_start`, `__bss_end` already use the same pattern.
- **Rejected alternatives:**
  - **No EL drop (status quo before T-013).** Works on QEMU virt's default but breaks under `-machine virtualization=on` (delivered at EL2) and on most real-hardware boot stacks (TF-A → U-Boot → kernel typically arrives at EL2). T-009's UNSAFE-2026-0016 would catch the violation but only as a panic, not a recoverable transition. ADR-0024 §"Considered options" has the full enumeration.
  - **Adapt the kernel to whichever EL it arrives in.** Multi-EL kernel code per ADR-0024 Option B; rejected because every `MRS DAIF`, `VBAR_EL1`, `TTBR0_EL1`, etc. call site would need an EL2-aware sibling. The maintenance tax compounds across Phase B; the savings (skip ~10 lines of boot asm) do not.
  - **Hard-fail on non-EL1 (ADR-0024 Option C).** Cheaper than the drop but loses compatibility with EL2-delivering boot environments. UNSAFE-2026-0016 already provides the post-condition assertion at the Rust level; promoting it to the only check leaves us less portable, not more.
  - **Move the DAIF mask to Rust (e.g. as the first thing `kernel_entry` does).** The window between reset and `kernel_entry` is then unprotected — a spurious interrupt during BSS zeroing or stack setup would jump into an uninstalled vector table. K3-12 in pure asm is the only way to close that window.
  - **Configure `SCTLR_EL1` before the `eret`.** The post-T-013 path inherits whatever reset value `SCTLR_EL1` had (MMU off, alignment-checks off — QEMU's default). The pre-T-013 EL1-direct path also relied on this and worked. Adding a `SCTLR_EL1` configuration would be a separate scope expansion; deferred to the MMU bring-up in Phase B2 where it belongs.
  - **Halt on EL3 with a panic frame.** No Rust runtime is up at this point — the panic handler depends on `Pl011Uart` construction (UNSAFE-2026-0001) which has not run. `wfe; b .-` is the visible silence; ADR-0024 §Open questions records the future EL3→EL2→EL1 chain for hardware that requires it.
- **Reviewed by:** @cemililik (+ Claude Opus 4.7 agent).
- **Status:** Active.

  **Amendment (2026-04-27, PR #9 first review-round, commit `9a8e312`): HCR_EL2 literal-write rationale added to §Invariants relied on.** The first review-round asked why `boot.s`'s `HCR_EL2 = (1 << 31)` is a literal write rather than a read-modify-write that would preserve firmware's reset value. The reasoning was added at the time but landed via in-place edit of the §Invariants list; the second review-round flagged the asymmetry against §3 ("must not be rewritten once committed"). This Amendment is the canonical record of that reasoning, preserved here in the discipline-correct form so future readers see it dated and SHA'd. The §Invariants list above was reverted to its `f289d4d`-introducing-commit shape.

  Additional invariant: **Literal `HCR_EL2 = 0x80000000` (RW=1 only) is intentional, not a read-modify-write.** RMW (`mrs ; orr ; msr`) would preserve firmware's reset value of `E2H`, `TGE`, `IMO`, `FMO`, `AMO` — bits the kernel must have at zero for non-VHE EL1, EL1-local trap handling, and EL1-bound IRQ/FIQ/SError. Boot stacks that drop the kernel at EL2 (TF-A, U-Boot pass-through, `-machine virtualization=on`) sometimes leave one of these set; the literal write forces the known shape every kernel module assumes regardless of the firmware's choices. ARMv8.0 baseline has no RES1 bits in `HCR_EL2`; ARMv8.1+ extensions (VHE, RAS, PAuth, NV2, MTE, TWED) all add functional bits with default RES0 when the corresponding feature is unimplemented, not RES1 — so the literal write does not violate any architecture-mandated bit. Audit this assumption when adding a BSP for a target with architecture extensions beyond the v8.0 + v8.1 baseline currently supported.

  **Discipline note for future readers.** This Amendment is the result of a deliberate discipline call made in the second review-round: the first round added this reasoning in-place; per `unsafe-policy.md §3` ("the original body of an entry — fields written when the entry was introduced — must not be rewritten once committed"), additions to body fields after the introducing commit (here, `f289d4d`) belong in Amendment blocks even within the same PR. UNSAFE-2026-0006's 2026-04-23 Amendment for the post-T-009 `QemuVirtCpu` struct shape is the precedent. The introducing-commit boundary — not the merge-to-main boundary — is what locks an entry's body. Future PRs should use Amendments for any post-introduction body change, regardless of whether the change lands before or after merge.

  **Amendment (2026-04-27, PR #9 second review-round, commit `39dd978`): GAS halt-loop syntax correction.** Two body fields used non-existent / malformed aarch64 GAS comments referring to the EL3-halt loop:
  - **§Operation** said *"halt via `wfe; b -1b`"* — `-1b` is not valid GAS syntax (`1b` is the back-reference to local label `1:`, but a leading `-` is meaningless there); the actual asm uses a named label loop (`halt_unsupported_el: wfe ; b halt_unsupported_el`).
  - **§Rejected alternatives → "Halt on EL3 with a panic frame"** said *"`wfe; b .-` is the visible silence"* — `b .-` is a similar malformed token (`.` is the current address, but `b .-` with no offset is not a valid branch target); the visible silence is the same named-label loop above.

  Both occurrences are corrected to *"halt via `halt_unsupported_el: wfe ; b halt_unsupported_el`"* / *"`wfe ; b halt_unsupported_el` is the visible silence"*. The behaviour the audit describes is unchanged; only the prose's asm rendering was wrong. The original body is left intact above per `unsafe-policy.md §3`; this rider is the canonical correction. The actual `boot.s` source has always used the named-label form.

### UNSAFE-2026-0018 — `tyrne_hal::cpu::current_el` safe wrapper around `MRS CurrentEL`

- **Introduced:** 2026-04-27, T-013. Provides a safe-Rust entry point for code that needs to read the Exception Level — formalises the inline-asm pattern UNSAFE-2026-0016 introduced inside `QemuVirtCpu::new`.
- **Location:** [`hal/src/cpu.rs`](../../hal/src/cpu.rs) — the free function `pub fn current_el() -> u8`, gated by `#[cfg(all(target_arch = "aarch64", target_os = "none"))]`.
- **Operation:** Single inline-asm `MRS xN, CurrentEL` read inside an `unsafe` block. Bits [3:2] are masked + shifted to produce a `u8` in `0..=3`. The function's outer signature is `safe`; the `unsafe` block is contained.
- **Invariants relied on:**
  - Same MRS-of-`CurrentEL` invariants as UNSAFE-2026-0016: read-only system register, available at every EL ≥ 0, no state mutation, `options(nostack, nomem)` correct.
  - **Cfg-gating.** The function is *absent* on non-bare-metal targets. On `aarch64-apple-darwin` (host tests on Apple Silicon) and other hosted Unix-like targets, user code reading `CurrentEL` would trap or yield `EL0` with no useful information — the gate prevents accidental host-side use. Test mocks must declare their own EL rather than calling this helper.
  - The 2-bit EL field returned via `(raw >> 2) & 0b11` fits in `u8` trivially; the `as u8` cast is annotated with the appropriate `#[allow(clippy::cast_possible_truncation, reason = …)]`.
- **Rejected alternatives:**
  - **`Cpu::current_el(&self) -> u8` trait method instead of a free function.** ADR-0024 §Open questions documents the trade-off: a method aligns with the rest of the HAL trait surface but forces every Cpu implementor (including test-hal mocks and the kernel's `FakeCpu` in `sched` tests) to declare an EL it does not really have. The early-boot path also needs this before any `Cpu` instance has been constructed (`kernel_entry` could call `current_el()` to validate the EL drop's outcome before constructing `QemuVirtCpu`). The free function serves both the early-boot use and avoids the test-side ergonomic cost.
  - **Inline the MRS at every call site.** Two call sites today (`QemuVirtCpu::new`'s self-check; future kernel-side validation) — each duplicates the mask + shift dance. Centralising in one auditable helper turns N audit-log entries into one reusable abstraction.
  - **Use a higher-level crate.** Same dependency-policy argument as UNSAFE-2026-0015 / 0016 — disproportionate.
  - **Make the function `unsafe fn`.** The MRS upholds every invariant required for a safe abstraction (read-only, side-effect-free, available at every EL ≥ 0); requiring callers to write `unsafe { current_el() }` would push noise upward without adding safety. The `unsafe` block in the body is the correctly-narrow scope.
- **Reviewed by:** @cemililik (+ Claude Opus 4.7 agent).
- **Status:** Active.

  **Amendment (2026-04-27, PR #9 review-round): cfg-gating prose tightened.** §Invariants relied on → "Cfg-gating" originally read *"user code reading `CurrentEL` would trap or yield `EL0` with no useful information"*. The "or yield `EL0`" alternative is wrong: per ARM ARM §D11.2 / §C5.2, `MRS x, CurrentEL` at EL0 is **undefined** — the system register is not accessible at EL0 and the read raises an Undefined Instruction exception (which becomes `SIGILL` on hosted Unix-like targets such as `aarch64-apple-darwin`). There is no fallback EL0 read. The corrected reading: *"user code reading `CurrentEL` is undefined at EL0 and traps with an Undefined Instruction exception (`SIGILL` on hosted Unix-like targets) — the cfg-gate prevents the helper from being reachable on those targets."* The original body is left intact above per `unsafe-policy.md §3`; this rider is the canonical correction. The cfg-gate's behaviour and the rationale for it are unchanged.

### UNSAFE-2026-0019 — `QemuVirtGic` MMIO surface

- **Introduced:** 2026-04-28, [T-012 — Exception infrastructure and interrupt delivery](../analysis/tasks/phase-b/T-012-exception-and-irq-infrastructure.md). First implementation of the [`IrqController`](../../hal/src/irq_controller.rs) trait (per [ADR-0011](../decisions/0011-irq-controller-trait.md)) for QEMU virt's GIC v2 controller.
- **Location:** [`bsp-qemu-virt/src/gic.rs`](../../bsp-qemu-virt/src/gic.rs) — `QemuVirtGic` struct, `unsafe const fn new`, `unsafe fn init`, `IrqController` trait impl methods (`enable` / `disable` / `acknowledge` / `end_of_interrupt`), and the four private MMIO helpers (`read_distributor` / `write_distributor` / `read_cpu_interface` / `write_cpu_interface`). Also covers the `unsafe impl Send for QemuVirtGic` and `unsafe impl Sync for QemuVirtGic` markers in the same module.
- **Operation:** Volatile reads and writes against the GIC v2 distributor (`0x0800_0000`) and CPU interface (`0x0801_0000`) MMIO windows on QEMU virt. The constructor (`new`) stores the two `usize` bases verbatim — no MMIO. The initialisation method (`init`) performs the boot-time programming sequence per ARM GICv2 architecture spec §4 (disable distributor → mask all SPIs → set priorities → route SPIs to CPU 0 → enable distributor → set CPU interface PMR → enable CPU interface). The four `IrqController` methods perform single-register MMIO writes (`enable` / `disable` / `end_of_interrupt`) or a single-register read with state side-effect (`acknowledge`).
- **Invariants relied on:**
  - **MMIO bases match the QEMU virt `virt` machine layout.** `0x0800_0000` is the distributor base; `0x0801_0000` is the CPU interface base. Sourced from QEMU's `hw/arm/virt.c` and confirmed against the device tree exposed by `qemu-system-aarch64 -M virt -dumpdtb`. Stable across every QEMU version Tyrne targets.
  - **Single-core v1.** No concurrent writer to either window. The GIC's CPU-interface registers are inherently per-CPU (banked); a future SMP world would need per-CPU `init` plus per-CPU enable/EOI bookkeeping, but the `IrqController` trait surface is unchanged.
  - **Volatile semantics suppress compiler reordering.** Every register touch goes through `core::ptr::read_volatile` / `write_volatile`; the compiler is forbidden from coalescing or reordering these against unrelated memory operations. `MaybeUninit<u32>` does not appear; the typed `*const u32` / `*mut u32` casts are layout-trivial because the GIC registers are 32-bit-wide MMIO.
  - **`init` runs with `DAIF.I` masked.** The kernel sequences "install vector table → init GIC → unmask DAIF" in `kernel_entry`; `init` therefore cannot race with an in-flight ISR for itself.
  - **`GICD_TYPER` IT-line decoding is bottom-5-bits + 1, times 32.** ARM IHI 0048B §4.3.2; the maximum implementation reports 0x1F (32 IRQ-line groups, 1024 IRQs total) which is the architecture cap.
  - **Spurious INTID 1023 maps to `None`.** The GIC architecture reserves INTID 1023 for "no real IRQ pending"; the trait contract folds this to `None` so the kernel-side handler does not need to know the magic value.
- **Rejected alternatives:**
  - **Combine `new` and `init`.** Folding init's MMIO into the constructor would prevent the kernel from sequencing "install vector table first, then init GIC" — that ordering matters because if init faulted, the vector table is what catches the fault visibly. Splitting `new` (no MMIO) from `init` (MMIO) is the explicit hook that makes the boot order correct.
  - **A higher-level `gicv2` crate.** No-`std` aarch64 GIC v2 crates exist (`arm_gic`, `cortex-a-rt`-adjacent), but they bring their own type / abstraction surface that would have to be threaded through Tyrne's `IrqController` trait. The trait is small enough that an in-tree implementation is shorter than the adapter would be — this is the same dependency-policy reasoning UNSAFE-2026-0015 / 0016 / 0018 applied to the `cortex-a` MRS family.
  - **Read-modify-write on GICD_CTLR / GICC_CTLR.** Some implementations RMW these registers to preserve unrelated bits firmware may have set. Tyrne deliberately writes literal values (`0` then `1`) because the v1 single-core / non-VHE / non-secure-world surface only uses Group 0 forwarding; firmware-set bits would be at best benign and at worst cross-purpose with the kernel's intent. ARMv8.0/8.1+ GICv2 has no RES1 bits in either register.
  - **`Mutex` or `RwLock` around the `QemuVirtGic` instance.** Single-core v1 does not need synchronisation; the GIC's per-CPU registers are banked by hardware so each core's accesses cannot race with another core's. A future SMP world would need careful design here — possibly per-CPU `QemuVirtGic` instances rather than a global lock — but that is a multi-core ADR, not a v1 concern.
- **Reviewed by:** @cemililik (+ Claude Opus 4.7 agent).
- **Status:** Active. Pending QEMU smoke verification at the maintainer's first opportunity per the T-012 review-history row; the trait method bodies are kernel-build clean but the actual MMIO addresses + GICv2 register-layout match are observable only in QEMU smoke.

  **Amendment (2026-05-06, smoke run at HEAD `214052d`): partial verification — `init` MMIO sequence reached and pre-IRQ state observable; the IRQ-take + `acknowledge`/`end_of_interrupt` round-trip is still unverified.** First end-to-end QEMU smoke ran 2026-05-06; serial trace got through `kernel_entry`'s `gic.init()` call and `gic.enable(TIMER_IRQ)` invocation (which sit before the `tyrne: starting cooperative scheduler` line that the trace prints), then through `MSR DAIFClr, #0x2`, then into the cooperative scheduler. The `init` MMIO sequence (distributor disable → SPI mask → priority/route → distributor enable → CPU-interface PMR → CPU-interface enable) therefore produced *some* observable behaviour: the kernel did not fault on any of those writes and the device-tree-derived MMIO bases are correct against the QEMU 10.x `virt` machine. **Not verified by this smoke:** the `acknowledge` read (with its INTID-1023-spurious folding) and the `end_of_interrupt` write — neither is reached in the demo path, since v1's `task_a` / `task_b` never call `arm_deadline` and no other IRQ source is enabled. The "IRQ delivery actually works" claim therefore still rests on the kernel-build + datasheet-match argument, not on observed silicon. This `Pending QEMU smoke verification` status persists; the next Amendment lifts it only when a full IRQ-fire path runs (likely the first preemption-using task after T-014's idle-dispatch fix, or whenever a B-series task arms a deadline). The smoke surfaced an unrelated kernel regression in idle dispatch (see [`docs/analysis/reviews/business-reviews/2026-05-06-B1-smoke-regression.md`](../analysis/reviews/business-reviews/2026-05-06-B1-smoke-regression.md)) — that regression does not invalidate the partial verification this Amendment records, but it does prevent the smoke trace from reaching any code path that would trigger an IRQ.

  **Amendment (2026-05-06, post-T-014 smoke): partial verification reaffirmed; no further IRQ-path observation possible from the v1 demo.** After T-014 + ADR-0026 closed the idle-dispatch regression, the smoke now produces the full demo trace through `tyrne: all tasks complete` (eight serial lines plus the boot-to-end timing line at ~6.3 ms). Repeating the QEMU run at the post-T-014 HEAD confirms: (a) the `init` MMIO sequence still completes cleanly — every write happens before `kernel_entry` reaches `start()`, and the kernel does not fault on any of them; (b) `MSR DAIFClr, #0x2` continues to leave the CPU in a healthy IRQ-unmasked state with no spurious exception fired during the entire demo run; (c) the `-d int,unimp,guest_errors` trace stays empty across the whole run — zero exceptions, zero PSCI calls, zero guest errors. **Still not verified:** `acknowledge` and `end_of_interrupt` — the v1 cooperative IPC demo has no `arm_deadline` caller, so PPI 27 never asserts and `irq_entry`'s GIC dispatch arm never runs. `Pending QEMU smoke verification` status therefore **stays** until a future task arms a real deadline. The post-T-014 smoke is the strongest evidence the partial-verification claim above can carry; full clearance gates on the first preemption-using task (currently planned for B5+ per [`phase-b.md`](../roadmap/phases/phase-b.md)).

### UNSAFE-2026-0020 — EL1 exception vector table install + asm trampolines

- **Introduced:** 2026-04-28, T-012. The vector table assembled in [`bsp-qemu-virt/src/vectors.s`](../../bsp-qemu-virt/src/vectors.s) plus the Rust handlers in [`bsp-qemu-virt/src/exceptions.rs`](../../bsp-qemu-virt/src/exceptions.rs) (`irq_entry`, `panic_entry`, `TrapFrame`) plus the `MSR VBAR_EL1` + `MSR DAIFClr` blocks in [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs)'s `kernel_entry`.
- **Operation:** Three contiguous capabilities:
  1. **Vector table assembly.** 16 entries in a 2 KiB-aligned `.text.vectors` section, each containing one `b <trampoline>` instruction. Each entry's address is `tyrne_vectors + 0x80 * n` per the aarch64 architecture's exception-vector layout (ARM ARM §D1.10).
  2. **Vector table install.** A `MSR VBAR_EL1, x0` instruction in `kernel_entry` that writes the 2 KiB-aligned base of `tyrne_vectors` into the `VBAR_EL1` system register. Followed by `ISB` so the new base is observed before any later instruction that might fault.
  3. **Trampolines + Rust handlers.** The IRQ trampoline (`tyrne_irq_curr_el_trampoline`) saves the AAPCS64-caller-saved register set (`x0..x18` + `x30` + `ELR_EL1` + `SPSR_EL1`) onto the kernel stack as a 192-byte `TrapFrame`, calls `extern "C" irq_entry(*mut TrapFrame)` which acknowledges + dispatches via the GIC, then restores and `eret`s. The unhandled-class trampolines save a minimal frame and call `panic_entry(class, esr) -> !`.
- **Invariants relied on:**
  - **`tyrne_vectors` is 2 KiB-aligned.** Enforced by `.balign 2048` in `vectors.s` plus the matching `. = ALIGN(2048)` in `linker.ld`'s `.text` section. ARM ARM requires the bottom 11 bits of `VBAR_EL1` to be zero; misalignment causes `eret`s to land at the wrong byte offset (still within the table region because the architecture clears bits[10:0] of the addressed entry, but the zero-fill skews the entire table layout).
  - **Each vector entry fits in 32 instructions.** Every entry uses one `b <label>` instruction (4 bytes); the rest of the 0x80-byte slot is filled by the next `.balign 0x80` directive. The CPU branches to the entry corresponding to the exception class; the entry then branches to the trampoline label.
  - **Tyrne runs at EL1 with `SPSel = 1`.** Per ADR-0024, `SPSR_EL2 = 0x3c5` sets EL1h (mode `0b0101` = EL1 + SP_EL1). An IRQ taken from kernel code therefore lands at vector offset `+0x280` (Current EL with SP_ELx, IRQ class). The other 15 entries are unreachable in v1 by construction.
  - **Trampoline frame layout matches `TrapFrame`'s `#[repr(C)]`.** The asm `stp` sequence at offsets `0x00..0xA0` mirrors `TrapFrame`'s field order: `x0_x1`, `x2_x3`, ..., `x18_lr`, `elr_spsr`, `_reserved`. Each `stp` writes 16 bytes; the total is 192 bytes (16-byte-aligned). Mismatches between asm and `repr(C)` are caught at first IRQ fire (the saved register values would be in the wrong slots), not at compile time — discipline maintained by reading the two definitions side-by-side.
  - **AAPCS64 callee-saved (`x19..x29`) preserved across `bl irq_entry`.** Rust upholds the C ABI; the trampoline does not save these explicitly. If the Rust handler bug were to clobber them, AAPCS64 violation would be detectable but rare.
  - **`DAIF.I` is unmasked only after `VBAR_EL1` install + `gic.init()`.** The sequence in `kernel_entry` is "install VBAR_EL1 → init GIC → `MSR DAIFClr, #0x2`". Reordering would either (a) take an exception with no vector table installed (jumps to address 0; silent hang), or (b) take a GIC-routed IRQ before the GIC is initialised (undefined behaviour at the controller).
  - **`MSR DAIFClr, #0x2`** clears the `I` bit in `DAIF` only — `D`, `A`, `F` stay masked. Bit value `0x2` matches `PSTATE.DAIF[1]` (cf. ARM ARM §C5.2.7 — DAIFClr operand encoding).
  - **`tyrne_vectors` symbol resolution via `extern "C"` static.** Rust's `extern "C" { static tyrne_vectors: u8 }` declaration makes the linker symbol's address available; `core::ptr::addr_of!(tyrne_vectors) as u64` reads the address without dereferencing the (zero-byte) symbol value. The cast to `u64` is layout-trivial on aarch64.
- **Rejected alternatives:**
  - **Generate the vector table with a Rust macro.** Possible (e.g. `repeat! { vector_entry(class, mode) }`), but the macro would have to expand to inline `naked_asm!` blocks or a `global_asm!` template; the per-entry asm is small enough that hand-writing all 16 entries is shorter and explicitly auditable. The macro would obscure the `0x80` per-entry stride that the architecture requires.
  - **Save the full 31-GPR set in the trampoline.** Done by some kernels for simplicity; redundant here because Rust upholds AAPCS64 callee-save naturally. Saving the full set would grow the frame from 192 bytes to ~272 and cost ~10 extra `stp` instructions per IRQ entry. v1's IRQ rate is low enough that the win is invisible, but the smaller frame is preferred for cache discipline.
  - **Install `VBAR_EL1` in `boot.s` rather than `kernel_entry`.** boot.s currently does only the EL drop + stack/BSS setup; routing `VBAR_EL1` through Rust keeps the Rust-side discipline consistent (the symbol is `extern "C"` from Rust, the `MSR` instruction is in a small inline-asm block with a `# Safety` comment). A future tightening could move this to `boot.s` between the EL drop and the `bl kernel_entry`; for now the post-`kernel_entry`-start sequence is easier to audit because the entire boot chain reads top-to-bottom in one Rust function.
  - **Mask all `DAIF` bits except `I` permanently.** `D` (debug), `A` (SError), `F` (FIQ) currently stay masked because v1 has no debugger, no SError-recoverable scenario, and no FIQ source. Each becomes its own follow-up when a driver / debugger / safety-critical SError path needs it.
  - **Decoding `ESR_EL1` in `panic_entry`.** Adding ESR-class / ISS decoding would give richer "why did this fault?" diagnostics. Deferred — v1's panic message includes the raw `ESR_EL1` value, and a richer decoder is a small follow-up that does not change the entry semantics.
- **Reviewed by:** @cemililik (+ Claude Opus 4.7 agent).
- **Status:** Active. Pending QEMU smoke verification at the maintainer's first opportunity per the T-012 review-history row; the asm trampolines and the `MSR VBAR_EL1` block are kernel-build clean but the actual exception-take + dispatch path is observable only in QEMU smoke.

  **Amendment (2026-05-06, smoke run at HEAD `214052d`): partial verification — `MSR VBAR_EL1` reached and `DAIF.I` unmask sequenced correctly; the asm trampoline's exception-entry path is still unverified.** First end-to-end QEMU smoke ran 2026-05-06; the kernel reached `kernel_entry`'s `MSR VBAR_EL1, x0` block and the subsequent `MSR DAIFClr, #0x2` block (the `tyrne: starting cooperative scheduler` line is printed *after* both, and the smoke trace shows it). No fault occurred in either write, no exception fired in the kernel-entry-to-scheduler-start interval, and the kernel proceeded into the cooperative scheduler. The 2 KiB-aligned `tyrne_vectors` symbol therefore exists at a valid address and `VBAR_EL1`'s bottom-11-bit-zero requirement is satisfied; `DAIFClr, #0x2` cleared `I` only and left `D`, `A`, `F` masked (otherwise an SError or FIQ source — there are none in v1, but the discipline matters — would have surfaced). **Not verified by this smoke:** the `tyrne_irq_curr_el_trampoline` asm at `tyrne_vectors+0x280`, the 192-byte `TrapFrame` save sequence, and the `bl irq_entry` → return → `eret` round-trip. No IRQ ever fires in the smoke trace because the demo's idle-dispatch regression hangs the kernel in `WFI` before any timer is armed (see [`docs/analysis/reviews/business-reviews/2026-05-06-B1-smoke-regression.md`](../analysis/reviews/business-reviews/2026-05-06-B1-smoke-regression.md)). The "exception take + dispatch actually works" claim therefore still rests on the asm-correct + `repr(C)`-byte-match arguments, not on observed dispatch. This `Pending QEMU smoke verification` status persists; the next Amendment lifts it only when a full IRQ-fire path runs end-to-end (post-T-014 fix + first `arm_deadline` call from any task body).

  **Amendment (2026-05-06, post-T-014 smoke): partial verification reaffirmed; trampoline exception-entry path still unverified.** Post-T-014 + ADR-0026 smoke produces the full demo trace through `tyrne: all tasks complete`. The `MSR VBAR_EL1` install + `MSR DAIFClr, #0x2` sequence is now confirmed to leave the CPU in a healthy IRQ-unmasked state across the *entire* run, not just up to the scheduler start: `-d int,unimp,guest_errors` shows zero events for the full ~6.3 ms boot-to-end window. The asm vector table's 2 KiB alignment and the `VBAR_EL1` bottom-11-bit-zero invariant therefore hold under sustained execution with `DAIF.I` unmasked — a stronger result than the 2026-05-06 morning smoke (which hung before reaching scheduler steady state). **Still not verified:** the trampoline's `stp` save sequence, the `TrapFrame` byte-for-byte layout match, and the `bl irq_entry` → return → `eret` round-trip. The v1 demo continues to provide no IRQ source that would fire `+0x280`; clearance gates on the first task that arms a deadline. `Pending QEMU smoke verification` status **stays**.

### UNSAFE-2026-0021 — EL1 virtual generic-timer compare-register writes

- **Introduced:** 2026-04-28, [T-012 — Exception infrastructure and interrupt delivery](../analysis/tasks/phase-b/T-012-exception-and-irq-infrastructure.md). Lights up the IRQ-armed half of the [`Timer`](../../hal/src/timer.rs) trait the time-source half (T-009 / UNSAFE-2026-0015) left as `unimplemented!()`.
- **Location:** [`bsp-qemu-virt/src/cpu.rs`](../../bsp-qemu-virt/src/cpu.rs) — `<QemuVirtCpu as Timer>::arm_deadline` and `<QemuVirtCpu as Timer>::cancel_deadline`. Also [`bsp-qemu-virt/src/exceptions.rs`](../../bsp-qemu-virt/src/exceptions.rs) — the `MSR CNTV_CTL_EL0, x` write inside the IRQ-27 dispatch arm of `irq_entry`.
- **Operation:** Three architected writes to the EL1 virtual generic-timer registers:
  - `MSR CNTV_CVAL_EL0, x` — sets the comparator value in counter ticks. The timer raises `PPI 27` when `CNTVCT_EL0` reaches this value (with `ENABLE = 1` and `IMASK = 0`).
  - `MSR CNTV_CTL_EL0, 0b01` — `ENABLE = 1, IMASK = 0`, arming the timer.
  - `MSR CNTV_CTL_EL0, 0b10` — `ENABLE = 0, IMASK = 1`, masking + disabling. Used by `cancel_deadline` and by `irq_entry`'s timer dispatch arm so the same deadline does not re-fire before the next `arm_deadline` re-arm.
- **Invariants relied on:**
  - **EL1 + non-VHE.** Per ADR-0024 + UNSAFE-2026-0017, `boot.s` drives the kernel to EL1 with `HCR_EL2.{E2H, TGE} = {0, 0}`. In this configuration the `CNTV_*_EL0` registers are unconditionally accessible from EL1 — the `CNTHCTL_EL2.EL1{V,P}TEN` gating that exists in VHE mode does not apply. The runtime EL check in `QemuVirtCpu::new` (UNSAFE-2026-0016) is the load-bearing post-condition.
  - **Counter family alignment.** `arm_deadline`'s `CNTV_CVAL_EL0` write uses the same register family as `now_ns`'s `CNTVCT_EL0` read (per UNSAFE-2026-0015's first Amendment "register-family swap"). Mixing virtual-counter reads with physical-comparator writes would silently misbehave once `CNTVOFF_EL2 ≠ 0`; the all-virtual choice is correct in every plausible boot stack.
  - **`PPI 27` is the EL1 virtual timer's IRQ on QEMU virt.** ARM Generic Timer architecture (ARM ARM §D11) reserves PPI 27 for the EL1 virtual timer; QEMU virt follows this. Future BSPs (Pi 4) follow the same convention.
  - **`ns_to_ticks` is the inverse of `ticks_to_ns`.** Both use 128-bit intermediate arithmetic with saturating cast back to `u64`; round-tripping a tick count through `ticks_to_ns -> ns_to_ticks` returns the original value at any frequency that divides evenly into 1e9 ns/s, and stays within the integer-truncation drift documented in [`hal/src/timer.rs`](../../hal/src/timer.rs)'s `ns_to_ticks_round_trips_against_ticks_to_ns_at_qemu_frequency` test for non-divisor frequencies.
  - **`options(nostack, nomem)` is correct.** All three `MSR` instructions write a system register; none touches memory or the stack.
  - **GIC interaction goes through `IrqController`.** `arm_deadline` and `cancel_deadline` call `gic.enable(TIMER_IRQ)` / `gic.disable(TIMER_IRQ)`; the GIC MMIO discipline lives in UNSAFE-2026-0019. The Timer impl does not touch the GIC directly.
- **Rejected alternatives:**
  - **Use `CNTV_TVAL_EL0` (relative timer-value) instead of `CNTV_CVAL_EL0` (absolute compare).** TVAL is a 32-bit relative-from-now register; CVAL is 64-bit absolute. ADR-0010's `arm_deadline(deadline_ns: u64)` semantics are explicitly absolute time, so CVAL is the correct match. TVAL would also constrain deadlines to ~136 seconds at 1 GHz (32-bit overflow), which is unacceptable for `time_sleep_until` syscalls in B5+.
  - **Read-modify-write `CNTV_CTL_EL0`.** The control register has only three live bits in v1 (ENABLE, IMASK, ISTATUS); ISTATUS is read-only. RMW would preserve any unrelated bits future architecture extensions might add — but ARMv8.0/8.1+ has no RES1 bits in this register, so the literal write `0b01` / `0b10` is correct and audit-friendly.
  - **Skip masking the timer at the source in the IRQ handler.** Acknowledging at the GIC alone does not stop the timer's pending state at the timer level; if the comparator is still less than CNTVCT, the timer will re-fire as soon as the GIC line is re-enabled. Masking at `CNTV_CTL_EL0` is the architected way to silence the timer until the next re-arm.
  - **Inline the `ns_to_ticks` math at the call site.** Rejected because the host-testable helper in `tyrne_hal::timer` is the safe abstraction; ADR-0010 + UNSAFE-2026-0015's saturation discipline applies symmetrically to both directions.
- **Reviewed by:** @cemililik (+ Claude Opus 4.7 agent).
- **Status:** Active. Pending QEMU smoke verification at the maintainer's first opportunity per the T-012 review-history row; the writes are kernel-build clean but IRQ delivery + deadline-fire timing is observable only in QEMU smoke. v1's cooperative IPC demo does not arm any deadline, so the smoke trace should remain unchanged from commit `a043079` (T-012 step 1+2+3).

  **Amendment (2026-05-06, smoke run at HEAD `214052d`): no verification possible from this smoke — the timer-write sites are not reached in the demo path.** First end-to-end QEMU smoke ran 2026-05-06. Per the entry's *Status* line above, v1's cooperative IPC demo does not call `arm_deadline` or `cancel_deadline`, and the only `MSR CNTV_CTL_EL0` write site reached at runtime is the IRQ-handler-side mask in `irq_entry` — which itself never executes because no IRQ ever fires (the timer is never armed). The smoke therefore confirmed nothing about the three architected writes this entry audits; the kernel-build-clean + datasheet-match argument remains the sole basis for the claim. The smoke also surfaced an unrelated idle-dispatch regression (see [`docs/analysis/reviews/business-reviews/2026-05-06-B1-smoke-regression.md`](../analysis/reviews/business-reviews/2026-05-06-B1-smoke-regression.md)) which prevents reaching any deadline-arming code path even after T-014 fixes the regression — `task_a` / `task_b` never call `arm_deadline` regardless. This `Pending QEMU smoke verification` status persists; the verification is gated on a future B-phase task introducing a real `arm_deadline` caller (likely the first preemption-using task — currently planned for B5+ per [`phase-b.md`](../roadmap/phases/phase-b.md)), at which point a full smoke trace can exercise the deadline-fire → IRQ → `irq_entry` → `MSR CNTV_CTL_EL0 = 2` mask round-trip.
