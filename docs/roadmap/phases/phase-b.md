# Phase B — Real userspace

**Exit bar:** A userspace task (a real separate binary, not a kernel-level stub) runs in its own address space, with its own capability table, and can make syscalls back into the kernel.

**Scope:** Land the Phase A exit-hygiene fixes surfaced by the 2026-04-21 reviews, drop to EL1, activate the MMU with a kernel mapping, introduce per-task address spaces, build a task loader, define the syscall entry / dispatch, run the first userspace "hello world" in EL0. Still single-core; Pi 4 is Phase D; drivers are Phase E.

**Out of scope:** Multi-core, real hardware, userspace drivers, network, filesystem, cryptography.

**Source reviews informing this plan:**
- [Code review — Tyrne → Phase A exit](../../analysis/reviews/code-reviews/2026-04-21-tyrne-to-phase-a.md)
- [Security review — Tyrne → Phase A exit](../../analysis/reviews/security-reviews/2026-04-21-tyrne-to-phase-a.md)
- [A3–A6 business review / Phase A retrospective](../../analysis/reviews/business-reviews/2026-04-21-A6-completion.md)

Items flagged with 🚩 are decisions that must be settled during the named milestone before code lands. They are listed in the *Open questions* section as well, so nothing drops.

---

## Milestone B0 — Phase A exit hygiene

Cleans up the items the 2026-04-21 Phase-A code and security reviews surfaced. Every Phase-B capability that follows rides on top of the fixes here: preemption and SMP cannot start while `UNSAFE-2026-0012` (aliasing) is live; userspace cannot reach the scheduler while `ipc_recv_and_yield` can panic on deadlock; the subsystem architecture docs need to exist before external contributors navigate the code. B0 therefore runs **before** the EL-drop / MMU / syscall pipeline.

### Sub-breakdown

