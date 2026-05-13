---
name: justify-unsafe
description: Introduce or audit an `unsafe` region in Tyrne — writing the `SAFETY:` comment, adding the audit-log entry, and queuing security review.
when-to-use: Whenever a change introduces, modifies, or broadens an `unsafe` block, `unsafe fn`, `unsafe impl`, or `unsafe trait`.
---

# Justify `unsafe`

## Inputs

- The **location** of the `unsafe` use (file and line range, or a description of the change).
- The **concrete reason** `unsafe` is needed — the operation being performed that Rust's type system cannot otherwise express.
- The **invariants** the code relies on for safety.
- The **safer alternative** that was considered, and why it was rejected.

## Procedure

1. **Confirm `unsafe` is actually necessary.** Re-read [unsafe-policy.md — Rules §5 and §6](../../../docs/standards/unsafe-policy.md). Is the reason in the "permitted" list (MMIO, hardware-defined structures, context switching, FFI, intrinsics/asm)? Is it *not* in the "not permitted" list (ergonomic shortcut, unmeasured optimization, skipping bounds checks)? If unclear, prefer the safer alternative.

2. **Write the `SAFETY:` comment** directly adjacent to the `unsafe` region. Three elements are required:
   - **Invariants upheld.** State specifically what memory-safety, aliasing, initialization, lifetime, or concurrency invariants must hold for this block to be sound, and where they come from.
   - **Rejected alternatives.** Which safer pattern did you consider and why does it not work here? Performance alone is not acceptable unless there is a measurement.
   - **Audit reference.** A reference to the audit entry — either an issue ID or the audit-log tag `UNSAFE-YYYY-NNNN`.

   Example shape:

   ```rust
   // SAFETY: <invariants>. <why safer alternative was rejected>.
   // Audit: UNSAFE-YYYY-NNNN.
   unsafe {
       // the operation
   }
   ```

3. **For `unsafe fn`**, add a `# Safety` section to the function's doc-comment that lists the invariants callers must uphold before calling. This is distinct from the `SAFETY:` comment on internal `unsafe` blocks — the doc `# Safety` is the caller-facing contract.

4. **Assign the next audit tag.**
   - Open [`docs/audits/unsafe-log.md`](../../../docs/audits/unsafe-log.md) (create it if this is the first `unsafe` in the project — see *Audit log format* below).
   - Find the highest existing `UNSAFE-YYYY-NNNN` for the current year.
   - Your tag is the next increment. If no entries exist for this year, start at `UNSAFE-YYYY-0001`.

5. **Append the audit-log entry.** Use this format:

   ```markdown
   ### UNSAFE-YYYY-NNNN — one-line description

   - **Introduced:** YYYY-MM-DD in commit `<sha-short>` (may be filled in post-commit).
   - **Location:** `path/to/file.rs:function_name` (line range optional).
   - **Operation:** one-sentence description of what the `unsafe` does.
   - **Invariants relied on:** the bullet list matching the `SAFETY:` comment.
   - **Rejected alternatives:** one or two sentences.
   - **Reviewed by:** @<reviewer>, plus @<security-reviewer> if security-sensitive.
   - **Status:** Active. (`Removed YYYY-MM-DD in <sha>` when the block is deleted.)
   ```

6. **Request security review.** Per [security-review.md](../../../docs/standards/security-review.md), any change that introduces, modifies, or broadens `unsafe` triggers the security-review pass. In solo phase, this is a separate self-review pass by the maintainer using [perform-security-review](../perform-security-review/SKILL.md). In multi-contributor phase, a second reviewer is required.

7. **Commit** with per [commit-style.md](../../../docs/standards/commit-style.md):
   - Subject: a conventional-commits line; scope is the subsystem affected.
   - Body: state the reason for `unsafe`, the invariants, the alternatives considered.
   - Trailers: `Audit: UNSAFE-YYYY-NNNN` and `Security-Review: @<reviewer>` once the review is complete.

## Audit log format

If this is the first `unsafe` audit entry in the project, create `docs/audits/unsafe-log.md` with this header:

```markdown
# `unsafe` audit log

This log tracks every `unsafe` block, `unsafe fn`, `unsafe impl`, and `unsafe trait` introduced into Tyrne. See [unsafe-policy.md](../standards/unsafe-policy.md) for the policy this log implements.

Entries are append-only. When an `unsafe` region is removed, its entry gains a `Removed` status with date and commit; the entry is not deleted.

## Entries

<entries go below, newest at the bottom>
```

## Acceptance criteria

- [ ] `SAFETY:` comment present directly adjacent to every new or modified `unsafe` region.
- [ ] Comment states invariants, rejected alternatives, and audit reference.
- [ ] `unsafe fn` has a `# Safety` section in its doc-comment.
- [ ] Audit log entry appended with the correct `UNSAFE-YYYY-NNNN` tag.
- [ ] Security review requested (and completed for merge).
- [ ] Commit trailer `Audit: UNSAFE-YYYY-NNNN` present.

## Anti-patterns

- `unsafe { /* no comment */ }` — unreviewable.
- `// SAFETY: trust me.` — unreviewable.
- `// SAFETY: this is faster.` — performance alone is not justification.
- Wrapping an entire function body in `unsafe` when only a few lines need it.
- Audit-log entry that only says "unsafe in `foo`" — needs the actual invariants.
- Skipping the security-review pass because the `unsafe` "looks small".

## References

- [unsafe-policy.md](../../../docs/standards/unsafe-policy.md) — the policy this skill implements.
- [perform-security-review](../perform-security-review/SKILL.md) — the review pass required after this skill.
- [code-style.md](../../../docs/standards/code-style.md) — general Rust style.
- [commit-style.md](../../../docs/standards/commit-style.md) — commit format and trailers.
