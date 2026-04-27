# 0024 — EL drop to EL1 policy

- **Status:** Proposed
- **Date:** 2026-04-27
- **Deciders:** @cemililik

## Context

aarch64 has four Exception Levels (EL0..EL3, with EL2 sometimes virtualised via VHE). Tyrne's kernel runs at exactly one of them at any time, and every line of kernel code so far assumes that level is **EL1**: `MRS DAIF`, `MRS MPIDR_EL1`, `MRS CNTFRQ_EL0` / `CNTVCT_EL0`, `MRS CurrentEL`, `MSR DAIFSet`, the future `VBAR_EL1` install, the future `TTBR0_EL1` / `TCR_EL1` for the MMU, the future `SVC` syscall path. EL2 has different register names (`MRS DAIF` is the same name but different bits; `MPIDR_EL1` is still readable but `CNTHCTL_EL2.EL1PCTEN` gating doesn't apply; `VBAR_EL2` is the EL2 vector base, not `VBAR_EL1`; etc.). Running EL1 code at EL2 either traps, reads/writes the wrong register, or — worst case — silently produces wrong behaviour.

Today, Tyrne's `boot.s` performs **no** EL transition. It relies on the firmware/emulator to deliver the kernel at EL1. QEMU virt does this by default; with `-machine virtualization=on` it delivers at EL2 instead; real ARM hardware with a Trusted-Firmware / U-Boot stack varies by configuration. T-009's `QemuVirtCpu::new` ships a runtime `CurrentEL == 1` assertion (UNSAFE-2026-0016) that catches the violation loudly — but the assertion stops the boot, it does not make the kernel work at the unintended EL.

Phase B's later milestones make the gap actively dangerous. B2 (MMU activation) is deeply EL1-specific: the page-table base lives in `TTBR0_EL1`. B5 (syscall ABI) routes through `SVC` and the EL0→EL1 transition the syscall implies — a kernel running at EL2 has no clean way to handle that. Continuing to ignore the EL question is a debt that compounds.

This ADR settles the policy: **what EL does the kernel run at, and how does it get there?** A separate task ([T-013](../analysis/tasks/phase-b/T-013-el-drop-to-el1.md)) implements the policy.

## Decision drivers

- **One target EL.** The kernel must run at exactly one EL, predictably, regardless of where the firmware/emulator drops us. Anything else is a maintenance tax that compounds across every later HAL impl.
- **Forward compatibility with virtualization-aware boot paths.** The `-machine virtualization=on` QEMU mode and most real-hardware boot stacks (Trusted Firmware-A → U-Boot → kernel) deliver at EL2. Tyrne should run on those stacks without a rebuild.
- **Minimum boot.s complexity.** The EL transition asm is ~10 lines and runs once. It should not blossom into a configuration matrix.
- **Fail-loud on unsupported configurations.** If a future hardware target boots at EL3 (e.g. before the firmware drops us to EL2 in some Cortex-A profiles), the failure must be visible at boot, not silently miscoded later.
- **Composes with the existing UNSAFE-2026-0016 assert.** The `CurrentEL == 1` runtime check in `QemuVirtCpu::new` should keep working as the post-condition of whatever transition strategy is chosen; it should not become trivially-always-true (loses defensive value) or trivially-always-false (kernel cannot boot under the strategy).
- **No VHE.** Tyrne does not use Virtualization Host Extensions in v1. The kernel is a plain EL1 OS; running it as a hosted-EL2 kernel (`HCR_EL2.{E2H, TGE} = {1, 1}`) is out of scope. The decision must explicitly leave VHE off.

## Considered options

1. **Option A — Always drop to EL1.** `boot.s` reads `CurrentEL`. If EL1, no-op. If EL2, configure `HCR_EL2` (E2H=0, TGE=0), `SPSR_EL2` (mode=EL1h, DAIF mask), `ELR_EL2` (the address of the next instruction post-`ERET`), and issue `ERET`. If EL3, halt with a clear failure (out of scope for v1; future task).
2. **Option B — Adapt to whichever EL we got.** The kernel detects its EL at boot and selects between EL1- and EL2-specific code paths throughout the codebase (HAL impls, syscall path, MMU activation). EL2 becomes a first-class target.
3. **Option C — Hard-fail on non-EL1.** `boot.s` checks `CurrentEL`; if not EL1, halts. The expectation is that all supported boot environments deliver at EL1 (existing QEMU virt default; Trusted Firmware setups that explicitly drop). T-009's UNSAFE-2026-0016 assertion already provides this at the Rust level; Option C just moves the check to asm.

## Decision outcome

