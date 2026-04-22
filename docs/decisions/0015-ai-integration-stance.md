# 0015 — AI integration stance: userspace-only, kernel-neutral

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

The project has been asked, more than once, whether AI / LLM integration belongs inside the kernel. Three candidate directions surfaced in a design-review conversation: a maximalist "AI-native OS" with an LLM in a kernel-adjacent Exception Level; a moderate "semantic microkernel" with intent-resolving IPC; and a classical microkernel with no AI claim at all. Each resembles a different kernel genre.

Tyrne today is close to the third — a capability-based microkernel with explicit principles against proprietary blobs ([P7](../standards/architectural-principles.md#p7--no-proprietary-blobs)) and with a smallest-defensible-TCB commitment ([P2](../standards/architectural-principles.md#p2--smallest-defensible-trusted-computing-base)). These principles already make "AI in the kernel" architecturally incompatible, but the incompatibility has never been written down. When the question is asked again in six months (or by an AI agent reading the repo cold), the project should have an explicit answer rather than a derived one.

This ADR is that answer. It records **why AI in the kernel is out of scope**, what **userspace AI workloads** Tyrne will be ready to host, and which **hooks the kernel and HAL deliberately leave open** so a future Phase J can build an AI-native userspace layer without changing the kernel's shape.

## Decision drivers

- **TCB minimization ([P2](../standards/architectural-principles.md#p2--smallest-defensible-trusted-computing-base)).** Even a small LLM runtime (weights, tokenizer, inference graph) is measured in tens to hundreds of megabytes. Pulling any of it into the kernel's TCB inverts the project's defining ratio — the kernel core is under 10 000 lines of Rust; an AI runtime is larger than everything else combined.
- **Determinism.** Kernel-mode operations (scheduling, IPC, capability checks) require predictable latency. LLM inference is inherently non-deterministic in both output and timing; an interrupt handler or syscall that invokes inference is not a kernel.
- **Side-channel surface.** LLM inference touches cache, branch predictors, and microarchitectural state in ways that are hard to bound. Putting it on the privileged side of the trust boundary exposes every other privileged operation to its side-channel leakage.
- **Prompt injection at the kernel boundary.** An "intent-understanding" IPC or syscall surface turns userspace input into a natural-language prompt for the kernel. The adversarial-prompt problem is an open research frontier in userspace; proposing to solve it inside a kernel is premature.
- **Formal verification path.** [ADR-0001](0001-microkernel-architecture.md) preserves a formal-verification direction for core kernel primitives. LLMs and their runtimes cannot be verified to the same standard; letting them into the kernel forecloses that path.
- **No proprietary blobs ([P7](../standards/architectural-principles.md#p7--no-proprietary-blobs)).** Trained model weights are not open source in any meaningful sense (the training data and process are typically not available, and the weights themselves are often under non-OSI licenses). Linking weights into a privileged image is a blob by any reasonable definition.
- **But AI-aware userspace has real value.** Smart-home deployments benefit from on-device inference (anomaly detection on sensor streams), semantic indexing (search over documents by meaning), intent analysis (higher-level policy atop capabilities), and TEE-backed confidential inference. These are all legitimate features — just not of the kernel.
- **Not foreclosing the future.** Tyrne should not have to be rewritten to host an AI-native userspace layer later. The kernel and HAL leave specific hooks open so that, when the ecosystem matures, a Phase J can add the layer without structural change.

## Considered options

### Option A — full AI in the kernel

Embed an LLM (or tokenizer + small model + TEE-backed weights) into the kernel's trust boundary. Expose natural-language syscalls, intent-resolving IPC, and AI-driven scheduling at the privileged layer.

### Option B — semantic microkernel (AI-adjacent kernel services)

Keep the kernel Rust-only and small, but introduce "semantic" services — intent-resolving IPC, NL-aware `open`, AI-driven scheduling hints — as first-class, trusted userspace services wired into the kernel's fast path.

### Option C — AI-ready kernel, userspace-only AI (chosen)

Kernel remains strictly AI-neutral. AI workloads run exclusively in userspace on top of Tyrne's normal capability-mediated services. The kernel and HAL deliberately provide a small number of **hooks** that enable a rich AI-userspace layer to be built later:

- **TEE support in the HAL** (landing in Phase G alongside measured boot).
- **NPU driver trait** in `tyrne-hal` when the first NPU target is selected (Phase H or later).
- **Userspace → scheduler hint** syscall (a narrow, bounded interface for advisory priority hints; does not allow userspace to dictate scheduling, only to inform it).
- **Capability intent-extension point** — a userspace policy layer can interpose on capability invocations and apply semantic checks before the kernel's structural check proceeds.

Tyrne's long-horizon roadmap gains a dedicated **Phase J** for the AI-native userspace layer; detail stays light until its turn.

### Option D — classical microkernel with no stated AI position

Stay close to the current ADRs and hope the topic does not resurface. Stateless on AI integration.

## Decision outcome

**Chosen: Option C.**

The kernel is AI-neutral. No LLM, no inference, no intent-resolution, no semantic IPC, no natural-language syscalls — inside the kernel's trust boundary. The kernel and HAL provide exactly four hooks, each gated by its own ADR at the time of implementation, that keep a future AI-userspace layer feasible without foreclosing it.

### The four hooks

1. **TEE support (confidential compute).**
   - Purpose: makes it possible to run integrity-sensitive userspace code — including, later, inference services with private weights or private inputs — in a hardware-backed isolated region.
   - Scope: a `hal::tee` trait (name tentative) plus per-BSP implementations where the hardware offers ARM TrustZone or equivalent.
   - Timing: lands in Phase G (Security maturation), alongside measured boot.
   - Non-AI value: confidential boot attestation, secrets protection, high-assurance crypto regardless of whether AI is ever wired in.

2. **NPU driver trait.**
   - Purpose: abstracts neural-accelerator hardware so an AI-userspace service can request inference from open NPUs (Rockchip NPU, Hailo, Google Coral, future ARM Ethos-U) without proprietary kernel code.
   - Scope: a trait in `tyrne-hal` (or a sibling crate if the surface diverges from the current HAL shape).
   - Timing: when the first NPU target is committed to (Phase H or later; not a Phase A–C concern).
   - Non-AI value: none, strictly — but costs nothing to leave open as a future trait.
   - Constraint: no proprietary kernel blob. If a target NPU requires closed firmware at the kernel level, that target is out of scope per [ADR-0004](0004-target-platforms.md) + [P7](../standards/architectural-principles.md#p7--no-proprietary-blobs).

3. **Userspace → scheduler hint syscall.**
   - Purpose: a userspace service (eventually an AI-driven one) can provide *advisory* information to the scheduler — expected-wait-for-IO class, latency-sensitivity tier, predicted burstiness.
   - Scope: a single narrow syscall. The kernel's scheduler is the sole authority; the hint is purely advisory.
   - Timing: Phase C or later, once the preemptive scheduler exists. The hint interface itself is a small ADR at the time.
   - Non-AI value: even without AI, a profiling / monitoring userspace service can use the hint syscall.

4. **Capability intent-extension point.**
   - Purpose: a userspace policy task can be granted a capability to observe other tasks' capability invocations and apply additional checks (semantic policy, rate limiting, suspicious-pattern detection) *before* the kernel's structural check runs.
   - Scope: expressed through the capability system itself — a capability conveying the "policy-authority" right. No kernel-level special-casing.
   - Timing: once the capability system is mature and a real policy use-case arrives (Phase E or later).
   - Non-AI value: generalizes to all policy layers, not just AI ones.

Each hook is a **placeholder**: it imposes no engineering cost today. Hooks turn into concrete ADRs at their implementation time; if any hook turns out to be wrong or unneeded, dropping it is not painful because it does not exist yet.

### Phase J

Tyrne's roadmap gains Phase J — **AI-native userspace layer** — at the long-horizon end. Its shape is intentionally sketchy until it becomes current; likely milestones include a userspace inference runtime, a semantic file indexer, an intent-analysis policy service, a natural-language shell alternative, and an on-device LLM service backed by TEE. See [`docs/roadmap/phases/phase-j.md`](../roadmap/phases/phase-j.md).

### What this ADR explicitly rejects

So that a future reader does not have to ask:

- **No LLM in any privileged code path.** Not in the scheduler, not in IPC, not in syscall dispatch.
- **No natural-language syscalls.** `open("yesterday's meeting notes")` is a userspace search-service operation, not a kernel call.
- **No "semantic IPC."** IPC exchanges bytes and capabilities; the kernel does not interpret payload meaning.
- **No neural scheduler.** The scheduler is deterministic; userspace hints are advisory.
- **No closed-source model weights in the kernel image.** Per [P7](../standards/architectural-principles.md#p7--no-proprietary-blobs).
- **No intent-resolving kernel services** — intent resolution lives entirely in userspace.

## Consequences

### Positive

- **All existing architectural properties preserved.** Small TCB, deterministic kernel, auditable `unsafe`, formal-verification path, no-blob posture.
- **Clear boundary for future AI work.** Phase J has a dedicated slot; the four hooks name the integration points.
- **Decision is documented.** A returning maintainer, a reviewer, or an AI agent reading the repo cold sees the answer without re-deriving it.
- **Non-AI benefits of the hooks stand alone.** TEE support is useful for boot attestation and confidential services regardless of AI. The intent-extension point generalizes to any policy layer. The scheduler-hint syscall helps profilers. The NPU trait slot costs nothing.
- **Composability.** An AI-native userspace layer can be added later by anyone — the maintainer, a contributor, a downstream fork — without changing the kernel.

### Negative

- **AI features arrive later than in an AI-native alternative.** If someone else's OS ships first with LLM-driven features, Tyrne is behind on that dimension. Accepted — we optimize for durability and auditability, not first-mover.
- **Some users may want AI-integrated OS experience now.** Those users are out of current scope; the target audience is people who value security, auditability, and device ownership over LLM-powered conveniences.
- **The four hooks must be maintained.** Each is one open question that becomes an ADR at the right time; letting them drift into the kernel accidentally (e.g., a scheduler hint that grows into a full policy API) would breach the boundary. Mitigation: each hook's future ADR must explicitly re-confirm the kernel-neutral posture.

### Neutral

- **Supersession is possible.** If the ecosystem shifts — LLMs become formally verifiable, inference runtimes shrink to kernel size, side channels are solved — a future ADR can supersede this one. The path is not foreclosed; it is deferred.
- **Phase J is a placeholder.** It may be renumbered, split, or deprecated later as its substance grows.

## Pros and cons of the options

### Option A — full AI in the kernel

- Pro: most visible and differentiating feature story.
- Con: violates P2, P7; breaks formal-verification path; non-deterministic kernel; prompt-injection at the privileged boundary; unauditable model weights; incompatible with the entire current architecture.
- Con: incompatible with smart-home reliability — a device that cannot boot because an LLM inference fails is unacceptable firmware.
- Con: requires a from-scratch rewrite of 14 existing ADRs.

### Option B — semantic microkernel (AI-adjacent kernel services)

- Pro: bounded LLM blast radius (in a trusted userspace service, not the kernel proper).
- Con: "trusted userspace" with first-class fast-path access to kernel internals is TCB expansion by another name.
- Con: non-determinism still bleeds into call-site timing everywhere the semantic service is on the path.
- Con: creates a privileged-userspace class that violates the uniform capability model.

### Option C — AI-ready kernel, userspace-only AI (chosen)

- Pro: zero cost today; concrete hooks tomorrow.
- Pro: preserves every current architectural guarantee.
- Pro: the four hooks' non-AI value covers their maintenance cost.
- Con: "AI-native" messaging is not Tyrne's story. Accepted.

### Option D — no stated position

- Pro: no new document.
- Con: the question resurfaces quarterly and costs hours of re-deriving the answer.
- Con: silence is read as "not decided," which is not the truth.

## Open questions

- **TEE target selection.** Which hardware offers a TEE we can realistically use first? ARM TrustZone on Pi 4 is possible but configured in a non-standard way by the Pi firmware; Jetson has its own chain; QEMU can emulate. A Phase G ADR picks.
- **First NPU target.** Rockchip NPUs are the most open; Hailo is available; Google Coral exists. The first to be engineered for is an open question.
- **Scheduler-hint shape.** A single integer tier? A structured hint? An open-ended key-value pair? The first use-case shapes the first version.
- **Intent-extension representation.** What does "observe capability invocations" look like as a capability? A bystander endpoint? A hooked IPC path? Real design needed when the first policy use-case lands.
- **Phase J's first milestone.** Semantic file indexer is the obvious candidate (concrete, testable, valuable), but other candidates (userspace inference runtime, intent policy) are possible.

## References

- [ADR-0001: Capability-based microkernel architecture](0001-microkernel-architecture.md).
- [ADR-0004: Target hardware platforms and tiers](0004-target-platforms.md).
- [ADR-0013: Roadmap and planning process](0013-roadmap-and-planning.md).
- [Architectural principles](../standards/architectural-principles.md) — especially P1, P2, P6, P7.
- [`security-model.md`](../architecture/security-model.md) — the model AI-in-kernel would violate.
- [`docs/roadmap/phases/phase-g.md`](../roadmap/phases/phase-g.md) — TEE hook lands here.
- [`docs/roadmap/phases/phase-j.md`](../roadmap/phases/phase-j.md) — the AI-native userspace layer.
- Azure Confidential Computing — prior art for TEE-backed inference as a userspace service.
- Apple Private Cloud Compute — prior art for architectural isolation of inference.
- Fuchsia / Zircon component model — prior art for OS-level AI-aware userspace composition.
