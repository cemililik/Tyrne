#!/usr/bin/env bash
# Multi-run boot-to-end aggregation for the Tyrne kernel under QEMU.
#
# Wraps `tools/run-qemu.sh` in an iteration loop, parses the kernel's
# `boot-to-end elapsed = <ns> ns` line out of each run's serial output, and
# prints aggregate statistics (min, p10, p50, p90, p99, max, mean, stddev).
# The harness is the canonical source for boot-to-end timing claims per
# `docs/standards/infrastructure.md` §"Performance harness"; single-run
# numbers in PR bodies are deprecated in favour of the bands this script
# emits.
#
# Usage:
#   tools/perf-harness.sh                                     - 20 iterations
#   tools/perf-harness.sh --iterations=K                      - K iterations
#   tools/perf-harness.sh --timeout=SECONDS                   - per-run timeout
#   tools/perf-harness.sh --release                           - forwarded to run-qemu
#   tools/perf-harness.sh --quiet                             - no per-iter progress
#   tools/perf-harness.sh --report=CONTEXT                    - also write a report
#
# The report (when requested) is written to
#   docs/analysis/reports/perf-baseline-YYYY-MM-DD-<context>.md
# matching the Inputs / Methodology / Metric / Verdict shape used by the
# performance-optimization review master plan.
#
# Per-run timeout: each QEMU invocation is wrapped in a watchdog because the
# Tyrne kernel halts in a WFI loop after the demo finishes — QEMU does not
# exit on its own. The default 5 s gives the demo (~5-10 ms typical wall
# clock under TCG) plenty of headroom; bump it with --timeout if the build
# is unusually slow on the host.
#
# Failure handling: a run that does not emit a boot-to-end line within the
# timeout is counted as a failure. If fewer than 50 % of runs produced a
# valid sample, the harness exits non-zero — that threshold is treated as
# environmental (kernel image missing, QEMU not in PATH, host under heavy
# load). If 50-100 % of runs are valid, statistics are computed over the
# valid samples only and the failure count is reported alongside.
#
# Exits 0 on success (>= 50 % valid runs), 1 on environmental failure,
# 2 on argument errors.

set -euo pipefail

# ─── Defaults ─────────────────────────────────────────────────────────────────

ITERATIONS=20
TIMEOUT_S=5
REPORT_CONTEXT=""
QUIET=""
PROFILE_FLAG=""
PROFILE_LABEL="debug"

# ─── Argument parsing ─────────────────────────────────────────────────────────

usage() {
    sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//' >&2
    exit 2
}

for arg in "$@"; do
    case "$arg" in
        --iterations=*)
            ITERATIONS="${arg#--iterations=}"
            ;;
        --timeout=*)
            TIMEOUT_S="${arg#--timeout=}"
            ;;
        --report=*)
            REPORT_CONTEXT="${arg#--report=}"
            ;;
        --release)
            PROFILE_FLAG="--release"
            PROFILE_LABEL="release"
            ;;
        --quiet)
            QUIET="yes"
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "error: unknown argument: $arg" >&2
            usage
            ;;
    esac
done

# Validate numeric arguments.
case "$ITERATIONS" in
    ''|*[!0-9]*)
        echo "error: --iterations must be a positive integer (got: $ITERATIONS)" >&2
        exit 2
        ;;
esac
if [[ "$ITERATIONS" -lt 1 ]]; then
    echo "error: --iterations must be >= 1 (got: $ITERATIONS)" >&2
    exit 2
fi
case "$TIMEOUT_S" in
    ''|*[!0-9]*)
        echo "error: --timeout must be a positive integer (seconds; got: $TIMEOUT_S)" >&2
        exit 2
        ;;
esac
if [[ "$TIMEOUT_S" -lt 1 ]]; then
    echo "error: --timeout must be >= 1 second (got: $TIMEOUT_S)" >&2
    exit 2
fi

# Validate report context if supplied: only [A-Za-z0-9._-] to keep the
# resulting filename clean and predictable.
if [[ -n "$REPORT_CONTEXT" ]]; then
    case "$REPORT_CONTEXT" in
        *[!A-Za-z0-9._-]*)
            echo "error: --report context may only contain [A-Za-z0-9._-]" >&2
            exit 2
            ;;
    esac
fi

