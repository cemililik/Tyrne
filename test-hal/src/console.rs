//! Deterministic fake [`tyrne_hal::Console`] for host-side tests.

use std::sync::Mutex;
use tyrne_hal::Console;

/// A [`Console`] that captures every byte written into an internal buffer.
///
/// Used by unit tests to verify that code under test produced the expected
/// console output without requiring real hardware or QEMU. `FakeConsole`
/// is `Send + Sync` and is safe to share across threads in tests that
/// exercise concurrent write paths.
pub struct FakeConsole {
    captured: Mutex<Vec<u8>>,
}

impl FakeConsole {
    /// Construct a new, empty `FakeConsole`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            captured: Mutex::new(Vec::new()),
        }
    }

    /// Return a snapshot of every byte captured so far.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned. In test code this
    /// indicates a bug worth investigating rather than a condition to handle.
    #[must_use]
    pub fn captured(&self) -> Vec<u8> {
        self.captured
            .lock()
            .expect("FakeConsole mutex poisoned")
            .clone()
    }

    /// Return the captured output as a `UTF-8` string.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex has been poisoned, or if the captured
    /// bytes are not valid `UTF-8`. In tests, non-`UTF-8` output from the
    /// code under test is typically itself a bug.
    #[must_use]
    pub fn captured_str(&self) -> String {
        String::from_utf8(self.captured()).expect("FakeConsole captured non-UTF-8 bytes")
    }
}

impl Default for FakeConsole {
    fn default() -> Self {
        Self::new()
    }
}

impl Console for FakeConsole {
    fn write_bytes(&self, bytes: &[u8]) {
        self.captured
            .lock()
            .expect("FakeConsole mutex poisoned")
            .extend_from_slice(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::FakeConsole;
    use core::fmt::Write;
    use tyrne_hal::{Console, FmtWriter};

    #[test]
    fn captures_successive_byte_writes() {
        let c = FakeConsole::new();
        c.write_bytes(b"hello");
        c.write_bytes(b" world");
        assert_eq!(c.captured_str(), "hello world");
    }

    #[test]
    fn fmt_writer_produces_formatted_output() {
        let c = FakeConsole::new();
        let mut w = FmtWriter(&c);
        write!(w, "cpu {} online", 3).expect("FmtWriter is infallible");
        assert_eq!(c.captured_str(), "cpu 3 online");
    }

    #[test]
    fn default_fake_console_is_empty() {
        let c = FakeConsole::default();
        assert!(c.captured().is_empty());
    }
}
