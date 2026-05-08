# Track 4 — PR #21 perf-harness implementation

- **PR:** [#21](https://github.com/cemililik/Tyrne/pull/21)
- **Branch:** p10-wall-clock-bench-harness
- **Commits reviewed:** `1de8143` + `abf26b9`
- **Reviewer:** Claude Opus 4.7 sub-agent (Track 4) — paper review only; harness not executed
- **Verdict:** **Approve-with-2-followups**

## Summary

The harness is unusually careful for a 495-line bash + awk tool: bash-3.2-clean,
BSD-awk-clean, watchdog-correct, percentile/stddev convention named in code
*and* report, and `--report=CONTEXT` validated against `[A-Za-z0-9._-]` so path
traversal is structurally impossible. The two follow-ups below are non-blocking
hardening (orphan-process trap on Ctrl-C, `p99 == max` artefact at small N).

This is a paper review — runtime regressions on the actual harness execution
are outside scope; the project user owns the QEMU smoke runtime and produced
the baseline numbers under review.

## Findings

### Blocker

None.

### Major

None.

### Minor

1. **No `trap '...' EXIT INT TERM` cleanup.** `tools/perf-harness.sh:140-167`.
   Inside `run_with_timeout`, the watchdog is killed cleanly after each
   `wait`. But if the operator hits Ctrl-C *between* iterations of the outer
   `while` loop (`tools/perf-harness.sh:182-215`), the in-flight QEMU child
   plus its sibling watchdog can outlive the harness for up to `TIMEOUT_S`
   seconds. A `trap` that records the active `cmd_pid` / `watchdog_pid` in
   shell globals and `kill -KILL`s them on `EXIT INT TERM` would close this.
   Bash 3.2 friendly (no `declare -A` needed; two scalars suffice).

2. **`p99 == max` is a property of nearest-rank at N=20.** With
   `idx = ceil(p/100 * n)` and N=20, p99 → ceil(19.8) = 20 = `a[n]` = max.
   The baseline report already shows this (p99 = max = 6.558 ms). Not a bug —
   the convention is documented (`tools/perf-harness.sh:225`, baseline
   Methodology) — but the *report's* Metric table would be more honest if it
   noted "p99 collapses to max for N < 100 under the nearest-rank
   convention", or if the harness simply suppressed p99 when `n < 100`. Pure
   reporting hygiene; the underlying number is correct.

### Nit

3. **`read_stats` re-parses awk output via four `echo | awk` invocations**
   (`tools/perf-harness.sh:266-273`). One `read` loop or a single
   `eval "$(awk ... | sed 's/^/STAT_/')"` would shave ~7 forks. Not
   load-bearing — runs once per harness invocation.

### Praise

- `set -euo pipefail` *and* explicit `set +e` / `set -e` brackets around
  `wait "$cmd_pid"` (`tools/perf-harness.sh:160-164`) and around the per-iter
  `OUTPUT=$(run_with_timeout ...)` (`tools/perf-harness.sh:191-194`). Most
  bash authors get `pipefail` wrong on watchdogs.
- `--report=CONTEXT` validated against `[A-Za-z0-9._-]` at
  `tools/perf-harness.sh:101-106` *before* it ever flows into a path. Path
  traversal (`../../../etc/passwd`) is rejected by the character class, not
  by post-hoc string surgery.
- Insertion sort *inside* awk rather than relying on `gawk`-only `asort`
  (`tools/perf-harness.sh:233-241`). BSD-awk-clean by construction.
- Percentile + stddev convention named in **both** the awk comment and the
  generated report's Methodology section. No future archeologist has to
  reverse-engineer which of the seven percentile flavours is in use.
- Pre-flight checks: `run-qemu.sh` executable, kernel ELF exists,
  `qemu-system-aarch64` in PATH (`tools/perf-harness.sh:114-135`). Fail-fast
  before burning 100 s of iteration time.

## Bash correctness audit

| Item | Required | Present | Notes |
|------|----------|---------|-------|
| `set -euo pipefail` | yes | yes | line 47 |
| Bash 3.2 — no `declare -A` | yes | yes | indexed array `SAMPLES` only; comment at line 175 calls this out |
| Bash 3.2 — no `[[ -v var ]]` | yes | yes | uses `[[ -n "$VAR" ]]` and `[[ -z "$VAR" ]]` |
| Bash 3.2 — no `${var,,}` | yes | yes | no lowercasing |
| Empty-array idiom under `set -u` | yes | n/a | `SAMPLES` is only iterated after the empty-check; `printf '%s\n' "${SAMPLES[@]}"` is reached only when `VALID_COUNT >= 1` |
| Argument parsing matches `run-qemu.sh` style | yes | yes | `for arg in "$@"; case` block |
| Unknown args | error | error | `tools/perf-harness.sh:80-83` (stricter than `run-qemu.sh`, which silently treats unknowns as kernel paths) |
| Path traversal on `--report=CONTEXT` | rejected | rejected | character-class validation at `tools/perf-harness.sh:101-106` |
| Quoting on shell expansions | yes | yes | every `$VAR` in a path / arg context double-quoted; `$PROFILE_FLAG` deliberately unquoted at line 193 because it may be empty (works under `set -u` because the var is initialised) |
| `eval` / `bash -c "$user_input"` | absent | absent | none |

## Awk statistical formulas

| Metric | Convention | Awk line | Correct? |
|--------|------------|----------|----------|
| Sort | Insertion sort, ascending, integer-coerced (`$1 + 0`) | `tools/perf-harness.sh:233-241` | Yes; n ≤ a few thousand makes O(n²) free |
| min | `a[1]` after sort | `tools/perf-harness.sh:250` | Yes |
| max | `a[n]` after sort | `tools/perf-harness.sh:255` | Yes |
| Percentile (p10/p50/p90/p99) | Nearest-rank, 1-indexed: `idx = ceil((p/100) * n)`, clamped to `[1, n]` | `tools/perf-harness.sh:223-228` | Yes, consistent across all four |
| Mean | `sum / n`, single-pass accumulator | `tools/perf-harness.sh:243-244` | Yes; awk's double float is fine for ~20 samples in `[10⁶, 10⁷]` ns |
| Stddev | **Population** (n divisor): `sqrt(sumsq/n - mean²)` with `var < 0` round-off guard | `tools/perf-harness.sh:245-249` | Yes; named "population" in code and report |

Sanity-check the convention against the baseline: N=20, sorted samples
ascending. Index map: p10 → ceil(2.0)=2, p50 → ceil(10.0)=10, p90 →
ceil(18.0)=18, p99 → ceil(19.8)=20. p99 collapses to `max` by construction —
not a bug, but a small-N artefact (see Minor 2).

## Edge case matrix

| Case | Behavior | Verdict |
|------|----------|---------|
| 0 valid samples | Explicit early exit non-zero at `tools/perf-harness.sh:197-200` *before* awk runs; division-by-zero impossible | Pass |
| 1 valid sample | awk runs; `pct(p)` clamps `idx` to `[1, n]`; mean = sample; stddev = `sqrt(0)` = 0; no NaN | Pass |
| `--iterations=0` | Rejected upfront at `tools/perf-harness.sh:90-92` (`ITERATIONS < 1`) | Pass |
| `--timeout=0` | Rejected upfront at `tools/perf-harness.sh:97-99` (`TIMEOUT_S < 1`) | Pass |
| Half-failed (N=20, valid=10) | `HALF = (20+1)/2 = 10`; `valid < HALF` is `10 < 10` = false → continues, stats over 10 samples + failure count reported | Pass |
| Half-failed (N=21, valid=10) | `HALF = 11`; `10 < 11` = true → exits non-zero | Pass |
| QEMU stdout missing the timing line | `grep -oE` returns empty; `[[ -n "$SAMPLE" ]]` false; `FAIL_COUNT++`; iteration loop continues | Pass |
| `tools/run-qemu.sh` exits non-zero | Status discarded by design (`tools/perf-harness.sh:191-194` documents that the watchdog kills QEMU, so non-zero is the *expected* successful outcome). Cargo-build failure is caught upstream by the kernel-ELF pre-flight at `tools/perf-harness.sh:124` | Pass |
| Watchdog kills mid-stdout-flush | `grep -oE 'boot-to-end elapsed = [0-9]+ ns'` requires the trailing literal ` ns` — partial output like `boot-to-end elapsed = 4123` (truncated) does *not* match. No mis-parse. | Pass |
| Operator Ctrl-C between iterations | In-flight QEMU + watchdog can survive up to `TIMEOUT_S` seconds | **Minor 1** above |
| Report directory missing | Explicit error at `tools/perf-harness.sh:325-328` | Pass |
| Report file already exists | **Silently overwritten** via `> "$REPORT_PATH"` (`tools/perf-harness.sh:402`); no `[[ -e ]]` guard | Acceptable — the file naming scheme makes collisions intentional (re-running with the same context is a deliberate re-baseline). Worth noting but not a bug. |

## Filename / title dedup

Pattern: shell `case` glob `[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]-*`,
anchored at start of context (`tools/perf-harness.sh:316-322` for filename,
`tools/perf-harness.sh:331-340` for H1 title). **Same pattern in both
places — no drift.**

| Input context | Filename produced | H1 title produced | Verdict |
|---------------|-------------------|-------------------|---------|
| `2026-05-08-post-pr-19-pre-adr-0027` | `perf-baseline-2026-05-08-post-pr-19-pre-adr-0027.md` | "Boot-to-end perf baseline — 2026-05-08 — post-pr-19-pre-adr-0027" | Matches PR brief; observed in baseline report |
| `post-pr-19` (no leading date) | `perf-baseline-2026-05-08-post-pr-19.md` (today prepended) | "… — 2026-05-08 — post-pr-19" | Correct |
| `foo-2026-05-08` (date in middle) | `perf-baseline-2026-05-08-foo-2026-05-08.md` (today prepended; embedded date untouched) | "… — 2026-05-08 — foo-2026-05-08" | Correct — glob is anchored, no over-match |
| `2026-05-08-2026-05-09-foo` (two leading-ish dates) | `perf-baseline-2026-05-08-2026-05-09-foo.md` (only first date treated as the date head) | "… — 2026-05-08 — 2026-05-09-foo" | Correct — strips one date, not both. Also: `--report` validation at line 101 *forbids* such pathological inputs anyway in normal use |

Same dedup applied to filename and H1: yes (cross-checked
`tools/perf-harness.sh:316-322` against `tools/perf-harness.sh:331-340`).

## Statistical sanity check on the baseline report

Numbers from
`docs/analysis/reports/perf-baseline-2026-05-08-post-pr-19-pre-adr-0027.md`:
min=3.862, p10=3.884, p50=4.642, p90=5.584, p99=6.558, max=6.558,
mean=4.711, stddev=0.709 (all ms; N=20).

| Check | Expected | Actual | Pass? |
|-------|----------|--------|-------|
| Ordering min ≤ p10 ≤ p50 ≤ p90 ≤ p99 ≤ max | yes | 3.862 ≤ 3.884 ≤ 4.642 ≤ 5.584 ≤ 6.558 ≤ 6.558 | Yes |
| Mean ≈ p50 (slight right-skew under TCG cache warmup) | within 5–10 % of p50 | mean=4.711 vs p50=4.642 → +1.5 % above median | Yes (right-skew, plausible) |
| Range vs 6σ rule | range ≤ 6×stddev = 4.254 ms | range = max − min = 6.558 − 3.862 = 2.696 ms | Yes — range *under* 6σ → no heavy tails, clean unimodal-ish distribution |
| CV = stddev / mean | 5–30 % is typical for QEMU TCG | 0.709 / 4.711 = 15.0 % | Yes — tight for TCG |
| p99 == max (artefact) | yes at N=20 under nearest-rank | yes | Property of convention, not a bug; flagged as Minor 2 |
| Cross-check vs raw samples | sort raw samples; verify a[2]=p10, a[10]=p50, a[18]=p90, a[20]=p99=max | a[2]=3884000, a[10]=4642000, a[18]=5584000, a[20]=6558000 — **all four match the table to the integer ns** | Yes |

The raw-samples block in the report lets a reader independently re-derive
every reported statistic. That alone earns a praise note above.

## Cross-doc consistency

| Doc | Check | Result |
|-----|-------|--------|
| `docs/standards/infrastructure.md` §"Performance harness" | Flag list (`--iterations=K`, `--timeout=SECONDS`, `--release`, `--quiet`, `--report=CONTEXT`) matches harness `case` block | Match. No drift. |
| `docs/roadmap/current.md` 2026-05-08 banner | Avoids citing a PR number (originally said #20 per brief; corrected to no number) | Banner says "P10 wall-clock harness landed" with no PR number — defensible against future renumber drift |
| `docs/roadmap/current.md` 2026-05-08 banner | Quotes p10 / p50 / p90 / p99 + mean + stddev consistent with the report | All six numbers match the report to three decimals |
| `2026-05-07-B1-closure.md` "Post-amendment update (2026-05-08, on its own PR)" | One-line cross-ref to the harness + the new baseline report | Clean; relative paths correct (`../../../../tools/perf-harness.sh`, `../../reports/perf-baseline-2026-05-08-...md`) |
| `infrastructure.md` Reporting discipline section | Mentions future `--baseline=<file>` regression mode | **Not mentioned**. Worth a one-line "future mode TBD when ADR-0027 / B2 lands" note — see refactor backlog |

## Refactor backlog (non-blocking)

- **Bash + awk vs small Rust binary.** Defensible at 495 LOC: bash + awk is
  zero-build (no `cargo` dance just to measure perf), runs on the same
  toolchain as `run-qemu.sh`, and the awk math is auditable in 25 lines.
  A Rust port becomes worth its weight when (a) `--hardware` mode lands and
  the harness drives serial USB instead of QEMU stdout, or (b) a
  `--baseline=<file>` regression-detection mode appears with hypothesis
  testing (Welch's *t*, Mann-Whitney). Today neither applies.
- **No unit tests for the awk math.** A small fixture file with hand-computed
  p10 / p50 / p90 / p99 / mean / stddev for N ∈ {1, 2, 5, 20} would catch
  any future "we'll switch to linear interpolation" regression. Cheap to
  add as `tools/tests/perf-harness-stats.sh` (golden-file diff) — defer
  until B2 closure if at all.
- **Future `--baseline=<file>` regression-detection mode** is mentioned in
  the PR brief as out-of-scope; not yet captured in
  `docs/standards/infrastructure.md` §"Performance harness". A one-line
  "regression-detection mode is a B2-or-later follow-up" sentence in the
  same paragraph that says "promotion to a CI gate is a B2-or-later
  follow-up" would close the loop.
- **`p99` suppression at small N.** See Minor 2 — pure reporting hygiene,
  trivial.

## References

- `tools/run-qemu.sh` — idiom precedent (bash 3.2 + BSD awk, `INT_LOG_FLAGS`
  empty-array idiom, `for arg in "$@"; case`)
- BSD awk man page — no `asort` (gawk-only); insertion-sort fallback used
  here is portable
- Bash 3.2 limitations — no associative arrays (`declare -A`), no `[[ -v ]]`,
  no `${var,,}`; harness avoids all three
- `docs/analysis/reports/perf-baseline-2026-05-08-post-pr-19-pre-adr-0027.md`
  — the artefact under measurement
- `docs/standards/infrastructure.md` §"Performance harness" — the policy
  this harness implements
- ADR-0027 (kernel virtual memory layout) — the next ADR; the band this
  harness produces is what T-016 (MMU activation) regressions will be
  measured against
