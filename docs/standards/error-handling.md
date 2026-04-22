# Error handling

How Tyrne code reports, propagates, and recovers from failures. This standard covers kernel code, HAL, and userspace services uniformly, with explicit carve-outs for contexts where the default rules would be wrong (kernel panics, init paths, ISRs).

## Scope and goals

- Make the **failure model explicit** at every boundary.
- Make the **happy path obvious** and the error path auditable.
- Keep the kernel from crashing on recoverable conditions.
- Keep userspace driver faults out of the kernel.

## Rules

### 1. `Result<T, E>` is the default

Any operation that can fail for reasons outside the caller's invariants returns `Result<T, E>`, not `Option<T>`, and not a sentinel value.

Use `Option<T>` only for *genuine* absence (lookup that may legitimately return nothing). Failure is a `Result`, not a `None`.

### 2. Per-module error enums

Each module that can fail defines a local `Error` enum that captures the distinct failure modes callers might want to discriminate. Use `#[non_exhaustive]` so adding a variant in a future change is backward-compatible.

```rust
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum IpcError {
    /// The target endpoint has no waiting receiver and the call was non-blocking.
    NoReceiver,
    /// The receiving task's capability table is full.
    CapsExhausted,
    /// The sending task does not hold a send capability for this endpoint.
    NotAuthorized,
    /// The message payload exceeds the configured per-endpoint size.
    PayloadTooLarge,
}
```

`thiserror` is not used in `no_std` kernel code; derive `Debug` by hand or write a short `Display` impl only where needed.

### 3. Convert at boundaries, do not forward

Modules must not leak their own error types across public boundaries unless the boundary is specifically designed for that type. When calling a lower module and bubbling the error up, convert it at the boundary:

```rust
pub fn syscall_send(/* ... */) -> Result<(), SyscallError> {
    ipc::send(endpoint, msg).map_err(SyscallError::from)
}
```

The `From` impl makes the conversion explicit and localized. Downstream callers see one error type per layer, not a leaking hierarchy.

### 4. No `unwrap` / `expect` on hot paths

In the kernel and in the HAL, `unwrap()` and `expect()` are forbidden outside of:

- **One-shot `init` paths.** Code that runs once at boot, before any user-controllable input has been accepted, may use `expect()` with a message that describes the invariant being asserted.
- **Test-only code.** Unit and integration tests may `unwrap()` freely.

Clippy enforces this: `clippy::unwrap_used` and `clippy::expect_used` are `deny` in kernel crates.

### 5. `panic!` is an assertion, not error handling

`panic!` is reserved for situations where the program has detected a broken invariant that makes continuing undefined or unsafe. It is not a substitute for returning a `Result`.

Examples of legitimate panics (all in init paths):

- The bootloader handed us an invalid memory map.
- An impossible discriminant appeared in a match because the caller violated `Safety`.

Kernel panic behavior is defined by the configured panic strategy:

- **Development / QEMU CI:** panic prints to the serial console, dumps register state, and halts. CI treats any panic as a failed test.
- **Production / real hardware:** the panic handler either (a) triggers a supervised reset via the HAL, or (b) drops into a minimal diagnostic shell if a debug capability is present. The choice is board-specific and defined in the board's BSP.

Userspace tasks that panic do *not* take down the kernel. The task is terminated and the supervisor is notified (see §8).

### 6. Return errors, do not log-and-return

A function that returns a `Result` does not also log the error. Logging is the caller's decision, because only the caller knows whether the error is expected and how it should be surfaced.

Bad:

```rust
fn open(&self, name: &str) -> Result<Handle, FsError> {
    let h = self.lookup(name).map_err(|e| {
        log::error!("failed to open {}: {}", name, e);   // do not do this
        e
    })?;
    Ok(h)
}
```

Good: return the error; let the caller decide whether `error!`, `warn!`, or silent retry is right.

### 7. Preserve root cause through the stack

When converting between layers, preserve root-cause information. In `no_std`, `thiserror` and boxed errors are unavailable; encode root cause as an enum variant that carries a smaller `Error` from the layer below:

```rust
#[non_exhaustive]
pub enum SyscallError {
    NotAuthorized,
    Ipc(IpcError),
    Memory(MemoryError),
}

impl From<IpcError> for SyscallError {
    fn from(e: IpcError) -> Self {
        Self::Ipc(e)
    }
}
```

Do not swallow. Do not collapse distinct IPC failures into a generic "internal error."

### 8. Userspace task faults are kernel-visible errors

A userspace task that divides by zero, dereferences an unmapped address, executes an illegal instruction, or panics is **not** a kernel error. The fault handler:

1. Suspends the faulting task.
2. Encodes the fault as a `TaskFault` message.
3. Sends the message on the task's supervisor endpoint (held by whoever started the task).

The supervisor decides whether to restart, terminate, or log-and-drop. The kernel does not decide policy.

### 9. Interrupt service routines minimize error paths

ISRs run with restricted state. Inside an ISR:

- **Do not allocate.** No heap, no growable structures.
- **Do not log with blocking facilities.** Use the trace buffer or a lock-free ring; see [logging-and-observability.md](logging-and-observability.md).
- **Do not return `Result` that the ISR entry cannot handle.** An ISR either fully handles the event or marks a deferred work item for a task-context handler. An error inside an ISR is an assertion failure and should be a `panic!` in debug builds, or a quiet hardware-fault counter bump in release builds (with an entry in the audit log explaining why).

### 10. `todo!()` and `unimplemented!()` are transient markers

- `todo!()` is permitted only during active development of a feature behind a cfg flag or in a PR that will not land before the todo is resolved.
- `unimplemented!()` is permitted for genuinely unreachable branches that Rust's type system cannot prove (e.g. match arms on an enum whose other variants are excluded by earlier match arms).
- Neither may remain in main at the end of a feature's implementation. CI flags their presence.

## Panic strategy

The workspace sets `panic = "abort"` in `Cargo.toml` for kernel crates. Unwinding in kernel mode across a context switch boundary is undefined behavior without substantial runtime support, which we are not adding. `abort` is a hard stop that the kernel panic handler intercepts.

Userspace task crates may differ in later phases; that will be revisited in an ADR.

## Error-type design checklist

When introducing a new `Error` enum, ask:

- Does each variant represent a **distinct case a caller could handle differently**? If two variants always get the same treatment, merge them.
- Is the enum `#[non_exhaustive]`? It should be.
- Does it derive `Debug`, `Copy`, `Clone`, `Eq`, `PartialEq`? (Skip `Copy` if any variant carries non-`Copy` payload.)
- Does the enum's name include the subsystem, so `use`d names remain clear? (`IpcError`, not just `Error`.)
- Is there a `From` impl for every error type that should compose into this one?

## Anti-patterns to reject

- `.unwrap()` in kernel code outside init.
- `panic!` used as a shortcut for "return an error up there somewhere."
- Single monolithic `Error` enum that aggregates every subsystem's failures.
- Logging and returning the same error.
- Discarding error context by matching and re-panicking with a generic message.
- Using `bool` as a return type for "success or silent failure."

## Tooling

- `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic` are `deny` in kernel crates.
- `clippy::result_large_err` is `warn` — error types larger than ~128 bytes bloat the `Result` return value.
- `clippy::missing_errors_doc` is `warn` — every public `fn -> Result` should document its errors.

## References

- Rust API Guidelines, Error types: https://rust-lang.github.io/api-guidelines/interoperability.html#error-types
- `#[non_exhaustive]` RFC: https://rust-lang.github.io/rfcs/2008-non-exhaustive.html
- Hubris task supervision / restart model (prior art): https://hubris.oxide.computer/
