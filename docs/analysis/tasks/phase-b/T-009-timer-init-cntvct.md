# T-009 — Timer init + `CNTVCT_EL0` measurement

- **Phase:** B
- **Milestone:** B0 — Phase A exit hygiene
- **Status:** Done
- **Created:** 2026-04-23
- **Author:** @cemililik (+ Claude Opus 4.7 agent)
- **Dependencies:** none — independent of T-006/T-007/T-008/T-011 within B0; only touches `QemuVirtCpu` and the BSP demo flow.
- **Informs:** Unblocks the first hypothesis-driven performance review cycle (the [A6 baseline](../../reviews/performance-optimization-reviews/2026-04-21-A6-baseline.md) is gated on a working monotonic counter). Future *IRQ wiring* task — when one is opened — will build on `QemuVirtCpu`'s `Timer` impl to enable [ADR-0022](../../../decisions/0022-idle-task-and-typed-scheduler-deadlock.md)'s WFI deferral.
- **ADRs required:** none new — [ADR-0010](../../../decisions/0010-timer-trait.md) already pins the trait shape; T-009 is implementation only.

---

## User story

As the Tyrne kernel running on QEMU virt aarch64, I want a working monotonic time source via the ARM Generic Timer's `CNTVCT_EL0` (virtual counter) and `CNTFRQ_EL0` system registers — so that performance reviews can measure IPC round-trip latency and context-switch overhead in real nanoseconds rather than instruction counts, and so that future preemption / scheduling work has the time-source primitive it depends on already in place. The virtual counter is chosen over the physical (`CNTPCT_EL0`) so the read side and the deferred deadline-arming side (`CNTV_CVAL_EL0` / `CNTV_CTL_EL0`, named in ADR-0010's References list) live in the same register family — preventing a silent break the moment any boot path leaves a non-zero `CNTVOFF_EL2`.

## Context

[ADR-0010](../../../decisions/0010-timer-trait.md) accepted the `Timer` trait shape on 2026-04-20 — four object-safe methods with `u64` nanoseconds throughout. Phase A shipped the trait declaration in `tyrne-hal` and a `FakeTimer` in `tyrne-test-hal`, but the BSP's `QemuVirtCpu` never implemented it. The 2026-04-21 [A6 baseline performance review](../../reviews/performance-optimization-reviews/2026-04-21-A6-baseline.md) §"What we did not measure" notes that measuring IPC latency requires a free-running timer readable at EL1 and `CNTFRQ_EL0` must be initialised — neither is done in Phase A. (The baseline-review wording mentioned `CNTPCT_EL0`; T-009's implementation reads `CNTVCT_EL0` instead per the register-family alignment described in the User story.)

T-009 closes that gap for the **measurement-only** path. The full deadline-arming half of `Timer` (`arm_deadline` / `cancel_deadline`) requires GIC + interrupt-vector-table wiring that is its own future task — phase-b.md §B0 item 5 explicitly says *"wire a free-running counter so IPC round-trip latency and context-switch overhead can be measured"*, scoping out IRQ delivery. The two unimplemented methods are left as `unimplemented!()` with an explicit deferral message so a regression that wires arm_deadline elsewhere does not silently no-op.

ADR-0022's first revision-notes rider expects T-009 to bring WFI back into idle's loop: *"When T-009 wires a timer IRQ (and a fallback wake source is guaranteed), this loop's body becomes `cpu.wait_for_interrupt(); yield_now`."* That rider's precondition (timer IRQ wired) is **not** satisfied by T-009 — only by a follow-up IRQ task. So T-009 leaves idle's body unchanged (`spin_loop + yield_now`) and updates the rider to be explicit about the two-task path.

## Acceptance criteria

- [x] **`QemuVirtCpu` implements `tyrne_hal::Timer`** with four methods. Commit `beb0963` (initial); register-family swap to `CNTVCT_EL0` in commit `39fb66c`.
  - [x] `now_ns(&self) -> u64` — reads `CNTVCT_EL0`, forwards to [`tyrne_hal::timer::ticks_to_ns`] which performs `count * 1e9 / frequency_hz` via a 128-bit intermediate. Monotonic by hardware contract; saturating cast back to `u64` preserves monotonicity at the ~584-year extreme.
  - [x] `resolution_ns(&self) -> u64` — round-to-nearest result of [`tyrne_hal::timer::resolution_ns_for_freq`] cached at construction; floor of 1 ns for high-frequency counters > 2 GHz.
  - [x] `arm_deadline(&self, _: u64)` — `unimplemented!()` with a message naming the missing IRQ-wiring task ([T-012](T-012-exception-and-irq-infrastructure.md), Draft).
  - [x] `cancel_deadline(&self)` — `unimplemented!()` with the same naming.
