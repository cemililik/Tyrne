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
/// Panics if `frequency_hz == 0`. BSP impls should validate the frequency
/// at construction (e.g. by reading `CNTFRQ_EL0` and asserting non-zero)
/// so the panic is unreachable in production.
#[allow(
    clippy::cast_possible_truncation,
    reason = "saturating cast handled explicitly by the if/else guard above"
)]
#[must_use]
pub const fn ticks_to_ns(count: u64, frequency_hz: u64) -> u64 {
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
/// # Panics
///
/// Panics if `frequency_hz == 0`. See [`ticks_to_ns`].
#[must_use]
pub const fn resolution_ns_for_freq(frequency_hz: u64) -> u64 {
    // (1e9 + freq/2) / freq is the standard nearest-integer division
    // pattern for positive integers. Overflow analysis: u64::MAX / 2
    // ≈ 9.2e18, so adding 1e9 stays well within u64 for any frequency
    // a real timer would report.
    (NANOS_PER_SECOND + frequency_hz / 2) / frequency_hz
}

#[cfg(test)]
mod tests {
    use super::{resolution_ns_for_freq, ticks_to_ns, NANOS_PER_SECOND};

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
}
