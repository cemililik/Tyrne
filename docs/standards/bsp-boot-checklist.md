# BSP boot checklist

Every new BSP target must satisfy this checklist before any Rust code runs.
Learned from A5 debugging: three silent hangs, each caused by a missing boot
step that was invisible without the previous one being fixed first.

Work through the items in order — each failure mode masks the next.

---

## 1. Exception level

**Question:** At what EL does QEMU (or hardware) drop us?

- QEMU `virt` (default) → EL1.
- QEMU `virt -machine virtualization=on` → EL2.
- RPi4 → EL2 by default; must drop to EL1 before enabling kernel features.
- TF-A / U-Boot stacks → typically EL2.

**Action:** Always drop to EL1 in the BSP's reset vector per
[ADR-0024](../decisions/0024-el-drop-policy.md), regardless of where the
firmware delivers the kernel. The BSP must:

1. Read `CurrentEL`; mask bits[3:2] to get the EL field.
2. If EL2: configure `HCR_EL2` (`RW=1`, `E2H=0`, `TGE=0` — non-VHE EL1),
   `SPSR_EL2` (mode = EL1h, DAIF masked), `ELR_EL2` (post-`eret` label),
   then `eret`.
3. If EL1: skip the drop (no-op).
4. If EL3 (or any unexpected value): halt loudly. v1 has no EL3-aware
   infrastructure; ADR-0024 §Open questions tracks the future
   EL3→EL2→EL1 chain.

T-009's `UNSAFE-2026-0016` runtime check inside `QemuVirtCpu::new`
asserts `CurrentEL == 1` after the drop has run — a load-bearing
post-condition rather than a defensive guard.

**Reference implementation:** [`bsp-qemu-virt/src/boot.s`](../../bsp-qemu-virt/src/boot.s)
+ audit entry [`UNSAFE-2026-0017`](../audits/unsafe-log.md).

**What goes wrong if skipped:** System-register writes target the wrong EL;
writes silently have no effect, or trap. The kernel runs `MRS DAIF`,
`MRS MPIDR_EL1`, `MRS CNTVCT_EL0`, and (eventually) `VBAR_EL1` /
`TTBR0_EL1` accesses that all assume EL1 — at EL2 they are different
registers with different layouts and the kernel silently misbehaves.

---

## 1a. K3-12: mask DAIF at the very head of `_start`

**Question:** Does `_start` mask DAIF before any other instruction?

The reset vector runs with whatever PSTATE the firmware/emulator delivered.
If interrupts are not masked, a spurious IRQ or asynchronous abort during
the brief window between reset and the first vector-table install (later,
in T-012) jumps into an uninstalled vector and hangs.

**Action:** First instruction at `_start` must be `msr daifset, #0xf` —
sets D, A, I, F mask bits atomically. The mask carries across the EL drop
because `SPSR_EL2.DAIF` propagates to PSTATE on `eret`.

```asm
_start:
    msr     daifset, #0xf       /* (1) mask everything before anything else */
    /* ... EL drop, stack setup, BSS zero, bl kernel_entry, ... */
```

Tasks unmask interrupts explicitly via `Cpu::restore_irq_state(IrqState(0))`
when they need them. No code path in `_start` or the early kernel
unmasks DAIF.

**What goes wrong if skipped:** A spurious interrupt during BSS zeroing
or stack setup jumps into address 0x0 / 0x200 (the reset value of
VBAR_EL1), fetches zeros there, and hangs silently with no output. The
failure mode is identical to the FP/SIMD-trap-without-VBAR case below
(item 2 / item 3) but harder to diagnose because the trigger is
asynchronous.

---

## 2. FP/SIMD enable

**Question:** Are FP/SIMD instructions enabled at the current EL?

On aarch64 the reset value of `CPACR_EL1` is `0`, which traps every
FP/SIMD instruction at EL1 as an Undefined Instruction exception. The Rust
compiler routinely emits NEON instructions for zero-initialisation
(`movi v0.2d, #0`), struct copy, and even some integer operations at
higher optimization levels.

**Action:** Set `CPACR_EL1.FPEN = 0b11` (bits[21:20]) before zeroing BSS
and before any Rust code runs. Follow with `ISB`.

```asm
mov     x0, #0x300000   // FPEN = 0b11
msr     cpacr_el1, x0
isb
```

**What goes wrong if skipped:** The first NEON instruction raises an
Undefined Instruction exception. Without a configured VBAR the exception
vector is address 0x200; fetching zeros there causes an infinite hang with
no output and no error.

