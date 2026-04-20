# 0011 — `IrqController` HAL trait signature (v1)

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

The fifth and final HAL trait in Phase 4b, after Console (ADR-0007), Cpu (ADR-0008), Mmu (ADR-0009), and Timer (ADR-0010). `IrqController` is the kernel's view of the board's interrupt controller: enable and disable specific lines, acknowledge the pending IRQ at ISR entry, and signal end-of-interrupt at exit. Everything else — driver-facing IRQ capability grants, wakeup delivery via asynchronous notifications — sits *above* this trait and converts kernel-side acknowledgements into userspace messages.

The v1 scope is, like Cpu and Timer v1, single-core. The target hardware surface is ARM GIC v2 (Pi 4's GIC-400) and GIC v3 (QEMU `virt`); the trait abstracts what they share. RISC-V's PLIC will add a second implementation lineage when it arrives.

## Decision drivers

- **Kernel ISR has entry and exit primitives.** Every interrupt produces one acknowledge call and one end-of-interrupt call, with driver logic (or notification delivery) in between. These two halves are the core of the trait.
- **Enable / disable is the other half.** New IRQ-holding capabilities (`IrqCap`) want to enable lines; revocations disable them.
- **ARM GIC quirks should be hidden, not surfaced.** SGI / PPI / SPI distinctions, priority registers, routing bits, split Priority-Drop vs. Deactivation on GICv3 — all belong in the BSP. The kernel sees "a line, a number, enable it, ack it, EOI it."
- **IRQ number width.** u32 accommodates every realistic controller; GICv3 INTID is ≤ 16 M.
- **Object-safe.** The kernel uses `&'static dyn IrqController`.
- **`Send + Sync`.** Standard.
- **No allocation.** Standard.
- **No pending-clear in v1.** Edge-triggered lines that need to be cleared without firing can be added later via a trait extension — not common enough for the initial kernel.
- **No multi-core IPIs in v1.** SGI / inter-processor interrupts are deferred with the rest of multi-core.

## Considered options

### Option A — fused acknowledge-and-handle closure

```rust
fn handle_interrupt<F>(&self, f: F) where F: FnOnce(IrqNumber);
```

### Option B — split acknowledge / end_of_interrupt (chosen)

```rust
fn acknowledge(&self) -> Option<IrqNumber>;
fn end_of_interrupt(&self, irq: IrqNumber);
```

### Option C — split Priority-Drop from Deactivation (GICv3-style)

Three methods at the exit path: `priority_drop(irq)` followed by `deactivate(irq)`. Matches GICv3's split-mode precisely.

### Option D — unified request / complete handle

`acknowledge` returns an opaque `IrqHandle` whose `Drop` performs EOI.

## Decision outcome

**Chosen: Option B — a four-method trait with explicit acknowledge and end_of_interrupt, and an `IrqNumber` newtype over `u32`.**

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct IrqNumber(pub u32);

pub trait IrqController: Send + Sync {
    fn enable(&self, irq: IrqNumber);
    fn disable(&self, irq: IrqNumber);
    fn acknowledge(&self) -> Option<IrqNumber>;
    fn end_of_interrupt(&self, irq: IrqNumber);
}
```

Semantics:

- `enable(irq)` permits `irq` to be delivered to the CPU when raised. Idempotent; enabling an already-enabled line is a no-op.
- `disable(irq)` suppresses delivery of `irq`. Idempotent; already-pending deliveries of `irq` may still be observed at the next `acknowledge` on some hardware — the BSP documents platform specifics if they matter.
- `acknowledge()` is called at ISR entry. It reads the controller's ack register, marks the top-pending IRQ as active, and returns its number. Returns `None` for a spurious interrupt (e.g., GIC's INTID 1023) or a race where the IRQ disappeared before the CPU got to it.
- `end_of_interrupt(irq)` is called at ISR exit with the number `acknowledge` returned. Completes servicing: on GICv3 the BSP performs both Priority Drop and Deactivation (split-mode); on GICv2 the single EOI does both. The kernel does not distinguish.

Option A (fused closure) was rejected because the kernel's ISR structure prefers explicit entry and exit points — the closure would cross the ISR boundary in a way that complicates nested interrupt handling (if we ever enable it) and stack-frame management. Option C (split Priority Drop / Deactivation) was rejected because the kernel does not currently need the expressive power — priority-managed nested interrupts are out of v1 scope. Option D (handle with `Drop`) was rejected because a `Drop` impl that performs hardware operations is awkward to reason about in panic paths and during kernel development generally.

## Consequences

### Positive

- **Minimal object-safe surface.** Four methods, one newtype, one optional-return.
- **Direct mapping to GIC.** `acknowledge` → read `ICC_IAR1_EL1`; `end_of_interrupt` → write `ICC_EOIR1_EL1` (and `ICC_DIR_EL1` on v3 split mode); `enable` / `disable` → write to the corresponding ISENABLER / ICENABLER registers.
- **Spurious handling is explicit.** `Option<IrqNumber>` makes the callsite read the return value and handle the spurious case; implicit "ignore id 1023" conventions do not propagate into the kernel.
- **Test fake is straightforward.** `FakeIrqController` exposes enabled-set state, an injectable pending queue, and an EOI history for assertions.

### Negative

- **No pending-clear primitive in v1.** An edge-triggered device whose driver wants to discard a stale pending bit cannot. Mitigation: not needed for the timer- and UART-driven kernel of Phase 4c; add a future ADR when a driver genuinely requires it.
- **No priority configuration in v1.** All IRQs are delivered at the BSP's default priority. Adequate for v1; insufficient for real-time scheduling.
- **No per-CPU routing in v1.** SPI targeting is a BSP-internal decision. Multi-core will need a method or a new trait.
- **No SGI / IPI in v1.** Multi-core inter-processor interrupts require their own primitive (future ADR alongside multi-core start in the Cpu trait).
- **No nested-interrupt support in v1.** The controller is used with all-or-nothing masking — the kernel disables IRQs at the CPU during its ISR. Fine for v1; a nested model would need priority and preemption support in the trait.

### Neutral

- `IrqNumber` is a newtype over `u32`, giving type-distinct signatures without sacrificing size. 4 billion lines is beyond any realistic hardware.
- `enable` / `disable` are infallible. A caller passing an unimplemented IRQ number gets BSP-defined behaviour (typically a no-op write to a register that the controller ignores).

## Pros and cons of the options

### Option A — fused closure

- Pro: caller cannot forget EOI.
- Con: closure crosses the ISR boundary; harder to write explicit assembly handlers.
- Con: nested interrupt support would require re-entrant closures.
- Con: unusual pattern for kernel interrupt dispatch.

### Option B — split acknowledge / EOI (chosen)

- Pro: direct mapping to entry / exit ISR halves.
- Pro: nested interrupts remain possible (future extension).
- Pro: matches every kernel's ISR structure.
- Con: caller can forget EOI. Mitigation: review discipline + a future `IrqGuard`-style helper if a recurring bug appears.

### Option C — split Priority Drop / Deactivation

- Pro: most faithful to GICv3.
- Con: redundant for GICv2 and for GICv3 when we do not use split mode.
- Con: leaks architectural detail the kernel does not need.

### Option D — `IrqHandle` with `Drop`-performs-EOI

- Pro: cannot forget EOI.
- Con: `Drop` performing hardware operations is unusual; behaviour during unwind / panic would need careful thought.
- Con: kernel ISR code typically does not want RAII in the hot path.

## Open questions

Each is a future ADR.

- **`clear_pending(irq)`** for edge-triggered devices that need to discard stale state.
- **Priority configuration** — a method or a separate trait?
- **Edge vs. level type configuration** — per-line, or only settable at BSP init?
- **Per-CPU routing for SPIs** — extend `enable` with a `CoreId` argument, or introduce a `route(irq, core)` method?
- **SGI / IPI primitives** — inter-processor notification surface. Cross-cutting with multi-core Cpu work.
- **Active-IRQ queries** — `active(irq) -> bool`, `pending(irq) -> bool`. Diagnostic / debug use.
- **Nested interrupt support** — requires priority, preemption, and a re-entrant ISR model.
- **LPIs (GICv3 locality-specific peripheral interrupts)** — virtualization-oriented; out of scope.
- **MSI / message-signalled interrupts** — PCI-adjacent; not relevant to initial targets.

## References

- [ADR-0006 through ADR-0010](.) — prior HAL ADRs.
- [`docs/architecture/hal.md`](../architecture/hal.md) — architectural role.
- ARM *GIC Architecture Specification* (GICv3 and GICv4.1) — `ICC_IAR1_EL1`, `ICC_EOIR1_EL1`, `ICC_DIR_EL1`, ISENABLER / ICENABLER.
- ARM *GIC-400 Technical Reference Manual* (Pi 4's GICv2 implementation).
- RISC-V Privileged Architecture — PLIC specification (future second implementation).
- Linux `irqchip` drivers — prior art.
- seL4 interrupt model — notification-delivery discipline.