# ─── Locate the harness's working directory ──────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RUN_QEMU="$SCRIPT_DIR/run-qemu.sh"

if [[ ! -x "$RUN_QEMU" ]]; then
    echo "error: tools/run-qemu.sh not found or not executable at $RUN_QEMU" >&2
    exit 1
fi

# Pre-flight: confirm the kernel ELF the run-qemu helper will pick exists.
KERNEL_ELF="$REPO_ROOT/target/aarch64-unknown-none/$PROFILE_LABEL/tyrne-bsp-qemu-virt"
if [[ ! -f "$KERNEL_ELF" ]]; then
    echo "error: kernel image not found at $KERNEL_ELF" >&2
    if [[ "$PROFILE_LABEL" = "release" ]]; then
        echo "hint: run 'cargo build --release --target aarch64-unknown-none -p tyrne-bsp-qemu-virt' first" >&2
    else
        echo "hint: run 'cargo kernel-build' first" >&2
    fi
    exit 1
fi

if ! command -v qemu-system-aarch64 >/dev/null 2>&1; then
    echo "error: qemu-system-aarch64 not found in PATH" >&2
    exit 1
fi

# ─── Portable per-run timeout ─────────────────────────────────────────────────
#
# macOS does not ship the GNU `timeout` binary by default, and Tyrne supports
# the system bash 3.2 (see the `INT_LOG_FLAGS` idiom in run-qemu.sh). The
# helper below uses a background watchdog: launch the command, launch a
# sibling `sleep + kill` watchdog, wait on the command, then reap the
# watchdog. The two `kill` retries (TERM then KILL) match GNU timeout's
# default escalation.
run_with_timeout() {
    local timeout_s=$1
    shift
    local cmd_pid watchdog_pid status

    "$@" &
    cmd_pid=$!

    (
        sleep "$timeout_s"
        kill -TERM "$cmd_pid" 2>/dev/null || true
        sleep 1
        kill -KILL "$cmd_pid" 2>/dev/null || true
    ) >/dev/null 2>&1 &
    watchdog_pid=$!

    # `set -e` would abort here on a non-zero wait; suppress.
    set +e
    wait "$cmd_pid" 2>/dev/null
    status=$?
    set -e

    kill -KILL "$watchdog_pid" 2>/dev/null || true
    wait "$watchdog_pid" 2>/dev/null || true
    return "$status"
}

# ─── Iteration loop ───────────────────────────────────────────────────────────

START_EPOCH=$(date +%s)
START_ISO=$(date -u +%Y-%m-%dT%H:%M:%SZ)
HOST_UNAME=$(uname -a)
QEMU_VERSION=$(qemu-system-aarch64 --version | head -n 1)
GIT_HEAD=$(cd "$REPO_ROOT" && git rev-parse --short HEAD 2>/dev/null || echo "(unknown)")
GIT_BRANCH=$(cd "$REPO_ROOT" && git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "(unknown)")

# Bash 3.2 has indexed arrays; no associative arrays needed for this script.
SAMPLES=()
FAIL_COUNT=0

if [[ -z "$QUIET" ]]; then
    echo "tyrne perf-harness: $ITERATIONS iterations, ${TIMEOUT_S}s per-run timeout, $PROFILE_LABEL build" >&2
    echo "tyrne perf-harness: kernel $KERNEL_ELF" >&2
    echo "tyrne perf-harness: git $GIT_HEAD on $GIT_BRANCH" >&2
fi

i=1
while [[ "$i" -le "$ITERATIONS" ]]; do
    # Capture stdout+stderr of the wrapped run; discard QEMU's exit status
    # (the watchdog kills it after the demo finishes, so non-zero is the
    # expected outcome on a *successful* iteration).
    OUTPUT=""
    set +e
    OUTPUT=$(run_with_timeout "$TIMEOUT_S" "$RUN_QEMU" $PROFILE_FLAG 2>&1)
    set -e

    # Extract the first boot-to-end ns sample from the captured output.
    SAMPLE=$(printf '%s\n' "$OUTPUT" \
        | grep -oE 'boot-to-end elapsed = [0-9]+ ns' \
        | head -n 1 \
        | grep -oE '[0-9]+' \
        | head -n 1 || true)

    if [[ -n "$SAMPLE" ]]; then
        SAMPLES+=("$SAMPLE")
        if [[ -z "$QUIET" ]]; then
            printf '  iter %2d/%d: %s ns\n' "$i" "$ITERATIONS" "$SAMPLE" >&2
        fi
    else
        FAIL_COUNT=$((FAIL_COUNT + 1))
        if [[ -z "$QUIET" ]]; then
            printf '  iter %2d/%d: FAIL (no boot-to-end line within %ss)\n' \
                "$i" "$ITERATIONS" "$TIMEOUT_S" >&2
        fi
    fi
    i=$((i + 1))
