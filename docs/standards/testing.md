# Testing

How Umbrix code is tested. The kernel / `no_std` context imposes constraints that ordinary Rust testing handles gracefully with some adaptation; this standard spells out those adaptations and fixes the vocabulary so that reviewers and CI can agree on what a test is and what it proves.

## Scope

Applies to all crates in the Umbrix workspace: kernel, HAL, BSPs, userspace services, and build/tooling. Tests are themselves code and subject to [code-style.md](code-style.md).

## Test layers

Umbrix uses four layers. A given change almost always adds or updates tests in more than one layer.

### 1. Unit tests

- Live in the source file they test (`#[cfg(test)] mod tests { ... }`).
- Run with the host standard library (`cargo test -p <crate>`), isolated from kernel concerns.
- Test pure logic: data structures, algorithms, capability-token invariants, encoding/decoding.
- A unit test for a kernel module runs in a harness that stubs out the kernel's HAL traits with host-friendly test doubles.
- Fast — the full unit-test suite must finish in under a minute on a contemporary laptop.

### 2. Integration tests

- Live in the crate's `tests/` directory.
- Run in `cargo test` but exercise across-module contracts inside a single crate.
- For kernel-facing crates, integration tests may require a host-side harness that instantiates a fake capability table, fake address space, etc.
- Fail fast on contract violations that unit tests cannot see because they cross module boundaries.

### 3. QEMU boot / smoke tests

- Boot the kernel under QEMU `virt` (primary target) and assert it reaches a known checkpoint.
- Driven by a test runner (`tools/testrun` or similar, to be built in Phase 4) that:
  - Compiles the kernel with the `test` configuration.
  - Launches QEMU with serial redirected to a captured stream.
  - Asserts that the kernel emits an "all-green" marker within a timeout, or fails with the serial dump.
- Each QEMU test is named by the scenario it proves ("boot + syscall round-trip", "IPC rendezvous", "page fault handling").
- Slower — seconds per test. Run as a CI gate but not on every file save.

### 4. Hardware smoke tests

- Exercise the kernel on real hardware (Pi 4, Pi 5, Jetson CPU) during release preparation.
- Manual or semi-automated depending on lab setup.
- Not a CI gate (yet). Required per Tier 2+ target before a release.

## What has tests

Every change meets this minimum:

- A **public API** added or changed has unit tests for its contract.
- An **unsafe invariant** documented in a `# Safety` section has an integration or unit test that exercises the path under normal and adversarial inputs, where that is feasible with a host-side harness.
- A **bug fix** has a regression test that fails before the fix and passes after. A bug fix with no regression test is suspect and returned to the author.
- A **syscall** added or changed has at least one QEMU-boot test that exercises it end-to-end.
- An **error path** — a new variant in an `Error` enum — has a test that provokes it.
- A **concurrency fix** is accompanied by a test or, where deterministic testing is impossible, a documented rationale in the PR.

## What does not need tests

- Trivially correct getters / setters that are generated or obvious.
- Compile-time impls (`From`, `Display`) whose behavior is implied by the types, when the type system already constrains the content.
- Test code itself — but helper functions in test code should be simple enough to be obviously correct.

These exemptions are narrow. When in doubt, write the test.

## Test naming

Pick one convention and hold it. Umbrix uses:

```
test_<subject>_<condition>_<expected_outcome>
```

Examples:

- `test_endpoint_send_with_no_receiver_returns_no_receiver`
- `test_capability_table_full_returns_caps_exhausted`
- `test_panic_handler_on_kernel_panic_halts_cpu`

The condition and outcome should be specific enough that a reader can diagnose a failure from the name alone.

## Fixtures and doubles

- Prefer explicit fixtures over hidden globals. A test that needs a fake `MemoryCap` constructs one locally.
- Test doubles for HAL traits live in a `test-hal` crate (to be added with the workspace). They implement the same traits with deterministic, inspectable behavior.
- Do not share mutable state between tests. Tests run in parallel by default; a shared mutable global will corrupt. Use `thread_local!` or per-test instances.

## Non-determinism

- Tests are deterministic. Random input is fine when seeded; unseeded randomness is not.
- Time-dependent tests use a mock clock, not `std::time`.
- QEMU tests avoid depending on precise timings. They assert on output content, not on how long a step took.

## Coverage

The project does not set a hard coverage number. Coverage is a lagging indicator; good tests are better than lots of tests. That said:

- Kernel core subsystems (IPC, capabilities, memory, scheduler) should approach or exceed 80% line coverage once the implementation lands.
- Hardware interaction code is harder to cover; the QEMU smoke layer compensates.

Coverage is measured with `cargo llvm-cov` on host-runnable tests. Hardware-only paths are excluded from the coverage denominator.

## CI gates

- `cargo test --workspace` — must pass.
- `cargo clippy --workspace --all-targets -- -D warnings` — must pass.
- `cargo fmt --all -- --check` — must pass.
- QEMU smoke tests — must pass on the primary target.
- Hardware smoke tests — run periodically, not per-PR.

A red CI is never ignored. Flaky tests are bugs, not facts of life.

## Failure mode

When a test fails in CI:

1. The author reproduces locally.
2. If the failure reveals a real bug, the test stays and the bug is fixed.
3. If the failure reveals a flaky test, the flaky test is fixed (not disabled) in the same PR.
4. Disabling a test with `#[ignore]` requires a comment referencing a tracking issue and a statement of what would re-enable it.

## Anti-patterns to reject

- Tests that assert nothing (a function is called; its output is not checked).
- Tests that mutate shared state and depend on test order.
- `#[ignore]`d tests without a tracking issue.
- Tests that hide failures behind `if let Err(_) = ... { /* ignore */ }`.
- Tests that take the exact code under test and restate it — test the *behavior*, not the implementation.
- QEMU tests that depend on exact timing rather than content.

## Tooling

- `cargo test` — host-side unit and integration tests.
- `cargo llvm-cov` — coverage measurement.
- `miri` — runs unit tests under an interpreter that catches some UB.
- QEMU test runner — planned in `tools/testrun`.
- `cargo-nextest` — optional, for faster test runs; acceptable but not required.

## References

- The Rust Book, Testing chapter: https://doc.rust-lang.org/book/ch11-00-testing.html
- Embedded Rust testing patterns: https://docs.rust-embedded.org/book/
- `cargo-nextest`: https://nexte.st/
- Hubris testing approach (prior art): https://hubris.oxide.computer/
