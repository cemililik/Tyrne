# T-009 — Timer init + `CNTPCT_EL0` measurement

- **Phase:** B
- **Milestone:** B0 — Phase A exit hygiene
- **Status:** In Progress
- **Created:** 2026-04-23
- **Author:** @cemililik (+ Claude Opus 4.7 agent)
- **Dependencies:** none — independent of T-006/T-007/T-008/T-011 within B0; only touches `QemuVirtCpu` and the BSP demo flow.
- **Informs:** Unblocks the first hypothesis-driven performance review cycle (the [A6 baseline](../../reviews/performance-optimization-reviews/2026-04-21-A6-baseline.md) is gated on a working monotonic counter). Future *IRQ wiring* task — when one is opened — will build on `QemuVirtCpu`'s `Timer` impl to enable [ADR-0022](../../../decisions/0022-idle-task-and-typed-scheduler-deadlock.md)'s WFI deferral.
- **ADRs required:** none new — [ADR-0010](../../../decisions/0010-timer-trait.md) already pins the trait shape; T-009 is implementation only.

---

## User story

As the Tyrne kernel running on QEMU virt aarch64, I want a working monotonic time source via the ARM Generic Timer's `CNTPCT_EL0` and `CNTFRQ_EL0` system registers — so that performance reviews can measure IPC round-trip latency and context-switch overhead in real nanoseconds rather than instruction counts, and so that future preemption / scheduling work has the time-source primitive it depends on already in place.

## Context

[ADR-0010](../../../decisions/0010-timer-trait.md) accepted the `Timer` trait shape on 2026-04-20 — four object-safe methods with `u64` nanoseconds throughout. Phase A shipped the trait declaration in `tyrne-hal` and a `FakeTimer` in `tyrne-test-hal`, but the BSP's `QemuVirtCpu` never implemented it. The 2026-04-21 [A6 baseline performance review](../../reviews/performance-optimization-reviews/2026-04-21-A6-baseline.md) §"What we did not measure" explicitly notes: *"Measuring IPC latency requires a free-running timer readable at EL1. The system counter (`CNTPCT_EL0`) needs the `CNTKCTL_EL1.EL0PCTEN` bit set, and a counter frequency (`CNTFRQ_EL0`) must be initialised — neither is done in Phase A."*

T-009 closes that gap for the **measurement-only** path. The full deadline-arming half of `Timer` (`arm_deadline` / `cancel_deadline`) requires GIC + interrupt-vector-table wiring that is its own future task — phase-b.md §B0 item 5 explicitly says *"wire a free-running counter so IPC round-trip latency and context-switch overhead can be measured"*, scoping out IRQ delivery. The two unimplemented methods are left as `unimplemented!()` with an explicit deferral message so a regression that wires arm_deadline elsewhere does not silently no-op.

ADR-0022's first revision-notes rider expects T-009 to bring WFI back into idle's loop: *"When T-009 wires a timer IRQ (and a fallback wake source is guaranteed), this loop's body becomes `cpu.wait_for_interrupt(); yield_now`."* That rider's precondition (timer IRQ wired) is **not** satisfied by T-009 — only by a follow-up IRQ task. So T-009 leaves idle's body unchanged (`spin_loop + yield_now`) and updates the rider to be explicit about the two-task path.

## Acceptance criteria

- [x] **`QemuVirtCpu` implements `tyrne_hal::Timer`** with four methods:
  - [ ] `now_ns(&self) -> u64` — reads `CNTPCT_EL0`, multiplies by the cached resolution, returns nanoseconds since boot. Monotonic by hardware contract.
  - [ ] `resolution_ns(&self) -> u64` — derived once at construction from `CNTFRQ_EL0` as `1_000_000_000 / freq_hz`.
  - [ ] `arm_deadline(&self, _: u64)` — `unimplemented!()` with a message naming the missing IRQ-wiring task.
  - [ ] `cancel_deadline(&self)` — `unimplemented!()` with the same naming.
