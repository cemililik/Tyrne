# 0020 — `Cpu` trait v2: context-switch extension

- **Status:** Accepted
- **Date:** 2026-04-21
- **Deciders:** @cemililik

## Context

[ADR-0008](0008-cpu-trait.md) introduced the `Cpu` HAL trait and explicitly deferred context-switch primitives with the note: *"They will be pinned down in a dedicated ADR at the start of scheduler work."* This is that ADR.

[ADR-0019](0019-scheduler-shape.md) settled the scheduler's data structure and described a `TaskContexts<C>` parallel array indexed by raw task slot — meaning the scheduler is generic over some type `C` that provides both the context-switch operation and the concrete `TaskContext` storage type. This ADR decides the shape of that extension.

**The core tension.** The existing `Cpu` trait is object-safe: the kernel holds `&dyn Cpu` and dispatches through a vtable. Context switch requires an **associated type** (`TaskContext`) to be useful — the scheduler must be able to store and index an array of task contexts, which means knowing the concrete size and layout at compile time. Associated types break `dyn` compatibility.

Three design paths exist:

1. Extend `Cpu` with associated type, abandon `dyn Cpu` for the scheduler.
2. Introduce a separate `ContextSwitch` trait with the associated type; `Cpu` remains object-safe and unchanged.
3. Keep `Cpu` object-safe by expressing context save/restore through raw pointers to caller-managed buffers of a fixed architecture-defined size.

The right choice depends on which consumers need `dyn Cpu` and whether the scheduler specifically can afford to be generic instead.

**Who uses `dyn Cpu`?** In Phase A, `kernel::run` receives `&C: Console` and calls `cpu.disable_irqs()` for critical sections. The interrupt-mask operations (`disable_irqs`, `restore_irq_state`, `wait_for_interrupt`, `instruction_barrier`) are genuinely object-safe and benefit from dynamic dispatch at the kernel entry point. The scheduler, however, is a static component that knows the BSP at link time — it does not need dynamic dispatch.

**aarch64 calling convention.** A cooperative context switch only needs to save callee-saved registers: `x19`–`x28`, the frame pointer (`x29 / fp`), the link register (`x30 / lr`), and the stack pointer (`sp`). The compiler already saves caller-saved registers around any ordinary function call; from the compiler's view, `context_switch` is an ordinary function. This means the assembly is minimal: save 13 values (12 registers + sp), restore 13 values, return.

## Decision drivers

- **Preserve `Cpu` object-safety.** The interrupt-mask surface is used with `&dyn Cpu` throughout the kernel; widening that trait to include associated types would force generic parameters everywhere `Cpu` is passed, which is an unnecessary churn on stable code.
- **Scheduler is generic, not dynamic.** The scheduler is instantiated once per BSP; it never needs to dispatch across multiple CPU implementations at runtime. Generic bounds (`<C: ContextSwitch>`) are natural and add zero runtime cost.
- **No heap, no allocation.** `TaskContext` must be a plain `Copy`/`Default` struct that can live in a bounded array. No `Box`, no `dyn`, no allocation.
- **Minimal `unsafe` surface.** The context-switch assembly is unavoidably `unsafe`. Everything else must be safe. The trait's methods carry `unsafe` only where the assembly invariants cannot be expressed in Rust's type system.
- **`no_std` compatible.** The trait and its implementations live in HAL and BSP crates that are `#![no_std]`.
- **Auditable.** Each `unsafe impl` block must have a `# Safety` section that states the invariants. See [unsafe-policy.md](../standards/unsafe-policy.md).
- **Testable without QEMU.** The `TestHal` fake can implement `ContextSwitch` with a `TaskContext` that is a simple counter or flag, allowing scheduler logic to be exercised in host tests without real assembly.

## Considered options

### Shape of the extension

**Option A — Separate `ContextSwitch` trait with associated `TaskContext`.**

