# `unsafe` policy

`unsafe` is Rust's escape hatch. It is also the place where all memory-safety guarantees are negotiated away. In Umbrix's security-first posture, `unsafe` is a feature we use — because kernels must — but one we account for explicitly.

This standard governs every appearance of the `unsafe` keyword in the repository.

## Scope

Every use of `unsafe` is covered: `unsafe fn`, `unsafe impl`, `unsafe trait`, `unsafe {}` blocks. The rules apply equally in kernel, HAL, and userspace code.

Safer alternatives are always preferred. `unsafe` is a last resort, not a convenience.

## Rules

### 1. Every `unsafe` use is justified in a `// SAFETY:` comment

Directly adjacent to the `unsafe` block or item, a `// SAFETY:` comment must state:

- **Invariants upheld.** What memory-safety, aliasing, initialization, lifetime, or concurrency invariants this code relies on, and why they hold here.
- **Rejected alternatives.** Why the safer alternative (a safe abstraction, a bounds-checking helper, a typed wrapper) cannot be used. Performance is not, by itself, a valid rejection.
- **Audit reference.** A reference to the audit entry — either an issue ID, an ADR number, or a tag in the audit log (see §3).

Example:

```rust
// SAFETY: `vaddr` is constructed from a kernel-owned physical address via
// the page mapper and is guaranteed to be valid, aligned, and exclusively
// owned for the lifetime of `self`. A safe `VolatileRef` wrapper cannot be
// used here because the driver must issue a sequenced pair of writes
// inside a single ISR critical section (see ADR-0012, §3.2).
// Audit: UNSAFE-2026-0007.
unsafe {
    ptr::write_volatile(vaddr.as_mut_ptr::<u32>().add(2), value);
}
```

The comment is part of the code, not a nice-to-have. A PR that introduces `unsafe` without a conforming `SAFETY:` comment is not reviewable and must be returned to the author.

### 2. `unsafe fn` requires a `# Safety` section in its doc-comment

Every `unsafe fn` has a rustdoc `# Safety` section describing the invariants callers must uphold before calling the function. This is enforced by `#[deny(clippy::missing_safety_doc)]`.

Example:

```rust
/// Map a physical page into the current address space at the given virtual address.
///
/// # Safety
///
/// - `phys` must be page-aligned and refer to memory that the caller has the
///   right to map (see `MemoryCap`).
/// - `vaddr` must be page-aligned and must not already be mapped in the current
///   address space, or the caller must hold the UNMAP authority to supersede the
///   existing mapping.
/// - The caller must not invalidate other address spaces' translations
///   concurrently; the function does not acquire the global TLB lock.
pub unsafe fn map_page(phys: PhysAddr, vaddr: VirtAddr, flags: PageFlags) { /* ... */ }
```

### 3. Every `unsafe` block has an audit entry

An audit log file (`docs/audits/unsafe-log.md`, created when the first kernel code lands) tracks every `unsafe` block in the repository. Each entry contains:

- A unique tag: `UNSAFE-YYYY-NNNN` (year + zero-padded sequence).
- File path and function containing the block.
- One-line description.
- Invariants relied on (summarized).
- Who reviewed the introduction (maintainer + any second reviewer if security-sensitive).
- Date introduced. If removed: date removed and the replacement.

The audit log is append-only. Removing an `unsafe` block flips its status to `Removed` with a removal date and commit; it does not delete the entry. `cargo-geiger` output is periodically reconciled against the log.

### 4. `unsafe impl` and `unsafe trait` follow the same discipline

An `unsafe impl` declares that the implementer upholds invariants that the trait cannot express. The doc-comment on the `unsafe impl` must explain how those invariants are upheld. `unsafe trait` declarations list the invariants in the trait's doc-comment's `# Safety` section.

Example:

```rust
// SAFETY: `PageFrame` does not implement any interior mutability and is
// guaranteed to own a unique 4 KiB physical region; `Send` is therefore
// safe because transferring ownership preserves the uniqueness invariant.
// Audit: UNSAFE-2026-0003.
unsafe impl Send for PageFrame {}
```

### 5. Where `unsafe` is permitted

- **Memory-mapped I/O (MMIO) access.** Reading and writing device registers through raw pointers. These blocks are typically isolated inside a safe wrapper (e.g., a typed `Mmio<T>` struct).
- **Hardware-defined structures.** Page tables, TLB operations, cache maintenance.
- **Low-level context-switching primitives.** Saving/restoring CPU state, switching stacks.
- **FFI at the kernel/HAL boundary.** Calls to assembly stubs or firmware services (PSCI, SMC).
- **Intrinsics and inline assembly** that `cargo check` cannot reason about.