**Chosen: Option A — Always drop to EL1.**

Option A has the property that **after `boot.s` completes, the kernel is at EL1, period.** Every kernel module from `kernel_entry` onwards reasons against a single, predictable EL. The complexity is paid once, in ~10 lines of boot asm, and never compounds again.

Option B's "kernel runs at any EL" framing is theoretically appealing but ignores how deep EL-specific knowledge is encoded in the kernel's HAL impls. Every `MRS DAIF` would need an EL-aware sibling; every `VBAR_EL{1,2}` install would branch; the MMU activation in B2 would need two parallel implementations because `TCR_EL1` and `TCR_EL2` have different layouts. The maintenance tax across Phase B alone is several tasks; over the project lifetime it is a permanent overhead. The benefit — running at EL2 when the firmware delivers there — is achievable with one boot-time `ERET` under Option A and no kernel-side changes.

Option C is essentially "Option A without the transition". It works as long as no supported boot environment delivers at EL2 — which excludes `-machine virtualization=on` QEMU and most virtualization-aware real-hardware stacks. The cost saving (skip the transition asm) is trivial; the loss of compatibility is not. UNSAFE-2026-0016's existing runtime check already provides an EL1-required defensive guard; promoting it to a hard-fail strategy without the transition leaves us less, not more, portable.

EL3 entry remains out of scope: v1 hardware targets (QEMU virt, Pi 4) do not start there. If a future BSP for hardware that boots at EL3 lands, that task adds the EL3→EL2→EL1 chain on top of Option A's existing EL2→EL1 transition. Today T-013 will halt or panic on EL3 entry — failure is visible.

VHE is explicitly off: `HCR_EL2.{E2H, TGE}` are both cleared to zero before the `ERET`, ensuring the post-drop EL1 runs in classic non-VHE mode. This is consistent with T-009's UNSAFE-2026-0015 Amendment, which describes the EL1 the kernel runs at as "non-VHE EL1 (HCR_EL2.{E2H, TGE} = {0, 0})". The Amendment was written assuming this ADR's outcome; the ADR ratifies the assumption.

### Dependency chain

For Option A to be fully in effect, the following must exist (every step grounded in a real T-NNN per ADR-0025 §Rule 2 (forward-reference contract)):

1. **`CurrentEL` read primitive.** ✅ Exists today — UNSAFE-2026-0016 in `QemuVirtCpu::new`, shipped with T-009 (commit `db3a4c7`). The same MRS pattern can be lifted to a HAL-level helper in step 3 below; the audit precedent is in place.
2. **`boot.s` EL2→EL1 transition asm.** Implemented by **[T-013](../analysis/tasks/phase-b/T-013-el-drop-to-el1.md)** (`Draft`, opened 2026-04-27 alongside this ADR). Configures `HCR_EL2` / `SPSR_EL2` / `ELR_EL2` and issues `ERET`. Includes K3-12 (explicit `msr daifset, #0xf` at the head of `_start`).
3. **Rust `current_el` accessor.** Implemented by T-013 — either as a free function `tyrne_hal::cpu::current_el() -> u8` or as a `Cpu::current_el` method (T-013 §Approach picks one).
4. **Tests.** T-013 covers QEMU smoke at both default config (EL1 entry) and `-machine virtualization=on` (EL2 entry).

T-012 (exception infrastructure) depends on this ADR's outcome being implemented (step 2) before its `VBAR_EL1` install runs. Both T-012 and T-013 are scoped under milestone B1.

## Consequences

### Positive

- **One EL, one set of register names.** Every later HAL impl, MMU work, and syscall path assumes EL1 unconditionally. No branching, no parallel impls.
- **Compatibility with virtualization-aware boot paths.** Tyrne now runs unmodified under `-machine virtualization=on` and on real-hardware stacks that deliver at EL2.
- **UNSAFE-2026-0016 keeps its defensive value.** The `CurrentEL == 1` assert in `QemuVirtCpu::new` is now the *post-condition* of T-013's transition; an unexpected EL after the transition (which should never happen) still surfaces loudly. Composition is clean.
- **Boot-time IRQ masking.** Bundling K3-12 means the kernel reset vector can no longer accidentally take an IRQ before its handler is installed. Defensive.
- **Forward compatibility with EL3-entry hardware.** The chosen shape (drop in `boot.s`) extends naturally to EL3→EL2→EL1 when a future hardware target requires it; the new code lives in the same place.

### Negative