```rust
// In umbrix-hal

pub trait ContextSwitch {
    type TaskContext: Default + Send;

    /// Save the current CPU register state into `current` and restore
    /// the state from `next`, switching execution to the task that
    /// previously saved into `next`.
    ///
    /// # Safety
    ///
    /// - `current` must point at storage that remains valid until this
    ///   task is switched back to and `context_switch` returns.
    /// - `next` must contain a valid context previously written by
    ///   `context_switch` or initialised by `init_context`.
    /// - Interrupts must be disabled by the caller for the duration of
    ///   the switch to prevent a preempting IRQ from observing a
    ///   partially saved state.
    unsafe fn context_switch(
        &self,
        current: &mut Self::TaskContext,
        next: &Self::TaskContext,
    );

    /// Initialise a `TaskContext` so that restoring it begins execution
    /// at `entry` with `stack_top` as the initial stack pointer.
    ///
    /// # Safety
    ///
    /// `stack_top` must point at the top of a sufficiently-sized,
    /// properly aligned stack that remains valid for the task's
    /// lifetime.
    unsafe fn init_context(
        &self,
        ctx: &mut Self::TaskContext,
        entry: fn() -> !,
        stack_top: *mut u8,
    );
}
```

`QemuVirtCpu` implements both `Cpu` (unchanged, object-safe) and `ContextSwitch`. The scheduler is generic over `C: ContextSwitch`. `Cpu` gains no new methods.

- Pro: `Cpu` is fully stable; no existing callsite changes.
- Pro: clear separation of concerns — interrupt management vs. context management.
- Pro: `ContextSwitch` can be implemented by the `TestHal` with a no-op or a trivial fake.
- Con: two separate traits for the same BSP struct; a caller that needs both must bound `C: Cpu + ContextSwitch`.
- Neutral: associated types on a non-`dyn` trait are idiomatic and add no runtime cost.

**Option B — Extend `Cpu` with `save_context_raw` / `restore_context_raw` using raw pointers and a const size.**

```rust
pub trait Cpu: Send + Sync {
    // … existing methods …
    const CONTEXT_SIZE: usize;
    unsafe fn save_context_raw(&self, buf: *mut u8);
    unsafe fn restore_context_raw(&self, buf: *const u8) -> !;
}
```

The scheduler allocates `[[u8; MAX_CONTEXT_SIZE]; N]` and passes raw pointers. `MAX_CONTEXT_SIZE` is a project-wide constant set conservatively large.

- Pro: `Cpu` stays a single trait; no second trait.
- Con: `const CONTEXT_SIZE` on a trait method breaks `dyn Cpu` (associated constants are not object-safe without `where Self: Sized`). Either abandons object-safety or adds a carve-out.
- Con: raw pointer buffers lose all type safety; the scheduler cannot statically verify it is passing a correctly-typed buffer.
- Con: `MAX_CONTEXT_SIZE` is a global constant that must be correct for every architecture — fragile.

**Option C — Extend `Cpu` with associated type `TaskContext`; abandon `dyn Cpu` universally.**

```rust
pub trait Cpu: Send + Sync {
    type TaskContext: Default + Send;
    // … existing + new methods …
}
```

Every consumer of `Cpu` becomes generic: `kernel::run<C: Cpu>(cpu: &C, …)`, etc.

- Pro: one unified trait.
- Con: all existing code that takes `&dyn Cpu` must be rewritten to `<C: Cpu>` — a large, low-value change touching stable, tested code.
- Con: `dyn Cpu` becomes impossible; if the kernel ever needs to swap BSPs at runtime, it loses that option entirely.

**Option D — Context switch through BSP-owned function pointers rather than a trait.**

The BSP exposes `static CONTEXT_SWITCH: fn(*mut TaskCtx, *const TaskCtx)`. The scheduler calls the function pointer directly.

- Pro: no trait complexity.
- Con: not object-oriented; hard to fake for host tests; no type safety; the scheduler must know the BSP's concrete `TaskCtx` type anyway.
- Con: contradicts the HAL-isolation principle established in [ADR-0006](0006-workspace-layout.md).

### `unsafe` placement

**Option E — Both `context_switch` and `init_context` are `unsafe fn`.**

Callers must uphold stack validity and context initialisation invariants.

**Option F — Wrapper safe API with `unsafe` inside the trait implementation.**

`context_switch` is `fn` (safe); the trait implementation uses `unsafe` internally. Safety relies on the implementation being correct.

- Con: a safe `context_switch` API implies the compiler guarantees correctness, but the invariants (valid stack, disabled interrupts) cannot be expressed in Rust's type system. A safe API would be misleading.

## Decision outcome

**Chosen: Option A — separate `ContextSwitch` trait with associated `TaskContext`; Option E — `unsafe fn` at the trait boundary.**

### Trait definition

