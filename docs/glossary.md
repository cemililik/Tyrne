# Glossary

Terminology used throughout Tyrne. Entries are alphabetical. If a term appears in documentation and is not obvious from general OS-development literacy, it should be listed here.

---

**ABI (Application Binary Interface).** The contract between a compiled component and the environment it runs in: calling convention, register usage, structure layout, system call numbers. ABIs are what let a binary run on another binary's output without recompilation.

**ADR (Architecture Decision Record).** A dated, numbered document recording a non-trivial decision, the context, the alternatives considered, and the consequences. Stored under [decisions/](decisions/). Tyrne uses the MADR format.

**Ambient authority.** The anti-pattern where a subject's power is determined by *who it is* or *where it runs*, rather than by capabilities it has been explicitly granted. Tyrne rejects ambient authority by design. See [ADR-0001](decisions/0001-microkernel-architecture.md).

**Arena.** A fixed-capacity slot array that backs a specific kernel-object kind (tasks, endpoints, notifications). Per [ADR-0016](decisions/0016-kernel-object-storage.md), every kernel-object type has its own arena; slots are handed out and returned without heap involvement. See also *Generation tag*.

**BSP (Board Support Package).** The concrete implementation of HAL trait surfaces for a specific board. A BSP plugs into the kernel at build time and provides drivers for on-board peripherals.

**Capability.** An unforgeable token, held by a subject (process, task, thread), that authorizes a specific operation on a specific object. In a capability-based system, *having the capability is the permission*; there is no separate access control list to consult.

**Capability-based security.** A security model where every action requires a capability, capabilities are unforgeable, capabilities can be shared but not leaked, and there is no ambient authority. See seL4, Hubris, KeyKOS, E.

**Capability rights (CapRights).** The bit-flag set attached to each capability that narrows what the holder may do with it. Tyrne's current rights (v1): `SEND`, `RECV`, `NOTIFY`, `DUPLICATE`, `DERIVE`, `REVOKE`, `TRANSFER`. See [ADR-0014](decisions/0014-capability-representation.md).

**Capability transfer.** The IPC operation in which a sender's `ipc_send` atomically removes a capability from its own table, embeds it in the message, and delivers it into the receiver's table on `ipc_recv`. If either half fails, neither table is left in an intermediate state. See [ADR-0017](decisions/0017-ipc-primitive-set.md).

**CDT (Capability Derivation Tree).** The parent/child tree of capabilities derived from one another via `cap_derive`. Revocation is transitive along this tree: revoking a parent revokes every descendant. In v1 the tree is per-table; cross-table transitivity is deferred per [ADR-0023](decisions/0023-cross-table-capability-revocation-policy.md) (Status: Deferred; conditions for reopening recorded in the placeholder body).

**`CNTFRQ_EL0`.** ARM aarch64 system register that reports the Generic Timer's counter frequency in Hz. Set by firmware before kernel entry; read at EL0/EL1 (subject to `CNTHCTL_EL2.EL1PCTEN` only in VHE mode, which Tyrne does not use). QEMU virt sets it to 62.5 MHz. On real Pi 4 / BCM2711 the rate is firmware-dependent (mainline Linux configures 54 MHz; older Pi-class boards used 19.2 MHz) — BSPs read the firmware-provided value rather than hard-coding. Tyrne reads `CNTFRQ_EL0` once at `QemuVirtCpu::new` and caches the derived `resolution_ns`.

**`CNTPCT_EL0`.** ARM aarch64 system register that exposes the 64-bit free-running **physical** counter of the Generic Timer. Monotonic by hardware contract. Tyrne does **not** read this register directly; the Timer impl reads `CNTVCT_EL0` instead so the read side stays aligned with the deferred deadline-arming side (`CNTV_*`).

**`CNTVCT_EL0`.** ARM aarch64 system register that exposes the 64-bit free-running **virtual** counter of the Generic Timer. Computed as `CNTPCT_EL0 - CNTVOFF_EL2`; on QEMU virt with `CNTVOFF_EL2 = 0` it coincides with the physical counter. Monotonic by hardware contract. Tyrne reads `CNTVCT_EL0` for `Timer::now_ns` so the read side is in the same family as the future deadline-arming registers (`CNTV_CVAL_EL0` / `CNTV_CTL_EL0`) per ADR-0010.

**Context switch.** The operation of saving the CPU state of one task and loading another so that the second task runs. Cost and frequency of context switches are the classic trade-off driver between monolithic and microkernel designs. In Tyrne the primitive is [`ContextSwitch::context_switch`](../hal/src/context_switch.rs) per [ADR-0020](decisions/0020-cpu-trait-v2-context-switch.md).

**Cooperative scheduling.** A scheduling model in which the CPU is only taken from a running task when that task voluntarily yields. Tyrne v1 is cooperative and single-core; preemption arrives later in Phase B / Phase C.

**Endpoint.** In seL4-style IPC, a kernel object used to rendezvous senders and receivers. Possessing a capability to an endpoint is what grants the right to send or receive.

**Generation tag.** The counter stored alongside every arena slot that detects stale handles. When a slot is freed and reused, its generation increments; a handle carries the generation it was issued with, so lookup can distinguish "same slot, new object" from "same slot, same object". See [ADR-0016](decisions/0016-kernel-object-storage.md).

