# 0008 — `Cpu` HAL trait signature (v1, single-core scope)

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

The second HAL trait in Phase 4b, after [ADR-0007: Console](0007-console-trait.md). The `Cpu` trait covers the privileged, architecture-specific operations the kernel invokes on the running CPU core: reading the core identifier, masking and restoring interrupts for critical sections, halting the core in the idle path, and synchronizing the instruction pipeline after privileged register writes.

Three adjacent capabilities are explicitly **out of scope** for this ADR and will get their own ADRs when we need them:

- **Context-switch primitives** (`save_context` / `restore_context`, stack swapping). These are the scheduler's tools and are deeply entangled with the scheduler's invariants. They will be pinned down in a dedicated ADR at the start of scheduler work, not bundled into `Cpu` now where they would invite premature design.
- **Secondary-core start** (PSCI `CPU_ON`, core-count discovery, rendezvous semantics). The initial kernel boots one core; multi-core start warrants its own ADR alongside the multi-core work.
- **Topology queries** (cache coherency domains, NUMA, cluster structure). Irrelevant until there is more than one core and they need to cooperate.

The goal here is the **smallest useful `Cpu` surface for single-core boot** — large enough that the kernel can mask interrupts, wait for them, and synchronize after system-register writes; small enough that each method has an obvious aarch64 mapping and an obvious test-hal fake.

## Decision drivers

- **Single-core first.** Multi-core is deferred; the surface must be usable without assuming a second core exists.
- **Composable critical sections.** Kernel code frequently wraps a region with interrupts disabled. The primitive should compose cleanly under nesting (an inner disable/restore inside an outer disable/restore leaves the outer state intact).
- **Object-safe trait.** The kernel holds `&'static dyn Cpu`; every method must dispatch through a vtable, so no associated types or `where Self: Sized` on the main surface.
- **`Send + Sync` enforced at the trait bound.** Multi-core safety is a future concern but the bound should be present now so implementations do not acquire thread-affine state by accident.
- **Primitives that map directly to aarch64 instructions** on the target (`MSR DAIFSET`, `WFI`, `MRS MPIDR`, `ISB`). No level of abstraction beyond "one call, one operation," to keep BSP implementations obvious.
- **Rust-atomic overlap avoided.** Data memory barriers are handled by `core::sync::atomic::fence` and friends; exposing them again on the trait would be redundant. Instruction synchronization (ISB) has no atomic equivalent and must live on the trait.
- **Idiomatic RAII available, not required.** Callers who want RAII for critical sections get it via a thin wrapper type in the HAL; callers who want the explicit disable/restore pair can use it directly. Both paths are equally supported.

## Considered options

### Option A — closure-based critical section

```rust
pub trait Cpu: Send + Sync {
    fn without_irqs<R>(&self, f: impl FnOnce() -> R) -> R;
    // …
}
```

### Option B — RAII guard via associated type on the trait

```rust
pub trait Cpu: Send + Sync {
    type IrqGuard<'a>: Drop where Self: 'a;
    fn disable_irqs(&self) -> Self::IrqGuard<'_>;
    // …
}
```

### Option C — explicit disable/restore pair plus a free-standing RAII wrapper

```rust
pub trait Cpu: Send + Sync {
    fn disable_irqs(&self) -> IrqState;
    fn restore_irq_state(&self, state: IrqState);
    // …
}

pub struct IrqGuard<'a> { /* layered on top */ }
```

## Decision outcome

**Chosen: Option C — explicit disable/restore pair plus a free-standing RAII wrapper.**

The `Cpu` trait carries five methods in v1:

```rust
pub trait Cpu: Send + Sync {
    fn current_core_id(&self) -> CoreId;
    fn disable_irqs(&self) -> IrqState;
    fn restore_irq_state(&self, state: IrqState);
    fn wait_for_interrupt(&self);
    fn instruction_barrier(&self);
}
```

Supporting types live in the same module:

```rust
pub type CoreId = u32;

#[derive(Copy, Clone)]
pub struct IrqState(pub usize);
```

And an RAII wrapper is offered as a separate `IrqGuard<'a>` struct that borrows `&'a dyn Cpu`, not a trait-associated type:

```rust
pub struct IrqGuard<'a> {
    cpu: &'a dyn Cpu,
    prev: IrqState,
}

impl<'a> IrqGuard<'a> {
    pub fn new(cpu: &'a dyn Cpu) -> Self { /* disable, remember prev */ }
}

impl Drop for IrqGuard<'_> { /* restore prev */ }
```

The explicit pair matches the convention Linux and most Rust embedded kernels (Hubris, Tock) have converged on — `local_irq_save` / `local_irq_restore` — and is the least-magical way to express "mask interrupts, remember the previous mask, do work, restore." The free-standing `IrqGuard` gives callers the RAII ergonomics without forcing trait-level associated types that would reduce object-safety.

