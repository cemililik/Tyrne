# Logging and observability

How Tyrne code produces diagnostic output, what it promises about that output, and how operators and developers extract it. The constraints of kernel / `no_std` environments make naive logging unsafe; this standard describes an approach that works for both kernel and userspace while remaining auditable and secret-safe.

## Scope and goals

- Provide **structured, level-controlled, capability-gated logging** that is safe to call from any context in the system, including interrupt service routines.
- Support **tracing** — timing and causal relationships — without the heavyweight infrastructure of desktop observability stacks.
- Guarantee that **secrets never appear in logs**.
- Keep the **storage and transport of logs** out of the kernel; the kernel emits log records via a capability; userspace services handle persistence, egress, and presentation.

## Log levels

Five levels, evaluated in order of increasing severity:

| Level | Use for |
|-------|---------|
| `trace` | Very fine-grained events; path tracing, every IPC, every scheduler decision. Default off even in debug builds. |
| `debug` | State changes useful during development. Default off in release. |
| `info` | Notable expected events. Default on. |
| `warn` | Something is unusual but the system continues. Default on. |
| `error` | A recoverable failure occurred. Default on. |

There is no `fatal`. A fatal condition triggers the panic path (see [error-handling.md](error-handling.md)), which produces its own, richer diagnostic output.

## Structured records

Every log record is structured, not a formatted string:

```rust
// Conceptual shape, not real code.
struct LogRecord {
    timestamp: MonotonicNanos,
    level: Level,
    target: &'static str,      // e.g. "tyrne::ipc::endpoint"
    message: &'static str,     // short, literal
    fields: &'static [(&'static str, Value)], // typed key-value pairs
    span: Option<SpanId>,      // causal / timing context
    cpu: CpuId,
    task: Option<TaskId>,
}
```

- **`message`** is a short, literal English sentence without interpolation. ("IPC send rejected.")
- **`fields`** carry the variable data. ("endpoint" = EndpointId(7), "reason" = "no receiver").
- Rendering (human-readable vs. JSON vs. binary) is done by the consumer, not the producer.

The rationale is standard observability: searchable, machine-processable logs beat formatted strings.

## Logging APIs

The project provides a logging facade (planned crate `tyrne-log`) with macros mirroring the `log` crate's shape but producing structured records:

```rust
use tyrne_log::{trace, debug, info, warn, error};

info!(target = "ipc::endpoint", "IPC send rejected", endpoint = id, reason = "no receiver");
```

Crates **never** use `log::info!`, `tracing::info!`, or `std::println!` directly. The facade is the only approved entry point, for the reasons below.

## Context rules

### Kernel mode

- **ISR context:** `trace`/`debug`/`info` may be emitted. The macro writes to a per-CPU lock-free ring buffer; a kernel worker drains the buffer into the log transport at task context. ISRs never block on logging.
- **Task context in kernel:** same facade; same ring path by default.
- **Early boot:** before the log transport is up, records go to a bootstrap UART sink (see [architecture/hal.md](../architecture/hal.md), Phase 3) configured by the BSP. Records lost if the boot ring fills; this is acceptable for early boot.

### Userspace

- Services call the facade; the facade sends the record to a **log service** (a userspace task) via IPC.
- The log service holds the capabilities to any persistent sinks (serial console, flash, network).
- Services **cannot bypass the log service** to reach sinks directly. This is the capability discipline; it also simplifies redaction.

## Secrets and PII

- **No secret may be logged.** Not at any level. Not in debug builds.
- Types that carry secrets (`KeyMaterial`, `CapabilityToken`, `Secret<T>`) do **not** implement `Display` or unredacted `Debug`. Their `Debug` impl prints a type name and an object ID, never the bytes.
- When a function must log about a secret, it logs about its *identity*, not its *value*: `"key rotated", "key_id" = KeyId(42)`.
- PII (user names, device identifiers, network addresses) is treated as sensitive by default. Logs include it only when the subsystem's documented contract says to, and the field is tagged `sensitive = true` so downstream sinks can apply additional controls.

This is not just hygiene: a capability-based OS loses its meaning if capabilities show up in logs.

## Sampling and rate limits

- `trace` and `debug` are always candidates for sampling. The facade supports per-target rate limits (records-per-second) to avoid flooding the ring.
- A flooded ring drops the oldest records, not the newest. The log service reports dropped-record counts as an `info` event.
- Rate-limited records should be deterministic (e.g. 1-in-N) rather than time-based, to keep repro logs useful.

## Tracing and spans

- Causal / timing context is represented as **spans**. A span is opened before a unit of work and closed after; records emitted inside the span are associated with it.
- Spans carry a parent pointer, allowing causal chains across IPC boundaries.
- Spans have unique 64-bit ids generated from a per-CPU counter + CPU id.

In practice this maps onto the `tracing` crate's model; Tyrne will either adopt `tracing` directly (if a `no_std` subset works) or implement a compatible shape internally. The choice is deferred to an ADR at implementation time.

## Metrics

- Metrics are counters, gauges, and histograms, emitted separately from logs.
- The metric facade (planned) lets subsystems register counters at crate init:
  - `ipc_send_total` (counter)
  - `ipc_send_error_total{reason=…}` (counter)
  - `scheduler_context_switch_total` (counter)
  - `capability_table_capacity_used` (gauge)
- Userspace metric service scrapes the kernel's metric store through a capability-gated syscall at operator-configurable intervals.
- Metrics do not replace logs; they summarize what logs detail.

## Dynamic configuration

- Levels can be changed at runtime per subsystem through an operator capability (`LogControlCap`).
- The default configuration in the kernel build is compiled in; runtime changes live until the next reboot unless persisted by an operator service.

## Output destinations

- **Serial UART** — the always-available diagnostic channel. Early boot uses it exclusively.
- **Ring buffer in RAM** — retained across warm reboot on boards that support it; inspected via debug port.
- **Operator tool over IPC** — a debug/observability service that a developer can attach to and stream from.
- **Persistent storage** — eventually; not in scope for Phase 3.

## Log format on the wire

- Binary, versioned, efficient. A single format that renders to JSON or human text offline.
- A future ADR selects the specific encoding (candidates: `postcard`, a custom TLV). Until then, implementations emit a documented binary shape.

## Anti-patterns to reject

- `log::info!("endpoint {}: send failed because {}", id, reason)` — formatted, not structured.
- Logging in a place that allocates (heap-free is a hard rule in kernel).
- Logging an object whose `Debug` impl dumps secret bytes.
- Logging at `error` for expected failures (e.g. a receiver having no pending message when the caller is in non-blocking mode; that is `trace` at most).
- Logging inside a critical section beyond what is necessary.
- Duplicate logging (a layer logs and a caller logs the same event).

## Tooling

- Ring buffer inspection CLI (planned).
- JSON renderer for offline analysis.
- `grep`-friendly text renderer for quick triage.
- Metric scraper / dashboard integration — open; decided per deployment.

## References

- `tracing` crate: https://docs.rs/tracing/
- OpenTelemetry — the industry standard for spans + metrics: https://opentelemetry.io/
- `log` crate API shape (the familiar facade): https://docs.rs/log/
- Fuchsia logging design — kernel-to-userspace log service model.
- Linux kernel `printk` vs. `tracefs` — the trade-offs we learn from.