- [x] **`QemuVirtCpu::new` reads `CNTFRQ_EL0` and asserts non-zero.** The frequency is cached on the struct as `frequency_hz`; resolution is pre-computed and cached as `resolution_ns`. Commit `beb0963`.
- [x] **No overflow on the conversion.** Implementation uses `tyrne_hal::timer::ticks_to_ns`, which performs the `count * 1e9 / frequency_hz` arithmetic via a `u128` intermediate and saturates the cast back to `u64`. Overflow margin: ~584 years at any frequency (since `count * resolution_ns ≈ elapsed_ns ≤ u64::MAX = ~584 years`); the saturating cast preserves `Timer::now_ns`'s monotonicity at the rare extreme rather than wrapping. **Earlier inline-multiply form (`count * resolution_ns` with `wrapping_mul`) was rejected during second-read review** because it (a) silently wraps on overflow, breaking ADR-0010 monotonicity, and (b) drifts on non-divisor frequencies (e.g. 19.2 MHz → 0.16 % drift over time). See review-history row dated 2026-04-23.
- [x] **Audit entries** for the new `unsafe` surface, all append-only per unsafe-policy §3:
  - **UNSAFE-2026-0015** (commit `beb0963`) for the original `MRS CNTPCT_EL0` / `MRS CNTFRQ_EL0` reads.
  - **UNSAFE-2026-0015 Amendment** (2026-04-23 second-read fix) recording the switch from `CNTPCT_EL0` to `CNTVCT_EL0` (register-family alignment with ADR-0010), the EL precondition tightening to cite ADR-0012 explicitly, and the saturating-arithmetic move into `tyrne_hal::timer::ticks_to_ns`.
  - **UNSAFE-2026-0006 Amendment** (2026-04-23 second-read fix) recording the post-T-009 struct shape — the `frequency_hz` and `resolution_ns` fields invalidated the original entry's "zero-size type with no fields" wording.
- [x] **BSP measurement instrumentation.** `kernel_entry` snapshots `now_ns()` into `BOOT_NS: StaticCell<u64>` after the timer banner; `task_a`'s "all tasks complete" path reads back the snapshot and prints the elapsed nanoseconds. Commit `55f2d10`.
- [x] **Tests stay green.** 77 kernel + 34 test-hal = 111 host tests; `cargo +nightly miri test --workspace --exclude tyrne-bsp-qemu-virt` remains clean; QEMU smoke now produces the A6 five-line trace plus a `tyrne: timer ready (62500000 Hz, resolution 16 ns)` line and a `tyrne: boot-to-end elapsed = … ns` line.
- [x] **Documentation:** ADR-0022 first rider's *Sub-rider — WFI activation requires *two* tasks, not one* spells out that T-009 is the time-source half and a separate IRQ-wiring task is the IRQ-delivery half; phase-b.md / current.md / task index updated; glossary gains `CNTPCT_EL0`, `CNTFRQ_EL0`, and `Generic Timer (ARM)`; README status line updated to reflect Phase B underway and the umbra-etymology line replaced with Tyrne's clean-slate identity per the project memory.

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

