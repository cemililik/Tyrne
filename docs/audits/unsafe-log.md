# `unsafe` audit log

This log tracks every `unsafe` block, `unsafe fn` declaration, `unsafe impl`, and `unsafe trait` introduced into Umbrix. See [unsafe-policy.md](../standards/unsafe-policy.md) for the policy this log implements and [security-review.md](../standards/security-review.md) for the review pass that signs each entry off.

Entries are **append-only**. When an `unsafe` region is removed, its entry gains a `Removed` status with date and commit; the entry itself is not deleted — the historical reasoning stays on record.

## Entries

### UNSAFE-2026-0001 — construct PL011 `Console` from kernel entry

- **Introduced:** 2026-04-20, Phase 4c bring-up commit.
- **Location:** [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs) — `kernel_entry`.
- **Operation:** `Pl011Uart::new(PL011_UART_BASE)` — wraps the MMIO base of the QEMU `virt` PL011 in the BSP's concrete `Console` type.
- **Invariants relied on:**
  - `0x0900_0000` is the QEMU `virt` PL011 MMIO base across all targeted QEMU versions.
  - The kernel is single-core in v1 and no other subsystem owns this MMIO window.
  - The window is mapped and addressable at the moment the constructor runs (identity-mapped by QEMU before kernel entry).
- **Rejected alternatives:** None viable — the kernel must have an early diagnostic console, and constructing the `Pl011Uart` is the only safe-wrapper entry point.
- **Reviewed by:** @cemililik (self-review per solo-phase discipline; see [security-review.md](../standards/security-review.md)).
- **Status:** Active.

### UNSAFE-2026-0002 — construct PL011 `Console` inside the panic handler

- **Introduced:** 2026-04-20, Phase 4c bring-up commit.
- **Location:** [`bsp-qemu-virt/src/main.rs`](../../bsp-qemu-virt/src/main.rs) — `panic` handler.
- **Operation:** `Pl011Uart::new(PL011_UART_BASE)` — reconstructs the UART in the panic path.
- **Invariants relied on:** Same as UNSAFE-2026-0001.
- **Rejected alternatives:** Reusing the original `Console` reference would require smuggling it into the panic handler via a `static` slot, which adds lifetime and initialization complexity. Constructing a fresh `Pl011Uart` is acceptable because `Console` writes are best-effort (ADR-0007): any concurrent writer at panic time may interleave, which is the intended failure mode.
- **Reviewed by:** @cemililik.
- **Status:** Active.

### UNSAFE-2026-0003 — `unsafe impl Send for Pl011Uart`

- **Introduced:** 2026-04-20, Phase 4c bring-up commit.
- **Location:** [`bsp-qemu-virt/src/console.rs`](../../bsp-qemu-virt/src/console.rs).
- **Operation:** Asserts that a `Pl011Uart` value can be transferred between threads.
- **Invariants relied on:** The only state inside `Pl011Uart` is a base address (a `usize`). The PL011 hardware itself is the synchronization domain; its TX FIFO serializes writes.
- **Rejected alternatives:** A wrapping type (e.g. `AtomicUsize`) buys nothing; the base address never changes and a simple `Send` bound is what callers need.
- **Reviewed by:** @cemililik.
- **Status:** Active.

### UNSAFE-2026-0004 — `unsafe impl Sync for Pl011Uart`

- **Introduced:** 2026-04-20, Phase 4c bring-up commit.
- **Location:** [`bsp-qemu-virt/src/console.rs`](../../bsp-qemu-virt/src/console.rs).
- **Operation:** Asserts that `&Pl011Uart` is safe to share across threads.
- **Invariants relied on:** Same as UNSAFE-2026-0003. Concurrent writes from multiple cores may interleave at the byte level, which the [`Console`](../../hal/src/console.rs) contract (see [ADR-0007](../decisions/0007-console-trait.md)) accepts as best-effort behaviour.
- **Rejected alternatives:** Interior-mutable synchronization (a spinlock around writes) would be safer but is overkill for a console whose contract explicitly permits interleaving. If the contract changes, revisit.
- **Reviewed by:** @cemililik.
- **Status:** Active.

### UNSAFE-2026-0005 — MMIO read/write in `Pl011Uart::write_bytes`

- **Introduced:** 2026-04-20, Phase 4c bring-up commit.
- **Location:** [`bsp-qemu-virt/src/console.rs`](../../bsp-qemu-virt/src/console.rs) — `Pl011Uart::write_bytes`.
- **Operation:** `read_volatile((base + UARTFR) as *const u32)` and `write_volatile((base + UARTDR) as *mut u32, byte_as_u32)` to drive the PL011 TX path.
- **Invariants relied on:**
  - `base` is the MMIO base of a PL011 window, as established by `Pl011Uart::new`'s safety contract (see UNSAFE-2026-0001).
  - `UARTFR` (offset `0x18`) and `UARTDR` (offset `0x00`) are 4-byte-aligned and within the window.
  - Volatile accesses prevent the compiler from reordering or eliding the reads and writes.
- **Rejected alternatives:** Using a `volatile_register` crate would wrap these in typed abstractions at some ergonomic cost; the plain-MMIO form is small enough and easy enough to audit here. Revisit if more registers join the picture.
- **Reviewed by:** @cemililik.
- **Status:** Active.
