# Localization

How Tyrne handles human languages in the code, in the kernel's runtime output, and in user-facing surfaces. The short version: **the kernel is locale-agnostic, all internal strings are English, and localization is a userspace concern**. This document spells that out and lists the few places where text encoding nevertheless requires an explicit choice.

## Scope

- Encoding choices for text inside the kernel.
- Language choice for kernel output (panic messages, logs, error descriptors).
- Where localization *does* belong and where it does *not*.
- What this standard commits us to avoid.

## Core position

Tyrne is a kernel. Kernels do not translate user interfaces; they transport bytes and manage hardware. Attempting to do I18N/L10N in the kernel would:

- Add code paths whose correctness depends on human-language nuance — outside the kernel's competence.
- Introduce large data tables (locale dictionaries) into privileged memory.
- Tangle capability handling with locale state in ways that make reasoning harder.

Localization is therefore pushed entirely to userspace, where it belongs.

## Rules

### 1. Internal encoding is UTF-8

All internal strings in the kernel and HAL are UTF-8. `&str` and `String` in Rust already give us this; the rule exists to forbid ad-hoc byte-array "strings" that carry implicit encodings.

When kernel code exchanges strings with userspace (e.g., a task name for diagnostics), the wire format specifies UTF-8 and the length-before-content shape; encoding lives in the type, not in the surrounding convention.

### 2. Kernel-produced strings are English

- Panic messages: English.
- Log record `message` fields: English.
- Error variant `Display` impls (when implemented): English.
- Rustdoc `# Safety`, `# Errors`, `# Panics`: English.

This matches [ADR-0005](../decisions/0005-documentation-language-english.md) (English in the repository) and the logging design ([logging-and-observability.md](logging-and-observability.md)), which requires that `message` be a `&'static str` with no interpolation. There is no runtime translation table in the kernel.

### 3. No locale in the kernel

- No `setlocale`-equivalent. No notion of "current locale" in the kernel.
- No date/time formatting in the kernel beyond monotonic timestamps (nanoseconds since boot) expressed as integers.
- No currency, no number-with-grouping-separators, no case folding outside of ASCII.
- No collation. Kernel strings compared are compared byte-for-byte.

### 4. String operations stay ASCII-aware but Unicode-safe

The kernel does not need Unicode normalization or grapheme clustering. Where kernel code looks at text:

- Ordering and equality use Rust's built-in byte-level `==` / `cmp`.
- Case operations are ASCII-only (`make_ascii_lowercase`, `eq_ignore_ascii_case`). Unicode case operations are userspace.
- The kernel does not slice UTF-8 at arbitrary byte indices that might fall inside a multi-byte sequence; it uses `chars()` or `char_indices()` when iteration is needed.

### 5. User-facing localization lives in userspace

- Localization catalogs (translated strings), plural rules, collation, date/time formatting, currency, number formatting — all userspace.
- The localization service (when there is one) is an ordinary userspace task, gated by capability, with no kernel involvement beyond transporting bytes.
- Applications that render translated UI do so entirely in userspace. The shell, if any, is a userspace task.

### 6. Turkish in conversation, English in artifacts — restated

- Committed artifacts: English (see [ADR-0005](../decisions/0005-documentation-language-english.md)).
- Chat with the maintainer: Turkish is natural and expected; it does not appear in committed files.
- A user-facing Tyrne deployment on the maintainer's devices may render Turkish UI through a userspace localization layer. That is a userspace choice and does not affect the kernel.

## Consequences

- Bug reports, logs, and dumps from the kernel are always in English. Operators of Tyrne devices will see English messages. This is consistent with the practice of most kernels (Linux, BSD, Fuchsia, seL4 all produce English-only kernel output) and with the project's international-accessibility posture.
- Adding a language to userspace does not require any kernel change.
- Kernel code review does not need to evaluate translation correctness.

## What this forecloses

- Translated panic messages. Accepted cost.
- Locale-aware error formatting. Accepted cost.
- Embedded localized logos, splash screens, or boot banners in the kernel. Accepted; those belong in a bootloader or a userspace splash task.

## Future work

A future ADR may specify the Tyrne approach to userspace localization (catalog format, pluralization library choice, translation workflow). That ADR will:

- Live in `docs/decisions/`.
- Operate strictly above the syscall boundary.
- Not affect this standard.

## Anti-patterns to reject

- `panic!("işlem başarısız")` — Turkish panic message in the kernel.
- Storing translation tables in kernel `.rodata`.
- Adding a `current_locale` field anywhere in the kernel.
- Using byte-array "strings" with undocumented encoding.
- String slicing at byte indices that could fall inside a multi-byte UTF-8 sequence.

## References

- [ADR-0005 — English as the documentation and code language](../decisions/0005-documentation-language-english.md)
- [logging-and-observability.md](logging-and-observability.md)
- UTF-8: https://datatracker.ietf.org/doc/html/rfc3629
- Unicode TR#31 (identifier and pattern syntax): https://unicode.org/reports/tr31/
- Rust `str` documentation: https://doc.rust-lang.org/std/primitive.str.html
