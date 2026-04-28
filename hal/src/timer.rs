//! Monotonic time and deadline arming.
//!
//! See [ADR-0010] for the v1 scope and the list of deferred capabilities.
//!
//! [ADR-0010]: https://github.com/cemililik/TyrneOS/blob/main/docs/decisions/0010-timer-trait.md

/// Monotonic time source and one-shot deadline support.
///
/// Covers the single-core primitives the kernel needs to read current time
/// and to schedule a wake-up as an interrupt. Per-core timers, periodic
/// APIs, and alternate clock sources are out of scope in v1 and will
/// arrive through their own ADRs.
///
/// # Contract
///
/// - **Object-safe.** The kernel uses `&dyn Timer`.
/// - **`Send + Sync`.** Compiler-enforced; the same `Timer` may be
///   referenced from any online core.
/// - **No allocation.** Implementations must not touch the heap.
/// - **Nanoseconds as the unit.** All four methods exchange `u64`
///   nanoseconds; mixing units at the HAL boundary is a bug.
/// - **Absolute deadlines.** `arm_deadline` takes the target time, not
///   a delay. Callers who want relative delays compute
///   `now_ns() + delay`.
/// - **Single armed deadline.** At most one deadline is pending; arming a
///   second replaces the first. Software multiplexing of many deadlines
///   is the scheduler's responsibility.
/// - **Past deadlines fire promptly.** Arming a deadline in the past
///   must not be silently dropped; the IRQ fires as soon as the hardware
///   can deliver it.
pub trait Timer: Send + Sync {
    /// Return nanoseconds elapsed since boot.
    ///
    /// Monotonic: never goes backwards. 64 bits are enough for ~584 years
    /// of runtime.
    fn now_ns(&self) -> u64;

    /// Arm a one-shot deadline at the given absolute time.
    ///
    /// When [`Timer::now_ns`] reaches or exceeds `deadline_ns`, an IRQ
    /// fires on the timer's interrupt line. Arming replaces any
    /// previously-armed deadline.
    fn arm_deadline(&self, deadline_ns: u64);

    /// Clear any pending deadline.
    ///
    /// A no-op when no deadline is armed.
    fn cancel_deadline(&self);

    /// Return the timer's resolution in nanoseconds.
    ///
    /// Deadlines round to the nearest multiple of this value; finer
    /// precision at the call site is silently lost.
    fn resolution_ns(&self) -> u64;
}

// ─── Tick-frequency arithmetic helpers ────────────────────────────────────────
//
// Pure, host-testable utilities used by any [`Timer`] implementation that
// is backed by a free-running counter at a known frequency in Hz (the ARM
// Generic Timer, x86 TSC + invariant-TSC frequency, RISC-V `time` CSR,
// etc.). They live in `tyrne-hal` rather than in any single BSP because
// the arithmetic is identical across all such implementations and unit
// tests can exercise it without inline assembly.

/// Nanoseconds in one second. Lifted to a named const so call sites read
/// as intent rather than as a magic literal.
pub const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// Convert counter ticks to nanoseconds, given the counter's frequency in Hz.
///
/// Uses 128-bit intermediate arithmetic so the multiplication is overflow-
/// free for any tick count up to `u64::MAX` and any sane frequency
/// (≥ 1 Hz). The cast back to `u64` is **saturating**, not wrapping:
/// extreme inputs return `u64::MAX` rather than wrapping to a small value.
/// This preserves the `Timer::now_ns` monotonicity contract — at the rare
/// extreme where elapsed nanoseconds exceed `u64::MAX` (~584 years),
/// `now_ns` plateaus at `u64::MAX` instead of wrapping to zero.
///
/// # Panics
///
/// Panics with a named message if `frequency_hz == 0`. BSP impls should
/// validate the frequency at construction (e.g. by reading `CNTFRQ_EL0`
/// and asserting non-zero) so the panic is unreachable in production —
/// but the explicit `assert!` here ensures the contract is honoured even
/// if a caller forgets, rather than degrading to an implicit
/// divide-by-zero (which produces an unfriendly compile-time error in
/// const context and an unfriendly runtime error otherwise).
#[allow(
    clippy::cast_possible_truncation,
    reason = "saturating cast handled explicitly by the if/else guard at the end of this function"
)]
#[must_use]
pub const fn ticks_to_ns(count: u64, frequency_hz: u64) -> u64 {
    assert!(
        frequency_hz != 0,
        "ticks_to_ns: frequency_hz must be > 0 (BSP must validate CNTFRQ_EL0 / equivalent at boot)",
    );
    let intermediate = (count as u128) * (NANOS_PER_SECOND as u128);
    let ns = intermediate / (frequency_hz as u128);
    if ns > u64::MAX as u128 {
        u64::MAX
    } else {
        ns as u64
    }
}

