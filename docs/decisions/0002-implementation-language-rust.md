# 0002 — Rust as the implementation language

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

Tyrne is a new, security-first microkernel (see [ADR-0001](0001-microkernel-architecture.md)). Kernel code is the highest-stakes code in the system: it runs in privileged mode, it touches hardware directly, and any memory-safety bug is potentially a full compromise. The implementation language is therefore not just an aesthetic choice but a security and maintainability decision.

The question is: in what language is Tyrne written?

## Decision drivers

- **Memory safety without a garbage collector.** Kernel code cannot tolerate GC pauses and must be able to manage its own memory, but the memory-safety track record of C / C++ in kernels (Linux, Windows, macOS) is poor.
- **Ability to target `no_std`.** The language must run without a standard library runtime, on bare metal, with no assumed heap.
- **Strong type system to model capabilities.** Capabilities, in a capability-based OS, benefit enormously from a linear / affine type treatment. The language should make it natural to express move-only, non-copyable tokens.
- **Compile-time elimination of whole bug classes.** Use-after-free, double-free, data races, buffer overflows — the language should make these categorical rather than recurring defects.
- **Embedded-class toolchain.** Cross-compilation to aarch64 and RISC-V must be a first-class, stable flow.
- **Ecosystem.** Crates for `cortex-a`, `aarch64-cpu`, `x86_64`, `volatile-register`, `embedded-hal`, `bootloader`, `qemu-exit`, etc. reduce bootstrap effort.
- **Maintainability by a small team plus AI agents.** The language should be legible to AI agents and amenable to linting/formatting/static analysis.
- **Verifiability or at least strong reasoning support.** We do not commit to formal verification now, but the language should not foreclose it.

## Considered options

1. **C.** The historical default for OS kernels.
2. **C++.** C with richer abstractions, widely used in some kernels (e.g., Fuchsia).
3. **Rust.** Modern systems language with ownership and borrow checking.
4. **Zig.** Small, explicit systems language with first-class cross-compilation and comptime.
5. **Ada / SPARK.** A high-assurance language with a verification dialect; used in safety-critical embedded domains.

## Decision outcome

**Chosen: Rust.**

Rust is the only option that simultaneously satisfies the memory-safety, `no_std`, ecosystem, and cross-compilation drivers in a way that is shippable by a small team today. The ownership model maps directly onto capability semantics (move-only tokens for unforgeable authority). `unsafe` is a local escape hatch rather than the default, which matches Tyrne's philosophy of confining risk.

Rust's ecosystem for bare-metal targets — `aarch64-cpu`, `cortex-a`, `cortex-m`, `riscv`, `embedded-hal`, `volatile-register`, QEMU integration — is mature and active. Multiple production microkernels in Rust exist (Hubris, Theseus, Redox), which de-risks the language choice.

C was rejected because its memory-safety history in kernels is the problem we are trying to escape. C++ was rejected because its added features (exceptions, RTTI, complex templates) bring new surface without addressing the core memory-safety problem. Zig is promising but its ecosystem for our target profile is still thinner, its compiler is not yet at 1.0, and its safety story (explicit but not compiler-enforced) is weaker than Rust's. Ada/SPARK is the strongest choice for formal verification specifically, but its toolchain, ecosystem, and AI-agent familiarity are all significantly thinner than Rust's; it is a better second-pass option if Tyrne commits to verification later.

## Consequences

### Positive

- Whole categories of kernel bugs (use-after-free, data races, buffer overflows) are eliminated at compile time for all safe code.
- Capability tokens map naturally to move-only types; borrowing maps to temporary capability loans.
- `no_std` is a first-class mode; the language does not assume a runtime.
- Cross-compilation to aarch64 / RISC-V / x86_64 is a stable, well-documented flow via `rustup target`.
- The ecosystem has ready crates for CPU primitives, MMIO, interrupt handling, and QEMU workflows.
- AI agents generally produce higher-quality Rust than higher-quality C, because the compiler catches a superset of errors the agent might miss.
- The language is evolving actively with explicit attention to safety-critical and embedded use.

### Negative

- **Nightly features needed for some kernel work.** Inline assembly, certain intrinsics, `alloc_error_handler`, and a few lang items still require nightly toolchain. Mitigation: pin a specific nightly via `rust-toolchain.toml` and track stabilization.
- **Learning curve for contributors unfamiliar with ownership.** Mitigation: ADRs, glossary, guides, and code style documents explain the idioms. The maintainer is committed to Rust and will backstop this.
- **Unsafe is still present and still dangerous.** Mitigation: see the unsafe-policy standard (planned) — every `unsafe` block has a justification, an invariant statement, and an audit entry.
- **Compile times.** Larger than C. Not catastrophic but noticeable.
- **Some older hardware targets lack good Rust support.** Tyrne's target list already avoids these.

### Neutral

- The language choice is visible in hiring/contribution patterns: Rust attracts a particular cohort. Neutral in that this cohort overlaps substantially with the security-minded contributor profile Tyrne wants.

## Pros and cons of the options

### C

- Pro: ubiquitous; every OS literature example is in C.
- Pro: simplest possible toolchain.
- Con: memory safety is a manual property; kernel CVEs bear out the cost.
- Con: no linear types — capabilities would be modeled conventionally, losing compile-time guarantees.

### C++

- Pro: richer than C, still pervasive.
- Pro: used by Fuchsia/Zircon, so modern microkernel precedent exists.
- Con: does not solve the memory-safety problem.
- Con: exceptions, RTTI, template-heavy styles add surface without value in a kernel.

### Rust

- Pro: memory safety, ownership, `no_std`, mature ecosystem, strong AI-agent fit.
- Pro: confirmed workable for microkernels (Hubris, Theseus, Redox).
- Con: nightly still needed for some kernel features.
- Con: learning curve for ownership.

### Zig

- Pro: explicit, first-class cross-compilation, comptime.
- Pro: simpler than Rust, smaller surface.
- Con: not 1.0; less stability commitment than Rust.
- Con: safety is explicit rather than enforced; weaker guarantee than Rust's borrow checker.

### Ada / SPARK

- Pro: strongest formal verification story of any production language.
- Pro: used in safety-critical aerospace / rail.
- Con: ecosystem and tooling thinner for bare-metal ARM hobbyist-scale targets.
- Con: AI-agent familiarity low; every generation needs heavy human review.

## References

- Oxide Computer, Hubris (Rust microkernel): https://hubris.oxide.computer/
- Theseus OS: https://www.theseus-os.com/
- Redox OS: https://www.redox-os.org/
- Rust Embedded Book: https://docs.rust-embedded.org/book/
- Rust RFC: Kernel work (various)
- Levy et al., Tock (Rust microkernel for embedded): https://www.tockos.org/