### 5a. Context-switch functions must use `#[unsafe(naked)]`

Any function whose asm body saves or restores the stack pointer (SP), or
whose correctness depends on SP having the caller's exact value on entry,
**must** be declared `#[unsafe(naked)]` and use `naked_asm!` as its sole
body. Use `extern "C"` so arguments arrive in x0, x1, … per AAPCS64.

**Why `#[inline(never)]` is not enough.** The compiler generates a standard
function prologue (`stp x29, x30, [sp, #-N]!`) for every non-naked function,
even when `#[inline(never)]` is set. This adjusts SP *before* inline asm
runs. A context-switch routine that reads SP after the prologue saves the
wrong value; on restore the caller's stack frame is misaligned by N bytes and
its epilogue reads callee-saved registers from incorrect addresses.

```rust
// CORRECT — no prologue/epilogue; sp is exactly the caller's sp.
#[unsafe(naked)]
unsafe extern "C" fn context_switch_asm(
    current: *mut TaskContext,
    next: *const TaskContext,
) {
    naked_asm!(
        "mov x8, sp",
        "str x8, [x0, #96]",
        // … save/restore …
        "ret",
    );
}

// WRONG — compiler adds stp x29,x30,[sp,#-16]! before the asm;
// saved sp is 16 bytes too low.
#[inline(never)]
unsafe fn context_switch_asm_broken(…) {
    unsafe { asm!("mov x8, sp", "str x8, [x0, #96]", …, options(nostack)); }
}
```

This rule is documented in `docs/standards/bsp-boot-checklist.md` §6 with
the diagnostic procedure.

### 6. Where `unsafe` is not permitted

- **Ergonomic shortcuts.** Bypassing a borrow check because it is inconvenient.
- **Micro-optimizations that are not measured.** Unsafe performance tweaks require a measurement, a comment citing it, and a commit to keep the benchmark.
- **Skipping bounds checks without a documented invariant** that proves the index is in range.
- **Duplicating a capability token** (capability duplication is a first-class operation; avoid going through raw bits).
- **Making a type `Send` / `Sync` to paper over a concurrency bug.** Fix the bug.

### 7. Scope of each `unsafe` block

Keep `unsafe` blocks as small as possible. Never wrap an entire function body when only a few lines need to be unsafe. This is enforced by `#![deny(unsafe_op_in_unsafe_fn)]`, which forbids the older pattern where calling `unsafe fn` inside another `unsafe fn` was implicit.

## Review

A change that introduces, modifies, or broadens an `unsafe` region requires:

1. A `// SAFETY:` comment meeting §1.
2. An entry (or update) in the audit log.
3. Explicit reviewer approval on the `unsafe` change specifically. The reviewer notes: *"Reviewed `unsafe` in <file>:<lines>; agree with SAFETY reasoning."*
4. For security-sensitive subsystems (capabilities, IPC, memory, boot, crypto), a **second reviewer** with security context must also approve. See [security-review.md](security-review.md).

A change that removes `unsafe` only needs routine review.

## Tooling

- `cargo-geiger` — counts `unsafe` occurrences per crate. Run periodically; reconcile with the audit log.
- `miri` — interpreter that catches some undefined behavior in tests. Run on the subset of tests that do not depend on real hardware.
- `clippy::undocumented_unsafe_blocks` — CI gate. Ensures every `unsafe` block has a nearby `SAFETY:` comment.
- `clippy::missing_safety_doc` — CI gate. Ensures every `unsafe fn` has a `# Safety` section.

## Enforcement

- Clippy and rustfmt lints are non-negotiable in CI.
- Undocumented `unsafe` is treated as a bug, not a style issue.
- Quarterly `unsafe` review: the maintainer walks through the audit log, re-reads each `SAFETY:` comment, confirms invariants still hold. Stale entries get re-justified or removed.

## Anti-patterns to reject

- `unsafe { /* no comment */ }` — not reviewable.
- `// SAFETY: trust me.` — not reviewable.
- `// SAFETY: this is faster.` — performance alone is not a justification.
- `unsafe fn foo() { unsafe { ... } unsafe { ... } }` — combined scopes hide where the risk actually lives.
- `unsafe impl Sync for Foo {}` on a type whose invariants could simply be made private — privacy is the better fix.

## References

- The Rustonomicon: https://doc.rust-lang.org/nomicon/
- "Unsafe Rust: How and When (Not) to Use It" — various community posts.
- `clippy::undocumented_unsafe_blocks`: https://rust-lang.github.io/rust-clippy/master/#undocumented_unsafe_blocks
- Hubris `unsafe` discipline (prior art): https://hubris.oxide.computer/