/// Round-to-nearest resolution in nanoseconds for the given counter frequency.
///
/// Per ADR-0010 §Decision outcome, `resolution_ns` is the *minimum
/// meaningful deadline granularity*. Round-to-nearest avoids understating
/// the hardware period (which would mislead a caller into thinking the
/// timer is finer-grained than it actually is). For a 62.5 MHz counter
/// (QEMU virt) this gives 16 ns exactly; for a 19.2 MHz counter
/// (52.0833… ns true period) it gives 52 ns.
///
/// # Floor of 1 ns
///
/// For `frequency_hz > 2_000_000_000` (2 GHz), the integer round-to-
/// nearest formula would return `0` (the true period is <0.5 ns). A
/// resolution of 0 ns is meaningless to callers and would risk
/// divide-by-zero in any code that uses the resolution as a divisor.
/// The implementation therefore clamps the result to at least `1` ns,
/// which is the smallest representable resolution at nanosecond unit.
/// Sub-nanosecond precision is silently lost — consistent with the
/// trait contract's "finer precision at the call site is silently
/// lost" wording. No timer Tyrne currently targets runs above 1 GHz,
/// but the clamp future-proofs against high-rate counters (e.g. x86
/// invariant TSCs at 3+ GHz).
///
/// # Panics
///
/// Panics with a named message if `frequency_hz == 0`. The assertion
/// is explicit — see [`ticks_to_ns`] for the reasoning.
#[must_use]
pub const fn resolution_ns_for_freq(frequency_hz: u64) -> u64 {
    assert!(
        frequency_hz != 0,
        "resolution_ns_for_freq: frequency_hz must be > 0 (BSP must validate CNTFRQ_EL0 / equivalent at boot)",
    );
    // (1e9 + freq/2) / freq is the standard nearest-integer division
    // pattern for positive integers. Overflow analysis: u64::MAX / 2
    // ≈ 9.2e18, so adding 1e9 stays well within u64 for any frequency
    // a real timer would report.
    let raw = (NANOS_PER_SECOND + frequency_hz / 2) / frequency_hz;
    // Clamp to ≥ 1: at frequencies above ~2 GHz the round-to-nearest
    // formula truncates to 0; see the "Floor of 1 ns" doc-section.
    if raw == 0 {
        1
    } else {
        raw
    }
}

/// Convert nanoseconds to ticks at the given counter frequency.
///
/// The inverse of [`ticks_to_ns`]; used by `Timer::arm_deadline`
/// implementations to translate an absolute deadline (in nanoseconds
/// since boot) into a comparator-register value (in counter ticks).
///
/// Per ADR-0010 §Decision outcome, `Timer::arm_deadline`'s argument
/// is `deadline_ns: u64` — an absolute monotonic time in ns. The
/// hardware timer's compare register (`CNTV_CVAL_EL0` on aarch64,
/// equivalent on other targets) is in counter ticks; this function
/// is the conversion at the BSP boundary.
///
/// # Rounding
///
/// Uses **ceiling division** so that any sub-tick remainder rounds up
/// to the next tick. This is the rounding direction required by
/// ADR-0010 §Decision outcome's "When `now_ns()` reaches or exceeds
/// `deadline_ns`, the hardware timer IRQ fires" — flooring would arm
/// the comparator at the largest tick whose `ticks_to_ns` is ≤
/// `deadline_ns`, which can fire the IRQ up to one sub-tick *before*
/// `deadline_ns`, violating the "reaches or exceeds" contract. With
/// ceiling, the comparator's tick-equivalent is always ≥
/// `deadline_ns`, so the IRQ fires at-or-after the requested time.
///
/// # Saturation
///
/// Uses 128-bit intermediate arithmetic and a saturating cast back
/// to `u64`. For pathological inputs where `ceil(ns * frequency_hz /
/// 1e9)` exceeds `u64::MAX` (~584 years × 1 GHz, or any frequency
/// above 1e9 Hz with `ns ≈ u64::MAX`), the returned tick count
/// saturates at `u64::MAX`. Matches [`ticks_to_ns`]'s saturation
/// discipline.
///
/// # Panics
///
/// Panics with a named message if `frequency_hz == 0`. Same explicit
/// `assert!` as [`ticks_to_ns`] — see its `# Panics` section for the
/// reasoning.
#[allow(
    clippy::cast_possible_truncation,
    reason = "saturating cast handled explicitly by the if/else guard at the end of this function"
)]
#[must_use]
pub const fn ns_to_ticks(ns: u64, frequency_hz: u64) -> u64 {
    assert!(
        frequency_hz != 0,
        "ns_to_ticks: frequency_hz must be > 0 (BSP must validate CNTFRQ_EL0 / equivalent at boot)",
    );
    let intermediate = (ns as u128) * (frequency_hz as u128);
    let nanos = NANOS_PER_SECOND as u128;
    // Ceiling division so any sub-tick remainder rounds up to the
    // next tick (see § Rounding above).
    let ticks = intermediate.div_ceil(nanos);
    if ticks > u64::MAX as u128 {
        u64::MAX
    } else {
        ticks as u64
    }
}

