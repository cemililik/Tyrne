# Full-tree comprehensive review plan — 2026-05-06

- **Plan author:** @cemililik (+ Claude planner agent)
- **Plan date:** 2026-05-06
- **Target HEAD at plan time:** `214052d` on `development`
- **Type:** **Plan only.** This file is a *blueprint* for executing a multi-agent, full-tree review. The review *artifacts* are produced by the agent runs the maintainer launches against this plan; this file does not itself contain findings.

> *This plan is a one-off, holistic re-review of every committed source and documentation file in the Tyrne tree, run after B1 closure (PR #11). It complements — but does not replace — the per-change skills [`perform-code-review`](../../../../.claude/skills/perform-code-review/SKILL.md) and [`perform-security-review`](../../../../.claude/skills/perform-security-review/SKILL.md). Use it once the maintainer wants a global pass before opening Phase B2 implementation work.*

---

## 1. Why this plan exists

The last *holistic* code review covered the tree at `cba5b16` (Phase A exit, 2026-04-21; ~5 370 LOC, 109 host tests, 12 `unsafe` entries). Since then the tree has grown to ~9 872 LOC of Rust/asm across 42 source files, 21 audited `unsafe` entries, 25 ADRs, 174 docs, and 149+ host tests — landed across B0 (T-006/T-007/T-008/T-009/T-011), B1 (T-012/T-013), and the PR #9, PR #10, PR #11 review-fix sweeps. The B0 and B1 *closure* reviews (business + security + performance) are narrow-scope and event-triggered; none of them re-examine the *whole* surface from scratch.

The maintainer wants a precise, parallel, full-tree pass that:

- Re-reads every source file end-to-end (not just the diff against Phase A).
- Re-validates every doc against the code as it stands now.
- Examines optimization, security, correctness, style, integration, and supply-chain axes simultaneously.
- Produces a merged artifact whose verdict can gate Phase B2 implementation.

## 2. Scope

**In scope** (review surface):

| Surface | Path | Approx LOC / file count |
|---|---|---|
| Kernel crate | [kernel/src/](../../../../kernel/src/) | 3 654 LOC across 10 files |
| HAL crate | [hal/src/](../../../../hal/src/) | 1 198 LOC across 7 files |
| Test-HAL crate | [test-hal/src/](../../../../test-hal/src/) | 998 LOC across 6 files |
| BSP (QEMU virt) | [bsp-qemu-virt/src/](../../../../bsp-qemu-virt/src/) | 2 359 LOC across 8 files (incl. 333 LOC asm) |
| Build / linker | [bsp-qemu-virt/build.rs](../../../../bsp-qemu-virt/build.rs), [bsp-qemu-virt/linker.ld](../../../../bsp-qemu-virt/linker.ld), [.cargo/config.toml](../../../../.cargo/config.toml), workspace `Cargo.toml`, per-crate `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml` | ~290 LOC |
| ADRs | [docs/decisions/](../../../decisions/) (0001-0025 minus 0023 reserved) | 25 ADRs |
| Architecture docs | [docs/architecture/](../../../architecture/) | 8 files |
| Standards | [docs/standards/](../../../standards/) | 15 files |
| Audit log | [docs/audits/unsafe-log.md](../../../audits/unsafe-log.md) | 21 entries (1 Removed) |
| Guides | [docs/guides/](../../../guides/) | 4 files |
| Roadmap & tasks | [docs/roadmap/](../../../roadmap/), [docs/analysis/tasks/](../../tasks/) | phase-a–j + per-task user stories |
| Glossary, root docs | [docs/glossary.md](../../../glossary.md), [README.md](../../../../README.md), [CLAUDE.md](../../../../CLAUDE.md), [AGENTS.md](../../../../AGENTS.md), [CONTRIBUTING.md](../../../../CONTRIBUTING.md), [SECURITY.md](../../../../SECURITY.md), [LICENSE](../../../../LICENSE), [NOTICE](../../../../NOTICE) | — |
| Skills index | [.claude/skills/](../../../../.claude/skills/) | each `SKILL.md` |

**Out of scope:**

- `target/` (build artefacts).
- `docs/analysis/technical-analysis/` (third-party-OS study notes — **not** Tyrne source-of-truth; review ergonomics only if a doc is referenced by a Tyrne ADR or guide).
- Prior review artifacts under `docs/analysis/reviews/` (these are *inputs* to context, not subjects of review).
- Closed task user-story files (read-only history).

## 3. Pre-flight

Performed once, by a single agent, before parallel tracks start.

1. Capture current git HEAD SHA, branch, and `git status` (must be clean working tree).
2. Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`. Record results in the pre-flight artifact. **If any of these fail, abort the review until the failure is fixed; a code review on a red tree wastes agent context.**
3. Run `cargo +nightly miri test --workspace` if the toolchain is available; record outcome.
4. Capture: total LOC, file count per crate, `unsafe` block count grep, ADR count, audit-log entry count. Snapshot is the "authoritative inventory" referenced by every track.
5. Confirm the ten parallel tracks (§5) are unblocked: each track's input set is reachable from HEAD; no path drift since this plan was written.

**Output:** `docs/analysis/reviews/code-reviews/2026-05-06-full-tree/00-preflight.md` (HEAD SHA, CI/test/miri status, inventory table).

## 4. Risk class

The change-set re-examined here is **security-sensitive** (every kernel subsystem, IPC, capabilities, scheduler, exception path, EL drop, GIC, every `unsafe`, asm). The merged artifact's verdict is therefore **conditional on the paired security-review track (Track C) and performance-review track (Track D)** also returning Approve. The merged code review records the cross-references inline.

## 5. Parallel agent tracks

Ten tracks. Each track is one agent. Tracks are **independent** — none reads another's outputs during execution. The merge step (§6) is the only place track outputs interact.

Each track agent must:

1. Read **only its assigned input files** end-to-end. *Do not skim.* The first pass is for understanding; the second pass is for findings.
2. Cross-reference its findings to file paths and line numbers in the form `path/to/file.rs:NNN` so blockers are linkable.
3. Classify every finding as **blocker** (must fix before the review approves), **non-blocking** (worth surfacing, may be deferred), or **observation** (informational, no action needed).
4. Write its sub-artifact to `docs/analysis/reviews/code-reviews/2026-05-06-full-tree/<track-id>-<short-name>.md` using the per-track template in §7.
5. Stay strictly within the track's scope. Cross-track findings (e.g. a security agent spotting a perf issue) are recorded as a one-line "Cross-track note" and routed during merge.

### Track A — Kernel correctness

**Agent:** A single agent walks the kernel crate end-to-end. The kernel is small enough (3 654 LOC, zero `unsafe`) that splitting per submodule is unnecessary; one careful pass is better than four shallow ones.

**Inputs:**

- [kernel/src/lib.rs](../../../../kernel/src/lib.rs)
- [kernel/src/cap/mod.rs](../../../../kernel/src/cap/mod.rs), [cap/table.rs](../../../../kernel/src/cap/table.rs), [cap/rights.rs](../../../../kernel/src/cap/rights.rs)
- [kernel/src/obj/mod.rs](../../../../kernel/src/obj/mod.rs), [obj/arena.rs](../../../../kernel/src/obj/arena.rs), [obj/task.rs](../../../../kernel/src/obj/task.rs), [obj/endpoint.rs](../../../../kernel/src/obj/endpoint.rs), [obj/notification.rs](../../../../kernel/src/obj/notification.rs)
- [kernel/src/ipc/mod.rs](../../../../kernel/src/ipc/mod.rs)
- [kernel/src/sched/mod.rs](../../../../kernel/src/sched/mod.rs)
- [kernel/Cargo.toml](../../../../kernel/Cargo.toml)

**Cross-references it must consult while reading** (not subject to its own review here — those are other tracks):

- ADR-0014 (cap representation), ADR-0016 (kernel-object storage), ADR-0017 (IPC primitive set), ADR-0019 (scheduler shape), ADR-0021 (raw-pointer scheduler bridge), ADR-0022 (idle task + typed deadlock).
- Phase A code-review artifact ([2026-04-21-tyrne-to-phase-a.md](2026-04-21-tyrne-to-phase-a.md)) for known prior findings to confirm-or-flag-as-regressed.
- The B0 + B1 closure security reviews — to avoid re-litigating their already-cleared items.

**Per-subsystem checklist** (every item gets OK / blocker / non-blocking / observation):

- *Capabilities:* `cap_copy` peer-depth invariant; `cap_derive` saturating depth cap; `cap_revoke` BFS reachability invariants under release-mode `break`-on-overflow; `cap_take` ordering vs `free_slot`; `CapRights::from_raw` reserved-bit masking; `CapabilityTable::new` build-time capacity assertion.
- *Kernel objects:* `Arena<T,N>::new` for `N == 0`; generation reuse correctness; `destroy_*` reachability contract (callers must check `references_object`); `#[cfg(test)]` test-handle hygiene.
- *IPC:* `ipc_send` cap-take-before-state-mutation atomicity; `ipc_recv` `ReceiverTableFull` pre-flight guard; `IpcQueues::sync_generation` slot-reuse reset; `ipc_notify` `&CapabilityTable` immutability vs `&mut` for send/recv; `IpcError::InvalidCapability` granularity (still collapsed across three failure modes — re-check whether T-006/T-009/T-012 introduced new conflations); v2 `Reply` / `ReplyRecv` deferred per ADR-0018 (verify still absent).
- *Scheduler:* `SchedQueue` const-N==0 case; `yield_now` split-borrow correctness via `ctx_ptr.add(idx)` (the ADR-0021 raw-pointer bridge); `ipc_recv_and_yield` post-resume re-check + `debug_assert` on `Pending`; `unblock_receiver_on` single-waiter contract; `Scheduler::start` empty-queue panic; idle-task path (T-007) and typed `SchedulerError::Deadlock` (ADR-0022); the timer-IRQ path introduced in T-012's `irq_entry` and the WFI in `idle_entry` (T-012); `arm_deadline` / `cancel_deadline` exposure via the scheduler.
- *Top-level lib:* `#![no_std]` posture, workspace lint propagation (vs the per-crate `#![deny(...)]` block — confirm whether the prior review's "double-stated" nit was acted on or left), feature flags.

**Output:** sub-artifact `track-a-kernel.md`. Sections: per-subsystem findings + a "regression vs Phase A code review" diff line for each prior non-blocking item (status: still-applies / fixed / regressed / superseded by later finding).

### Track B — HAL & test-HAL & shared trait surface

**Agent:** one agent.

**Inputs:**

- [hal/src/lib.rs](../../../../hal/src/lib.rs), [console.rs](../../../../hal/src/console.rs), [cpu.rs](../../../../hal/src/cpu.rs), [context_switch.rs](../../../../hal/src/context_switch.rs), [mmu.rs](../../../../hal/src/mmu.rs), [timer.rs](../../../../hal/src/timer.rs), [irq_controller.rs](../../../../hal/src/irq_controller.rs)
- [test-hal/src/lib.rs](../../../../test-hal/src/lib.rs) and the five fakes (console, cpu, mmu, timer, irq_controller)
- [hal/Cargo.toml](../../../../hal/Cargo.toml), [test-hal/Cargo.toml](../../../../test-hal/Cargo.toml)

**Per-trait checklist** (every trait gets OK / blocker / non-blocking / observation):

- *`Console`:* trait surface, `FmtWriter` blanket, `Send`/`Sync` discipline, error model.
- *`Cpu`:* object-safety (post-ADR-0020 split with `ContextSwitch`), `enable_irqs`/`disable_irqs`/`restore_irq_state`/`wait_for_interrupt` contracts, `IrqGuard<C>` generic-vs-trait-object reasoning (preserve the post-mortem comment per Phase A review).
- *`ContextSwitch`:* the `unsafe` trait split (ADR-0020); doc-comment `# Safety` invariants; whether each method's contract is tight enough that an *adversarial* impl can't subvert kernel state.
- *`Mmu`:* trait surface (still placeholder per Phase A — flag if any A6/B-era code accidentally consumed it).
- *`Timer`:* `now_ticks` / `freq_hz` / `arm_deadline` / `cancel_deadline` (T-009 + T-012 added the live halves of ADR-0010); cross-check the `ns_to_ticks` ceiling-rounding behaviour exposed via the trait.
- *`IrqController`:* `enable_irq` / `disable_irq` / `claim` / `complete` / `set_priority` etc.; verify the `GIC_MAX_IRQ` range check called out in PR #10 review-round-2 lives in the BSP impl, not the trait.
- *`IrqGuard`/`IrqState`:* `IrqState(pub usize)` synthetic-construction concern (Phase A non-blocker — recheck disposition).

**Test-HAL specific:**

- Fakes' fidelity: do `FakeCpu`, `FakeConsole`, `FakeMmu`, `FakeTimer`, `FakeIrqController` actually capture observable invariants of the real impls, or are they degenerate? An over-permissive fake hides bugs.
- `[dev-dependencies]`-only linkage on kernel — confirm production builds cannot pick up fakes.
- Generation drift: any fake whose surface is *behind* its real-trait counterpart after T-009/T-012?

**Output:** sub-artifact `track-b-hal.md`.

### Track C — Security pass (adversarial axis sweep)

**Agent:** one agent. **Must operate per the [security-reviews master plan](../security-reviews/master-plan.md)** — this plan's Track C does not replicate that procedure; it *invokes* it. The agent's output is itself a security-review-shaped artifact.

**Inputs:** the entire kernel + HAL + BSP source set, the audit log, the security-model doc, every ADR, and the latest B1 closure security review.

**Eight axes to walk** (verbatim from the master plan):

1. Capability correctness — privileged ops gated by capabilities; checks before side-effects; transfer is move-only.
2. Trust boundaries — userspace → kernel input validation (currently no userspace; flag any premature surface).
3. Memory safety — every `unsafe` (currently 21 audited) re-validated against its `SAFETY:` block + audit entry; aliasing under cooperative + interrupt boundary; uninitialised-memory exposure.
4. Kernel-mode discipline — no allocation in ISR (`irq_entry` post-T-012); no unbounded loops; minimal critical sections; typed-error-not-panic on exhaustion.
5. Cryptography — N/A this iteration (no crypto in tree).
6. Secrets & logging — confirm capability bits, raw indices, generation counters never appear in `Debug` output reachable from userspace surface (currently kernel-only).
7. Dependencies — workspace remains zero-extern (ADR-0006 stance); confirm `add-dependency` skill not silently exercised.
8. Threat-model impact — security-model doc still aligned with the current scheduler + GIC + EL1 drop.

**Special requirements (must not be skipped):**

- A row-by-row cross-check of all 21 audit entries against in-code `SAFETY:` blocks. The Phase A review built such a table for entries 0001–0012; the agent extends it to 0013–0021 and re-validates 0001–0011 (0012 Removed; verify the *removal* really took every prior `&mut` aliasing site with it).
- Specifically scrutinise the new B0/B1 surface: ADR-0021 raw-pointer scheduler bridge (UNSAFE-2026-0014), GIC v2 MMIO surface (UNSAFE-2026-0019), EL1 vector trampolines (UNSAFE-2026-0020), virtual-timer compare-register writes (UNSAFE-2026-0021), boot.s EL drop (UNSAFE-2026-0017), `current_el` helper (UNSAFE-2026-0018).
- The "Pending QEMU smoke verification" status notes on UNSAFE-2026-0019/0020/0021 are a known maintainer-side workitem — the security agent does *not* lift them, but it must confirm the Pending notation is still in place and call out anything that depends on the smoke completing.

**Output:** sub-artifact `track-c-security.md` (eight-section security-review shape; verdict per the master plan: Approve / Changes requested / Escalate).

### Track D — Performance & optimization sweep

**Agent:** one agent. **Operates per the [performance master plan](../performance-optimization-reviews/master-plan.md)** — but with the explicit understanding that this is a *holistic optimization audit*, not a hypothesis-driven cycle on a single concern. The agent records baselines where they exist (the [A6 baseline](../performance-optimization-reviews/2026-04-21-A6-baseline.md) and [B1 closure](../performance-optimization-reviews/2026-04-28-B1-closure.md) are inputs, not outputs); identifies new hotspots opened by B0/B1 code; and proposes — but does not implement — optimizations.

**Inputs:** all source, the two prior perf review artifacts, the ipc/sched/cap hot paths.

**Axes to examine:**

- *Hot-path inspection:*
  - IPC send/recv round trip (kernel/src/ipc/mod.rs end-to-end).
  - `Scheduler::yield_now` and the context-switch entry/exit (sched/mod.rs + hal/src/context_switch.rs + bsp cpu.rs `context_switch_asm`).
  - `irq_entry` from T-012 (vectors.s asm trampoline + Rust-side dispatch + `arm_deadline`).
- *Memory layout:*
  - Struct sizes, `#[repr(C)]` discipline on `Aarch64TaskContext`, alignment vs cache-line behaviour (64 B aarch64).
  - `CapabilityTable` slot packing — the freelist + sibling-ptrs + rights bitfield; recompute `size_of::<CapEntry>()` and judge whether a smaller representation is viable without a soundness regression.
  - `Arena<T, N>` per-slot overhead (generation counter + free-link).
- *Branch / inline / instruction-count opportunities:*
  - `#[inline]` posture across hot helpers (none currently? worth measuring).
  - `cold` annotations on error paths (the `IpcError` dispatch is one place).
  - `core::hint::assert_unchecked` opportunities where invariants are *already* enforced by debug_assert.
- *Asm hand-checks:*
  - `boot.s` BSS-zero loop (8-byte stride; correct under the linker's 8-B align).
  - `vectors.s` 16 vectors at 0x80 stride; trampoline minimality vs the dispatch table.
  - `context_switch_asm` callee-save set for AAPCS64 + d8–d15; opportunistically verify d8–d15 are still required for v1's no-NEON-in-tasks stance (cheaper switch if NEON isn't used).
- *Const correctness / compile-time avoidance:*
  - `const fn` posture on `CapRights::from_raw`, `Message::default()`, `Aarch64TaskContext::default()` etc.
  - Const-eval'd invariant assertions vs runtime `debug_assert` (the prior review flagged a few; track whether the suggested `const { assert!(...) }` migrations happened).

**Output:** sub-artifact `track-d-performance.md`.

### Track E — Documentation accuracy & ADR consistency

**Agent:** one agent.

**Inputs:**

- Every ADR (0001–0025; 0023 reserved-empty; verify no live link points to a missing 0023).
- Every architecture doc.
- Every standards doc.
- Every guide.
- The audit log.
- The glossary, README, CLAUDE.md, AGENTS.md, CONTRIBUTING.md, SECURITY.md.
- Roadmap `current.md` + `phases/` files (status accuracy only — not their *plans*, which are out of scope).
- Task user-story files for closed tasks (T-001 through T-013): verify `Status: Done` actually matches reality (closed PR linked, code in tree, no open follow-ups outside the standard table).

**Per-doc-class checklist:**

- *ADRs:*
  - Frontmatter fields (status, date, decision-makers) populated.
  - "Supersedes" / "Superseded by" symmetry between any pair.
  - "Dependency chain" section present on ADRs from 0024 onward (per ADR-0025 §Rule 1).
  - Every Accepted ADR's referenced T-NNN actually exists and has a Done status (or is explicitly Deferred).
  - "Revision notes" appended-only; no in-place rewrites that erase history.
  - Cross-references to architecture docs and audit-log entries are bidirectional and resolvable.
- *Architecture:* every `.md` reflects current code; in particular boot.md (post-T-013 EL drop), exceptions.md (post-T-012), scheduler.md / ipc.md (post-T-008), hal.md (post-T-009 timer addition + post-T-012 irq controller addition), security-model.md (post-B1).
- *Standards:* every standards doc still describes what the project actually does. unsafe-policy.md is the most load-bearing — verify every recent UNSAFE-2026-NNNN entry follows it.
- *Audit log:* one row per `unsafe` block / impl in the source. Run a grep diff: every `// SAFETY:` block in source must trace to a UNSAFE-2026-NNNN tag, and every UNSAFE-2026-NNNN must have a live `// SAFETY:` block (or be marked Removed with a removal commit SHA).
- *Glossary:* every term used in three or more docs is defined once; entries are alphabetical; no orphaned entries from the umbrix→tyrne rename.
- *Root docs:* CLAUDE.md / AGENTS.md / README.md / SECURITY.md / CONTRIBUTING.md still resolve every link they cite; no umbrix references remain (the [10e3351](../../../../docs/) sweep should have closed this — verify).
- *Guides:* `two-task-demo.md` execution trace still matches main.rs's actual `writeln!` / `write_bytes` sites; `run-under-qemu.md` matches `.cargo/config.toml`'s runner; `ci.md` matches the (still-absent) workflow files or is honest about the absence.

**Output:** sub-artifact `track-e-docs.md` — flat list of drift findings with `path:line` references.

### Track F — Tests & coverage depth

**Agent:** one agent.

**Inputs:** every `#[cfg(test)]` block across the workspace, the [coverage-baseline report](../../reports/2026-04-23-coverage-baseline.md), the [coverage-rerun report](../../reports/2026-04-23-miri-validation.md and 2026-04-27-coverage-rerun.md), the `run-qemu.sh` smoke harness, and the QEMU expected-output table in two-task-demo.md.

**Checklist:**

- *Per-error-variant coverage:* every variant of `IpcError`, `SchedulerError`, `CapError` must have at least one test that provokes it. The Phase A review flagged `IpcError::ReceiverTableFull` as untested; T-011 was supposed to close that. Verify; if it didn't, raise as blocker.
- *Per-subsystem coverage thresholds:* compare current per-file coverage from the latest rerun against the working-thresholds in [testing.md](../../../standards/testing.md). Flag regressions.
- *Property / fuzz / Miri:* document which subsystems already run under Miri and which are still pending; list candidates for property tests (e.g. `CapabilityTable` derivation-tree invariants are a natural fit).
- *Smoke-as-regression:* the QEMU smoke is still maintainer-launched; flag the absence of CI-wired regression and the consequent risk that UNSAFE-2026-0019/0020/0021's "Pending QEMU smoke verification" notes are non-self-clearing.
- *Test hygiene:* `#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, reason = "...")]` block present and reason-populated on every test module; no test silently swallows panic via `#[should_panic]` without `expected = "..."`.
- *Test-helper hygiene:* `pub(crate)` test_handle helpers + `#[cfg(test)]`-gating; no test helper leaks into release.

**Output:** sub-artifact `track-f-tests.md`.

### Track G — BSP & boot path

**Agent:** one agent.

**Inputs:**

- [bsp-qemu-virt/src/main.rs](../../../../bsp-qemu-virt/src/main.rs)
- [bsp-qemu-virt/src/boot.s](../../../../bsp-qemu-virt/src/boot.s), [vectors.s](../../../../bsp-qemu-virt/src/vectors.s)
- [bsp-qemu-virt/src/cpu.rs](../../../../bsp-qemu-virt/src/cpu.rs), [console.rs](../../../../bsp-qemu-virt/src/console.rs), [exceptions.rs](../../../../bsp-qemu-virt/src/exceptions.rs), [gic.rs](../../../../bsp-qemu-virt/src/gic.rs)
- [bsp-qemu-virt/build.rs](../../../../bsp-qemu-virt/build.rs), [bsp-qemu-virt/linker.ld](../../../../bsp-qemu-virt/linker.ld), [bsp-qemu-virt/Cargo.toml](../../../../bsp-qemu-virt/Cargo.toml)
- [docs/standards/bsp-boot-checklist.md](../../../standards/bsp-boot-checklist.md)

**Checklist:**

- *Boot ordering:* SP → CPACR → BSS-zero → EL2-to-EL1 drop → `kernel_entry` (post-T-013); confirm DAIF mask state across the drop, the `eret` target, and that `kernel_entry`'s console init still runs *before* anything that could panic.
- *Linker script:* `.text` / `.rodata` / `.data` / `.bss` placement, alignment, `PROVIDE(__bss_start = ...)` symbols matched by `boot.s`, `_estack` definition, the load address (0x40080000 for QEMU virt).
- *Vector table install:* `vectors.s` 16-vector layout at 0x80 stride; `vbar_el1` write site; trampoline → `irq_entry` / `panic_entry` Rust-side bridge (post-PR #10 `unsafe extern "C"` change).
- *GIC v2 driver:* `GIC_MAX_IRQ` range check (PR #10 review round 2); CPU-interface vs distributor MMIO ranges; ack/EOI ordering in `claim`/`complete`; priority masking discipline.
- *`Pl011Uart`:* `wrapping_add` on base+offset (Phase A non-blocker — reverify); MMIO read/write `SAFETY:` discipline; `panic_entry` reconstruction of UART for "panic-then-spin" path.
- *`QemuVirtCpu`:* `CurrentEL` self-check (UNSAFE-2026-0016); inline-asm `mrs`/`msr`/`wfi`/`isb` (UNSAFE-2026-0007); the timer system-register reads (UNSAFE-2026-0015); virtual-timer compare writes (UNSAFE-2026-0021).
- *Demo tasks:* `task_a`/`task_b` post-T-006 use the raw-pointer scheduler bridge (no `&mut` across yield) — verify.
- *Crate posture:* `#![no_std]`, `#![no_main]`, panic strategy (panic=abort scoped to aarch64-unknown-none), `#![allow(unreachable_pub, ...)]` reason populated.

**Output:** sub-artifact `track-g-bsp.md`.

### Track H — Build, toolchain, infrastructure & supply chain

**Agent:** one agent.

**Inputs:**

- [Cargo.toml](../../../../Cargo.toml) (workspace), per-crate `Cargo.toml`, [Cargo.lock](../../../../Cargo.lock), [rust-toolchain.toml](../../../../rust-toolchain.toml), [rustfmt.toml](../../../../rustfmt.toml), [clippy.toml](../../../../clippy.toml), [.cargo/config.toml](../../../../.cargo/config.toml).
- [.github/](../../../../.github/) (whatever lives there now).
- [.gitignore](../../../../.gitignore), [.gitattributes](../../../../.gitattributes) if present.
- [tools/](../../../../tools/) (the `run-qemu.sh` and any helpers).

**Checklist:**

- *Workspace lints:* `[workspace.lints]` rust + clippy denylist; verify it matches infrastructure.md's stated set; verify no per-crate override silently relaxes a kernel-scoped deny.
- *Cargo aliases:* `cargo kernel-build`, `cargo host-test` etc. resolve to working invocations; `default-members` correctly excludes BSP.
- *Toolchain pin:* nightly date, components (rust-src, miri, llvm-tools) — confirm the date is recent enough for any unstable feature the kernel uses.
- *Lockfile drift:* zero external deps in `Cargo.lock`; if anything appears, that's a blocker.
- *Linker rustflags:* the `target.aarch64-unknown-none` block in `.cargo/config.toml`; `-C link-arg=-Tbsp-qemu-virt/linker.ld`; `-C panic=abort`; `runner` for QEMU.
- *CI absence:* no GitHub Actions / circle / etc. workflow at HEAD. **This is the largest documented gap in the project.** Re-flag it as a blocker for the *project*, not for this review (consistent with how Phase A handled it: the absence is documented; we don't gate on it).
- *Audit-log integrity:* the audit log is the supply-chain record for `unsafe`; verify the indexing scheme is intact (no hole in 0001–0021 numbering except the documented 0012 Removed).
- *Skill index:* `.claude/skills/` indices match the `SKILL.md` files actually present; no orphaned skill or broken cross-link.

**Output:** sub-artifact `track-h-infra.md`.

### Track I — Cross-track integration

**Agent:** one agent — this track is *deliberately* run in parallel with the other eight, not after them, because it is checking *the seams* the others are too narrow to see.

**Inputs:** every source file (read superficially — looking for inter-crate edges) + every ADR (read for trait-contract claims) + the architecture docs (read for "X talks to Y" claims).

**Checklist:**

- *Trait-contract drift:* every HAL trait method documented in `hal/src/*.rs` matches its impl in `bsp-qemu-virt/src/*.rs` (signature, semantics, `# Safety` invariants). Spot-check by signature, then by behaviour.
- *Generic-vs-trait-object boundary:* the `IrqGuard<C>` post-mortem from Phase A — has anyone introduced a `&dyn Cpu` since? Verify.
- *ABI boundary:* every `#[repr(C)]` struct that crosses the kernel↔asm boundary (`Aarch64TaskContext`, `TrapFrame`, etc.) has an in-source comment naming its asm consumer; the asm offsets match.
- *Symbol mangling:* `extern "C"` on every function the linker / asm trampoline expects; `#[unsafe(no_mangle)]` on `kernel_entry`, `irq_entry`, `panic_entry` etc.
- *Phase ↔ ADR ↔ task ↔ audit-entry linkage:* the roadmap `phase-b.md` plan claims a set of ADRs; those ADRs claim a set of tasks; those tasks claim a set of audit entries. Walk the chain end-to-end and flag breaks.
- *Skill consistency:* the `perform-code-review` and `perform-security-review` skills point at this directory's master plans; verify the pointers still resolve.

**Output:** sub-artifact `track-i-integration.md`.

### Track J — Localization, naming, and umbrix→tyrne residue

**Agent:** one agent. Lightweight; designed to soak up cross-cutting hygiene findings the heavier tracks would otherwise lose.

**Inputs:** the entire tree.

**Checklist:**

- *Umbrix residue:* `git grep -i 'umbrix' -- ':!docs/analysis/technical-analysis/'` should be empty. Anywhere it isn't, raise.
- *English-only commits/code/docs:* per [localization.md](../../../standards/localization.md), every committed artefact is English. Spot-check recent commits, source comments, doc bodies.
- *Naming consistency:* `Tyrne` capitalization (proper noun); `tyrne_*` snake-case for crate names and identifiers; project name in plain prose vs code identifiers.
- *Phantom symbols:* any `pub` item that is never referenced from any other crate or `#[cfg(test)]` block; any `#[allow(dead_code)]` whose reason field lies.
- *Comment hygiene:* `// TODO`, `// FIXME`, `// HACK` — list every occurrence with file:line; flag any without an issue / task / ADR reference.

**Output:** sub-artifact `track-j-hygiene.md`.

## 6. Merge step

After all ten tracks land their sub-artifacts:

1. A single merge agent reads all ten files plus the pre-flight artifact.
2. It produces the final consolidated artifact at `docs/analysis/reviews/code-reviews/2026-05-06-full-tree-comprehensive.md`, structured per the existing [code-review master-plan output template](master-plan.md#output-template) — five sections (Correctness / Style / Test coverage / Documentation / Integration) — but with the security and performance findings folded into the section that owns them and explicit cross-references to the tracks where the raw evidence lives.
3. The verdict is computed conservatively: any single track-level **blocker** → final verdict is **Request changes**; any track returning **Comment-only** without blockers → final verdict is **Comment**; otherwise **Approve**.
4. The merge agent updates the [README.md index](README.md) with one row pointing at the final artifact (and a parenthetical "see also the per-track artefacts under `2026-05-06-full-tree/`").
5. The merge agent does **not** edit any source. Findings that need code changes become a follow-up task list in the artifact's *Verdict* section.

## 7. Per-track output template

Every sub-artifact uses this shape so the merge step is mechanical:

```markdown
# Track <ID> — <name>

- **Agent run by:** <model + date>
- **Scope:** <one-sentence reaffirmation of the track's input set>
- **HEAD reviewed:** <SHA from pre-flight>

## Findings

### Blocker
- [path/to/file.rs:NNN] <one-line statement> — <2-3 sentence justification>.
  - Suggested resolution: <action item>.

### Non-blocking
- [path/to/file.rs:NNN] <one-line statement> — <justification>.

### Observation
- [path/to/file.rs:NNN] <one-line statement>.

## Cross-track notes (route to merge)

- → Track X: <one-line>.

## Sub-verdict

**Approve | Request changes | Comment**
```

## 8. Acceptance criteria

The full-tree review is "complete" only when **all** of the following are true:

- [ ] `00-preflight.md` exists, lists HEAD SHA, and confirms `cargo test` + `cargo clippy` clean.
- [ ] All ten track sub-artifacts exist under `docs/analysis/reviews/code-reviews/2026-05-06-full-tree/`.
- [ ] Each sub-artifact has a verdict and at least one entry in each finding bucket *or* an explicit "no findings" note.
- [ ] The merged artifact `2026-05-06-full-tree-comprehensive.md` exists, references every track, and states a verdict.
- [ ] The code-reviews [README.md index](README.md) has a row for the merged artifact.
- [ ] If the merged verdict is **Request changes**: a follow-up task list is in the verdict section, with each blocker traced to a track + path + line.
- [ ] If the merged verdict is **Approve**: the verdict notes any cross-references to the (Track C) security artifact and (Track D) performance artifact and confirms both are non-blocking.

## 9. Anti-patterns specific to this plan

- **Skim-then-comment.** A track that returns "no findings" after 30 seconds of context is worse than no review. Each track's first pass *must* be an end-to-end read of its inputs.
- **Re-litigating the Phase A code review.** That artifact stands. Track A's job is to confirm the Phase A non-blockers' current disposition, not to re-derive them from scratch.
- **Cross-track scope creep.** Use the "Cross-track note" mechanism. A perf agent that writes a security finding inline pollutes the merge step.
- **Inventing tasks.** Findings name a problem and a path. They do not pre-populate T-NNN identifiers; the maintainer + start-task skill own task creation.
- **Approving while a track is incomplete.** The merge step's verdict requires every track to have closed.

## 10. Execution sequencing

Recommended invocation order for the maintainer:

1. **Pre-flight** (one agent, ~5 min). Must succeed before parallel tracks start.
2. **Tracks A–J** (ten agents, ideally launched as one batch). Independent; no shared state. Wall-clock time is dominated by the slowest track (likely Track A, kernel correctness).
3. **Merge step** (one agent). Reads pre-flight + all ten sub-artifacts.

Single-agent fallback: walk the tracks sequentially in the order listed (A → J), then merge. Same artifacts produced, ~10x wall-clock.

## 11. Out-of-scope but worth noting for the next review cycle

- Phase B2 (kernel virtual memory layout, ADR-0027) work has not started; this review intentionally does not anticipate it.
- The technical-analysis subtree (`docs/analysis/technical-analysis/`) is research notes on third-party kernels; it grew substantially during B0/B1 prep and may itself benefit from a hygiene pass — but **not as part of this review**. File a separate task if the maintainer wants it.
- A formal `cargo-vet` or `cargo-deny` configuration does not exist; the workspace's zero-extern-deps stance makes it unnecessary today, but flag for revisit when ADR-0027 / Phase C drivers might pull crates in.

## 12. Plan-level amendments

- _2026-05-06_ — initial version; authored by Claude planner agent at maintainer's request, run targeting `214052d`.