done

END_EPOCH=$(date +%s)
WALL_S=$((END_EPOCH - START_EPOCH))

VALID_COUNT=${#SAMPLES[@]}
if [[ "$VALID_COUNT" -eq 0 ]]; then
    echo "error: zero valid samples collected across $ITERATIONS iterations" >&2
    echo "hint: try a larger --timeout, confirm the kernel ELF runs under run-qemu.sh manually" >&2
    exit 1
fi

# 50 % failure-rate threshold: below it, we treat the run as environmental
# rather than a measurement worth aggregating. The brief explicitly asked
# for a clear error in this case.
HALF=$(( (ITERATIONS + 1) / 2 ))
if [[ "$VALID_COUNT" -lt "$HALF" ]]; then
    echo "error: only $VALID_COUNT/$ITERATIONS iterations produced a boot-to-end sample" >&2
    echo "       (threshold is $HALF; below this we treat the run as environmental rather" >&2
    echo "       than a measurement worth aggregating)" >&2
    echo "hint: try a larger --timeout, confirm the kernel ELF runs under run-qemu.sh manually" >&2
    exit 1
fi

# ─── Aggregation (awk) ────────────────────────────────────────────────────────
#
# Single awk invocation: read the samples on stdin (one per line), sort,
# compute min / p10 / p50 / p90 / p99 / max + mean + population stddev.
# Percentiles use the "nearest-rank" definition (index = ceil(p/100 * n),
# 1-indexed) — same convention as numpy's `interpolation='higher'`. Cheap
# and well-defined for small n; for n >= 20 the choice doesn't move much.

read_stats() {
    # awk program: sort the samples ascending, compute min/max + nearest-rank
    # percentiles + mean + population stddev. Nested function definitions are
    # not portable to BSD awk (macOS default), so `pct` is top-level and `n`
    # is shared via the global awk namespace.
    printf '%s\n' "${SAMPLES[@]}" | awk '
    function pct(p,    idx) {
        idx = int((p/100.0) * n + 0.9999999)
        if (idx < 1) idx = 1
        if (idx > n) idx = n
        return a[idx]
    }
    {
        a[NR] = $1 + 0
        sum += $1 + 0
        sumsq += ($1 + 0) * ($1 + 0)
    }
    END {
        n = NR
        # Ascending sort with insertion sort (simple, n <= a few thousand).
        for (i = 2; i <= n; i++) {
            v = a[i]
            j = i - 1
            while (j >= 1 && a[j] > v) {
                a[j+1] = a[j]
                j--
            }
            a[j+1] = v
        }
        mean = sum / n
        # Population stddev (n divisor) — descriptive, not inferential.
        var = (sumsq / n) - (mean * mean)
        if (var < 0) var = 0  # floating-point round-off guard
        sd = sqrt(var)
        printf "min %d\n", a[1]
        printf "p10 %d\n", pct(10)
        printf "p50 %d\n", pct(50)
        printf "p90 %d\n", pct(90)
        printf "p99 %d\n", pct(99)
        printf "max %d\n", a[n]
        printf "mean %.0f\n", mean
        printf "stddev %.0f\n", sd
    }'
}

# Parse the awk output back into named shell variables.
STATS=$(read_stats)
STAT_MIN=$(echo "$STATS"    | awk '$1=="min"    {print $2}')
STAT_P10=$(echo "$STATS"    | awk '$1=="p10"    {print $2}')
STAT_P50=$(echo "$STATS"    | awk '$1=="p50"    {print $2}')
STAT_P90=$(echo "$STATS"    | awk '$1=="p90"    {print $2}')
STAT_P99=$(echo "$STATS"    | awk '$1=="p99"    {print $2}')
STAT_MAX=$(echo "$STATS"    | awk '$1=="max"    {print $2}')
STAT_MEAN=$(echo "$STATS"   | awk '$1=="mean"   {print $2}')
STAT_STDDEV=$(echo "$STATS" | awk '$1=="stddev" {print $2}')

# Format helpers: thousands-separator on ns, three-decimal ms.
fmt_ns() {
    # awk handles thousands without depending on locale.
    awk -v n="$1" 'BEGIN {
        s = sprintf("%d", n)
        out = ""
        len = length(s)
        for (i = 1; i <= len; i++) {
            out = out substr(s, i, 1)
            tail = len - i
            if (tail > 0 && tail % 3 == 0) out = out ","
        }
        print out
    }'
}

