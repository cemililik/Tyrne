# 0001 — Capability-based microkernel architecture

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

Umbrix is a new operating system being built from scratch. The intended target envelope spans:

- Constrained smart-home devices (ARM Cortex-A / Cortex-M / RISC-V MCUs).
- Single-board computers (Raspberry Pi class, Jetson aarch64).
- Eventually, mobile-class aarch64 SoCs.

The overarching design commitment is **high assurance**: the OS must minimize attack surface, confine driver faults, and support capability-based access control rather than ambient authority. It must also scale in both directions — small enough to fit on resource-constrained hardware, structured enough to grow into a general-purpose system.

Because Umbrix is starting from a blank slate, the choice of kernel architecture is the most consequential decision we will make. Everything about memory management, IPC, driver model, security, portability, and even who can contribute how, descends from it.

## Decision drivers

- **Attack-surface minimization.** The smaller the privileged code, the smaller the trusted computing base (TCB) and the less surface for vulnerabilities.
- **Driver fault containment.** A buggy driver must not take down or compromise the kernel.
- **Capability-based access control.** Ambient authority makes security reasoning hard. A capability model aligns kernel primitives with the security model directly.
- **Portability across very different hardware.** The same kernel must run on a Cortex-A72 SBC and on a Cortex-M-class device with significantly different memory budgets and peripherals.
- **Rust ergonomics.** The implementation language is Rust (see [ADR-0002](0002-implementation-language-rust.md)). The architecture should make it easy to keep `unsafe` small and localized.
- **Formal verification path.** Full verification is not a near-term goal, but architectural choices should not foreclose it for core primitives (IPC, capabilities, scheduler).
- **Maintainer bandwidth.** Umbrix is being bootstrapped by a single maintainer plus AI agents. An architecture that requires a huge initial trusted computing base is more than can be reviewed at this scale.

## Considered options

1. **Monolithic kernel.** All drivers, filesystems, network stacks, and schedulers run in kernel mode in a single address space.
2. **Microkernel (capability-based, seL4 / Hubris lineage).** Only address spaces, threads/tasks, IPC, scheduling, and capability management live in kernel mode. Drivers and services run in userspace.
3. **Hybrid kernel (XNU / NT lineage).** Microkernel core with major subsystems (filesystem, network) collapsed back into kernel space for performance.
4. **Unikernel.** A single application compiled together with kernel code into one address space.
5. **Exokernel.** The kernel exports raw hardware with protection; libraries in userspace implement abstractions.

## Decision outcome

**Chosen: a capability-based microkernel in the lineage of seL4 and Hubris.**

The microkernel decision is driven primarily by the attack-surface, fault-containment, and verifiability drivers. A monolithic kernel can be made to work on all our target hardware and would be simpler to boot initially, but it fundamentally conflicts with the "high assurance" commitment: every driver bug becomes a kernel bug, and the TCB grows with every new device supported. The capability model is the second half of the decision: it gives us a single conceptual primitive ("hold a capability = may act") that threads through IPC, memory, and inter-task authority, making security reasoning uniform rather than ad-hoc.

Hubris, seL4, and Fuchsia/Zircon are the closest reference points. Hubris in particular — a Rust microkernel aimed at embedded hardware — demonstrates that the capability-based microkernel style is workable in Rust without dragging in a large runtime. Umbrix is closer to Hubris than to seL4 in initial ambition (no formal proof, less flexible task spawning), but closer to seL4 than to Hubris in long-term direction (we want dynamic task creation eventually and a verification path for core primitives).

A unikernel was rejected because Umbrix is explicitly a general-purpose OS across multiple applications. An exokernel was rejected because it pushes too much policy into userspace libraries, which multiplies inconsistency. A hybrid kernel was rejected because the performance gains historically attributed to it (late-1990s benchmarks) are far less decisive on modern hardware, and the architectural muddle damages the security story.

## Consequences

### Positive

- The trusted computing base is small — drivers, filesystems, network stacks, and most services are outside the kernel and are subject to capability checks like any other task.
- Driver bugs are contained: a faulting driver takes down its own task, not the system.
- Portability across very different hardware becomes tractable: the kernel is small, and porting effort is concentrated in the HAL plus the board-specific drivers (which are userspace).
- The capability model gives a single uniform story for security: there is no "root" to become; there are only capabilities that are either held or not.
- Rust's ownership model fits well: capabilities map to move-only tokens, and task isolation maps to separate crates with no shared `static mut`.
- The architecture leaves a verification path open for core primitives.

### Negative

- **IPC is the hot path.** Every cross-task operation crosses an address-space boundary. Mitigation: invest early in a fast, well-understood IPC primitive (likely synchronous rendezvous plus asynchronous notification, following seL4's split) and design syscalls to minimize round trips.
- **More upfront design discipline.** Monolithic kernels let you hack in a driver and see it work. Microkernels require you to design the service interface, the capabilities, and the IPC contract before you see pixels move. We accept this as a feature, not a cost.
- **Driver authors write userspace code with kernel-like constraints.** They must think about limited memory, capability hand-offs, and cooperation with a scheduler they do not control. Tooling and templates in `docs/guides/` will be needed to make this tractable.
- **Learning curve for contributors.** Capability-based thinking is less common than POSIX-style thinking. This cost is amortized by documentation (glossary, ADRs, architecture docs) and by not inviting contributions until the model is written down.

### Neutral

- Performance on feature-rich desktop workloads may trail well-tuned monolithic kernels. This is acceptable: desktop performance is not a primary driver.
- The eventual formal-verification path is not committed to in this ADR; it is preserved as an option.

## Pros and cons of the options

### Monolithic kernel

- Pro: simple to bring up; drivers see the kernel directly; fewer context switches.
- Pro: familiar to anyone who has read Linux or BSD code.
- Con: TCB grows linearly with drivers; every driver bug is a kernel bug.
- Con: capability-based security grafted on top of monolithic kernels is inconsistent and easy to bypass.
- Con: verification is effectively impossible at monolithic sizes.

### Microkernel (capability-based)

- Pro: smallest defensible TCB; driver isolation; capability model flows naturally from kernel primitives.
- Pro: scales from embedded to mobile without architectural changes.
- Pro: Rust fits the constraints of a small privileged core.
- Con: IPC cost on the hot path.
- Con: more upfront design per subsystem.

### Hybrid kernel

- Pro: pragmatic performance escape hatch.
- Pro: has real-world deployments at very large scale (macOS, Windows).
- Con: blurs the security model; "what's in the TCB?" becomes ad-hoc.
- Con: historical performance advantage is now marginal on modern CPUs with good IPC paths.

### Unikernel

- Pro: smallest possible footprint for a single application.
- Pro: strong isolation *between* unikernels on the same host.
- Con: not a general-purpose OS — wrong shape for multi-application devices.

### Exokernel

- Pro: maximum flexibility; applications pick their own abstractions.
- Con: policy duplication across libraries; consistency hard to enforce.
- Con: more research territory than production territory.

## References

- Heiser, Elphinstone et al., *"L4 Microkernels: The Lessons from 20 Years of Research and Deployment"* (2016).
- Klein et al., *"seL4: Formal Verification of an OS Kernel"* (2009).
- Oxide Computer Company, Hubris microkernel: https://hubris.oxide.computer/
- Google, Fuchsia / Zircon: https://fuchsia.dev/
- Levy et al., *"Multiprogramming a 64kB Computer Safely and Efficiently"* (Tock, 2017).
- Shapiro et al., *"EROS: A Fast Capability System"* (1999).