**Generic Timer (ARM).** The standard aarch64 timekeeping subsystem: a free-running 64-bit counter exposed in two views (physical `CNTPCT_EL0` and virtual `CNTVCT_EL0 = CNTPCT_EL0 - CNTVOFF_EL2`), a firmware-reported frequency (`CNTFRQ_EL0`), and per-EL compare registers (`CNTP_*` / `CNTV_*`) that fire interrupts when the counter passes a programmed deadline. Tyrne's `Timer` HAL trait wraps the **virtual** counter half (read-only, no IRQ) in v1; the deadline-firing half is gated on GIC wiring (a future task).

**HAL (Hardware Abstraction Layer).** The set of traits and types that decouple the kernel from any specific CPU or board. A BSP implements HAL traits; the kernel depends only on the traits.

**Handle (typed).** A small value (`TaskHandle`, `EndpointHandle`, `NotificationHandle`) that identifies a specific kernel object. Internally it is a slot index + generation tag; publicly it is an opaque `Copy` type. Distinct from `CapHandle`, which is an index into a `CapabilityTable`.

**Hubris.** A Rust microkernel from Oxide Computer Company, designed for embedded management controllers. Emphasizes compile-time task definition, minimal runtime flexibility, strict memory isolation. A major inspiration for Tyrne.

**IPC (Inter-Process Communication).** The mechanism by which tasks in separate address spaces exchange data and capabilities. In microkernels IPC is the hot path and its design dominates performance.

**Kernel.** The trusted, privileged core of the operating system. In Tyrne, the kernel is deliberately small: it manages capabilities, scheduling, IPC, and memory, and does almost nothing else.

**MADR (Markdown Architectural Decision Records).** A lightweight markdown template for ADRs, with explicit sections for decision drivers, considered options, and pros/cons. Tyrne uses a slightly simplified MADR; see [decisions/template.md](decisions/template.md).

**Microkernel.** A kernel design in which only the minimum necessary mechanisms live in privileged mode: typically address spaces, threads/tasks, IPC, and scheduling. Device drivers, filesystems, and network stacks run as ordinary userspace tasks.

**Miri.** A Rust interpreter that runs tests under a model-level checker for undefined behaviour, including the Stacked Borrows aliasing rules. Tyrne runs `cargo +nightly miri test` to validate the `unsafe` surface dynamically; see [docs/analysis/reports/2026-04-23-miri-validation.md](analysis/reports/2026-04-23-miri-validation.md).

**MMU (Memory Management Unit).** The hardware that translates virtual addresses to physical addresses and enforces per-page access rights. The MMU is what makes address-space isolation possible.

**Notification.** A kernel object for asynchronous bit-OR signalling. Unlike an endpoint, a notification carries no message body — just a saturating 64-bit word into which callers set bits via `ipc_notify`. A task can wait on a notification (future work) to be woken when any bit is set. See [ADR-0017](decisions/0017-ipc-primitive-set.md).

**PSCI (Power State Coordination Interface).** The ARM standard for boot, CPU-on/off, and system reset. On aarch64 QEMU `virt` and Raspberry Pi 4, PSCI is the portable way to bring secondary cores online.

**QEMU.** An open-source machine emulator and virtualizer. Tyrne primary development uses QEMU's aarch64 `virt` machine.

**Ready queue.** The scheduler's bounded FIFO of task handles that are runnable and waiting for the CPU. Tyrne's queue capacity equals the task arena capacity, so it can never refuse an enqueue when the total task count is within the limit. See [ADR-0019](decisions/0019-scheduler-shape.md).

**Rendezvous IPC.** A synchronous IPC model where `ipc_send` and `ipc_recv` meet at an endpoint: the first caller records a waiter, the second delivers and unblocks it, both return with the transfer complete. Tyrne uses rendezvous IPC per [ADR-0017](decisions/0017-ipc-primitive-set.md).

**seL4.** A formally verified microkernel in the L4 family. Its verified correctness and capability-based design are reference points for Tyrne, even though Tyrne is not aiming for full formal verification in its first years.

**Stacked Borrows.** A model for Rust's pointer-aliasing rules that tracks a stack of tags per memory location and requires every access to present a valid tag. Violations are UB. Miri enforces Stacked Borrows; Tree Borrows is a stricter successor. Tyrne's raw-pointer bridge ([ADR-0021](decisions/0021-raw-pointer-scheduler-ipc-bridge.md)) is designed to honour Stacked Borrows.

**StaticCell.** A BSP helper in [bsp-qemu-virt](../bsp-qemu-virt/src/main.rs) that wraps `UnsafeCell<MaybeUninit<T>>` to provide write-once-at-boot, share-afterwards static storage for kernel state. It exposes `as_mut_ptr` so callers can derive raw pointers without materialising a `&mut` (see [ADR-0021](decisions/0021-raw-pointer-scheduler-ipc-bridge.md)).

**Trust boundary.** A line in the system at which assumptions about integrity, confidentiality, or availability change. Crossing a trust boundary should require an explicit capability check. Trust boundaries are drawn in [architecture/security-model.md](architecture/security-model.md).

**Unsafe (Rust).** A block of Rust code that opts out of some compiler-enforced invariants (e.g., to dereference raw pointers or call FFI). In Tyrne, every `unsafe` block is commented with justification (invariants, rejected alternatives, audit tag) per [`unsafe-policy.md`](standards/unsafe-policy.md), and tracked in the audit log.

**UnsafeCell.** Rust's primitive for interior mutability: a `&UnsafeCell<T>` is allowed to produce a `*mut T`. Tyrne's BSP uses `UnsafeCell` (usually inside a `StaticCell`) to hold kernel state in `static` storage without `static mut` aliasing hazards.

**Userspace.** Code that runs outside the kernel, with no privileged instructions and no direct access to hardware. In Tyrne, drivers, filesystems, network stacks, and services all live in userspace.
