# Glossary

Terminology used throughout Tyrne. Entries are alphabetical. If a term appears in documentation and is not obvious from general OS-development literacy, it should be listed here.

---

**ABI (Application Binary Interface).** The contract between a compiled component and the environment it runs in: calling convention, register usage, structure layout, system call numbers. ABIs are what let a binary run on another binary's output without recompilation.

**ADR (Architecture Decision Record).** A dated, numbered document recording a non-trivial decision, the context, the alternatives considered, and the consequences. Stored under [decisions/](decisions/). Tyrne uses the MADR format.

**Ambient authority.** The anti-pattern where a subject's power is determined by *who it is* or *where it runs*, rather than by capabilities it has been explicitly granted. Tyrne rejects ambient authority by design. See [ADR-0001](decisions/0001-microkernel-architecture.md).

**BSP (Board Support Package).** The concrete implementation of HAL trait surfaces for a specific board. A BSP plugs into the kernel at build time and provides drivers for on-board peripherals.

**Capability.** An unforgeable token, held by a subject (process, task, thread), that authorizes a specific operation on a specific object. In a capability-based system, *having the capability is the permission*; there is no separate access control list to consult.

**Capability-based security.** A security model where every action requires a capability, capabilities are unforgeable, capabilities can be shared but not leaked, and there is no ambient authority. See seL4, Hubris, KeyKOS, E.

**Context switch.** The operation of saving the CPU state of one task and loading another so that the second task runs. Cost and frequency of context switches are the classic trade-off driver between monolithic and microkernel designs.

**Endpoint.** In seL4-style IPC, a kernel object used to rendezvous senders and receivers. Possessing a capability to an endpoint is what grants the right to send or receive.

**HAL (Hardware Abstraction Layer).** The set of traits and types that decouple the kernel from any specific CPU or board. A BSP implements HAL traits; the kernel depends only on the traits.

**Hubris.** A Rust microkernel from Oxide Computer Company, designed for embedded management controllers. Emphasizes compile-time task definition, minimal runtime flexibility, strict memory isolation. A major inspiration for Tyrne.

**IPC (Inter-Process Communication).** The mechanism by which tasks in separate address spaces exchange data and capabilities. In microkernels IPC is the hot path and its design dominates performance.

**Kernel.** The trusted, privileged core of the operating system. In Tyrne, the kernel is deliberately small: it manages capabilities, scheduling, IPC, and memory, and does almost nothing else.

**MADR (Markdown Architectural Decision Records).** A lightweight markdown template for ADRs, with explicit sections for decision drivers, considered options, and pros/cons. Tyrne uses a slightly simplified MADR; see [decisions/template.md](decisions/template.md).

**Microkernel.** A kernel design in which only the minimum necessary mechanisms live in privileged mode: typically address spaces, threads/tasks, IPC, and scheduling. Device drivers, filesystems, and network stacks run as ordinary userspace tasks.

**MMU (Memory Management Unit).** The hardware that translates virtual addresses to physical addresses and enforces per-page access rights. The MMU is what makes address-space isolation possible.

**PSCI (Power State Coordination Interface).** The ARM standard for boot, CPU-on/off, and system reset. On aarch64 QEMU `virt` and Raspberry Pi 4, PSCI is the portable way to bring secondary cores online.

**QEMU.** An open-source machine emulator and virtualizer. Tyrne primary development uses QEMU's aarch64 `virt` machine.

**seL4.** A formally verified microkernel in the L4 family. Its verified correctness and capability-based design are reference points for Tyrne, even though Tyrne is not aiming for full formal verification in its first years.

**Trust boundary.** A line in the system at which assumptions about integrity, confidentiality, or availability change. Crossing a trust boundary should require an explicit capability check. Trust boundaries are drawn in [architecture/security-model.md](architecture/security-model.md) (planned).

**Umbra (Latin).** Shadow. The origin of the name *Tyrne* — a silent, minimal, always-present guardian.

**Unsafe (Rust).** A block of Rust code that opts out of some compiler-enforced invariants (e.g., to dereference raw pointers or call FFI). In Tyrne, every `unsafe` block is commented with justification, invariants, and alternatives considered, and tracked through the audit process defined in the standards.

**Userspace.** Code that runs outside the kernel, with no privileged instructions and no direct access to hardware. In Tyrne, drivers, filesystems, network stacks, and services all live in userspace.