The closure form (Option A) was rejected because the kernel has several places where the critical-section scope needs to cross a function boundary or nest, which is awkward under closure composition and pushes lifetime issues into type annotations that are hard to read. Option B (associated type) was rejected because the kernel uses `&dyn Cpu` as its canonical handle; associated-type methods force either generic code everywhere or `where Self: Sized` escape hatches that defeat the dyn dispatch we rely on.

`core_count`, `start_core`, `save_context`, and `restore_context` are **not** in v1. Each will come with its own ADR when its scope arrives (multi-core start; scheduler bring-up). The `Cpu` trait is expected to grow, and growth is compatible because the trait is not considered stable API outside the project.

## Consequences

### Positive

- **Small, object-safe, dyn-friendly surface.** Five methods, no associated types, no generics at the method level.
- **RAII for free via [`IrqGuard`].** The common pattern (`let _g = IrqGuard::new(cpu);`) is one line of setup and auto-restores on scope exit.
- **Explicit pair is composable.** Nested critical sections trivially stack; inner restore puts the outer state back in place because `IrqState` is the *saved* state, not a fixed value.
- **Direct aarch64 mapping.** `current_core_id` reads `MPIDR_EL1`; `disable_irqs` does `MSR DAIFSET, #0xF` and returns the prior value; `wait_for_interrupt` is `WFI`; `instruction_barrier` is `ISB`. Each BSP method is a handful of inline-asm lines.
- **Test fake is straightforward.** `FakeCpu` tracks IRQ state as a bool and counts `wait_for_interrupt` / `instruction_barrier` calls; tests can assert on everything.

### Negative

- **Caller must opt into RAII explicitly.** `let _g = IrqGuard::new(cpu)` is one line, but it's still one more line than `cpu.critical_section()` would be on an associated-type design. Accepted: object-safety is worth more than one keystroke.
- **`IrqState(pub usize)` exposes its field.** The type is a newtype over a machine word, not a truly opaque capability. BSPs construct it from raw `DAIF` bits or an equivalent per architecture. The exposed field is not "private data the caller can tamper with"; it is "a word the caller passes back unmodified." Documented in the rustdoc; revisit if it causes bugs.
- **No multi-core primitives now means a second ADR later.** When multi-core start arrives, it will either extend `Cpu` (a breaking change for implementers, though not for the kernel's dyn callers) or introduce a sibling trait (`CpuMultiCore`). We prefer the sibling-trait path to keep `Cpu` stable once the single-core surface settles.
- **No context-switch methods means the scheduler cannot be written against `Cpu` alone.** Correct. The scheduler will depend on a separate `ContextSwitch` trait or a BSP-provided function, introduced in its own ADR.

### Neutral

- `CoreId = u32`. Supports 4 billion cores; padding / size is irrelevant on 64-bit targets.
- The trait does not include data memory barriers. Rust's `core::sync::atomic::fence(Ordering::SeqCst)` covers those; duplicating them on `Cpu` would only create two paths to the same operation.

## Pros and cons of the options

### Option A — closure-based critical section

- Pro: caller cannot forget to restore.
- Pro: the critical section has a clear syntactic scope.
- Con: composition is awkward — nesting produces verbose closures with long lifetime annotations.
- Con: early return / `?` operator across the closure boundary gets tangled.
- Con: the return type of `without_irqs` must itself be a value, which is fine for most uses but complicates code that wants to mutate borrowed state.

### Option B — RAII via associated type

- Pro: most ergonomic call site (`let _g = cpu.critical_section();`).
- Pro: the guard's `Drop` runs automatically.
- Con: associated types on trait methods reduce object-safety; `dyn Cpu` requires awkward workarounds.
- Con: BSPs implement both the trait and the guard, doubling the per-BSP surface.
- Con: leaks implementation types through the trait boundary; changing a BSP's guard shape is a source-breaking change.

### Option C — explicit pair plus free-standing guard (chosen)

- Pro: trait is minimal, object-safe, dyn-friendly.
- Pro: RAII available through `IrqGuard::new(cpu)` — one-line setup.
- Pro: explicit pair matches long-established kernel convention.
- Pro: guard can be extended or replaced without touching the `Cpu` trait.
- Con: callers write one extra line for RAII. Accepted.

## References

- [ADR-0006: Workspace layout](0006-workspace-layout.md).
- [ADR-0007: Console HAL trait signature](0007-console-trait.md).
- [`docs/architecture/hal.md`](../architecture/hal.md) — `Cpu`'s architectural role.
- [`docs/standards/error-handling.md`](../standards/error-handling.md) — panic strategy, why these primitives are infallible.
- ARM *Architecture Reference Manual*, ARMv8-A — `PSTATE.DAIF`, `WFI`, `MPIDR_EL1`, `ISB` semantics.
- Linux kernel `local_irq_save` / `local_irq_restore` — prior art for the explicit-pair convention.
- Hubris `CpuCore` abstractions — https://hubris.oxide.computer/
- Tock kernel `kernel::platform::chip::Chip::{atomic, sleep}` — a closure-based alternative considered.
