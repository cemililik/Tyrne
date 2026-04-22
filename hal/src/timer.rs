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