#[cfg(test)]
mod tests {
    use super::{ns_to_ticks, resolution_ns_for_freq, ticks_to_ns, NANOS_PER_SECOND};

    // ── ticks_to_ns ──────────────────────────────────────────────────────────

    #[test]
    fn ticks_to_ns_zero_count_is_zero() {
        assert_eq!(ticks_to_ns(0, 62_500_000), 0);
    }

    #[test]
    fn ticks_to_ns_qemu_virt_one_second() {
        // 62.5 MHz × 1 second = 62_500_000 ticks → exactly 1e9 ns.
        assert_eq!(ticks_to_ns(62_500_000, 62_500_000), NANOS_PER_SECOND,);
    }

    #[test]
    fn ticks_to_ns_qemu_virt_single_tick() {
        // 1 tick at 62.5 MHz = 16 ns (exact, since 62.5 MHz divides 1e9).
        assert_eq!(ticks_to_ns(1, 62_500_000), 16);
    }

    #[test]
    fn ticks_to_ns_pi3_class_non_divisor() {
        // 19.2 MHz: 1 tick true period 52.0833… ns. floor → 52 ns; this
        // documents that ticks_to_ns truncates per-tick, but the multi-
        // tick form is exact at the second boundary because the u128 mul
        // happens before the divide.
        assert_eq!(ticks_to_ns(1, 19_200_000), 52);
        assert_eq!(ticks_to_ns(19_200_000, 19_200_000), NANOS_PER_SECOND);
    }

    #[test]
    fn ticks_to_ns_high_frequency_one_gigahertz() {
        // 1 GHz → 1 tick = 1 ns exactly.
        assert_eq!(ticks_to_ns(1, 1_000_000_000), 1);
        assert_eq!(ticks_to_ns(123_456_789, 1_000_000_000), 123_456_789);
    }

    #[test]
    fn ticks_to_ns_saturates_at_u64_max() {
        // u64::MAX ticks at 1 Hz would be u64::MAX seconds → vastly more
        // than u64::MAX ns. Saturate, not wrap.
        assert_eq!(ticks_to_ns(u64::MAX, 1), u64::MAX);
    }

    #[test]
    fn ticks_to_ns_no_silent_wrap_at_64bit_boundary() {
        // count = u64::MAX / NANOS_PER_SECOND + 1 is the smallest count
        // that triggers saturation at 1 Hz. Verify it returns u64::MAX
        // rather than wrapping. (u64::MAX / 1e9 ≈ 1.844e10.)
        let just_over = (u64::MAX / NANOS_PER_SECOND) + 1;
        assert_eq!(ticks_to_ns(just_over, 1), u64::MAX);
    }

    #[test]
    fn ticks_to_ns_const_fn_works_in_const_context() {
        // Compile-time evaluation guard — confirms `const fn` annotation.
        const NS: u64 = ticks_to_ns(62_500_000, 62_500_000);
        assert_eq!(NS, NANOS_PER_SECOND);
    }

    // ── resolution_ns_for_freq ───────────────────────────────────────────────

    #[test]
    fn resolution_qemu_virt_is_16_ns() {
        assert_eq!(resolution_ns_for_freq(62_500_000), 16);
    }

    #[test]
    fn resolution_one_gigahertz_is_one_ns() {
        assert_eq!(resolution_ns_for_freq(1_000_000_000), 1);
    }