- [ ] **`QemuVirtCpu::new` reads `CNTFRQ_EL0` and asserts non-zero.** The frequency is cached on the struct; subsequent `now_ns` calls do not re-read it.
- [ ] **No overflow on the multiplication.** Resolution is computed once (≤ 64-bit divide); `now_ns = ticks * resolution_ns` cannot overflow before ~150 years of boot time at the lowest QEMU virt frequency. Documented inline; no `u128` intermediate needed.
- [ ] **Audit entry UNSAFE-2026-0015** for the new `MRS CNTPCT_EL0` and `MRS CNTFRQ_EL0` reads. Same shape as UNSAFE-2026-0007 (read-only system registers, defined behaviour at EL1, no state mutation).
- [ ] **BSP measurement instrumentation.** `kernel_entry` records `now_ns()` at boot start and at "all tasks complete"; the demo prints elapsed nanoseconds for the boot-to-end window so QEMU output makes the measurement visible.
- [ ] **Tests stay green.** 77 kernel + 34 test-hal = 111 host tests; `cargo +nightly miri test --workspace --exclude tyrne-bsp-qemu-virt` clean; QEMU smoke now produces the A6 five-line trace **plus** the elapsed-ns line.
- [ ] **Documentation:** ADR-0022 first rider updated to make the two-step (T-009 measurement → future IRQ task → WFI in idle) sequence explicit; phase-b.md / current.md / task index updated; README capabilities list updated.

## Out of scope

- **`arm_deadline` / `cancel_deadline` real implementation.** Needs GIC v2 / v3 wiring on QEMU virt + interrupt-vector-table + handler dispatch. That is its own task whenever it is opened.
- **Idle's WFI activation.** Depends on a wake source which T-009 does not provide. ADR-0022 first rider stays open.
- **`CNTKCTL_EL1.EL0PCTEN` setup.** Only relevant for EL0 access; the kernel runs at EL1 in v1.
- **Per-core time alignment.** Single-core only; multi-core counter coordination is Phase C / SMP work.
- **Timer-trait `Mutex` / locking around the cached frequency.** A bare `u64` field works because the value is set once at `new()` and read-only afterwards.
- **Performance review document.** T-009 makes measurement *possible*; writing the first hypothesis-driven perf review is a separate cycle (and skill-driven via [`conduct-review`](../../../../.claude/skills/conduct-review/SKILL.md)).
- **Architecture doc for the Timer subsystem.** Bundled with T-008 if the pattern repeats; not on T-009.

## Approach

ADR-0010 settled the trait shape; T-009 is implementation. In commit order:

1. **Implement `Timer` for `QemuVirtCpu`** in [`bsp-qemu-virt/src/cpu.rs`](../../../../bsp-qemu-virt/src/cpu.rs):
   - Add a `frequency_hz: u64` field and a `resolution_ns: u64` field. Both populated in `new()` from a `MRS CNTFRQ_EL0` read; resolution is `1_000_000_000 / freq` rounded down. Drop `const fn` because reading a system register at construction is not const.
   - `now_ns` issues `MRS xN, CNTPCT_EL0` and returns `count * self.resolution_ns`. Inline comment explains the overflow margin and the precision contract from ADR-0010.
   - `arm_deadline` / `cancel_deadline` panic with a message naming the missing IRQ-wiring task — not silent `()` returns, because a silent no-op would let a future caller think the deadline was armed.
2. **Audit entry UNSAFE-2026-0015** in [`docs/audits/unsafe-log.md`](../../../audits/unsafe-log.md). Append-only — no edits to existing entries. Cite UNSAFE-2026-0007 for prior precedent; rejected alternatives section explains why no safe HAL wrapper exists.
3. **BSP instrumentation** in [`bsp-qemu-virt/src/main.rs`](../../../../bsp-qemu-virt/src/main.rs): record `now_ns()` at the top of `kernel_entry` (cached in a local), and at "all tasks complete" inside `task_a`. Print the delta as a final line.
4. **Verification.** Full gate sweep: `cargo fmt`, `cargo host-test`, `cargo host-clippy`, `cargo kernel-build`, `cargo kernel-clippy`, `cargo +nightly miri test --workspace --exclude tyrne-bsp-qemu-virt`, QEMU smoke shows new line.
5. **Documentation** sweep: ADR-0022 rider, phase-b row, task index, current.md, README, glossary if needed.

