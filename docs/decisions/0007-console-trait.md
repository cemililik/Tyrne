# 0007 — `Console` HAL trait signature

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

Phase 4b begins with the HAL's `Console` trait. Its job is to provide the earliest-possible diagnostic byte sink: the kernel must be able to produce output **before** the scheduler is up, **before** IPC exists, **before** the userspace log service is running, and **during** panic — when nothing else in the system can be trusted. Every later diagnostic facility (the `tyrne-log` facade, the structured log service) layers *on top* of what `Console` guarantees; `Console` itself is the last line of visibility when things go wrong.

The trait's method surface will be a contract that every BSP implements and that the kernel depends on for the lifetime of the project. Small choices — fallibility, `&self` vs. `&mut self`, formatting — are not small in aggregate.

See [`docs/architecture/hal.md`](../architecture/hal.md) for the HAL's overall shape and [`docs/standards/logging-and-observability.md`](../standards/logging-and-observability.md) for where `Console` fits inside the broader logging story.

## Decision drivers

- **Works pre-MMU, pre-scheduler, pre-IPC.** Called from BSP early-init before most of the kernel exists. Synchronous, no allocation, no dependency on a running runtime.
- **Works from the panic handler.** Nothing about the kernel's state can be assumed trustworthy at panic time. The console must not require locks it cannot safely acquire, must not allocate, and must be best-effort — dropping bytes under contention is preferable to deadlocking or panicking recursively.
- **Simple to implement per BSP.** A BSP will implement this trait with a handful of MMIO writes to a UART. The more complex the signature, the more BSP authors get wrong.
- **Ergonomic for formatted output in non-panic paths.** The kernel wants to say `write!(con, "boot CPU {id} online")` without allocating or depending on `std::io`.
- **Multi-core safe.** On systems with multiple cores online, two cores may race to write during concurrent panics. The console is globally shared; the trait bounds must permit that.
- **No formatting, no levels, no buffering in the trait itself.** Those belong to the userspace log service and the `tyrne-log` facade (see [logging-and-observability.md](../standards/logging-and-observability.md)). Keeping `Console` byte-level keeps the BSP surface minimal.

## Considered options

1. **Byte-slice primitive.** A single method `fn write_bytes(&self, bytes: &[u8])`. Minimal, infallible, easy to implement.
2. **`core::fmt::Write` as the primary trait.** Implement `core::fmt::Write` directly so `write!(con, "...")` works natively. Ergonomic but commits the HAL surface to a standard-library trait.
3. **Byte-slice primitive plus an adapter type** that implements `core::fmt::Write` on top of it. Best of both: BSPs implement the small primitive; callers who want formatted output import the adapter.

## Decision outcome

**Chosen: Option 3 — byte-slice primitive plus adapter.**

The primitive `Console` trait carries a single method:

```rust
pub trait Console: Send + Sync {
    fn write_bytes(&self, bytes: &[u8]);
}
```

An adapter `FmtWriter<'a>` lives alongside it and provides `core::fmt::Write` for formatted output:

```rust
pub struct FmtWriter<'a>(pub &'a dyn Console);

impl core::fmt::Write for FmtWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.0.write_bytes(s.as_bytes());
        Ok(())
    }
}
```

The primitive is `&self` (immutable) because a kernel `Console` is almost always a global or per-CPU singleton reached through interior mutability (MMIO is inherently interior-mutable through `unsafe`; a test fake uses a `Mutex`). `&self` is strictly more usable than `&mut self` and does not compromise safety, since the implementation is responsible for whatever synchronization it needs.

The primitive is **infallible** — no `Result` return. A UART that has physically died cannot be "recovered from" by the kernel; the goal of `Console` is best-effort communication, not reliable transport. Any error conditions a specific BSP might detect are its internal responsibility to log (ironically) or silently drop. Callers never have to reason about Console-failure paths.

`Send + Sync` is a trait bound, not a `# Safety` contract: the compiler enforces it. A BSP that implements `Console` with interior state must make that state `Send + Sync`, or the implementation does not compile. This is load-bearing for multi-core correctness.