```rust
// umbrix-hal/src/cpu.rs  (addition; existing Cpu trait unchanged)

/// Context-switch extension for BSPs that support cooperative task switching.
///
/// Separate from [`Cpu`] to preserve `Cpu`'s object-safety. The scheduler
/// is generic over `C: ContextSwitch`; it never needs dynamic dispatch.
///
/// # Safety contract
///
/// Implementations must ensure that `context_switch` atomically saves
/// all callee-saved registers of the current execution context and
/// restores all callee-saved registers of the next context. On aarch64
/// that is `x19`–`x28`, `x29` (fp), `x30` (lr), and `sp`. The switch
/// appears to return normally to both the saving call site (when this
/// task is resumed) and the resuming call site returns immediately.
pub trait ContextSwitch {
    /// The saved register state for one task.
    ///
    /// Must be `Default` so the scheduler can zero-initialise a slot
    /// before `init_context` fills it in. Must be `Send` so contexts
    /// can be moved between (future) CPU cores.
    type TaskContext: Default + Send;

    /// Save the calling task's register state into `current` and resume
    /// the task whose state was saved in `next`.
    ///
    /// When this task is later resumed (by another call to
    /// `context_switch` with `current` as the `next` argument),
    /// execution continues as if `context_switch` returned normally.
    ///
    /// # Safety
    ///
    /// - Interrupts must be disabled before this call and remain
    ///   disabled until the caller re-enables them after the switch.
    ///   An IRQ firing mid-switch would observe a partially saved state.
    /// - `current` must be valid for the entire time this task is
    ///   suspended; the caller is responsible for keeping the
    ///   `TaskContexts` array alive.
    /// - `next` must contain a context previously written by
    ///   `context_switch` or fully initialised by `init_context`.
    ///   Restoring an uninitialised or partially written context is
    ///   undefined behaviour.
    unsafe fn context_switch(
        &self,
        current: &mut Self::TaskContext,
        next: &Self::TaskContext,
    );

    /// Write an initial register state into `ctx` so that the first
    /// restore of `ctx` begins executing `entry` with `stack_top` as
    /// the stack pointer.
    ///
    /// # Safety
    ///
    /// - `stack_top` must point one byte past the top of a
    ///   sufficiently-sized (≥ 512 bytes recommended for aarch64),
    ///   16-byte-aligned stack region that remains valid for the
    ///   task's entire lifetime.
    /// - `entry` must be a `fn() -> !` that never returns; returning
    ///   from a task entry function is undefined behaviour.
    unsafe fn init_context(
        &self,
        ctx: &mut Self::TaskContext,
        entry: fn() -> !,
        stack_top: *mut u8,
    );
}
```

### aarch64 concrete type (in `bsp-qemu-virt`)

```rust
// bsp-qemu-virt/src/cpu.rs

/// Saved callee-register state for one cooperative task on aarch64.
///
/// Layout must match the offsets used by the `context_switch_asm`
/// routine in `context_switch.s`. `#[repr(C)]` ensures the compiler
/// does not reorder fields.
#[derive(Default)]
#[repr(C)]
pub struct Aarch64TaskContext {
    /// x19–x28: callee-saved general-purpose registers.
    pub x19_x28: [u64; 10],
    /// x29 — frame pointer (callee-saved).
    pub fp: u64,
    /// x30 — link register (return address; callee-saved in AAPCS64).
    pub lr: u64,
    /// Stack pointer — saved explicitly (not a general-purpose register).
    pub sp: u64,
}
// Total: 13 × 8 = 104 bytes per task context.
```

The `context_switch` implementation calls an assembly routine `context_switch_asm(current: *mut Aarch64TaskContext, next: *const Aarch64TaskContext)` via `core::arch::asm!` or a separate `.s` file. The routine:

1. Saves `x19`–`x28`, `x29`, `x30` into `[current]`.
2. Saves `sp` (via `mov x2, sp`) into `[current + offset_of(sp)]`.
3. Loads `sp`, `x29`, `x30`, `x19`–`x28` from `[next]` in reverse order.
4. Returns via `ret` — which now jumps to the `lr` loaded from `next`.

For the first run of a task, `init_context` sets `lr` to the entry function address and `sp` to `stack_top`. The first `ret` in the assembly begins executing the entry function.

### Scheduler integration

The scheduler becomes `Scheduler<C: ContextSwitch>`. The `TaskContexts<C>` parallel array (from ADR-0019) is:

```rust
struct TaskContexts<C: ContextSwitch> {
    contexts: [C::TaskContext; TASK_ARENA_CAPACITY],
}
```

`Scheduler::yield_now` calls `cpu.context_switch(&mut contexts[current_idx], &contexts[next_idx])` inside a critical section (interrupts disabled via `cpu.disable_irqs()`).

### `TestHal` fake

```rust
// For host-side tests: no assembly, just records which context was last switched to.
#[derive(Default)]
pub struct FakeTaskContext { switched_to: usize }

