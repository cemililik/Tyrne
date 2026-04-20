# Phase B — Real userspace

**Exit bar:** A userspace task (a real separate binary, not a kernel-level stub) runs in its own address space, with its own capability table, and can make syscalls back into the kernel.

**Scope:** Drop to EL1, activate the MMU with a kernel mapping, introduce per-task address spaces, build a task loader, define the syscall entry / dispatch, run the first userspace "hello world" in EL0. Still single-core; Pi 4 is Phase D; drivers are Phase E.

**Out of scope:** Multi-core, real hardware, userspace drivers, network, filesystem.

---

## Milestone B1 — Drop to EL1 in boot

Extend the BSP reset stub so that when QEMU delivers us at EL2, we configure `HCR_EL2`, `SPSR_EL2`, `ELR_EL2`, and issue `ERET` to land in EL1. When QEMU delivers at EL1, the stub is a no-op on that axis.

### Sub-breakdown

1. **ADR-0021 — EL drop policy.** Always-to-EL1 vs. keep-whichever. When does the drop happen (earliest possible; before kernel_main). How do we handle the case where the drop fails (panic).
2. **Asm extension** in `bsp-qemu-virt/src/boot.s` for EL2→EL1 transition.
3. **Rust helpers** for reading current EL (`CurrentEL` system register); probably a new method on `Cpu` or a free function.
4. **Tests** — boot at EL1 under QEMU (default) and at EL2 (via `-machine virtualization=on`) and verify both land at EL1 in `kernel_entry`.

### Acceptance criteria

- ADR-0021 Accepted.
- Kernel boots at EL1 in all QEMU configurations we care about.
- Smoke test boots both QEMU variants and asserts the greeting still appears.

---

## Milestone B2 — MMU activation (kernel-half mapping)

Turn on the MMU with an identity map for the kernel image region and its stack. This is the foundation that per-task address spaces will layer atop.

### Sub-breakdown

1. **ADR-0022 — Kernel virtual memory layout.** Identity at `0x4000_0000` vs. high-half split; memory type attributes (normal cached for RAM, device-nGnRnE for MMIO). Mapping-mutation calls on the [`Mmu`](../../../hal/src/mmu.rs) trait should return a **typed "must-acknowledge" flush token** (analogous to `x86_64::structures::paging::MapperFlush`): each mutation produces a token that must be either explicitly `.flush()`-ed (executes the required TLB invalidation) or `.ignore()`-ed. Silent drop produces a compile-time warning. This keeps "did you remember to flush?" out of the reviewer's head and into the type system.
2. **Physical frame allocator** — a minimal bitmap or free-list allocator in the kernel. Needed before page tables can be populated.
3. **Initial page-table construction** — kernel mappings for `.text`, `.rodata`, `.data`, `.bss`, stack; MMIO mappings for the active UART and GIC.
4. **MMU activation sequence** — the exact `TTBR`, `TCR`, `MAIR`, `SCTLR` writes and the required barriers.
5. **Physical-frame capability (`MemoryRegionCap`) first real use.** Wires the capability system to actual memory.
6. **Tests** — kernel survives MMU activation (post-MMU `write_bytes` still produces greeting); deliberate invalid access traps (for exception-handler work, which lands here or in B5).

### Acceptance criteria

- ADR-0022 Accepted.
- Kernel runs with the MMU on.
- Physical frame allocator has host-tested correctness and a QEMU integration smoke.
- Deliberate traps route through the exception-vector table.

### Informs

B3 builds per-task address spaces on top of this. B5 (syscall trap) reuses the exception-vector work.

---

## Milestone B3 — Address space abstraction