- [x] `cargo fmt --all -- --check` clean.
- [x] `cargo host-clippy` clean with `-D warnings`.
- [x] `cargo kernel-clippy` clean.
- [x] `cargo host-test` passes (111 host tests; T-009 adds none — implementation tested via QEMU smoke + miri stays clean).
- [x] `cargo +nightly miri test --workspace --exclude tyrne-bsp-qemu-virt` clean.
- [x] `cargo kernel-build` clean.
- [x] QEMU smoke reproduces the A6 five-line trace plus an elapsed-ns line; observed value on QEMU virt: `boot-to-end elapsed = 10240992 ns` (QEMU-virtual time, not wall-clock realistic — value sanity-checks because it is positive and within order-of-magnitude expectation).
- [x] No new `unsafe` block without an audit entry; UNSAFE-2026-0015 written.
- [x] Commit messages follow [`commit-style.md`](../../../standards/commit-style.md) with `Refs: ADR-0010` and `Audit: UNSAFE-2026-0015` trailers.
- [x] Task status updated to `In Review`; [`docs/roadmap/current.md`](../../../roadmap/current.md) updated.

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
| 2026-04-23 | @cemililik (+ Claude Opus 4.7 agent) | Implementation complete. Three commits landed: `beb0963` (`QemuVirtCpu` Timer impl + UNSAFE-2026-0015 audit entry; `frequency_hz` and `resolution_ns` cached at `new()`; `arm_deadline` / `cancel_deadline` `unimplemented!()` with explicit deferral messages), `55f2d10` (BSP boot-to-end instrumentation: `BOOT_NS` snapshot + `tyrne: timer ready` banner + `tyrne: boot-to-end elapsed` final line). Verification: 111 host tests green, miri 111/111 clean, fmt/host-clippy/kernel-clippy clean, kernel-build clean, QEMU smoke shows new lines bracketing the unchanged A6 trace. Documentation sweep: ADR-0022 sub-rider clarifying T-009 (time source) vs. future IRQ-wiring task (IRQ delivery); glossary +3 entries (`CNTPCT_EL0`, `CNTFRQ_EL0`, `Generic Timer`); README status + identity lines refreshed; phase-b.md + task index + current.md status flipped. Status → `In Review`. |
| 2026-04-23 | @cemililik (+ two independent review agents) | Second-read review surfaced three High findings, all addressed in this commit arc. (1) **Register family**: ADR-0010's *References* and ADR-0022's first-rider sub-rider both name the **virtual** family (`CNTVCT_EL0`, `CNTV_*`); the original implementation read `CNTPCT_EL0` and would have silently mismatched the deferred deadline-arming side once `CNTVOFF_EL2 ≠ 0`. Switched the read to `CNTVCT_EL0`; UNSAFE-2026-0015 gains an Amendment recording the change. (2) **Arithmetic correctness**: `count * resolution_ns` (with `wrapping_mul`) was *not* nanoseconds on non-divisor frequencies — at 19.2 MHz, drift ≈ 0.16 % ≈ 138 s/day; *and* `wrapping_mul` would silently break ADR-0010 monotonicity at the wrap edge. Conversion extracted to `tyrne_hal::timer::ticks_to_ns`, which uses 128-bit intermediate arithmetic and a saturating cast back to `u64`. Pure helpers (`ticks_to_ns`, `resolution_ns_for_freq`, `NANOS_PER_SECOND`) live in the HAL crate so host unit tests can exercise them without inline asm — 13 new tests in `hal/src/timer.rs` cover the QEMU-virt-divisor, Pi-3-class non-divisor, 1 GHz, saturation, and round-to-nearest cases. (3) **Audit-policy compliance**: UNSAFE-2026-0006's body still claimed "zero-size type with no fields"; T-009 had silently rewritten the in-source SAFETY comment without amending the audit entry. Appended an Amendment recording the post-T-009 struct shape. Also corrected: SAFETY comments now cite ADR-0012's "QEMU virt delivers at EL1, non-VHE" precondition rather than asserting "unconditionally available at EL1"; the overflow-margin comment that said "18 millennia at QEMU virt" was arithmetically wrong (the answer is ~584 years independent of frequency since `count * resolution_ns ≈ elapsed_ns ≤ u64::MAX`); glossary's Pi 4 frequency claim softened (BCM2711 mainline rate is 54 MHz, not the 19.2 MHz Pi 3 figure originally written) — BSPs read firmware regardless. Verification after fixes: 77 kernel + 13 hal + 34 test-hal = **124 host tests green**, miri 124/124 clean, all clippy/fmt/build gates clean, QEMU smoke unchanged shape. |
| 2026-04-27 | @cemililik (+ Claude Opus 4.7 agent) | Self-audit of the 2026-04-23 second-read fix-up commit (`39fb66c`) found that Review 1's High #1 was only **half** addressed: the documentation half (ADR-0012 cite, EL precondition wording, removal of "unconditional" claim) landed correctly, but the *runtime check* the recommendation also asked for ("(b) ... and add a runtime check that panics with a clear error if not") was silently skipped. Closed by adding a `MRS CurrentEL` boot-time self-check at the head of `QemuVirtCpu::new`, asserting EL == 1 with a named panic message ("must run at EL1 per ADR-0012; observed EL{n} instead"). One new audit entry — **UNSAFE-2026-0016** — covers the new MRS in the same shape as UNSAFE-2026-0007 (read-only system register at EL≥1, no state mutation). Acceptance criteria and DoD remain met: 124 host tests green, miri clean, all clippy/fmt/build gates clean, QEMU smoke shows the assertion path passes (we are at EL1) and the boot trace continues unchanged. T-009's full Review 1 + Review 2 finding-set is now closed; the lesson — second-read review-recommendations have AND/OR structure, partial accept = unfinished — is worth carrying into the next task's DoD checklist. |
| 2026-04-27 | @cemililik (+ Claude Opus 4.7 agent) | R3 round (commits `01010f4` + `328cab5`): three additional findings closed — install guard for `cargo install --locked` race, explicit `frequency_hz == 0` panic at the helper boundary, and a resolution-floor edge case for >2 GHz counters where the round-to-nearest formula would truncate to `0` ns. Three new tests in `hal/src/timer.rs` (`ticks_to_ns_is_monotonic_across_frequencies`, `ticks_to_ns_plateaus_at_u64_max_after_saturation`, plus the floor cases), bringing the host-test total from 124 → 130 (77 kernel + **19 hal** + 34 test-hal). Audit-log readability fix: UNSAFE-2026-0015's Amendment lead lines reflowed for casual-reader scanability. miri still 130/130 clean; all gates green. |
| 2026-04-27 | @cemililik (+ independent approval-review agent) | Promoted `In Review → Done` after independent agent verified all R1/R2/R3 accepted findings landed and that the host-test count is genuinely 130, not the older 124 cited above. Verdict captured in the [T-009 mini-retro second follow-up note](../../reviews/business-reviews/2026-04-27-T-009-mini-retro.md). |