fmt_ms() {
    awk -v n="$1" 'BEGIN { printf "%.3f\n", n / 1000000.0 }'
}

# ─── Stdout summary ───────────────────────────────────────────────────────────

echo
echo "================================================================"
echo "tyrne boot-to-end aggregation ($VALID_COUNT/$ITERATIONS valid; $FAIL_COUNT failed)"
echo "  build:   $PROFILE_LABEL"
echo "  timeout: ${TIMEOUT_S}s per run"
echo "  wall:    ${WALL_S}s total"
echo "  git:     $GIT_HEAD on $GIT_BRANCH"
echo "  qemu:    $QEMU_VERSION"
echo "================================================================"
printf '  %-7s | %15s | %10s\n' "metric" "ns" "ms"
printf '  %-7s-+-%15s-+-%10s\n' "-------" "---------------" "----------"
for metric in min p10 p50 p90 p99 max mean stddev; do
    case "$metric" in
        min)    val=$STAT_MIN ;;
        p10)    val=$STAT_P10 ;;
        p50)    val=$STAT_P50 ;;
        p90)    val=$STAT_P90 ;;
        p99)    val=$STAT_P99 ;;
        max)    val=$STAT_MAX ;;
        mean)   val=$STAT_MEAN ;;
        stddev) val=$STAT_STDDEV ;;
    esac
    ns_str=$(fmt_ns "$val")
    ms_str=$(fmt_ms "$val")
    printf '  %-7s | %15s | %10s\n' "$metric" "$ns_str" "$ms_str"
done
echo "================================================================"

# ─── Optional report ─────────────────────────────────────────────────────────