    #[test]
    fn resolution_round_to_nearest_for_non_divisor() {
        // 19.2 MHz: 1e9 / 19_200_000 = 52.0833… → rounded-to-nearest = 52.
        assert_eq!(resolution_ns_for_freq(19_200_000), 52);
        // 54 MHz (BCM2711 generic-timer rate per mainline Linux):
        // 1e9 / 54_000_000 = 18.5185… → rounded-to-nearest = 19.
        assert_eq!(resolution_ns_for_freq(54_000_000), 19);
    }

    #[test]
    fn resolution_floor_vs_round_difference_documented() {
        // 33_333_333 Hz: floor = 30, round-to-nearest = 30 (true value
        // 30.000000300000003... → still 30). This test is here to lock
        // the rounding policy: if a future change accidentally switched
        // from round-to-nearest to floor, the QEMU virt and 1 GHz cases
        // above would still pass (both are exact divisors). This case
        // and the 19.2 MHz case above are the discriminating ones.
        assert_eq!(resolution_ns_for_freq(33_333_333), 30);
    }

    #[test]
    fn resolution_const_fn_works_in_const_context() {
        const RES: u64 = resolution_ns_for_freq(62_500_000);
        assert_eq!(RES, 16);
    }

    #[test]
    fn resolution_clamps_to_one_above_2ghz() {
        // Naive formula `(1e9 + freq/2) / freq` truncates to 0 for
        // freq > 2 * NANOS_PER_SECOND. The clamp makes the floor 1 ns
        // — the smallest representable resolution at nanosecond unit.
        // 3 GHz: true period ≈ 0.333 ns → clamped to 1 ns.
        assert_eq!(resolution_ns_for_freq(3_000_000_000), 1);
        // 10 GHz: true period 0.1 ns → still clamped to 1 ns.
        assert_eq!(resolution_ns_for_freq(10_000_000_000), 1);
        // u64::MAX Hz: even more extreme → clamped to 1 ns.
        assert_eq!(resolution_ns_for_freq(u64::MAX), 1);
    }

    #[test]
    fn resolution_two_ghz_is_one_ns_exactly() {
        // Boundary case: exactly 2 GHz gives raw = 1 ns (no clamp
        // needed). (1e9 + 1e9) / 2e9 = 1.
        assert_eq!(resolution_ns_for_freq(2_000_000_000), 1);
    }

    // ── Property-style monotonicity guards (per ADR-0010 contract) ────────────

    /// Sweep increasing tick counts and assert `ticks_to_ns` never
    /// decreases. This locks the trait-level monotonicity guarantee
    /// against future regressions — `wrapping_mul` would silently
    /// fail this; `saturating_mul` (the chosen behaviour) passes it.
    /// Run for representative frequencies (1 Hz extreme, QEMU virt
    /// 62.5 MHz, Pi-3-class 19.2 MHz, 1 GHz). Lightweight: ≤ 200
    /// samples per frequency, all on the host.
    #[test]
    fn ticks_to_ns_is_monotonic_across_frequencies() {
        for &freq in &[1, 19_200_000, 62_500_000, 1_000_000_000_u64] {
            let mut prev = 0;
            // Step pattern that exercises both small counts and the
            // saturation neighbourhood — geometric progression with
            // a final value past the saturation boundary at freq=1.
            for &count in &[
                0,
                1,
                100,
                10_000,
                1_000_000,
                100_000_000,
                10_000_000_000,
                1_000_000_000_000,
                u64::MAX / 2,
                u64::MAX,
            ] {
                let now = ticks_to_ns(count, freq);
                assert!(
                    now >= prev,
                    "monotonicity violated at freq={freq}, count={count}: \
                     prev={prev}, now={now}"
                );
                prev = now;
            }
        }
    }

    /// Confirm that the saturation branch fires at `u64::MAX` for at
    /// least one realistic frequency, and that successive calls past
    /// that point all return `u64::MAX` (no wrap to 0). Pairs with
    /// `ticks_to_ns_saturates_at_u64_max` above; this version tests
    /// the *plateau* property rather than just the single value.
    #[test]
    fn ticks_to_ns_plateaus_at_u64_max_after_saturation() {
        // At 1 Hz, u64::MAX ticks = u64::MAX seconds = vastly more than
        // u64::MAX ns. Any count past the saturation boundary returns
        // u64::MAX; further increases stay at u64::MAX (plateau).
        let just_over = u64::MAX / NANOS_PER_SECOND + 1;
        let way_over = u64::MAX / 2;
        let max = u64::MAX;
        assert_eq!(ticks_to_ns(just_over, 1), u64::MAX);
        assert_eq!(ticks_to_ns(way_over, 1), u64::MAX);
        assert_eq!(ticks_to_ns(max, 1), u64::MAX);
        // Plateau holds: saturated value never decreases as count grows.
        assert!(ticks_to_ns(way_over, 1) >= ticks_to_ns(just_over, 1));
        assert!(ticks_to_ns(max, 1) >= ticks_to_ns(way_over, 1));
    }