## Definition of done

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo host-clippy` clean with `-D warnings`.
- [ ] `cargo kernel-clippy` clean.
- [ ] `cargo host-test` passes (111 host tests; T-009 adds none — implementation tested via QEMU smoke + miri stays clean).
- [ ] `cargo +nightly miri test --workspace --exclude tyrne-bsp-qemu-virt` clean.
- [ ] `cargo kernel-build` clean.
- [ ] QEMU smoke reproduces the A6 five-line trace **plus** an elapsed-ns line; numerical value within the expected QEMU virt range (typically 100k–1M ns).
- [ ] No new `unsafe` block without an audit entry; UNSAFE-2026-0015 written.
- [ ] Commit messages follow [`commit-style.md`](../../../standards/commit-style.md) with `Refs: ADR-0010` and `Audit: UNSAFE-2026-0015` trailers.
- [ ] Task status updated to `In Review`; [`docs/roadmap/current.md`](../../../roadmap/current.md) updated.

## Design notes

- **Why drop `const fn` from `new()`?** Reading `CNTFRQ_EL0` is a runtime system-register access; it cannot run in const context. The existing `pub const unsafe fn new() -> Self` was defensive — no caller actually used it as `const`. Verified: `bsp-qemu-virt/src/main.rs:451` is the only construction site, called from `kernel_entry` at runtime.
- **Why cache resolution rather than dividing on every `now_ns`?** ARM ARM does not require `CNTFRQ_EL0` to be a constant across the boot lifetime, but in practice it is — set once by firmware. Computing `1_000_000_000 / freq` once at construction saves a 64-bit divide per `now_ns` call (~tens of cycles on Cortex-A72). The trait contract permits sub-resolution precision loss, so the integer division is acceptable.
- **Why `unimplemented!()` rather than silent `()` for `arm_deadline` / `cancel_deadline`?** A silent no-op breaks the trait contract: callers expect "the IRQ fires when now_ns reaches deadline_ns" and would receive nothing. Loud panic with an explicit deferral message ("requires IRQ-wiring task — not yet implemented") makes the gap unambiguous. v1 has no caller of these methods (idle still spin-loops); the panic is unreachable today.
- **Why no `arm_deadline` test today?** No callers, no IRQ wiring, no hardware path. Adding a test of `unimplemented!()` would only assert the panic, which is policy-defensive but not load-bearing. Routed to whichever task wires GIC + IRQ vector; that task's tests assert real arm/fire behaviour.
- **Why no `ISB` before `MRS CNTPCT_EL0`?** ARM ARM allows the counter read to be reordered with respect to prior memory operations, but two consecutive `MRS CNTPCT_EL0` reads are guaranteed to return non-decreasing values (counter monotonicity holds at the architecture level). For latency measurement we want a tight read; the ISB is added later if drift shows up in measurements.
- **Frequency on QEMU virt vs. real hardware.** QEMU virt sets `CNTFRQ_EL0 = 62_500_000` (62.5 MHz, resolution 16 ns). Cortex-A72 on Pi 4 has 19.2 MHz (resolution 52 ns). The implementation handles both because it reads the firmware-provided value rather than hard-coding.
- **Projected commit sequence.**
  1. `docs(roadmap): open T-009 — timer init + CNTPCT_EL0 (B0)` (this opening commit).
  2. `feat(bsp): implement Timer for QemuVirtCpu via CNTPCT_EL0 / CNTFRQ_EL0 (T-009)` + audit entry.
  3. `feat(bsp): instrument kernel_entry boot-to-end measurement (T-009)`.
  4. `docs(roadmap): T-009 → In Review`.

## References

- [ADR-0010 — Timer HAL trait signature](../../../decisions/0010-timer-trait.md) — the trait shape T-009 implements.
- [ADR-0008 — Cpu HAL trait](../../../decisions/0008-cpu-trait.md) — establishes the inline-asm / system-register pattern T-009 reuses.
- [ADR-0022 — Idle task and typed scheduler deadlock](../../../decisions/0022-idle-task-and-typed-scheduler-deadlock.md) — its first rider expects T-009 to enable WFI; T-009 closes the *measurement* half, not the WFI half.
- [Phase B plan §B0 item 5](../../../roadmap/phases/phase-b.md) — scope statement.
- [A6 baseline performance review](../../reviews/performance-optimization-reviews/2026-04-21-A6-baseline.md) — gating measurement work.
- [UNSAFE-2026-0007](../../../audits/unsafe-log.md) — precedent audit entry for inline-asm system-register reads.
- ARM *Architecture Reference Manual* DDI 0487G.b §D11 — Generic Timer (`CNTPCT_EL0`, `CNTFRQ_EL0`).

## Review history

| Date | Reviewer | Note |
|------|----------|------|
| 2026-04-23 | @cemililik (+ Claude Opus 4.7 agent) | opened with status `In Progress`. Scope deliberately narrow: measurement only; deadline arming + WFI activation belong to a follow-up IRQ-wiring task. ADR-0022's first rider stays open. `current.md` will be updated to point at T-009 in the same commit. |
