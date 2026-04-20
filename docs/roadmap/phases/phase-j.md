# Phase J — AI-native userspace layer

**Exit bar:** An optional AI-native userspace layer runs on top of Umbrix, providing semantic indexing, intent-aware policy, and on-device inference — all above the kernel's trust boundary, with the kernel unchanged.

**Scope:** The dedicated home for AI-integrated features, mandated by [ADR-0015](../../decisions/0015-ai-integration-stance.md). Umbrix ships AI-ready (TEE support, NPU HAL trait, scheduler-hint syscall, capability intent-extension point) and this phase is where the userspace layer that uses those hooks is built. Everything here lives above the kernel and is opt-in for a deployment.

**Out of scope:** AI inside the kernel (forbidden by ADR-0015). AI as a dependency of any Phase A–I feature — Umbrix must be fully usable without Phase J ever running.

**Timing:** Long-horizon. This phase is sketch-level until its turn approaches. Detail grows as Phase I completes, as the AI landscape matures, and as specific deployments demand the features described here.

---

## Milestone J1 — Userspace inference runtime

A userspace service that loads a small model (ONNX / GGUF / a custom format) and runs inference on behalf of capability-holding clients. TEE-backed when the hook from Phase G is active.

### Likely sub-breakdown

1. **ADR — runtime choice.** Hand-rolled vs. port of an existing no-std-compatible crate (Candle, Burn subset, tract subset).
2. **Model-format ADR.** GGUF has the most community momentum; ONNX has the broadest tool support; a custom format has the smallest footprint. Pick when a concrete use-case lands.
3. **Inference service task** — a userspace task holding the NPU capability (if any) and serving inference requests on a capability-gated endpoint.
4. **Sandboxing story** — the inference task's failure domain.

## Milestone J2 — Semantic file indexer

A userspace indexer that produces embeddings for files a client has granted read capability to, maintains a vector store, and serves semantic-search queries on a capability-gated endpoint.

### Likely sub-breakdown

1. **ADR — indexer scope.** Which file types, how embeddings are updated, privacy posture (embeddings may leak content).
2. **Vector store choice.** HNSW / flat / product-quantized; all-in-memory for v1.
3. **Indexer task** — watches file-system events through a capability, computes embeddings via the J1 inference runtime, stores, serves.

## Milestone J3 — Intent-analysis policy service

A userspace policy task that receives notifications of capability invocations (via the intent-extension point from the capability system) and applies semantic checks — rate limits, pattern matching, LLM-driven anomaly detection — before the kernel's structural check completes.

### Likely sub-breakdown

1. **ADR — policy-service interface.** What capability grants policy authority; how invocations are observed; what the policy service can do (allow / deny / delay / alert).
2. **Default policies** — a small set of audit-friendly rules that ship by default.
3. **LLM-driven extension** — optional; uses J1 inference for natural-language policy expression.

## Milestone J4 — Natural-language shell

An alternative shell running alongside the primary shell (not replacing it). Parses natural-language commands into Umbrix service calls.

### Likely sub-breakdown

1. **ADR — shell scope.** How ambitious; what operations are reachable through NL vs. structured form.
2. **Shell task** — a userspace binary wrapping J1 inference.
3. **Reliability disclaimer** — NL-shell cannot be used for automation; guide makes this explicit.

## Milestone J5 — Scheduler-hint daemon

A userspace daemon that reads system metrics (via capability-granted observation endpoints), applies a simple model, and emits scheduler hints through the hint syscall from the kernel. Advisory only; the scheduler remains authoritative.

### Likely sub-breakdown

1. **Metrics collection task.**
2. **Hint emission** via the scheduler-hint syscall.
3. **Evaluation** — does the daemon actually improve any measurable metric vs. the kernel's unhinted scheduler? If not, the daemon is archived.

## Milestone J6 — On-device LLM with TEE backing

Combines J1 + TEE support from Phase G into a confidential inference service. A userspace task loads weights into a TEE-protected region and serves queries that never expose the weights or the inputs to the rest of the system.

### Likely sub-breakdown

1. **ADR — TEE-inference integration.** How weights enter the enclave; how queries flow; how the enclave communicates with non-enclave userspace through capabilities.
2. **Reference model** — a small, open-weight model.
3. **Attestation** — the TEE's measured-boot evidence flows into the service's capability discipline.

---

## Prerequisites and hooks

Phase J assumes the four hooks from [ADR-0015](../../decisions/0015-ai-integration-stance.md) are in place. Each hook's concrete implementation ADR lands at the phase that delivers it, not here:

| Hook | Source phase | Required for |
|------|--------------|--------------|
| TEE support in the HAL | Phase G (G1 / G1.5) | J6 |
| NPU driver trait | Phase H (when first NPU target lands) | J1 (optional acceleration) |
| Userspace → scheduler hint syscall | Phase C or later extension | J5 |
| Capability intent-extension point | Phase E or later extension | J3 |

---

## ADR ledger for Phase J

All ADRs are expected to be authored when their milestone activates. None are written today.

| ADR | Purpose | Expected state |
|-----|---------|----------------|
| _(future)_ | Inference runtime choice | J1 |
| _(future)_ | Model-format choice | J1 |
| _(future)_ | Indexer scope | J2 |
| _(future)_ | Policy-service interface | J3 |
| _(future)_ | NL-shell scope | J4 |
| _(future)_ | TEE-inference integration | J6 |

---

## Open questions

- **First milestone of Phase J.** Is it J2 (semantic indexer, concrete and valuable for smart-home search), J1 (inference runtime as foundation), or J3 (policy service as a force multiplier for the capability system)? The right answer depends on what Umbrix is actually being used for at the time.
- **Opt-in packaging.** Phase J features are optional. How they are packaged so that a deployment can include them or omit them without touching the kernel is itself a small design question.
- **Supersession.** ADR-0015 may be superseded if the ecosystem shifts. Phase J's existence is conditional on ADR-0015 remaining authoritative.
- **Governance.** Who decides when Phase J begins? The answer today: the maintainer, when Phase I or a specific deployment makes it the highest-value next step.

---

## Living-document note

This file is a sketch. It will be filled in as Phase J's turn approaches — either by a future business review that declares Phase J active, or by a specific deployment that requires one of its milestones ahead of the general sequence.