1. **ADR-0021 — Raw-pointer scheduler API.** Reshape `Scheduler::ipc_send_and_yield` / `Scheduler::ipc_recv_and_yield` so no `&mut` reference to `SCHED` / `EP_ARENA` / `IPC_QUEUES` / `TABLE_*` is live across the cooperative context switch. Resolves UNSAFE-2026-0012 (Security review §1 / §3 blocker #1).
2. **ADR-0022 — Idle task + typed `SchedError::Deadlock`.** Register a kernel idle task at boot so the ready queue is never empty; convert the `panic!("deadlock: …")` at [`kernel/src/sched/mod.rs:388-395`](../../../kernel/src/sched/mod.rs#L388-L395) to a typed error. Bundle: also convert `Scheduler::start`'s empty-queue panic at [sched/mod.rs:246-253](../../../kernel/src/sched/mod.rs#L246-L253) to `Err(SchedError::QueueEmpty)`; also harden the `debug_assert!` in the `ipc_recv_and_yield` resume path at [sched/mod.rs:417-421](../../../kernel/src/sched/mod.rs#L417-L421) to a release-mode `Err(...)`. Security review §4; code review §Correctness (Scheduler bullets 2, 4).
3. **ADR-0023 — Cross-table capability revocation policy.** Record the v1 single-table scope of the "Revocation is transitive" invariant (already qualified in [`docs/architecture/security-model.md`](../../architecture/security-model.md) by commit `de66d68`). 🚩 **Decision:** accept-deferred (option a; recommended — no code work, document the limitation and push cross-table CDT to Phase C) vs. implement-now (option b; substantial storage + IPC rewiring, only justified if a multi-task server appears in B3–B6 that needs post-transfer revocation).
4. **Architecture docs × 3** via the [`write-architecture-doc`](../../../.claude/skills/write-architecture-doc/SKILL.md) skill: `docs/architecture/kernel-objects.md` (ADR-0016 + Arena pattern), `docs/architecture/ipc.md` (ADR-0017 + ADR-0018 + state machine), `docs/architecture/scheduler.md` (ADR-0019 + ADR-0020 + IPC bridge + UNSAFE-2026-0008). Code review §Documentation follow-up #2.
5. **Timer initialisation** — populate `QemuVirtCpu`'s `Timer` trait impl with `CNTVCT_EL0` (virtual counter, register-family-aligned with the deferred `CNTV_*` deadline-arming registers per ADR-0010) and `CNTFRQ_EL0` reads; wire a free-running counter so IPC round-trip latency and context-switch overhead can be measured. Unlocks the first hypothesis-driven performance-review cycle (baseline at [`2026-04-21-A6-baseline.md`](../../analysis/reviews/performance-optimization-reviews/2026-04-21-A6-baseline.md) is blocked on this). *Note: the original phase-plan wording said "CNTPCT_EL0"; T-009 second-read review surfaced the register-family mismatch and switched to `CNTVCT_EL0`.*
6. **Scheduler / IPC hardening bundle.** Grouped in T-010 with ADR-0022's implementation:
   - `const { assert!(N > 0) }` on `SchedQueue::new` and `CapabilityTable::new` so zero-capacity constructions are a build-time error, matching `Arena::new`'s pattern.
   - `debug_assert_ne!(current_idx, next_idx)` before the split-borrow `unsafe` blocks in `yield_now` / `ipc_recv_and_yield` to catch regressions that stop dequeuing the running task.
   - Replace `debug_assert!` in the resume path with a hard `Err(SchedError::Ipc(...))` return (see item 2 above).
7. **`TaskArena` local → global StaticCell migration.** Bundled with T-006 (ADR-0021) to avoid two rounds of BSP static-cell churn. Brings `TaskArena` into the same reachability story as `EP_ARENA` / `TABLE_{A,B}` and satisfies the ADR-0016 "arenas belong to the kernel" framing. Post-A6 inline review feedback #15.
8. **Missing-tests bundle.** T-011 adds:
   - `IpcError::ReceiverTableFull` provoked + cap-retention assertion (code review §Test coverage).
   - Slot-reuse with a pending transfer cap in a destroyed endpoint (code review §Test coverage).
   - Once ADR-0022 lands, the `Scheduler::start` empty-queue and `ipc_recv_and_yield` deadlock should-panic tests are replaced by returns-Err tests.

### Acceptance criteria

- ✅ ADR-0021, ADR-0022 Accepted; ADR-0023 deferred (Phase B6+ revocation work; "accept-deferred" path per the original B0 plan).
- ✅ No `panic!(...)` remaining in `kernel/src/sched/mod.rs` reachable in production; the `start` / `start_prelude` "empty ready queue" panic survives as a kernel-programming-error guard rendered structurally unreachable by ADR-0022's idle-task-at-boot rule.
- ✅ `docs/audits/unsafe-log.md` UNSAFE-2026-0012 entry marked `Removed` with the resolution commit (`f9b72f8` — T-006 / ADR-0021).
- ✅ Two architecture docs committed (`scheduler.md` + `ipc.md`) with the `hal.md` Timer subsection update; linked from `docs/architecture/README.md` as Accepted. The originally-projected third doc was subsumed: `kernel-core.md` and `scheduling.md` were collapsed into `scheduler.md` + `ipc.md` per T-008's scope-discipline call.
- ✅ `QemuVirtCpu` implements `Timer`; IPC round-trip latency measurable via `CNTVCT_EL0` (virtual counter, register-family-aligned with the future deadline-arming `CNTV_*` registers per UNSAFE-2026-0015's first Amendment).
- ✅ 143 host tests green (130 → 143 via T-011's +13 tests; B0 final count exceeds the 109+ target by 31%).
- ✅ QEMU smoke still matches the A6 trace; T-013 added two more boot-path lines (DAIF mask + EL drop) without changing the kernel-entry trace.
- ✅ Phase-A security-review blocker #1 (UNSAFE-2026-0012) closed; #2 (cross-table revocation) deferred per ADR-0023's accept-deferred path; #3 (idle task + typed deadlock) closed in ADR-0022.

**Status: B0 closed 2026-04-27** with PR #9's merge to `main` (merge commit `9a66e8b`). All required tasks Done; T-010 (optional split) explicitly not opened. Phase-A exit hygiene complete.

### Tasks under B0

- [T-006 — Raw-pointer scheduler API refactor + TaskArena global migration](../../analysis/tasks/phase-b/T-006-raw-pointer-scheduler-api.md) — Done (2026-04-27)
- [T-007 — Idle task + typed `SchedError::Deadlock` + resume-path hardening](../../analysis/tasks/phase-b/T-007-idle-task-typed-deadlock.md) — Done (2026-04-27)
- [T-008 — Architecture docs (scheduler.md + ipc.md + hal.md/overview.md updates)](../../analysis/tasks/phase-b/T-008-architecture-docs.md) — Done (2026-04-27)
- [T-009 — Timer init + `CNTVCT_EL0` measurement](../../analysis/tasks/phase-b/T-009-timer-init-cntvct.md) — Done (2026-04-27)
- T-010 — (optional) Split of T-007 if ADR-0022 scope grows past one task *(not opened — T-007 closed without needing the split)*
- [T-011 — Missing tests bundle](../../analysis/tasks/phase-b/T-011-missing-tests-bundle.md) — Done (2026-04-27)

### Flags to resolve during B0

- 🚩 **Cross-table CDT (ADR-0023).** accept-deferred (preferred) vs. implement-now. Revisit if a multi-task server surfaces in B3–B6.
- 🚩 **`IpcError` split (K2-5).** Deferred from B0 into B5 so the full userspace-exposed error taxonomy is designed once. See open questions below.
- 🚩 **Architecture-doc scope.** Whether `docs/architecture/ipc.md` also covers notifications or a separate file later (notifications are A3-aware but v1 has no waiter list — the full semantics arrive with the first notification user).

### Informs

B1 / B2 / B3 all depend on a panic-free scheduler and a non-UB aliasing story. B5's syscall ABI design rides on the typed error pattern established by B0's hardening.

---

## Milestone B1 — Drop to EL1 in boot, install exception infrastructure

Extend the BSP reset stub so that when QEMU delivers us at EL2, we configure `HCR_EL2`, `SPSR_EL2`, `ELR_EL2`, and issue `ERET` to land in EL1. When QEMU delivers at EL1, the stub is a no-op on that axis.

The scope of this milestone was extended on 2026-04-27 (after T-009 — the time-source half of `Timer` — landed in `In Review`) to include the *exception delivery infrastructure* that ADR-0022's first-rider sub-rider gated on. Concretely: **GICv2 distributor + CPU interface** configuration on QEMU virt (GICv2 has no redistributor — that is GICv3 terminology; QEMU virt defaults to GICv2 unless `-machine gic-version=3` is set), an EL1 exception vector table install at `VBAR_EL1`, a thin handler-dispatch loop, and the generic-timer-IRQ wiring that lets `Timer::arm_deadline` and `Timer::cancel_deadline` actually fire interrupts. Without this work, `arm_deadline` / `cancel_deadline` remain `unimplemented!()` and idle's body cannot move from `spin_loop` to `wfi`.

### Sub-breakdown

1. ✅ **[ADR-0024](../../decisions/0024-el-drop-policy.md) — EL drop to EL1 policy** *(Accepted 2026-04-27)*. Settled choice: always drop to EL1 in `boot.s`, regardless of where firmware/emulator delivers the kernel. EL3 entry halts; VHE explicitly off.
2. ✅ **Asm extension** in `bsp-qemu-virt/src/boot.s` for EL2→EL1 transition — delivered by [T-013](../../analysis/tasks/phase-b/T-013-el-drop-to-el1.md) (Done 2026-04-27). **Bundle K3-12:** explicit `msr daifset, #0xf` at the top of `_start` is in place per the [BSP boot checklist](../../standards/bsp-boot-checklist.md) §1a update.
3. ✅ **Rust helper for reading current EL** — delivered by T-013 as `pub fn tyrne_hal::cpu::current_el() -> u8` (free function chosen per ADR-0024 §Open questions). UNSAFE-2026-0018 audits the helper; UNSAFE-2026-0016's T-013 Amendment records the load-bearing-post-condition shift now that `boot.s` actually drives EL1.
4. ✅ **Tests** — QEMU smoke at default config (EL1 entry) and at `-machine virtualization=on` (EL2 entry) both verified by the maintainer 2026-04-27 prior to PR #9 merge; identical trace post-`post_eret` confirms the EL drop's correctness on both paths.
5. ✅ **Exception infrastructure and interrupt delivery** *(In Review 2026-04-28)* — delivered by [T-012](../../analysis/tasks/phase-b/T-012-exception-and-irq-infrastructure.md) across three commits. Closes the deferred halves of ADR-0010 (`Timer::arm_deadline` / `cancel_deadline` real on `QemuVirtCpu`) and ADR-0022 first rider's *Sub-rider* (idle's WFI activation; `idle_entry` body is now `wait_for_interrupt` + `yield_now`). Three new audit entries: UNSAFE-2026-0019 (GIC v2 MMIO), UNSAFE-2026-0020 (vector table + asm trampolines), UNSAFE-2026-0021 (CNTV_CTL/CVAL writes). UNSAFE-2026-0014 gains an Amendment naming `irq_entry` as a future site of the same momentary-`&mut` pattern; ADR-0021 §Revision notes gains an Amendment extending the no-`&mut`-across-switch rule to the IRQ frame. v1's `irq_entry` is *ack-and-ignore* — masks `CNTV_CTL_EL0` + EOIs the GIC + returns; no scheduler-state mutation today. Future scheduler-touching arcs (preemption, `time_sleep_until` wake) follow the ADR-0021 Amendment's discipline. T-012 did not split into T-012a / T-012b; the substantive arc landed as one `In Review` task. Maintainer-side QEMU smoke + Miri pass remain pending per the same disclaimer T-013 used.

### Acceptance criteria

- ADR-0024 Accepted.
- Kernel boots at EL1 in all QEMU configurations we care about.
- Smoke test boots both QEMU variants and asserts the greeting still appears.
- `boot.s` starts with explicit IRQ masking.
- BSP boot checklist updated with the "mask DAIF before anything else" rule.
- **T-012 delivered (pending verification):** `arm_deadline` fires real IRQs through the GIC; `idle_entry`'s body is `wait_for_interrupt` + `yield_now` (closing ADR-0022's first rider in full). *(2026-04-28 — T-012 promoted to `In Review`; the implementation half is met as kernel-build + host-test gates; full closure to `Done` waits on maintainer-side QEMU verification of the deliberate-deadline path.)*

---

## Milestone B2 — MMU activation (kernel-half mapping)

Turn on the MMU with an identity map for the kernel image region and its stack. This is the foundation that per-task address spaces will layer atop.

### Sub-breakdown

1. **ADR-0027 — Kernel virtual memory layout.** Identity at `0x4000_0000` vs. high-half split; memory type attributes (normal cached for RAM, device-nGnRnE for MMIO). Mapping-mutation calls on the [`Mmu`](../../../hal/src/mmu.rs) trait should return a **typed "must-acknowledge" flush token** (analogous to `x86_64::structures::paging::MapperFlush`): each mutation produces a token that must be either explicitly `.flush()`-ed (executes the required TLB invalidation) or `.ignore()`-ed. Silent drop produces a compile-time warning. This keeps "did you remember to flush?" out of the reviewer's head and into the type system.
2. **Physical frame allocator** — a minimal bitmap or free-list allocator in the kernel. Needed before page tables can be populated.
3. **Initial page-table construction** — kernel mappings for `.text`, `.rodata`, `.data`, `.bss`, stack; MMIO mappings for the active UART and GIC.
4. **MMU activation sequence** — the exact `TTBR`, `TCR`, `MAIR`, `SCTLR` writes and the required barriers.
5. **Physical-frame capability (`MemoryRegionCap`) first real use.** Wires the capability system to actual memory.
6. **Tests** — kernel survives MMU activation (post-MMU `write_bytes` still produces greeting); deliberate invalid access traps (for exception-handler work, which lands here or in B5).

### Acceptance criteria

- ADR-0027 Accepted.
- Kernel runs with the MMU on.
- Physical frame allocator has host-tested correctness and a QEMU integration smoke.
- Deliberate traps route through the exception-vector table.

### Flags to resolve during B2

- 🚩 **Generation wrap (K3-1).** Does `MemoryRegionCap` slot churn plausibly reach `2^32` free-reuse cycles on a single slot? If yes, widen generation to `u64` or switch to a monotonic system-wide counter (write a successor ADR); if no, document the bound and move on. Decide while `MemoryRegionCap` is being wired.

### Informs

B3 builds per-task address spaces on top of this. B5 (syscall trap) reuses the exception-vector work.

---

## Milestone B3 — Address space abstraction

Multiple per-task translation tables. Capability-gated map / unmap. Activation on context switch (tie-in to A5's context switch, now post-B0 with raw-pointer scheduler API).

### Sub-breakdown

1. **ADR-0028 — Address-space data structure.** How a BSP-specific `AddressSpace` is represented; who owns its page tables; how it integrates with the `Mmu` trait's associated type.
2. **`AddressSpace` kernel object** — a new kernel-object type, like those from A3, with `AddressSpaceCap`.
3. **Map / unmap operations** — wrappers around [`Mmu::map`](../../../hal/src/mmu.rs) / `Mmu::unmap` that validate the caller's capabilities.
4. **TLB invalidation on unmap** — single-core only; multi-core is Phase C.
5. **Activation on context switch** — the context-switch path invokes [`Mmu::activate`](../../../hal/src/mmu.rs) when crossing between tasks with different address spaces.
6. **Tests** — isolation between two address spaces (a map in AS-X is not visible in AS-Y); activation round-trip.

### Acceptance criteria

- ADR-0028 Accepted.
- Two address spaces coexist; the kernel activates each when its owning task runs.
- Isolation verified on QEMU: AS-X cannot read AS-Y's data.

### Flags to resolve during B3

- 🚩 **Cross-table revocation — revisit.** If ADR-0023 was accept-deferred in B0, B3 is the point where a two-task-with-shared-endpoint scenario becomes concrete. If the limitation bites any specific B3 test, promote cross-table CDT to B4 or B5; otherwise confirm the deferral holds through to Phase C.

---

## Milestone B4 — Task loader

Load a userspace binary into an address space. For B4 the binary is statically embedded in the kernel image (e.g., `include_bytes!`); the filesystem / dynamic loading comes later.

### Sub-breakdown

1. **ADR-0029 — Initial userspace image format.** Raw flat binary vs. minimal ELF subset. v1 favours raw flat (simplest).
2. **Loader** — maps the embedded binary into a fresh address space under its `MemoryRegionCap`, sets up the initial stack, marks the entry point.
3. **Task creation from a binary** — `task_create_from_image(image, as_cap, initial_caps) -> TaskCap`.
4. **Tests** — host-side loader correctness (given an image blob, produce the expected mapping); QEMU-side task creation without yet running the task (that's B6).

### Acceptance criteria

- ADR-0029 Accepted.
- A kernel test can load the embedded userspace image into an address space and report the entry point and initial stack pointer.

---

## Milestone B5 — Syscall boundary

Traps from EL0 into EL1 via `SVC` (or the chosen mechanism). Syscall dispatch validates the caller's capabilities. Establish the initial syscall set and the calling convention.

### Sub-breakdown

1. **ADR-0030 — Syscall ABI.** Register calling convention (which regs carry syscall number vs. arguments vs. return); maximum arg count; error-return convention (register + flag vs. `Result`-like encoding); asynchronous vs. synchronous semantics. **Bundle K2-5:** design the full userspace error taxonomy as part of this ADR — split `IpcError::InvalidCapability` into `StaleHandle` / `MissingRight` / `WrongObjectKind` (code review §Correctness IPC bullet 4) so the syscall error space and the in-kernel error space agree from the start.
2. **ADR-0031 — Initial syscall set for B-phase.** At minimum: `send`, `recv`, `console_write` (debug-gated), `task_yield`, `task_exit`. No more in v1.
3. **Exception-vector dispatch** — the EL0-synchronous vector routes to a Rust syscall dispatcher after saving user registers.
4. **Syscall dispatcher** — maps a syscall number to a handler, validates capabilities, performs the operation, returns. **Must be panic-free on every untrusted input** (typed error for every failure path), consistent with B0's hardening pattern.
5. **Copy-from / copy-to user** — validated access to userspace memory through the active address space. No raw dereferencing of user pointers.
6. **`Capability::Debug` redaction (K3-9).** Before `console_write` can log a `Capability` value (it never should, but userspace-reachable log paths demand defense-in-depth), redact the derived `Debug` impl on `Capability` — either a custom impl that prints `Capability { rights: …, object: <redacted> }` or a `Redacted<T>` wrapper type. Security review §6.
7. **Tests** — host-side ABI encoder/decoder tests; QEMU smoke where a kernel-stub "userspace" makes a syscall.

### Acceptance criteria

- ADR-0030 and ADR-0031 Accepted.
- Syscall entry works from EL0 back to EL1 and back; register state is preserved correctly.
- Invalid syscalls (bad number, missing capability, out-of-bounds pointer) return typed errors without panicking.
- Copy-from-user never dereferences raw user pointers outside the validated mapping.
- `IpcError` variants are split per ADR-0030's taxonomy; all call sites and tests updated.
- `Capability` `Debug` output redacts security-sensitive fields.

### Flags to resolve during B5

- 🚩 **Fault containment (K3-4).** Task-body `.expect` / `panic!` still halts the whole kernel today. The syscall dispatcher itself must be panic-free (acceptance criterion above), but full fault containment (a supervisor endpoint the crashing task's parent can observe) is Phase E work (first real driver task). Decision at B5: confirm the split — dispatcher panic-free now, supervisor design deferred. Recommendation: defer to Phase E.
- 🚩 **`IpcError` split timing.** If ADR-0030 becomes too large, split the error-taxonomy portion into a sibling ADR and implement it in parallel; ensure both land before the first userspace call.

---

## Milestone B6 — First userspace "hello"

A real userspace task, loaded by B4, running in EL0 in its own address space, makes a `console_write` syscall, and exits cleanly via `task_exit`.

### Sub-breakdown

1. **Userspace "hello" program** — a minimal `no_std, no_main` binary living in `userland/hello/` (new crate) that calls the syscall ABI directly.
2. **Wire-up** — kernel loads this binary on boot via B4, creates a task in its AS (via B3), schedules it (via A5 + B0), runs it (via B1/B2/B5).
3. **Syscall library** — a small `tyrne-user` crate exposing safe wrappers for the B5 syscalls.
4. **QEMU smoke** — trace shows kernel greeting + userspace greeting in correct order + task_exit + kernel shutdown message.
5. **Guide** — `docs/guides/first-userspace.md` explains what this demonstrates.
6. **Performance review** — first hypothesis-driven cycle using the timer introduced in B0. Measure IPC round-trip, context-switch, boot time; compare against A6 baseline.
7. **Business review** — Phase B retrospective.

### Acceptance criteria

- Userspace "hello from userspace" appears on the serial console after the kernel's greeting.
- Userspace can call `task_exit` cleanly; the kernel reports task termination.
- Guide: `docs/guides/first-userspace.md` committed.
- Performance review recording IPC round-trip and context-switch numbers against the A6 baseline.
- Business review recording Phase B retrospective.

### Flags to resolve during B6

- 🚩 **CI rollout (K3-7).** If a CI pipeline exists by B6, wire the QEMU smoke as a regression gate (`qemu-system-aarch64 ... | grep "all tasks complete"`). If CI is still absent, defer to Phase C.
- 🚩 **`cargo-vet init` (K3-8).** Required only if any external dependency landed by B6 (none planned, but if a crate is added anywhere in B1–B6 this becomes a prerequisite for that PR).
- 🚩 **`write_bytes` TX timeout (K3-5).** Only applies to non-QEMU BSPs. If bsp-qemu-virt is still the only BSP, defer to the first non-QEMU BSP (Pi 4 in Phase D). Otherwise add the timeout cap.

### Phase B closure

When B6 is Done, run a business review. Phase C becomes active after that review.

---

## ADR ledger for Phase B (post-review)

| ADR | Purpose | Expected state | Note |
|-----|---------|----------------|------|
| ADR-0021 | Raw-pointer scheduler API (UNSAFE-2026-0012 resolution) | B0 | new — from 2026-04-21 security review blocker #1 |
| ADR-0022 | Idle task + typed scheduler deadlock error | B0 | new — from 2026-04-21 security review blocker #3 |
| ADR-0023 | Cross-table capability revocation policy | B0 (accept-deferred expected) | new — from 2026-04-21 security review blocker #2 |
| ADR-0024 | EL drop policy | B1 (Accepted 2026-04-27) | was ADR-0021 in the pre-review plan |
| ADR-0025 | ADR governance amendments (forward-reference, riders) | meta-process (Accepted 2026-04-27) | new — captures the rules T-006/T-009 retros surfaced; not B-phase content. Cool-down rule withdrawn pre-Accept; see ADR-0025 §Revision notes |
| ADR-0026 | Exception-vector-table / handler-dispatch shape (T-012, conditional) | B1 | reserved by T-012 if non-obvious choices arise; may go unused if T-012 absorbs the exception-vector design without a separate ADR |
| ADR-0027 | Kernel virtual memory layout | B2 | was ADR-0025 in the pre-2026-04-27 plan; renumbered down by 2 because ADR-0025 (governance) and ADR-0026 (T-012 reservation) consumed slots |
| ADR-0028 | Address-space data structure | B3 | was ADR-0026 |
| ADR-0029 | Initial userspace image format | B4 | was ADR-0027 |
| ADR-0030 | Syscall ABI (includes `IpcError` taxonomy per K2-5) | B5 | was ADR-0028; scope still enlarged to cover error taxonomy |
| ADR-0031 | Initial syscall set | B5 | was ADR-0029 |

Numbers are tentative. Final numbers are assigned when the ADR is actually written, per [ADR-0013](../../decisions/0013-roadmap-and-planning.md).

---

## Open questions / flagged decisions (Phase B)

### Decisions that must close during their named milestone

- 🚩 **B0 — Cross-table capability revocation.** Accept-deferred (recommended) vs. implement-now. Answer locks ADR-0023.
- 🚩 **B0 — Architecture-doc scope.** Whether notifications get their own architecture doc in B0 or later.
- 🚩 **B2 — Generation wrap-around (K3-1).** Raise counter, monotonic scheme, or document the bound.
- 🚩 **B3 — Cross-table revocation, revisit.** If the deferred ADR-0023 decision bites any B3 test, promote.
- 🚩 **B5 — Fault containment scope (K3-4).** Confirm the split: dispatcher panic-free in B5, supervisor endpoint in Phase E.
- 🚩 **B5 — `IpcError` split timing.** Bundle with ADR-0030 or split into its own ADR.
- 🚩 **B6 — CI rollout timing (K3-7).** Wire QEMU-smoke regression gate if CI exists.
- 🚩 **B6 — `cargo-vet init` (K3-8).** Prerequisite only if an external dep lands during Phase B.
- 🚩 **B6 — `write_bytes` TX timeout (K3-5).** Only applies when a non-QEMU BSP exists.

### Watch-list items (monitored, no decision required unless triggered)

- **K3-13 — TPIDR_EL0 save-set.** If any Phase B milestone introduces TLS at EL1, extend `Aarch64TaskContext` and `context_switch_asm` in the same commit; update UNSAFE-2026-0008 audit entry.
- **Priority classes (ADR-0019 open question).** Single class remains the Phase B default; multiple classes may become a driver for Phase C's preemption work.

---

## How to start Phase B

1. Open **T-006** (raw-pointer scheduler API refactor) via the [`start-task`](../../../.claude/skills/start-task/SKILL.md) skill. Writing ADR-0021 is the first step inside that task.
2. After T-006 is In Progress, parallel work on **T-008** (architecture docs) is safe — they do not touch the same code.
3. **T-007** (idle task + typed deadlock) should follow T-006 so both changes land on top of the settled `Scheduler` shape.
4. **T-009** (timer init) can run in parallel with any of the above — it only touches `QemuVirtCpu` and does not intersect the scheduler refactor.
5. **T-011** (missing tests) comes last within B0 so the tests are written against the final shape of the code they exercise.
6. B1 starts only after B0 closes with its business review (short milestone retrospective per ADR-0013).