pub struct FakeCpu { /* existing fields */ }

impl ContextSwitch for FakeCpu {
    type TaskContext = FakeTaskContext;
    unsafe fn context_switch(&self, _current: &mut Self::TaskContext, _next: &Self::TaskContext) {
        // No-op in host tests; scheduler logic is tested without real switching.
    }
    unsafe fn init_context(&self, _ctx: &mut Self::TaskContext, _entry: fn() -> !, _stack_top: *mut u8) {}
}
```

## Consequences

### Positive

- **`Cpu` is fully unchanged.** All existing callsites, tests, and implementations compile without modification.
- **Scheduler is zero-overhead.** Monomorphisation of `Scheduler<QemuVirtCpu>` produces direct calls with no vtable indirection.
- **`unsafe` is localised.** The `context_switch_asm` routine and its safe wrapper in the BSP are the only new `unsafe` code. The scheduler's `yield_now` calls `unsafe { cpu.context_switch(…) }` — one `unsafe` block with a documented invariant.
- **Host tests for scheduler logic work without QEMU.** `FakeCpu: ContextSwitch` no-ops the switch; queue management, blocked-task transitions, and IPC bridge logic can all be tested in `cargo test`.
- **Forward-compatible with multi-core.** A future `ContextSwitch` implementation for multi-core simply provides a different `TaskContext` and assembly; the scheduler's generic parameter absorbs the change.

### Negative

- **Two bounds needed.** Callers that need both interrupt management and context switching must write `C: Cpu + ContextSwitch`. For `kernel_main` and the scheduler entry, this is one extra trait bound — acceptable.
- **One more `unsafe` audit entry.** The `context_switch_asm` assembly block is new `unsafe` that must be audited per [unsafe-policy.md](../standards/unsafe-policy.md). This is unavoidable for any real context switch.
- **`init_context` initialises `lr` to a `fn() -> !` raw address.** This is safe in practice (function pointers are always valid addresses in Rust) but requires care: the entry function must truly never return, or the `ret` at the end of the assembly falls into garbage.

### Neutral

- **`Aarch64TaskContext` is 104 bytes.** With `TASK_ARENA_CAPACITY = 16`, `TaskContexts` occupies 1 664 bytes — well within any reasonable stack or `.bss` section.
- **NEON / FP registers deferred.** The aarch64 AAPCS64 callee-saved NEON registers (`d8`–`d15`) are not saved in v1 because Phase A kernel tasks do not use floating point. A Phase B ADR will add them when userspace tasks run with FP enabled.
- **`sp` alignment.** The aarch64 ABI requires `sp` to be 16-byte-aligned at all `bl` / function call boundaries. `init_context` must receive a `stack_top` that is already 16-byte-aligned; callers that provide odd values trigger undefined behaviour. The `# Safety` doc notes this requirement.

## Open questions

- **Interrupt state across switch.** This ADR requires interrupts disabled during `context_switch`. A future ADR may permit switching with interrupts enabled (for preemption), but that changes the assembly invariants substantially; deferred.
- **FP / NEON context save.** Deferred to Phase B as noted above.
- **Per-task kernel stack allocation.** Where do task stacks come from? In A5 they are static arrays allocated at link time (one per task, sized conservatively). A Phase B memory-management ADR will provide dynamic stack allocation.

## References

- [ADR-0008: `Cpu` HAL trait signature (v1)](0008-cpu-trait.md) — the existing trait this ADR extends by addition.
- [ADR-0019: Scheduler shape](0019-scheduler-shape.md) — the scheduler that consumes `ContextSwitch`.
- [T-004: Cooperative scheduler](../analysis/tasks/phase-a/T-004-cooperative-scheduler.md) — the task implementing this ADR.
- ARM Architecture Reference Manual, ARMv8-A — §C5 "AAPCS64 procedure call standard": callee-saved register list (`x19`–`x28`, `x29`, `x30`, `sp`).
- Hubris context-switch model — saves/restores callee-saved GPRs cooperatively; closest prior art.
- seL4 `ksSwitchToThread` — saves full register file (preemptive); not adopted in v1.
- Rust `core::arch::asm!` — the mechanism for inline assembly in the BSP implementation.