Multiple per-task translation tables. Capability-gated map / unmap. Activation on context switch (tie-in to A5's context switch).

### Sub-breakdown

1. **ADR-0023 — Address-space data structure.** How a BSP-specific `AddressSpace` is represented; who owns its page tables; how it integrates with the `Mmu` trait's associated type.
2. **`AddressSpace` kernel object** — a new kernel-object type, like those from A3, with `AddressSpaceCap`.
3. **Map / unmap operations** — wrappers around [`Mmu::map`](../../../hal/src/mmu.rs) / `Mmu::unmap` that validate the caller's capabilities.
4. **TLB invalidation on unmap** — single-core only; multi-core is Phase C.
5. **Activation on context switch** — the context-switch path invokes [`Mmu::activate`](../../../hal/src/mmu.rs) when crossing between tasks with different address spaces.
6. **Tests** — isolation between two address spaces (a map in AS-X is not visible in AS-Y); activation round-trip.

### Acceptance criteria

- ADR-0023 Accepted.
- Two address spaces coexist; the kernel activates each when its owning task runs.
- Isolation verified on QEMU: AS-X cannot read AS-Y's data.

---

## Milestone B4 — Task loader

Load a userspace binary into an address space. For B4 the binary is statically embedded in the kernel image (e.g., `include_bytes!`); the filesystem / dynamic loading comes later.

### Sub-breakdown

1. **ADR-0024 — Initial userspace image format.** Raw flat binary vs. minimal ELF subset. v1 favours raw flat (simplest).
2. **Loader** — maps the embedded binary into a fresh address space under its `MemoryRegionCap`, sets up the initial stack, marks the entry point.
3. **Task creation from a binary** — `task_create_from_image(image, as_cap, initial_caps) -> TaskCap`.
4. **Tests** — host-side loader correctness (given an image blob, produce the expected mapping); QEMU-side task creation without yet running the task (that's B6).

### Acceptance criteria

- ADR-0024 Accepted.
- A kernel test can load the embedded userspace image into an address space and report the entry point and initial stack pointer.

---

## Milestone B5 — Syscall boundary

Traps from EL0 into EL1 via `SVC` (or the chosen mechanism). Syscall dispatch validates the caller's capabilities. Establish the initial syscall set and the calling convention.

### Sub-breakdown

1. **ADR-0025 — Syscall ABI.** Register calling convention (which regs carry syscall number vs. arguments vs. return); maximum arg count; error-return convention (register + flag vs. Result-like encoding); asynchronous vs. synchronous semantics.
2. **ADR-0026 — Initial syscall set for B-phase.** At minimum: `send`, `recv`, `console_write` (debug-gated), `task_yield`, `task_exit`. No more in v1.
3. **Exception-vector dispatch** — the EL0-synchronous vector routes to a Rust syscall dispatcher after saving user registers.
4. **Syscall dispatcher** — maps a syscall number to a handler, validates capabilities, performs the operation, returns.
5. **Copy-from / copy-to user** — validated access to userspace memory through the active address space. No raw dereferencing of user pointers.
6. **Tests** — host-side ABI encoder/decoder tests; QEMU smoke where a kernel-stub "userspace" makes a syscall.

### Acceptance criteria

- ADR-0025 and ADR-0026 Accepted.
- Syscall entry works from EL0 back to EL1 and back; register state is preserved correctly.
- Invalid syscalls (bad number, missing capability, out-of-bounds pointer) return errors without panicking.
- Copy-from-user never dereferences raw user pointers outside the validated mapping.

---

## Milestone B6 — First userspace "hello"

A real userspace task, loaded by B4, running in EL0 in its own address space, makes a `console_write` syscall, and exits cleanly via `task_exit`.

### Sub-breakdown

1. **Userspace "hello" program** — a minimal `no_std, no_main` binary living in `userland/hello/` (new crate) that calls the syscall ABI directly.
2. **Wire-up** — kernel loads this binary on boot via B4, creates a task in its AS (via B3), schedules it (via A5), runs it (via B1/B2/B5).
3. **Syscall library** — a small `umbrix-user` crate exposing safe wrappers for the B5 syscalls.
4. **QEMU smoke** — trace shows kernel greeting + userspace greeting in correct order + task_exit + kernel shutdown message.
5. **Business review** — Phase B retrospective.

### Acceptance criteria

- Userspace "hello from userspace" appears on the serial console after the kernel's greeting.
- Userspace can call `task_exit` cleanly; the kernel reports task termination.
- Guide: `docs/guides/first-userspace.md` explains what this demonstrates.

### Phase B closure

When B6 is Done, run a business review. Phase C becomes active after that review.

---

## ADR ledger for Phase B

| ADR | Purpose | Expected state |
|-----|---------|----------------|
| ADR-0021 | EL drop policy | B1 |
| ADR-0022 | Kernel virtual memory layout | B2 |
| ADR-0023 | Address-space data structure | B3 |
| ADR-0024 | Initial userspace image format | B4 |
| ADR-0025 | Syscall ABI | B5 |
| ADR-0026 | Initial syscall set | B5 |

## Open questions carried into Phase B

- Choose between ELF and a raw binary for the initial userspace image (ADR-0024).
- Decide whether syscalls are synchronous-only or also expose asynchronous variants from the start (ADR-0025).
- Determine whether the initial userspace lives in its own binary target or is embedded as bytes via `include_bytes!` (ADR-0024).
- Exception-handler strategy and whether fault messages go through the capability system or a special-case path.
