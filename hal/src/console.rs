//! Console primitive and its formatted-output adapter.
//!
//! See [ADR-0007] for the design rationale behind the trait shape.
//!
//! [ADR-0007]: https://github.com/cemililik/Tyrne/blob/main/docs/decisions/0007-console-trait.md

use core::fmt;

/// Byte-sink console for early-boot and panic-time diagnostic output.
///
/// `Console` is the lowest-level diagnostic channel in Tyrne. It works before
/// the MMU is active, before IPC exists, and during panic. Every higher-level
/// logging facility — the `tyrne-log` facade, the userspace log service —
/// layers on top of what this trait guarantees.
///
/// # Contract
///
/// - **Synchronous.** Calls do not return until the implementation has handed
///   the bytes off to its transport (for a `UART`, that typically means the
///   `FIFO` has accepted them; it does not mean the pins have cleared).
/// - **Infallible.** A console that cannot write silently drops bytes. The
///   goal is best-effort communication, not reliable transport; any failure
///   mode a caller could do something useful with belongs to a different
///   abstraction.
/// - **No allocation.** Implementations must not touch the heap.
/// - **Best-effort under contention.** On multi-core systems, concurrent
///   writers may interleave. Implementations should avoid deadlocks
///   (for example, by using `try_lock` patterns in panic paths) but are
///   free to produce garbled output rather than block indefinitely.
/// - **[`Send`] + [`Sync`].** The trait bound is compiler-checked, so
///   multi-core safety is not left to convention.
///
/// Formatted output is provided by the [`FmtWriter`] adapter.
pub trait Console: Send + Sync {
    /// Write the given bytes to the console.
    ///
    /// Best-effort; see the trait-level contract for the failure model.
    fn write_bytes(&self, bytes: &[u8]);
}

/// Adapter that implements [`core::fmt::Write`] on top of any [`Console`].
///
/// Enables the `write!` / `writeln!` macros against a `Console` without
/// committing the HAL's trait surface to `core::fmt::Write`. Formatted output
/// is allowed in non-panic paths; panic-handler code should prefer
/// [`Console::write_bytes`] directly, to avoid invoking `Display` impls that
/// could themselves panic.
///
/// # Example
///
/// ```ignore
/// use core::fmt::Write;
/// use tyrne_hal::FmtWriter;
///
/// // `console` is some `&dyn Console`:
/// let mut w = FmtWriter(console);
/// let _ = write!(w, "boot CPU {id} online");
/// ```
pub struct FmtWriter<'a>(pub &'a dyn Console);

impl fmt::Write for FmtWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.write_bytes(s.as_bytes());
        Ok(())
    }
}
