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

- **Introduced:** 2026-04-27, T-009 second-read review follow-up. Closes the runtime-check half of Review 1's Yüksek #1 finding (the documentation half landed in commit `39fb66c`).
- **Location:** [`bsp-qemu-virt/src/cpu.rs`](../../bsp-qemu-virt/src/cpu.rs) — `QemuVirtCpu::new`, prior to the generic-timer reads it audits.
- **Operation:** One read-only inline-asm `MRS xN, CurrentEL` instruction. The two-bit Exception-Level field (bits [3:2] of `CurrentEL`) is shifted into the low bits and asserted equal to `1` (EL1). A mismatch panics with the observed EL — turning a future boot-flow regression into a loud, named boot-time error rather than letting subsequent `MRS CNTVCT_EL0` / `MRS CNTFRQ_EL0` calls trap or read undefined values at EL2 / EL3.
- **Invariants relied on:**
  - `CurrentEL` is readable at every Exception Level. ARM ARM §D11.2 specifies the register layout; bits [3:2] hold the current EL.
  - The MRS does not modify any state; `options(nostack, nomem)` is correct (no stack pointer touch, no memory access from the asm itself).
  - The shift `(raw >> 2) & 0b11` extracts the EL field exactly; the implementation does not depend on RES0 bits being zero.
- **Rejected alternatives:**
  - **Skip the check, document only.** This is what commit `39fb66c` did; the second-read review's Yüksek #1 explicitly asked for the runtime check on top of documentation. Skipping leaves a boot-flow regression silently producing wrong timer values until much later behaviour falls out of spec.
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
