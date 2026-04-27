# T-013 — EL drop to EL1 in boot

- **Phase:** B
- **Milestone:** B1 — Drop to EL1 in boot, install exception infrastructure
- **Status:** In Review
- **Created:** 2026-04-27
- **Author:** @cemililik (+ Claude Opus 4.7 agent)
- **Dependencies:** [ADR-0024](../../../decisions/0024-el-drop-policy.md) — must be `Accepted` before this task moves to `In Progress`.
- **Informs:** Precondition for [T-012](T-012-exception-and-irq-infrastructure.md) — T-012's `VBAR_EL1` install assumes EL1; without T-013's drop, a future BSP that boots at EL2 would silently break the assumption T-009's `UNSAFE-2026-0016` runtime check catches.
- **ADRs required:** [ADR-0024 — EL drop to EL1 policy](../../../decisions/0024-el-drop-policy.md). ADR-0008 (Cpu trait) potentially extended with a `current_el` accessor — see §Approach.

---

## User story

As the Tyrne kernel boot path, I want a deterministic transition to EL1 regardless of the EL the firmware/emulator delivers — so that every later piece of code (`UNSAFE-2026-0016`'s `CurrentEL` self-check, the EL1-only system-register reads in `Cpu` and `Timer` impls, the future `VBAR_EL1` install in T-012, and every Phase B+ milestone that assumes EL1) has one stable starting state to reason against.

## Context

Tyrne's `boot.s` performs no EL transition today; it relies on QEMU virt's default behaviour (delivers at EL1). Real hardware and `-machine virtualization=on` deliver at EL2. T-009's `QemuVirtCpu::new` ships a runtime `CurrentEL == 1` assertion (UNSAFE-2026-0016) that catches a violation loudly, but the assertion stops the boot — it does not transition. T-013 closes the gap: implements the actual EL2→EL1 transition in `boot.s` per the policy ADR-0024 settles, and provides a Rust accessor for `CurrentEL` that other kernel code can call without re-introducing inline asm.

phase-b.md §B1 sub-breakdown items 2 + 3 (asm extension + Rust helpers) are this task's scope. Item 1 of B1 is ADR-0024 itself; items 4 (tests) belong here too. Item 5 is T-012 (exception infrastructure) which depends on this task.

## Acceptance criteria

- [ ] **ADR-0024 Accepted** before any code lands.
- [ ] **`boot.s` EL2→EL1 transition.** When `CurrentEL` reads as EL2 at the reset vector, `boot.s` configures `HCR_EL2`, `SPSR_EL2`, `ELR_EL2` (target = next instruction post-`ERET`), and issues `ERET`. When `CurrentEL` reads as EL1, the transition block is skipped (no-op). When `CurrentEL` reads as EL3, the boot panics (or halts; v1 has no EL3-aware infrastructure) — the failure mode ADR-0024 settles.
- [ ] **Bundle K3-12 (per phase-b.md §B1 item 2):** explicit `msr daifset, #0xf` at the head of `_start` as a BSP reset-vector standard, before any code that could be interrupted.
- [ ] **Cpu HAL helper for `current_el`.** Either (a) a free function `tyrne_hal::cpu::current_el() -> u8` (read CurrentEL inline-asm, return the 2-bit EL field), or (b) a `Cpu::current_el(&self) -> u8` method on the trait. The choice depends on whether the BSP needs to call this before `QemuVirtCpu` is constructed (early-boot path). Decision deferred to §Approach.
- [ ] **Audit entry.** New `unsafe` block(s) for the boot-time MRS / MSR sequence and any new Rust-side asm. Likely UNSAFE-2026-0017+ (see UNSAFE-2026-0016 for precedent — the existing `CurrentEL` read in `QemuVirtCpu::new` is covered, but the boot-asm path may need its own entry, and the EL2 system-register writes definitely need one).
- [ ] **Tests.**
  - QEMU smoke at default config (EL1 entry) — boot trace unchanged.
  - QEMU smoke at `-machine virtualization=on` (EL2 entry) — boot trace identical (proves EL drop landed correctly).
  - Host: helper function (if extracted as `fn current_el() -> u8`) is `cfg(target_arch = "aarch64")`-gated; trivially unit-testable on aarch64-targeted host (none today, so this test may be deferred until a CI runner with aarch64 emulation is available, OR via a `#[cfg(test)]`-gated mock).
- [ ] **`UNSAFE-2026-0016` assertion remains in place.** T-013 makes the assertion's success condition reliably reachable, but does not remove the runtime check — it is now a load-bearing invariant rather than a defensive guard.
- [ ] **Documentation.** ADR-0024 references; phase-b.md §B1 sub-breakdown items 2+3 marked complete; update `docs/architecture/boot.md` (when written, or as part of T-008) with the new EL transition flow; add `current_el` accessor to glossary or HAL docs as appropriate.

## Out of scope

- **EL3 → EL1 transition.** v1 hardware targets do not start at EL3; if they ever do, a separate task adds the EL3→EL2→EL1 chain. Today T-013 panics (or halts) on EL3 entry per ADR-0024.
- **EL1 → EL0 transition** (userspace entry). That is Phase B5+ work.
- **Hypervisor / EL2 hosting.** Tyrne running at EL2 itself (i.e. as a hypervisor) is explicitly not a goal in v1 or v2.
- **Per-CPU EL drop on SMP boot.** Single-core in v1; secondary CPUs come up via PSCI in Phase C.
- **`HCR_EL2.{E2H, TGE}` (VHE) configuration.** Tyrne explicitly runs in non-VHE EL1 (per T-009's UNSAFE-2026-0015 Amendment); T-013's HCR_EL2 setup confirms `E2H = TGE = 0` before the ERET.

## Approach

ADR-0024 will settle the policy questions before T-013's code lands. At sketch level (subject to ADR-0024's outcome):

1. **Asm extension in `boot.s`** before the existing stack-pointer setup. Read `CurrentEL`; branch on the EL field; for EL2, configure `HCR_EL2` (E2H=0, TGE=0), `SPSR_EL2` (mode=EL1h, DAIF mask), `ELR_EL2` (label of next instruction), then `ERET`. For EL1, skip. For EL3, branch to a halt loop (or panic via a minimal asm-level `b .` since panic infrastructure isn't up yet).
2. **Bundle K3-12:** `msr daifset, #0xf` immediately at `_start`, before everything else. Documented in [`docs/standards/bsp-boot-checklist.md`](../../../standards/bsp-boot-checklist.md) update.
3. **Rust `current_el` accessor.** Decision: free function vs. Cpu method. Free function is simpler (no `&self` needed; can be called before any CPU-handle exists, which matters at very early boot). Cpu method aligns with the rest of the HAL trait surface. **Default proposal:** `pub fn tyrne_hal::cpu::current_el() -> u8` as a free helper (host-testable on aarch64), plus a `Cpu::current_el(&self) -> u8` thin wrapper for ergonomic access from places that already hold a `&dyn Cpu`. Open for confirmation in the ADR-0024 review.
4. **Tests via QEMU smoke** at both `-machine` configs. Host tests for the helper function happen if the host CI gains aarch64 emulation; otherwise deferred.

## Definition of done

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo host-clippy` clean with `-D warnings`.
- [ ] `cargo kernel-clippy` clean.
- [ ] `cargo host-test` passes (T-013 may add a host test for the `current_el` helper if the architecture allows; otherwise none).
- [ ] `cargo +nightly miri test --workspace --exclude tyrne-bsp-qemu-virt` passes.
- [ ] `cargo kernel-build` clean.
- [ ] QEMU smoke reproduces the eight-line trace at default config; same trace at `-machine virtualization=on`.
- [ ] No new `unsafe` block without an audit entry; new entries cited from every introducing site.
- [ ] Commit messages follow [`commit-style.md`](../../../standards/commit-style.md) with `Refs: ADR-0024` and `Audit: UNSAFE-2026-NNNN` trailers.
- [ ] Task status updated to `In Review`; phase-b.md §B1 sub-breakdown items 2+3 marked complete; [`docs/roadmap/current.md`](../../../roadmap/current.md) updated.

## Design notes

- **Why not fold T-013 into T-012?** T-012 is "exception infrastructure and IRQ delivery" — substantial scope already. T-013 is "EL drop and current_el helper" — small, self-contained, runs first (T-012 depends on EL1 being settled). Splitting keeps the bisect-ability of B1: a regression in EL drop won't tangle with a regression in IRQ wiring.
- **Why a free function for `current_el` rather than a `Cpu` trait method?** Open question; the §Approach default proposes both (free for early boot, method for HAL ergonomics). ADR-0024 settles this.
- **Why panic / halt on EL3 rather than chaining the drop?** v1 has no EL3-aware code (no Secure-world, no firmware-call protocol). A future BSP for hardware that boots at EL3 (e.g. some Cortex-A profiles when running secure code) gets a follow-up task. Today the panic surfaces the unsupported configuration loudly.
- **Why is K3-12 bundled here?** "mask DAIF before anything else" is part of the same boot-stub discipline T-013 is touching. Bundling avoids two rounds of `boot.s` churn. The BSP boot checklist update is a doc-side echo of the same rule.

## References

- [ADR-0024 — EL drop to EL1 policy](../../../decisions/0024-el-drop-policy.md) — the policy this task implements.
- [ADR-0008 — Cpu HAL trait](../../../decisions/0008-cpu-trait.md) — possible extension point for `current_el` method.
- [ADR-0012 — Boot flow on QEMU virt](../../../decisions/0012-boot-flow-qemu-virt.md) — the existing boot-flow ADR this task augments.
- [T-009 task file](T-009-timer-init-cntvct.md) — UNSAFE-2026-0016 establishes the in-Rust `CurrentEL` read pattern.
- [T-012 task file](T-012-exception-and-irq-infrastructure.md) — depends on T-013's EL1 guarantee.
- [BSP boot checklist](../../../standards/bsp-boot-checklist.md) — gains the K3-12 "mask DAIF before anything else" rule.
- ARM *Architecture Reference Manual* DDI 0487G.b — `HCR_EL2`, `SPSR_EL2`, `ELR_EL2`, `ERET` semantics; `CurrentEL` register layout (bits [3:2]).

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-04-27 | @cemililik (+ Claude Opus 4.7 agent) | Opened with status `Draft`, paired with ADR-0024 (`Proposed`) per ADR-0025 §Rule 1 (forward-reference contract) (ADR-0024's *Dependency chain* requires a real T-NNN file for the implementation step; this task is that file). Will move to `In Progress` only after ADR-0024 is `Accepted`. |
| 2026-04-27 | @cemililik (+ Claude Opus 4.7 agent) | Promoted `Draft → In Progress → In Review` in a single arc (ADR-0024 became `Accepted` earlier the same day). Implementation: **`bsp-qemu-virt/src/boot.s`** — `_start` extended with K3-12 (`msr daifset, #0xf` as the very first instruction) + the EL drop dispatch (read `CurrentEL`, EL2 → configure HCR_EL2/SPSR_EL2/ELR_EL2 + `eret`, EL1 → fall through, EL3 → halt loudly via `wfe; b .-`). HCR_EL2 written with `RW=1` only — non-VHE EL1 explicit per ADR-0024. SPSR_EL2 = 0x3c5 carries DAIF=masked + mode=EL1h. **`hal/src/cpu.rs`** — new `pub fn current_el() -> u8` (cfg-gated to `target_arch = "aarch64"` + `target_os = "none"`); the safe-Rust wrapper around `MRS CurrentEL`. **`hal/src/lib.rs`** — `mod cpu;` → `pub mod cpu;` so `tyrne_hal::cpu::current_el()` is the public path the rest of the codebase calls (matches the existing `pub mod timer;` pattern). **`bsp-qemu-virt/src/cpu.rs`** — `QemuVirtCpu::new` now reads CurrentEL via the helper instead of duplicating the inline asm; UNSAFE-2026-0016 gains an Amendment recording both the load-bearing-post-condition shift and the helper-substitution. **Audit log** — UNSAFE-2026-0017 (boot.s reset-vector DAIF mask + EL2→EL1 transition; pure asm, two contiguous blocks) and UNSAFE-2026-0018 (the `current_el` helper; safe wrapper around the MRS) added per `unsafe-policy.md`. **Documentation** — `docs/architecture/boot.md` updated with the new three-phase `_start` description (K3-12 + EL drop + conventional setup), the line-by-line asm sample, and the new "kernel runs at EL1 unconditionally" invariant; `docs/standards/bsp-boot-checklist.md` §1 rewritten to describe the always-drop-to-EL1 procedure and gains a §1a (K3-12) covering the DAIF mask rule. **Verification** — `cargo fmt`, `cargo host-clippy`, `cargo kernel-clippy`, `cargo kernel-build`, `cargo host-test` (130 + 13 = 143 host tests), `cargo +nightly miri test` (143/143) all green. **QEMU smoke** — not run from this development environment (the harness here cannot drive `qemu-system-aarch64`); deferred to the maintainer or a CI runner. ADR-0008 (Cpu trait) was *not* extended with a `current_el` method — the free function was chosen per ADR-0024 §Open questions because the early-boot path needs to read the EL before any `Cpu` instance has been constructed and a trait method would force every test mock (`FakeCpu`, `ResetQueuesCpu`, test-hal's own mock) to declare an EL it does not really have. UNSAFE-2026-0016 stays load-bearing as an in-Rust post-condition. Status → `In Review`. |