- **One-way street.** Once the kernel drops to EL1, it cannot trap to EL2 (no hypercalls). A future "Tyrne as hypervisor" goal (running at EL2) would need a new ADR superseding this one. *Mitigation:* not a v1 or v2 goal; explicitly out of scope.
- **`HCR_EL2` configuration is fragile.** `E2H`, `TGE`, and a handful of other bits affect EL1 register access. Getting one bit wrong silently breaks `MRS CNTVCT_EL0` (EL1VCTEN gating in VHE) or makes EL1 traps go to EL2 instead of looping back to the EL1 handler. *Mitigation:* T-013's tests cover both QEMU configs; UNSAFE-2026-0016 catches the post-condition; ARM ARM citations in the audit entry pin which bits we cleared.
- **Boot.s gains a runtime branch.** Currently boot.s is a straight-line stack-pointer setup → BSS clear → `bl kernel_entry`. With T-013 it gains a `CurrentEL` read and a conditional `ERET`. Slightly larger code; no measurable performance impact (boot path runs once). *Mitigation:* the branch is documented in `boot.s` comments and `docs/architecture/boot.md` (when written).

### Neutral

- **No HAL trait change.** `Cpu` may gain a `current_el(&self)` method (T-013's choice) but the existing trait surface is unaffected.
- **No userspace-visible change.** EL1 is the kernel's home regardless of caller; userspace will run at EL0 in a later milestone, unrelated to this decision.
- **No effect on the existing T-009 assert.** UNSAFE-2026-0016 stays exactly as-is; it now becomes load-bearing.

## Pros and cons of the options

### Option A — Always drop to EL1 (chosen)

- Pro: one target EL across the entire kernel.
- Pro: compatible with both EL1-delivering and EL2-delivering boot environments.
- Pro: ~10 lines of boot asm, runs once.
- Pro: composes with UNSAFE-2026-0016.
- Con: requires `HCR_EL2` configuration discipline (mitigated above).
- Con: one-way (mitigated; not a goal to revisit).

### Option B — Adapt to whichever EL we got

- Pro: theoretically runs at EL2 if firmware delivers there; could one day enable hypervisor mode.
- Con: every EL-specific HAL impl needs an EL-aware sibling. MMU activation, syscall ABI, exception vectors — all branch. Several tasks of overhead in Phase B alone.
- Con: bug surface is much larger; "did I handle EL2 correctly here?" becomes a question every reviewer must ask.
- Con: complicates the kernel's mental model — "what EL am I at right now?" is a question that should not need answering after boot.

### Option C — Hard-fail on non-EL1

- Pro: zero new asm; reuses UNSAFE-2026-0016's existing pattern.
- Con: incompatible with `-machine virtualization=on` and most real-hardware stacks.
- Con: a regression we'd hit immediately when adding any non-EL1-delivering target.
- Con: the cost saved (one `ERET`) is not meaningful; the cost paid (lost compatibility) is.

## Open questions

- **`current_el` accessor shape.** Free function vs. `Cpu` trait method. Default proposal in T-013: provide both (free for early-boot use before any `Cpu` instance exists; method for ergonomic use elsewhere). Settle in T-013 implementation review.
- **EL3 boot behaviour.** Today: halt or panic. When a future BSP requires EL3 entry, a new task adds the EL3→EL2 transition. The current decision keeps the door open for that without committing to a shape.
- **Trusted Firmware / SMC interface.** Out of scope for this ADR; if future kernel work needs to call SMC for power management or secure-world services, that's a separate decision.

## References

- [ADR-0008 — Cpu HAL trait](0008-cpu-trait.md) — possible extension point for `current_el` method.
- [ADR-0012 — Boot flow on QEMU virt](0012-boot-flow-qemu-virt.md) — the existing boot-flow ADR this decision augments.
- [UNSAFE-2026-0016 audit entry](../audits/unsafe-log.md) — the precedent for `MRS CurrentEL` in the kernel; will compose with T-013's transition.
- [T-009 task file](../analysis/tasks/phase-b/T-009-timer-init-cntvct.md) — UNSAFE-2026-0015 Amendment describes the kernel's "non-VHE EL1" expectation that this ADR ratifies.
- [T-013 task file](../analysis/tasks/phase-b/T-013-el-drop-to-el1.md) — implements this policy.
- [T-012 task file](../analysis/tasks/phase-b/T-012-exception-and-irq-infrastructure.md) — depends on this ADR's outcome.
- [Phase B plan §B1](../roadmap/phases/phase-b.md) — sub-breakdown items 1–4 collectively realise this ADR.
- ARM *Architecture Reference Manual* DDI 0487G.b — `HCR_EL2`, `SPSR_EL2`, `ELR_EL2`, `ERET`, `CurrentEL`.
