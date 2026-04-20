# Phase G — Security maturation

**Exit bar:** Measured boot where hardware permits, cryptographic primitives available, TLS usable by network services, and a formal-verification pilot on a core primitive. Security posture becomes demonstrable rather than aspirational.

**Scope:** The "high assurance" claim of [ADR-0001](../../decisions/0001-microkernel-architecture.md) turns into shipped features. May overlap with Phase F if Phase F needs crypto before this phase formally starts.

**Out of scope:** Verifying the whole kernel (far horizon); hardware security modules (depends on hardware choice); certification.

---

## Milestone G1 — Measured boot

On hardware that supports it (Pi 4 via its secure-boot chain, or a future board with TPM / secure element), extend the boot flow to measure each stage.

### Sub-breakdown

1. **ADR-0046 — Boot measurement scheme.** PCR-like registers, event log, chaining algorithm (SHA-256 vs. -384), where measurements are stored.
2. **BSP integration** — measurement computed for the kernel image before `kernel_entry`; recorded somewhere the kernel can query.
3. **Verification path** — a post-boot service that reads the measurement log and compares against expected values (signed manifest).

### Acceptance criteria

- ADR-0046 Accepted.
- Measurement log produced on a supported board and inspectable from userspace.

## Milestone G1.5 — TEE support in the HAL

Introduce a Trusted Execution Environment trait in `umbrix-hal` and a BSP implementation where the hardware offers ARM TrustZone or an equivalent. This is one of the four "AI-readiness" hooks mandated by [ADR-0015](../../decisions/0015-ai-integration-stance.md); it also stands on its own for non-AI use (secrets protection, boot attestation, high-assurance crypto containment).

### Sub-breakdown

1. **ADR — TEE trait surface.** What operations (enter / exit, attested message exchange, key sealing), what errors, what capability model for granting TEE access.
2. **BSP implementation** for whichever target is chosen first (Pi 4 TrustZone via Pi firmware is plausible; QEMU has `-machine virt,secure=on`; Jetson has its own chain).
3. **Integration with G1** — boot measurements can be sealed to the TEE for attestation.
4. **Security review** per [`analysis/reviews/security-reviews/`](../../analysis/reviews/security-reviews/).

### Acceptance criteria

- TEE trait in `umbrix-hal` with at least one BSP impl.
- Security review recorded; attestation flow documented.
- Usable independently by Phase G2 (crypto) and — later — by Phase J6 (confidential inference).

### Why this pairs with G1

Measured boot and TEE are complementary: measured boot proves what was loaded, TEE protects what runs. Doing them together lets both land with a single security review and a consistent attestation model.

## Milestone G2 — Cryptographic primitives

A crypto crate with audited implementations: hash (SHA-2, SHA-3), AEAD (ChaCha20-Poly1305, AES-GCM), signature (Ed25519, ECDSA P-256), random-bytes source.

### Sub-breakdown

1. **ADR-0047 — Crypto crate choice.** RustCrypto crates vs. one curated alternative vs. in-tree impls. Auditability, formal guarantees where applicable, no-std support.
2. **`umbrix-crypto`** — the crate that wraps the chosen primitives with Umbrix-native types (`Hash`, `Key`, `Signature`, `Nonce`).
3. **Security review** per [`analysis/reviews/security-reviews/`](../../analysis/reviews/security-reviews/) — mandatory for every primitive.
4. **Constant-time audit** — where timing side channels matter, document which paths are constant-time and which are not.

### Acceptance criteria

- ADR-0047 Accepted.
- Each primitive's security review recorded.
- No primitive is in-tree hand-rolled without explicit justification.

## Milestone G3 — TLS / DTLS

For network services (Phase E6) to talk securely. Probably `rustls` if `no_std + alloc` works; otherwise a curated alternative.

### Sub-breakdown

1. **ADR-0048 — TLS library choice.** rustls (preferred), mbedtls, in-tree. Covers version-pinning, maintenance, supply-chain posture.
2. **Integration** with the network service from E6.
3. **Test** — TLS 1.3 handshake with a known server.

### Acceptance criteria

- ADR-0048 Accepted.
- Network service completes a TLS handshake and exchanges encrypted data.

## Milestone G4 — Formal verification pilot

Pick one kernel primitive — probably the capability table's derivation/revocation logic — and specify-and-verify it with Kani / Creusot / some tool suited to Rust.

### Sub-breakdown

1. **ADR-0049 — Verification tool choice.** What tool for what scope; how it interacts with the build.
2. **Specification** of the chosen primitive's invariants in machine-checkable form.
3. **Proof** (or counterexamples that correct the implementation).
4. **CI integration** — verification runs per-commit on the verified subset.

### Acceptance criteria

- ADR-0049 Accepted.
- At least one invariant proved on a real kernel primitive.
- CI fails if the verification breaks.

## Milestone G5 — Threat model v2

Revisit the threat model from [`security-model.md`](../../architecture/security-model.md) with the experience accumulated through Phases A–F. Some threats will have moved from "out of scope" to "in scope"; some will be deprecated.

### Sub-breakdown

1. **Review of the existing threat model** against Phase F deployment learnings.
2. **ADR-0050 — Threat model v2.** Explicit supersession of security-model's Phase-1 threat statements where warranted.
3. **`security-model.md` update** to reflect v2 (standards-update skill + ADR-first discipline).

### Acceptance criteria

- ADR-0050 Accepted.
- `security-model.md` updated.
- Business review captures "what the deployment taught us about the model."

### Phase G closure

Business review. Umbrix now has the security engineering to back the "high assurance" claim at a level beyond process.

## ADR ledger for Phase G

| ADR | Purpose | Expected state |
|-----|---------|----------------|
| ADR-0046 | Boot measurement scheme | G1 |
| ADR-0047 | Crypto crate choice | G2 |
| ADR-0048 | TLS library choice | G3 |
| ADR-0049 | Verification tool choice | G4 |
| ADR-0050 | Threat model v2 | G5 |

## Open questions carried into Phase G

- Whether verification is a standing practice after G4 or a one-off pilot.
- How deeply to audit crypto dependencies we do not own.
- Whether TLS lives in the network service or in its own crypto service (likely its own service for capability-scope reasons).