    // ── Explicit-panic-on-zero-frequency contract ────────────────────────────

    #[test]
    #[should_panic(expected = "ticks_to_ns: frequency_hz must be > 0")]
    fn ticks_to_ns_panics_on_zero_frequency() {
        // The doc-comment promises a named panic when freq == 0. Without
        // the explicit `assert!` this would fall through to a u128 div-by-
        // zero, which (a) produces a less informative panic message and
        // (b) is a compile-time error in const context. Keep the contract
        // honoured by an actual assert; this test guards it.
        let _ = ticks_to_ns(1, 0);
    }

    #[test]
    #[should_panic(expected = "resolution_ns_for_freq: frequency_hz must be > 0")]
    fn resolution_ns_for_freq_panics_on_zero_frequency() {
        let _ = resolution_ns_for_freq(0);
    }

    // ── ns_to_ticks ──────────────────────────────────────────────────────────

    #[test]
    fn ns_to_ticks_zero_ns_is_zero() {
        assert_eq!(ns_to_ticks(0, 62_500_000), 0);
    }

    #[test]
    fn ns_to_ticks_rounds_up_on_subtick() {
        // Locks the ceiling-rounding policy documented in the
        // `# Rounding` doc-section: any sub-tick remainder must round
        // *up* so the comparator's tick-equivalent is ≥ deadline_ns.
        // freq = 3 Hz means one tick = NANOS_PER_SECOND / 3 =
        // 333_333_333.333… ns; ns = 333_333_333 sits exactly on tick 1
        // (down) and ns = 333_333_334 is 1 ns past it, so the ceiling
        // tick count is 2. A floor-division regression would return 1
        // here and silently violate the "reaches or exceeds
        // deadline_ns" half of ADR-0010's contract.
        assert_eq!(ns_to_ticks(333_333_334, 3), 2);
        // Boundary check: exactly on the first tick stays at 1 tick.
        assert_eq!(ns_to_ticks(333_333_333, 3), 1);
    }

    #[test]
    fn ns_to_ticks_round_trips_against_ticks_to_ns_at_qemu_frequency() {
        // QEMU virt is 62.5 MHz; pick a tick count that is exactly
        // representable in ns (the inverse uses integer truncation,
        // so non-divisor frequencies introduce a small drift —
        // tested separately).
        let qemu_freq = 62_500_000;
        let count = 1_234_567;
        let ns = ticks_to_ns(count, qemu_freq);
        let round_trip_ticks = ns_to_ticks(ns, qemu_freq);
        assert_eq!(round_trip_ticks, count);
    }

    #[test]
    fn ns_to_ticks_one_second_yields_frequency_at_any_freq() {
        // 1 second = `frequency_hz` ticks, exactly, for any non-zero
        // frequency.
        for &freq in &[1u64, 1_000, 19_200_000, 62_500_000, 1_000_000_000] {
            assert_eq!(ns_to_ticks(NANOS_PER_SECOND, freq), freq);
        }
    }

    #[test]
    fn ns_to_ticks_saturates_at_u64_max() {
        // ns close to u64::MAX with a high frequency overflows the
        // u128 intermediate's u64 result; the function must saturate
        // rather than wrap.
        let huge_ns = u64::MAX;
        let high_freq = 1_000_000_000;
        assert_eq!(ns_to_ticks(huge_ns, high_freq), u64::MAX);

        // Exercise the saturation *branch* (not just the boundary):
        // pick a frequency strictly greater than NANOS_PER_SECOND so
        // the u128 quotient `ceil(ns * freq / 1e9)` exceeds u64::MAX
        // and the `if ticks > u64::MAX as u128` branch executes.
        let over_boundary_freq = 1_000_000_001;
        assert_eq!(ns_to_ticks(huge_ns, over_boundary_freq), u64::MAX);
        // Also verify the maximum possible frequency saturates.
        assert_eq!(ns_to_ticks(huge_ns, u64::MAX), u64::MAX);
    }

    #[test]
    #[should_panic(expected = "ns_to_ticks: frequency_hz must be > 0")]
    fn ns_to_ticks_panics_on_zero_frequency() {
        let _ = ns_to_ticks(1, 0);
    }
}