## Consequences

### Positive

- **Minimal BSP surface.** One method, one signature, one line of MMIO per byte (or a small FIFO burst). The simplest possible thing that could work.
- **Formatted output remains possible.** `write!(FmtWriter(&*console), "...")` is one import and one wrapper; no loss of ergonomics compared to a native `fmt::Write` design.
- **Infallibility removes a recursion class at panic.** A fallible `write_bytes` would return `Err` that the panic handler would want to log, which would call `write_bytes`, which could fail again. Infallibility short-circuits the loop.
- **`Send + Sync` bound is compiler-checked.** Multi-core safety is not a convention; it is a compile-error if violated.
- **Test fakes are trivial.** `FakeConsole` wraps a `Mutex<Vec<u8>>` and satisfies the trait in a handful of lines; see [`tyrne-test-hal`].

### Negative

- **Callers wanting formatted output import `FmtWriter` explicitly.** Minor ergonomic cost. A future convenience macro (`con_write!`) can hide it if the pattern becomes tedious.
- **No error signal means disabled / failing consoles are silent.** If a BSP's UART is broken, the kernel cannot know by calling `write_bytes`. The kernel panics for other reasons long before this matters; but it is a real limitation for diagnostic self-test code.
- **Formatting can itself fail (in exotic cases).** `core::fmt` rendering of user types can panic if a `Display` impl is buggy. The adapter does not isolate the kernel from that — it is the caller's responsibility to use only trusted `Display` impls in panic contexts. Documented in the rustdoc.

### Neutral

- The primitive takes `&[u8]`, not `&str`. Either would work; bytes are the more honest interface since a UART does not care about UTF-8. The `FmtWriter` bridge enforces UTF-8 at its seam.
- The trait has no `flush` method. UART writes on the BSPs we care about are synchronous — each byte is a register write; there is nothing to flush. A future ADR may introduce `flush` if a buffered-output BSP arrives.

## Pros and cons of the options

### Option 1 — byte-slice primitive only

- Pro: absolutely minimal; one method.
- Pro: infallible; panic-safe.
- Con: every caller that wants formatted output has to roll their own `write_fmt` helper.
- Con: duplicating the adapter across crates is a `DRY` failure.

### Option 2 — `core::fmt::Write` native

- Pro: `write!(con, "...")` works out of the box.
- Pro: no adapter type.
- Con: `fmt::Write` requires `&mut self`, which complicates the singleton-with-interior-mutability pattern.
- Con: `write_str` returns `Result<(), fmt::Error>`, so the trait is fallible; infallibility at the BSP level then requires ignoring the return — an unpleasant smell.
- Con: the HAL trait now depends on a `core::fmt` type; changing it means changing the standard library.

### Option 3 — byte-slice primitive plus adapter (chosen)

- Pro: BSPs implement the minimal thing.
- Pro: callers who want formatting get it via one-line import.
- Pro: `&self` + infallible primitive is safer at panic and simpler for MMIO.
- Pro: adapter is a thin type; changing or extending it does not affect BSP implementations.
- Con: two items to know about instead of one. Acceptable given the ergonomic win.

## References

- [ADR-0006: Workspace layout and initial crate boundaries](0006-workspace-layout.md).
- [`docs/architecture/hal.md`](../architecture/hal.md) — `Console` trait's architectural role.
- [`docs/standards/logging-and-observability.md`](../standards/logging-and-observability.md) — where `Console` sits relative to the log facade and the userspace log service.
- [`docs/standards/error-handling.md`](../standards/error-handling.md) — panic strategy; why infallibility at this layer is right.
- [`docs/standards/unsafe-policy.md`](../standards/unsafe-policy.md) — the discipline the MMIO inside any BSP `Console` impl lives under.
- `core::fmt::Write`: https://doc.rust-lang.org/core/fmt/trait.Write.html
- Hubris `ringbuf` prior art (for how embedded kernels handle early-diagnostic output): https://hubris.oxide.computer/
