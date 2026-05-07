# Track D — Performance / footprint (post-merge)

- **Agent run by:** Claude Opus 4.7 (1M context), 2026-05-07.
- **Scope:** Performance / footprint axis-pass over the 6 PRs that landed since the 2026-04-28 B1 closure baseline — PR #12 (T-014 idle dispatch), PR #13 (Track-E doc-fix sweep), PR #14 (TyrneOS → Tyrne URL rename + Yüksek → High), PR #15 (kernel/HAL/BSP code polish, including the `register_idle` `debug_assert! → assert!` upgrade and the γ.6 "defensive loop after `start()`" revert), PR #16 (closure-trio docs + the 2026-05-07 perf re-baseline), PR #17 (T-015 / ADR-0032 — `ipc_cancel_recv` + symmetric Deadlock rollback).
- **HEAD reviewed:** `c258ee3` (T-015 doc-side commit, branch `t-015-endpoint-rollback-cancel-recv`; PR #17 merged at `8dc433e`).
- **Comparison anchor:** the [2026-05-07 B1-closure perf re-baseline](../../performance-optimization-reviews/2026-05-07-B1-closure.md) (`.text` 21,792 / `.rodata` 2,928 / `.bss` 22,256) — i.e. post-PR-#16 / pre-T-015 / commit `95b15aa`. PRs #12–#16 are absorbed into that baseline; the only delta this Track-D measures live is **PR #17 / T-015**, plus a verification pass over the inputs the re-baseline already integrates.
- **Method:** released-profile section sizes via `rust-readobj --elf-output-style=GNU -S target/aarch64-unknown-none/release/tyrne-bsp-qemu-virt`, executed at both `95b15aa` (post-PR-#16) and `c258ee3` (post-PR-#17) on this host, plus per-symbol sizes via `rust-readobj --symbols`. Smoke timing claim (~4.1 ms) is paper-checked against `current.md`'s ~4–6.5 ms band; no fresh QEMU run was spun up because the workload is unchanged from PR #16.

## Footprint (release ELF, byte-accurate)

| Section | post-PR-#16 (`95b15aa`) | post-PR-#17 (`c258ee3`) | Δ bytes | Δ % |
|---------|-------------------------|-------------------------|---------|-----|
| `.text` | 21,792 (`0x5520`) | **22,020 (`0x5604`)** | **+228** | +1.05 % |
| `.rodata` | 2,928 (`0xb70`) | **2,928 (`0xb70`)** | **0** | 0 % |
| `.bss` | 22,256 (`0x56f0`) | **22,256 (`0x56f0`)** | **0** | 0 % |

PR #17 / T-015 contributes **+228 bytes `.text` only**; `.rodata` and `.bss` are byte-identical to PR #16. Consistent with the PR-side reasoning: `ipc_cancel_recv` is a non-generic free function over `&mut EndpointArena, &mut IpcQueues, CapHandle, &CapabilityTable`, the Deadlock branch in `ipc_recv_and_yield` adds one `let-else`-shaped cold arm, and no new error variants / no new statics / no new panic-message strings reach the release build (the new `debug_assert!` on `cancel_result.is_ok()` and the `ipc_cancel_recv on the same ep_cap …` panic string are compiled out in release; verified by `strings(1)` over the release ELF — that message is absent).

### Per-symbol decomposition of the +228 bytes

| Symbol | Pre (post-#16) | Post (post-#17) | Δ |
|--------|----------------|-----------------|---|
| `tyrne_kernel::sched::ipc_recv_and_yield<QemuVirtCpu>` | 740 | 752 | **+12** |
| `tyrne_kernel::sched::ipc_send_and_yield<QemuVirtCpu>` | 1,848 | 1,848 | 0 |
| `tyrne_kernel::ipc::ipc_cancel_recv` (non-generic) | — | 216 | **+216** |
| **Sum of the new IPC-bridge surface** | — | — | **+228** |

The bookkeeping closes exactly: +12 (extra cold branch + cancel call site) + 216 (the new function body) = +228 = section-level delta. Zero LLVM-side reshuffling residual; the layout is byte-stable around the change.

### Genericity / monomorphization

`ipc_cancel_recv` is **non-generic** (signature: `fn(&mut EndpointArena, &mut IpcQueues, CapHandle, &CapabilityTable) -> Result<(), IpcError>` — no `<C: ContextSwitch + Cpu>` parameter). The release ELF carries one symbol (`_RNvNtCs…3ipc15ipc_cancel_recv`), no `Scheduler<C>` instantiation chain. **No monomorphization fan-out; no risk of code-size growth when a second BSP comes online.**

`ipc_recv_and_yield<QemuVirtCpu>` remains a single instantiation (the only `Cpu` type in the workspace today is `QemuVirtCpu`). The cancel-call site inside it materialises three momentary borrows (`&mut EndpointArena`, `&mut IpcQueues`, `&CapabilityTable`) and dispatches to the non-generic `ipc_cancel_recv` — so future BSPs that introduce a new `Cpu` will monomorphize a fresh `ipc_recv_and_yield<C2>` at +12 bytes (the cold-path arm) but will share the single `ipc_cancel_recv` body. Code-size growth per-BSP is bounded by the pre-existing `<C>` chain, not amplified by T-015.

### `Result` / error surface

`IpcError` is unchanged — no new variant, no discriminant size growth, no new `Debug` impl bytes in `.text`. `ipc_cancel_recv` reuses `IpcError::InvalidCapability` for the cap-validation failure path (one of the existing 5 variants; fits in 1 discriminant byte). Cancel's `Ok`/`Err` plumbing through the `let cancel_result = unsafe { … }; debug_assert!(cancel_result.is_ok());` shape compiles to a discard in release (no branch on the success path; the `debug_assert!` is the entire consumer of the `Result`). **No `?`-cascade growth, no error-boxing.**

## Smoke trace claim — ~4.1 ms boot-to-end

PR #17 reports "boot-to-end ~4.1 ms, byte-for-byte unchanged from post-T-014." Two Track-D-relevant questions:

### (a) Is 4.1 ms inside variance, or suspiciously low?

`current.md` quotes "~4–6.5 ms typical on QEMU-default Cortex-A72"; the [2026-05-07 perf re-baseline §Metric 3](../../performance-optimization-reviews/2026-05-07-B1-closure.md) records three runs at 6.275 ms, 5.768 ms, and "~5.5–6.5 ms range". 4.1 ms sits at the bottom of `current.md`'s band but ~25 % below the lowest recorded re-baseline run.

Verdict: **inside the documented band, but the variance source is QEMU host-clock dependent and not characterised**. The re-baseline already flags this in its §Methodology notes ("the QEMU host-clock gives a ~10–15 % variance on boot-to-end timing across cold-vs-warm host caches"). A 25 % shift is at the high end of that estimate; the most plausible source is host-cache state (a warm host running the trace immediately after PR #16's smoke would land lower than a cold-host run from the re-baseline). PR #17 makes **zero changes that could shorten the hot path**: the demo doesn't exercise the new Deadlock branch (idle is registered → branch is structurally unreachable per ADR-0026 / current.md), and `ipc_cancel_recv` is not on any execution path the demo reaches. So the timing change is variance, not a genuine speedup, and it is not a Track-D regression.

**Recommendation (already on the queue as P10):** when the IPC round-trip benchmark harness lands, take a multi-run mean + variance for boot-to-end, retire the single-run "~4.1 ms" anecdote in PR-bodies, and let `current.md` quote a measured band. Until then, single-run boot-to-end claims should not be over-read.

### (b) "Byte-for-byte unchanged" trace — coverage gap?

Yes, the demo does not exercise the Deadlock branch — and yes, that is **expected**. ADR-0032 §Decision drivers explicitly says: "*In v1 this is benign: with `register_idle` having installed the BSP idle task per ADR-0026, Phase 2's dispatch fallback always finds **some** task to switch to (idle, if no real Ready task), so the Deadlock path is structurally unreachable.*" The demo's `task_a` / `task_b` / idle configuration matches that — `s.ready.dequeue().or(s.idle)` always returns `Some(idle)` even when `task_a` and `task_b` are both blocked, so Phase 2 dispatches to idle and `ipc_cancel_recv` is never called.

Coverage-wise, the cancel path is exercised by the **6 new host tests** (5 IPC unit tests + 1 scheduler regression test, all in `cfg(test)` and therefore **not in the kernel image**). That is the right surface for v1: the kernel image stays free of test infrastructure (no `bench`-feature flag, no test scaffolding in `.text`).

**Track-D point:** "no new code path runs in smoke" is equivalent to "no runtime-cost addition on the hot path" — but only because `ipc_cancel_recv`'s body is also not exercised. The 216 bytes in `.text` are pure footprint, not hot-path cost. This is the correct trade for v1 (the cost is paid at image-size time, not at IPC-throughput time), but it's worth recording explicitly: **+228 bytes of `.text` for a path that currently runs zero times per boot**. The path becomes live as soon as preemption / multi-waiter / userspace endpoint destroy lands (ADR-0032 §Context and §Consequences). The cost is structurally appropriate for v1 closing-out a forward-looking invariant gap; flagging it here so a future RAM-reduction cycle knows where the `cold` annotations would yield real i$ wins.

## Verification of inputs the 2026-05-07 re-baseline already absorbs

### PR #15 — `register_idle` `debug_assert! → assert!` upgrade

T-014 (PR #12, commit `c30f4ee`) introduced `register_idle` with `debug_assert!(s.idle.is_none(), ...)`. PR #15 (commit `d86746a`) upgraded it to an unconditional `assert!` wrapped in a `clippy::panic` allow-block. The 2026-05-07 re-baseline §Metric 1's `.rodata` +144-byte note explicitly attributes part of the growth to "*`register_idle called twice — idle slot is set-once by boot-time discipline`*". `strings(1)` over the post-PR-#17 release ELF confirms the message is present (`register_idle called twice`, `idle slot is set-once by boot-time discipline`) — so the `assert!` is in fact emitted in release (debug_assert form would have stripped it). Code-size cost: a few bytes of `cmp + b.eq + bl panic_handler` plus the `&str` slice descriptor in `.rodata`; entirely absorbed by the −116 net `.text` move recorded by the re-baseline.

**Verdict:** the +N bytes are accounted for. PR #15's upgrade to release-active assert is a correctness win (catching boot-time misuse cannot rely on `cfg(debug_assertions)` because production kernels build release-only); the cost is ~10–20 bytes of `.text` and ~80 bytes of `.rodata` for the message; both already inside the re-baseline's net `.text −116 / .rodata +144` numbers. **Praise (security/perf trade well-made).**

### PR #15 γ.6 — "defensive loop after `start()`" revert

The rejected version would have appended a `loop { core::hint::spin_loop(); }` (or similar 1–2-instruction park) after the `unsafe { start(SCHED.as_mut_ptr(), cpu); }` call in `bsp-qemu-virt/src/main.rs::kernel_entry`. Revert rationale (per the in-source comment block at `bsp-qemu-virt/src/main.rs:733–745`): `start` is `-> !`, so the type system already proves nothing after the call is reachable; clippy's `unreachable_code` + `too_many_lines` lints were tripping. The revert removed 1–2 instructions of code that would have been unreachable anyway (LLVM emits *some* parking instruction sequence at every `-> !` call site for ABI completeness; whether that is `b .` or a `wfe` loop or a fall-through into an `.fill` block depends on optimization phases).

**Verdict:** the revert is a **wash** at the section level. The rejected `loop {}` would have added ~4 bytes (a single `b .`) and ~12 bytes if the spin-loop hint expanded; LLVM's existing `-> !` epilogue at the call site already emits comparable bytes. The 2026-05-07 re-baseline absorbs whatever shape landed; nothing in the per-symbol decomposition above suggests the revert moved the needle outside the noise floor (`ipc_send_and_yield<QemuVirtCpu>` is byte-stable across PR #16 → PR #17, and `kernel_entry` was unchanged in PR #17). **Nit.**

### PR #13 / PR #14 — doc/URL sweeps

Verified by inspection: both touch only `//!`-prefixed doc-comments and intra-doc link targets, plus `.md` files. Doc-comments do not reach the release ELF. URL strings inside doc-comments are stripped by rustc — they do not appear in `.text` or `.rodata`. **Confirmed zero perf relevance.** (rustdoc HTML output size could grow marginally but rustdoc is not built for the kernel image.)

### PR #16 — perf re-baseline doc

Doc-only. Verified by `git diff 7b42bbe..95b15aa --stat | grep -v '\.md'` — every changed file is `.md`. Zero impact on the kernel image. (PR #16 is the *target* of this Track-D — it sets the comparison anchor.)

## Cross-PR pattern — footprint trajectory across PR #12 → PR #17

| Section | A6 (2026-04-21) | B1 baseline (2026-04-28) | post-PR-#15 / re-baseline (2026-05-07) | post-PR-#17 (today) | A6 → today |
|---------|-----------------|--------------------------|----------------------------------------|---------------------|------------|
| `.text` | 13,940 | 21,908 | 21,792 | **22,020** | +8,080 (+58.0 %) |
| `.rodata` | 1,960 | 2,784 | 2,928 | **2,928** | +968 (+49.4 %) |
| `.bss` | 17,872 | 22,248 | 22,256 | **22,256** | +4,384 (+24.5 %) |

Across the closure trio + T-015, the kernel image grew by **+228 bytes `.text` net** vs the 2026-05-07 re-baseline (+112 bytes vs the 2026-04-28 B1 baseline, since PR #12–#15 cumulatively shaved 116 bytes). The re-baseline's "footprint-neutral" framing was correct for the PR #12 → PR #15 window; **post-T-015 it is +228 bytes** — a real but bounded growth, in the same order of magnitude as a single ADR-0026-style structural addition.

## Findings

### Blocker

None. Static analysis + measured section sizes + per-symbol decomposition all close cleanly; no instruction-count regression; no monomorphization fan-out; no `.rodata` / `.bss` growth.

### Major

None.

### Minor

- **D1. PR #17 `.text` delta should be amended into the 2026-05-07 re-baseline.** The [2026-05-07 perf re-baseline](../../performance-optimization-reviews/2026-05-07-B1-closure.md) was authored before T-015 landed; its `Metric 1` table records `.text 21,792` as the closure number. With T-015 merged the same day, `current.md`'s "kernel image `.text` 21,792 bytes (-116 vs 2026-04-28)" line and the re-baseline's table row are now ~228 bytes stale. **Fix:** append a §"2026-05-07 post-T-015 amendment" sub-section to the re-baseline doc with the +228 byte attribution table from this Track D, and update `current.md`'s headline number to 22,020 (or equivalently, "`.text 22,020 bytes (+112 vs 2026-04-28; +228 vs PR-#16 baseline)`"). This is doc-debt, not perf-debt — but it is the kind of drift that compounds if not closed promptly. **Severity: Minor (doc-drift); cross-track signal: Track-E / Track-J.**
- **D2. ~4.1 ms single-run anecdote vs ~5.5–6.5 ms re-baseline.** The PR #17 body's "boot-to-end ~4.1 ms" sits at the bottom edge of `current.md`'s band and ~25 % below the re-baseline's lowest run. Almost certainly host-cache variance, but un-bounded variance estimates are exactly the perf surface P10 (IPC round-trip benchmark harness, queued by [Track-D 2026-05-06](../2026-05-06-full-tree/track-d-performance.md) §P10) is meant to retire. **Recommendation:** stop including single-run boot-to-end timings in PR bodies for the rest of v1; quote `current.md`'s band instead. Promote P10 to "next perf cycle" once B2 surfaces a measurement target. **Severity: Minor.**

### Nit

- **D3. `ipc_cancel_recv` 216 bytes for a path that runs 0 times per boot.** The Deadlock branch is structurally unreachable in v1 (idle registered ⇒ `s.ready.dequeue().or(s.idle)` is always `Some`). The 216-byte cancel function and the +12-byte cold arm in `ipc_recv_and_yield` are pure forward-looking footprint cost. Correct trade per ADR-0032's *Negative §3* ("*Adding the primitive without an immediate userspace consumer means the cancel path is exercised only by the Deadlock host test*"); flagging only because a future `#[cold]` annotation pass (P1 from Track-D 2026-05-06) on the cancel call site + on `ipc_cancel_recv`'s body would yield real i$ wins on the success-path of `ipc_recv_and_yield<QemuVirtCpu>`. ~5–10 bytes of `.text` movable from the warm-i$ working set. Not actionable today; flag for the eventual P1 cycle. **Severity: Nit.**
- **D4. PR #15 γ.6 revert is a wash.** The rejected `loop {}` after `start()` would have added ~4–12 bytes of unreachable padding; LLVM's existing `-> !` epilogue is comparable. No measurable section-level effect; the revert is correct per ADR-0024 / clippy hygiene; flagging only because the maintainer asked to verify it. **Severity: Nit (no action).**

### Praise

- **D5. T-015 hits the closure-trio's footprint-neutrality target almost perfectly.** Non-generic `ipc_cancel_recv` (zero monomorphization fan-out), unchanged `IpcError` discriminant (zero error-surface growth), zero `.rodata` / `.bss` growth (the only new strings are debug-only and stripped in release), +228 bytes `.text` for a forward-looking invariant fix that closes ADR-0032's symmetric-rollback gap. The per-symbol bookkeeping closes byte-exactly (+12 + 216 = +228 = section delta). This is the ADR-driven discipline that the comprehensive review's Track D and the 2026-05-07 re-baseline both pre-validated. **Severity: Praise.**
- **D6. `register_idle` `debug_assert! → assert!` upgrade is the right correctness/perf trade.** Boot-time misuse cannot rely on `cfg(debug_assertions)` for production builds; the unconditional `assert!` is ~10–20 bytes of `.text` + ~80 bytes of `.rodata` (already absorbed by the 2026-05-07 re-baseline's `.rodata +144` line). ADR-0026's single-idle invariant is load-bearing; `assert!` reflects that. **Severity: Praise.**

## Cross-track notes

- → **Track A (kernel correctness):** the cancel branch's debug-only `cancel_result.is_ok()` assert relies on the v1 single-thread cooperative invariant that "Phase 1 just validated `ep_cap` with RECV → cancel cannot surface a fresh `InvalidCapability`". When preemption / multi-waiter lands, that invariant weakens; the cancel return must then be threaded through, not asserted. This is Track A's call (correctness), but the perf cost of the threading change would be ~+5–10 bytes of `.text` per call site; not a Track-D blocker.
- → **Track E (docs):** D1 is doc-drift; cleanest closed by an amendment block on the 2026-05-07 perf re-baseline.
- → **Track F (tests):** the 6 new host tests are the right test surface (unit-level pinning of the cancel state machine + 1 scheduler regression test). They live in `cfg(test)` and contribute zero bytes to the kernel image; this is the right shape, not a perf concern. P10 (IPC round-trip benchmark) remains queued.
- → **Track I (integration):** the ~4.1 ms vs ~5.5–6.5 ms band question is integration-side (QEMU host-clock variance is not Track D's surface). D2's recommendation (stop quoting single-run boot-to-end in PR bodies; retire to a measured band once P10 lands) is integration-flavoured.

## Sub-verdict

**Approve.**

PR #17 / T-015's footprint cost is **+228 bytes `.text`, +0 `.rodata`, +0 `.bss`** — a real and accurately-attributed delta that closes ADR-0032's symmetric-rollback gap with zero monomorphization fan-out and zero error-surface growth. The per-symbol bookkeeping closes byte-exactly. PR #15's `register_idle` assert! upgrade and γ.6 revert are correctly absorbed by the 2026-05-07 re-baseline. PR #13 / PR #14 / PR #16 are zero-perf-relevance verified.

Two minor findings (D1: re-baseline doc needs a +228-byte amendment so `current.md` and the perf doc agree post-T-015; D2: ~4.1 ms single-run boot-to-end is anecdotal variance and should be retired in favour of `current.md`'s band once P10 lands) and two nits (D3: 216 bytes of cold cancel code is structurally unreachable in v1 — correct trade, flag for the eventual `#[cold]` cycle; D4: γ.6 revert is a section-level wash). Two praise items (D5: T-015 hits the footprint-neutrality target with byte-exact bookkeeping; D6: `register_idle` `assert!` upgrade is the right correctness/perf trade).

**Cross-PR pattern across PR #12 → PR #17:** `.text` +112 bytes vs the 2026-04-28 B1 baseline (PR #12–#15 net `−116`; PR #17 net `+228` ⇒ cumulative `+112`); `.rodata` +144 bytes (closure-trio panic-message clarity, all in PR #12–#15); `.bss` +8 bytes (T-014's `Option<TaskHandle>` idle slot, all in PR #12). The closure trio remained footprint-neutral *until T-015*; T-015 adds a bounded +228 bytes for a forward-looking invariant fix. The re-baseline framing of "B1 closes footprint-neutral" needs a one-paragraph amendment to remain accurate post-T-015.

The 11 P-numbered proposals from the [2026-05-06 Track-D paper review](../2026-05-06-full-tree/track-d-performance.md) remain queued as before; T-015 does not change their priority. P10 (wall-clock IPC harness) becomes more attractive every cycle that single-run boot-to-end variance is quoted in PR bodies.

## References

- [2026-05-07 B1-closure perf re-baseline](../../performance-optimization-reviews/2026-05-07-B1-closure.md) — the comparison anchor.
- [2026-04-28 B1-closure perf baseline](../../performance-optimization-reviews/2026-04-28-B1-closure.md) — the historical reference.
- [Track-D 2026-05-06 paper review](../2026-05-06-full-tree/track-d-performance.md) — the P1–P11 proposal queue.
- [ADR-0032 — Endpoint state rollback + `ipc_cancel_recv`](../../../decisions/0032-endpoint-rollback-and-cancel-recv.md) — the design driver for PR #17.
- [ADR-0026 — Idle dispatch via separate fallback slot](../../../decisions/0026-idle-dispatch-fallback.md) — the structural-unreachability invariant that makes the new cancel path zero-runtime-cost in v1.
- [ADR-0017 — IPC primitive set](../../../decisions/0017-ipc-primitive-set.md) — gains the §Revision notes rider for `ipc_cancel_recv`.
- [`docs/roadmap/current.md`](../../../roadmap/current.md) — quotes the now-stale `.text 21,792` headline; flagged in D1.
- [`kernel/src/ipc/mod.rs`](../../../../kernel/src/ipc/mod.rs) — `ipc_cancel_recv` body (216 bytes).
- [`kernel/src/sched/mod.rs`](../../../../kernel/src/sched/mod.rs) — `ipc_recv_and_yield`'s Deadlock branch (+12 bytes).
- [`bsp-qemu-virt/src/main.rs:733–745`](../../../../bsp-qemu-virt/src/main.rs#L733-L745) — the in-source comment recording PR #15 γ.6's revert rationale.