**Diagnostic:** `qemu-system-aarch64 -d int -D /tmp/q.log`; look for
`Taking exception 1 [Undefined Instruction]` with `ESR 0x1fe00000`
(ISS = 0, IL = 1, EC = 0x07 = FP/SIMD trap at EL1).

---

## 3. Exception vector (VBAR)

**Question:** Is a vector base address configured?

Until `VBAR_EL1` is set, any exception (undefined instruction, alignment,
prefetch abort) jumps to the reset value of VBAR (typically 0x0 or 0x200
on QEMU `virt`), fetches whatever is there, and hangs silently.

**Action:** Install a minimal exception vector **or** ensure that no
exception can occur in the boot path before Rust sets up a real handler.

For A-phase kernel (no MMU, no interrupts): document that no exception is
expected in boot; rely on CPACR_EL1 and SP alignment to prevent them.
For B-phase and beyond: configure VBAR before enabling interrupts.

**What goes wrong if skipped:** Any unexpected exception causes a silent hang.
The CPACR_EL1 bug (item 2) is the example: it was silent because VBAR was 0.

---

## 4. Stack pointer alignment

**Question:** Is SP 16-byte aligned before the first `bl` into Rust?

AAPCS64 requires 16-byte SP alignment at every function-call boundary.
The linker symbol `__stack_top` must itself be 16-byte aligned; verify in
`linker.ld`.

**Action:** Check that `__stack_top` alignment in the linker script is
`ALIGN(16)` or equivalent, and that the boot assembly sets `sp` from that
symbol before calling Rust.

**What goes wrong if skipped:** Misaligned SP causes a stack-alignment
exception on the first `stp` or `ldp` with SP-relative addressing
(SP alignment check is optional in EL1 but enabled by some CPU configs).

---

## 5. BSS zeroed before Rust entry

**Question:** Is BSS zero-initialised before `kernel_entry` is called?

Rust assumes `.bss` is zero. Any `static` or `static mut` with a zero
initializer lives in BSS. If BSS is not zeroed, those statics have garbage
values.

**Action:** Zero `[__bss_start, __bss_end)` in the assembly stub, after SP
and CPACR_EL1 setup but before the first `bl` into Rust. Use 8-byte stores
(`str xzr, [x0], #8`) and confirm both symbols are 8-byte aligned in the
linker script.

---

## 6. Context-switch assembly uses `#[unsafe(naked)]`

**Question:** Does any function manipulate SP directly in inline asm?

The compiler generates a standard function prologue
(`stp x29, x30, [sp, #-N]!`) for every non-naked function, adjusting SP
before inline asm runs. A context-switch routine that reads SP after the
prologue saves the wrong value; on restore the caller's stack frame is
misaligned and its epilogue reads saved registers from incorrect addresses.

**Action:** Any function whose asm body saves or restores SP (or whose
correctness depends on SP having the caller's exact value) **must** be
`#[unsafe(naked)]`. Use `naked_asm!` as the sole function body.

```rust
#[unsafe(naked)]
unsafe extern "C" fn context_switch_asm(
    current: *mut TaskContext,
    next: *const TaskContext,
) {
    naked_asm!(
        "mov x8, sp",
        "str x8, [x0, #96]",   // save caller's exact sp
        // ...
        "ret",
    );
}
```

`#[inline(never)]` alone is insufficient: the compiler still emits a prologue
for a regular function, even when it is not inlined.

**What goes wrong if skipped:** Saved SP is 16 bytes (or N bytes) too low.
After a context restore the caller's epilogue reads callee-saved registers
from the wrong stack offsets, then `ret`s to a garbage address. Output stops
after the first yield.

**Diagnostic:** Disassemble the function and check for
`stp x29, x30, [sp, #-N]!` before your asm. If present, add `#[unsafe(naked)]`.

---

## Diagnostic cheat sheet

| Symptom | First thing to check |
|---------|---------------------|
| Hangs before any output | CPACR_EL1.FPEN; run QEMU with `-d int` |
| Exception with ESR EC=0x07 | FP/SIMD trap — item 2 |
| Output stops after first yield | SP corruption in context switch — item 6; disassemble the switch function |
| Garbage `ret` address | Stack misalignment or wrong saved lr — item 6 |
| Static has non-zero garbage value | BSS not zeroed — item 5 |
| Exception jumps to 0x200 | VBAR not set — item 3 |

### Enabling QEMU exception logging

Add `--debug` to `tools/run-qemu.sh` or pass flags directly:

```sh
qemu-system-aarch64 ... -d int -D /tmp/qemu_int.log
```

Then `grep "Taking exception" /tmp/qemu_int.log` to see what fired.
