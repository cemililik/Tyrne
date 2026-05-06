# Pre-flight — full-tree comprehensive review

- **Date:** 2026-05-06
- **Branch:** `development`
- **HEAD SHA:** `214052d1bbcb0943ecdb236e7929f4698bac351a`
- **Working tree:** clean (sole untracked file is the plan artifact `2026-05-06-full-tree-comprehensive-review-plan.md`).

## Tooling state

| Check | Command | Result |
|---|---|---|
| Format | `cargo fmt --all -- --check` | **clean** (no diff) |
| Host clippy | `cargo host-clippy` (alias for `clippy --all-targets -- -D warnings`) | **clean** (no warnings) |
| Kernel clippy | `cargo kernel-clippy` (alias for `clippy --target aarch64-unknown-none -p tyrne-bsp-qemu-virt -- -D warnings`) | **clean** (no warnings) |
| Host tests | `cargo host-test` (alias for `cargo test`, default-members) | **149 / 149 pass** (25 hal + 90 kernel + 34 test-hal; 2 doc-tests ignored) |
| Kernel build | `cargo kernel-build` | **builds** for `aarch64-unknown-none` |
| Miri | `cargo +nightly miri test` | **running** in background (`bm2uv1pmo`); recorded in §Miri below when complete |

## Test count breakdown

| Crate | Tests |
|---|---|
| `tyrne-hal` | 25 |
| `tyrne-kernel` | 90 |
| `tyrne-test-hal` | 34 |
| Doc-tests (host) | 2 ignored |
| **Total** | **149** |

This matches the count recorded in [`docs/roadmap/current.md`](../../../../roadmap/current.md) at HEAD.

## Inventory

### Source code (Rust + asm)

| Crate / area | Files | LOC |
|---|---|---|
| `kernel/src/` | 10 | 3 654 |
| `hal/src/` | 7 | 1 198 |
| `test-hal/src/` | 6 | 998 |
| `bsp-qemu-virt/src/*.rs` | 6 | 2 026 |
| `bsp-qemu-virt/src/*.s` (boot.s + vectors.s) | 2 | 333 |
| `bsp-qemu-virt/build.rs` | 1 | 17 |
| `bsp-qemu-virt/linker.ld` | 1 | 57 |
| **Total source** | **33** | **8 283** |

(`find … -name '*.rs' -o -name '*.s' -o -name '*.S'` reports 9 590 total when including some build/test fixtures the workspace excludes from the per-crate sum; the per-crate table above is authoritative for review scope.)

### `unsafe` surface

- **92** `unsafe` occurrences total across `kernel/`, `hal/`, `bsp-qemu-virt/`, `test-hal/` (counting `unsafe { … }` blocks, `unsafe fn`, `unsafe impl`, `unsafe extern`, and `#[unsafe(...)]` attributes; comments excluded).
- Within the kernel crate: `kernel/src/sched/mod.rs` is the **only** file with `unsafe` (45 hits, all under UNSAFE-2026-0008/0009/0014). `kernel/src/cap/`, `kernel/src/obj/`, `kernel/src/ipc/`, and `kernel/src/lib.rs` are zero-`unsafe`. This matches the design claim from the Phase A code review.

### Audit log

- **21 entries** in [`docs/audits/unsafe-log.md`](../../../audits/unsafe-log.md): UNSAFE-2026-0001 through UNSAFE-2026-0021. UNSAFE-2026-0012 is `Removed` (removal commit `f9b72f8`); the remaining 20 are Active.
- Numbering has no gaps (0001–0021 contiguous; 0012 retained as a removed-but-archived marker).

### ADRs

- **25 ADRs** in [`docs/decisions/`](../../../decisions/) (0001–0025; 0023 reserved-empty, no file present at HEAD).
- ADR-0018 (badge scheme + reply-recv) is `Deferred`; remainder are `Accepted`.

### Documentation

- **174** markdown files under `docs/`, of which:
  - 25 ADRs.
  - 8 architecture docs.
  - 15 standards docs.
  - 21 audit-log entries (single file, multi-section).
  - 4 guides.
  - 12 phase / task index files.
  - ~70 task user-story files (open/closed across phase-a / phase-b / phase-c-j placeholders).
  - 9 review master-plan + README files + 14 prior review artefacts.
  - Root `glossary.md`, `README.md`, etc.

### Closed tasks since Phase A code review

T-006 (raw-pointer scheduler bridge), T-007 (idle task + typed deadlock), T-008 (architecture docs scheduler.md + ipc.md + hal.md updates), T-009 (timer init + CNTVCT), T-011 (missing-tests bundle), T-012 (exception + IRQ infrastructure), T-013 (EL drop to EL1). All `Done`.

## Risk class

**Security-sensitive.** The review surface includes every kernel subsystem, capabilities, IPC, scheduler, exception path, EL drop, GIC, every audited `unsafe`, and asm. The merged-artifact verdict is conditional on Track C (security) and Track D (performance) returning Approve.

## Outcome

Pre-flight gate **passes** — all parallel tracks (A through J) are clear to launch.

## Miri

- **Result:** **149 / 149 pass** (25 hal + 90 kernel + 34 test-hal; 2 doc-tests ignored, identical to non-miri host-test run). No undefined behaviour, no Stacked-Borrows violation, no aliasing violation reported. Wall clock ~20 s. Output captured to `/tmp/tyrne-miri-full.log`.
- Track F owns continuing miri telemetry; this entry is the gate snapshot.