if [[ -n "$REPORT_CONTEXT" ]]; then
    REPORT_DATE=$(date -u +%Y-%m-%d)
    REPORT_DIR="$REPO_ROOT/docs/analysis/reports"
    # If the caller already prefixed the context with an ISO date (e.g.
    # `2026-05-08-foo`), don't double it. Match `YYYY-MM-DD-...`.
    case "$REPORT_CONTEXT" in
        [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]-*)
            REPORT_PATH="$REPORT_DIR/perf-baseline-${REPORT_CONTEXT}.md"
            ;;
        *)
            REPORT_PATH="$REPORT_DIR/perf-baseline-${REPORT_DATE}-${REPORT_CONTEXT}.md"
            ;;
    esac

    if [[ ! -d "$REPORT_DIR" ]]; then
        echo "error: report directory does not exist: $REPORT_DIR" >&2
        exit 1
    fi

    # Strip a leading `YYYY-MM-DD-` from the context for the title so we don't
    # render `2026-05-08 — 2026-05-08-foo` on dated contexts.
    case "$REPORT_CONTEXT" in
        [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]-*)
            REPORT_TITLE_TAIL=$(printf '%s' "$REPORT_CONTEXT" | cut -c12-)
            REPORT_DATE_HEAD=$(printf '%s' "$REPORT_CONTEXT" | cut -c1-10)
            ;;
        *)
            REPORT_TITLE_TAIL="$REPORT_CONTEXT"
            REPORT_DATE_HEAD="$REPORT_DATE"
            ;;
    esac

    {
        echo "# Boot-to-end perf baseline — ${REPORT_DATE_HEAD} — ${REPORT_TITLE_TAIL}"
        echo
        echo "Generated by \`tools/perf-harness.sh\` — multi-run aggregation of the kernel's"
        echo "\`boot-to-end elapsed = X ns\` emission (P10 from the [2026-05-06 Track D"
        echo "review](../reviews/code-reviews/2026-05-06-full-tree/track-d-performance.md))."
        echo
        echo "## Inputs"
        echo
        echo "| Field | Value |"
        echo "|-------|-------|"
        echo "| Run timestamp (UTC) | \`${START_ISO}\` |"
        echo "| Iterations requested | ${ITERATIONS} |"
        echo "| Iterations valid | ${VALID_COUNT} |"
        echo "| Iterations failed | ${FAIL_COUNT} |"
        echo "| Per-run timeout | ${TIMEOUT_S} s |"
        echo "| Build profile | ${PROFILE_LABEL} |"
        echo "| Kernel ELF | \`${KERNEL_ELF#${REPO_ROOT}/}\` |"
        echo "| Git HEAD | \`${GIT_HEAD}\` on \`${GIT_BRANCH}\` |"
        echo "| QEMU | \`${QEMU_VERSION}\` |"
        echo "| Host \`uname -a\` | \`${HOST_UNAME}\` |"
        echo "| Wall-clock (full harness run) | ${WALL_S} s |"
        echo
        echo "## Methodology"
        echo
        echo "Each iteration invokes \`tools/run-qemu.sh\` under a per-run watchdog;"
        echo "QEMU emits the boot trace through to \`tyrne: all tasks complete\` plus"
        echo "the \`boot-to-end elapsed = X ns\` line, then halts in WFI. The watchdog"
        echo "kills the QEMU process after the per-run timeout (the kernel never"
        echo "exits on its own). The integer ns delta is parsed out of stdout."
        echo
        echo "Counter source: the kernel's \`now_ns()\` (\`hal::Timer\`) reads the EL1"
        echo "virtual generic-timer counter and converts to nanoseconds via the"
        echo "cached \`CNTFRQ_EL0\` resolution. Under QEMU TCG the counter advances"
        echo "based on emulated instructions rather than wall-clock time, so"
        echo "variance reflects translation-cache behaviour and host scheduler"
        echo "jitter, not real hardware performance."
        echo
        echo "Statistics are computed across the valid samples only. Percentile"
        echo "convention is *nearest-rank* (1-indexed; \`idx = ceil(p/100 * n)\`)."
        echo "Stddev is the population formula (\`n\` divisor) — descriptive."
        echo
        echo "## Metric — boot-to-end elapsed (nanoseconds)"
        echo
        echo "| Statistic | ns | ms |"
        echo "|-----------|---:|---:|"
        for metric in min p10 p50 p90 p99 max mean stddev; do
            case "$metric" in
                min)    val=$STAT_MIN ;;
                p10)    val=$STAT_P10 ;;
                p50)    val=$STAT_P50 ;;
                p90)    val=$STAT_P90 ;;
                p99)    val=$STAT_P99 ;;
                max)    val=$STAT_MAX ;;
                mean)   val=$STAT_MEAN ;;
                stddev) val=$STAT_STDDEV ;;
            esac
            ns_str=$(fmt_ns "$val")
            ms_str=$(fmt_ms "$val")
            printf '| %s | %s | %s |\n' "$metric" "$ns_str" "$ms_str"
        done
        echo
        echo "## Raw samples"
        echo
        echo "One ns value per line, in iteration order (NOT sorted):"
        echo
        echo '```'
        for sample in "${SAMPLES[@]}"; do
            echo "$sample"
        done
        echo '```'
        if [[ "$FAIL_COUNT" -gt 0 ]]; then
            echo
            echo "_${FAIL_COUNT} iteration(s) produced no boot-to-end line within the"
            echo "${TIMEOUT_S} s per-run timeout; those iterations are excluded above._"
        fi
        echo
        echo "## Verdict"
        echo
        echo "Baseline only — no proposal under measurement. Cite the band above"
        echo "(p10 / p50 / p90) when comparing later changes against this snapshot."
        echo "Single-run boot-to-end claims in PR bodies should be replaced with a"
        echo "fresh harness run when a non-trivial perf-relevant change lands; see"
        echo "[\`docs/standards/infrastructure.md\`](../../standards/infrastructure.md)"
        echo "§\"Performance harness\"."
    } > "$REPORT_PATH"

    echo
    echo "report written: ${REPORT_PATH#${REPO_ROOT}/}"
fi

exit 0
