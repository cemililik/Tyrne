# Track G — BSP & boot path

- **Agent run by:** Claude general-purpose agent, 2026-05-06
- **Scope:** QEMU virt BSP — `boot.s`, `vectors.s`, GIC v2 driver, `Pl011Uart`, EL drop, `linker.ld`, `build.rs`, `Cargo.toml`, and the Rust BSP code (`main.rs`, `cpu.rs`, `console.rs`, `exceptions.rs`, `gic.rs`).
- **HEAD reviewed:** 214052d

## Boot-checklist walkthrough

Every row of [docs/standards/bsp-boot-checklist.md](../../../../docs/standards/bsp-boot-checklist.md) checked against the post-T-013 / post-T-012 BSP at HEAD.

| # | Checklist item | Status | Where it lives |
|---|---|---|---|
| 1 | EL drop to EL1 (CurrentEL dispatch; HCR_EL2 / SPSR_EL2 / ELR_EL2; halt-on-EL3) | **pass** | [boot.s:45–98](../../../../bsp-qemu-virt/src/boot.s#L45) — RW=1, E2H=0, TGE=0; SPSR_EL2 = 0x3c5 (EL1h + DAIF masked); EL3 → `halt_unsupported_el: wfe; b .` |
| 1a | K3-12: `msr daifset, #0xf` as the literal first instruction | **pass** | [boot.s:47](../../../../bsp-qemu-virt/src/boot.s#L47) — first instruction at `_start`; mask propagates across `eret` via SPSR_EL2 |
| 2 | FP/SIMD enable (`CPACR_EL1.FPEN = 0b11` + ISB) | **pass** | [boot.s:114–116](../../../../bsp-qemu-virt/src/boot.s#L114) — runs *after* the EL drop, *before* BSS-zero, with the required ISB |
| 3 | VBAR install before any later boot bug can take an exception | **pass** | [main.rs:554–562](../../../../bsp-qemu-virt/src/main.rs#L554) — first thing `kernel_entry` does after the console+CPU statics are written; `MSR VBAR_EL1` + ISB; happens *before* GIC init and *before* `daifclr` |
| 4 | SP 16-byte aligned at the first `bl` into Rust | **pass** | [linker.ld:47–49](../../../../bsp-qemu-virt/linker.ld#L47) — `. = ALIGN(16); . = . + 64K; __stack_top = .;`. Reserved region starts 16-aligned, stack grows down so SP at `__stack_top` is 16-aligned |
| 5 | BSS zero before Rust entry, 8-byte-aligned bracket | **pass** | [linker.ld:39–45](../../../../bsp-qemu-virt/linker.ld#L39) — `.bss : ALIGN(8)` with `__bss_start` / `__bss_end` and trailing `ALIGN(8)`; [boot.s:118–127](../../../../bsp-qemu-virt/src/boot.s#L118) zeroes by `str xzr, [x0], #8` |
| 6 | Context-switch asm uses `#[unsafe(naked)]` | **pass** | [cpu.rs:347–398](../../../../bsp-qemu-virt/src/cpu.rs#L347) — `#[unsafe(naked)] unsafe extern "C" fn context_switch_asm(...)` with `naked_asm!`; saves x19–x28, fp, lr, sp via x8 scratch, d8–d15; symmetric restore + `ret` |

All six checklist items are satisfied; no item was N/A.

## Findings

### Blocker

- _None._

### Non-blocking

- [exceptions.rs:166–174](../../../../bsp-qemu-virt/src/exceptions.rs#L166) — `irq_entry`'s spurious-IRQ branch issues a `compiler_fence(Ordering::SeqCst)` immediately before `return` with no later code on the path. The fence is structurally redundant: the function's normal `return` already ends the borrow scope and the asm trampoline that follows is the next instruction stream the CPU sees, with no shared-memory synchronisation requirement that the fence buys. The flip side is a tiny readability cost (a reader reasonably asks "why a SeqCst fence here?"). Suggested resolution: drop the fence, or keep it and add a one-line note saying it is documentation, not a correctness load-bearing operation.

- [vectors.s:114–147](../../../../bsp-qemu-virt/src/vectors.s#L114) — the IRQ trampoline saves `x0..x18, x30, ELR_EL1, SPSR_EL1` (176 bytes used) into a 192-byte frame whose final 16 bytes are the `_reserved: [u64; 2]` slot in `TrapFrame`. The build-time `assert!(size_of::<TrapFrame>() == 192)` ([exceptions.rs:77](../../../../bsp-qemu-virt/src/exceptions.rs#L77)) catches a future Rust-side drift, but nothing catches an asm-side drift — e.g. someone changing `sub sp, sp, #192` to `#176` in vectors.s would slide every `stp` offset out of the frame and corrupt SP-adjacent stack memory; the const assertion would still hold and host tests would still pass. Suggested resolution: define `TYRNE_TRAP_FRAME_SIZE` in vectors.s as an `.equ`, source it from a single linker-visible constant, or — cheaper — add a comment cross-reference in the asm pointing at the exceptions.rs assertion so both sides are paired in the maintainer's mental model.

- [exceptions.rs:142–145](../../../../bsp-qemu-virt/src/exceptions.rs#L142) — `pub unsafe extern "C" fn irq_entry(_frame: *mut TrapFrame)` accepts `_frame` and never reads it; the parameter exists only to match the trampoline's `mov x0, sp` calling convention. The unused-leading-underscore convention conveys the intent, but a future reader looking at `vectors.s`'s `mov x0, sp; bl irq_entry` may try to remove the parameter as dead. Suggested resolution: leave the parameter as-is; add a one-line `// Receives the saved-frame pointer in x0 even though v1 ignores it; future arcs will read e.g. ELR/SPSR.` before the function. The current doc-comment hints at this in the "frame pointer is the trampoline's `sp`" sentence but does not explicitly justify the unused parameter.

- [main.rs:716–721](../../../../bsp-qemu-virt/src/main.rs#L716) — the final `start(SCHED.as_mut_ptr(), cpu);` call inside `kernel_entry` is *not* annotated `-> !`-style at the call site (the function itself is `-> !`, so this is fine), but the surrounding `kernel_entry` body does not have a defensive halt after `start`. A bug that caused `start` to return (e.g. a regression in scheduler bring-up) would walk off the end of `extern "C" fn kernel_entry() -> !`, which is undefined behaviour at the boundary. `boot.s`'s post-bl `wfe; b 2b` ([boot.s:133–136](../../../../bsp-qemu-virt/src/boot.s#L133)) catches the analogous case at the asm level, but a mismatch between the asm's expectations and the Rust signature can still surprise a refactor. Suggested resolution: append `loop { core::hint::spin_loop(); }` or an `unreachable!()` after `start(...)`. This is belt-and-braces; the current code is correct under the function's `-> !` contract.

### Observation

- [vectors.s:38–82](../../../../bsp-qemu-virt/src/vectors.s#L38) — vector-table layout matches ARM ARM §D1.10 exactly: 16 entries × 0x80 stride, 2 KiB total. `linker.ld:26–27` ([linker.ld:26](../../../../bsp-qemu-virt/linker.ld#L26)) explicitly aligns `.text.vectors` to 2 KiB before placing the section, satisfying VBAR_EL1's alignment requirement. The single `★` IRQ entry (`+0x280`, Current EL with SP_ELx) is the only one that fires in v1; the other 15 trampoline to `tyrne_unhandled_exception_trampoline` or `tyrne_unhandled_irq_trampoline`, which call `panic_entry`. Layout discipline is correct.

- [main.rs:526–598](../../../../bsp-qemu-virt/src/main.rs#L526) — boot-time hardware-init ordering: console-write banner → VBAR_EL1 install → ISB → GIC `new` + `init` → `daifclr #0x2`. This is the order the BSP-boot-checklist mandates: vector-table install precedes GIC init (so any fault in init() is caught visibly); GIC init precedes the IRQ unmask (so no IRQ source can deliver mid-init); `daifclr` sets only the `I` bit (`#0x2`), leaving `D, A, F` masked. Documentation in the surrounding comment block (lines 528–547) captures the rationale.

- [main.rs:711](../../../../bsp-qemu-virt/src/main.rs#L711) — `console.write_bytes(b"tyrne: starting cooperative scheduler\n")` runs *after* `daifclr`. No GIC source is enabled at this point (the only enable sites are `arm_deadline` and the demo never calls it), so any spurious IRQ that arrives is folded to `None` by `gic.acknowledge()` in `irq_entry` and the trampoline `eret`s back. The PSTATE.I=0 window between `daifclr` and `start()` is therefore inert. Correct.

- [cpu.rs:120–187](../../../../bsp-qemu-virt/src/cpu.rs#L120) — `QemuVirtCpu::new` reads `CurrentEL` via the safe-Rust `tyrne_hal::cpu::current_el()` wrapper (UNSAFE-2026-0018) and asserts EL == 1 *before* the `mrs cntfrq_el0` system-register read. Per ADR-0024, this assertion is now a load-bearing post-condition of the EL drop in `boot.s`; the assertion's panic message names ADR-0012/ADR-0024 explicitly. Composes correctly with `boot.s`'s drop sequence.

- [cpu.rs:484–530](../../../../bsp-qemu-virt/src/cpu.rs#L484) — `arm_deadline` writes `CNTV_CVAL_EL0` first, then `CNTV_CTL_EL0 = 0b01`, then `gic.enable(TIMER_IRQ)`. Order is correct: comparator must be set before the timer is enabled, and the GIC line must be enabled after the timer is armed (otherwise the GIC could deliver a spurious "no deadline pending" IRQ). The `cancel_deadline` body mirrors with `CNTV_CTL_EL0 = 0b10` (ENABLE=0, IMASK=1) followed by `gic.disable(TIMER_IRQ)`. Symmetric and correct.

- [cpu.rs:347–398](../../../../bsp-qemu-virt/src/cpu.rs#L347) — `context_switch_asm`'s save / restore matches AAPCS64 callee-save: x19–x28, x29 (fp), x30 (lr), sp (via x8 scratch, since SP cannot appear as `stp` source), d8–d15. The `#[unsafe(naked)]` + `naked_asm!` shape is the BSP-boot-checklist item 6 form; field offsets in the asm match `Aarch64TaskContext`'s `#[repr(C)]` layout (offsets 0, 80, 88, 96, 104). The total saved size is 168 B; matches `(10+1+1+1)*8 + 8*8`.

- [console.rs:71–78](../../../../bsp-qemu-virt/src/console.rs#L71) — `Pl011Uart::write_bytes` uses plain `+` for `self.base + UARTFR` and `self.base + UARTDR`, not `wrapping_add`. Phase-A code review flagged this as a non-blocker. With `base = 0x0900_0000` and offsets ≤ `0x18`, no overflow is possible; `clippy::arithmetic_side_effects` is denied at the kernel crate level only ([kernel/src/lib.rs:42](../../../../kernel/src/lib.rs#L42)) and the BSP crate doesn't inherit the deny. Status unchanged from Phase A: non-blocker, code-style nit, no functional issue.

- [main.rs:296–384](../../../../bsp-qemu-virt/src/main.rs#L296) (task_b) and [main.rs:390–473](../../../../bsp-qemu-virt/src/main.rs#L390) (task_a) — both demo tasks consistently use the raw-pointer scheduler bridge (`SCHED.as_mut_ptr()`, `EP_ARENA.as_mut_ptr()`, etc.), with `(*CPU.0.get()).assume_init_ref()` for the immutable `&Cpu` receiver. No `&mut` to any kernel static is materialised at the call site of `ipc_send_and_yield` / `ipc_recv_and_yield` / `yield_now`. The post-T-006 ADR-0021 discipline is honoured throughout. UNSAFE-2026-0014 audit-log claim verified.

- [main.rs:73–119](../../../../bsp-qemu-virt/src/main.rs#L73) — `StaticCell<T>` is the single pattern for write-once globals; `as_mut_ptr()` is the documented entry point for the raw-pointer bridge (UNSAFE-2026-0013). Two sites bypass `as_mut_ptr` and reach into `.0.get()` directly: [main.rs:259](../../../../bsp-qemu-virt/src/main.rs#L259) (idle's `(*CPU.0.get()).assume_init_ref()`), [cpu.rs:527](../../../../bsp-qemu-virt/src/cpu.rs#L527) and [cpu.rs:551](../../../../bsp-qemu-virt/src/cpu.rs#L551) (Timer impl's GIC access), and [exceptions.rs:165](../../../../bsp-qemu-virt/src/exceptions.rs#L165) (`irq_entry`'s GIC ref). Each of these wants a `&T` (immutable), not a `*mut T`, so `as_mut_ptr().read()`-style is not the right helper — the direct `.0.get()` deref + `assume_init_ref()` is the correct shape. A future ergonomics improvement could expose `StaticCell::as_ref_unchecked() -> &T` so the abstraction is total, but this is style, not correctness.

- [main.rs:484–489](../../../../bsp-qemu-virt/src/main.rs#L484) — `extern "C" { static tyrne_vectors: u8; }` declares the linker-resolved symbol as a single byte. `core::ptr::addr_of!(tyrne_vectors)` produces a `*const u8`; the cast to `u64` for `MSR VBAR_EL1` is correct. The 2 KiB alignment is enforced linker-side (`linker.ld:26 . = ALIGN(2048); KEEP(*(.text.vectors))`), so the address VBAR_EL1 receives is aligned per ARM ARM §D11.2.

- [exceptions.rs:43–77](../../../../bsp-qemu-virt/src/exceptions.rs#L43) — `TrapFrame` is `#[repr(C)]` with field-by-field 16-byte pairs that mirror the trampoline's `stp` offsets exactly. The const-assert `assert!(core::mem::size_of::<TrapFrame>() == 192)` at line 77 catches a Rust-side drift; combined with the boot-checklist's "build-time guard" intent it discharges the Rust half of the asm/Rust ABI agreement.

- [gic.rs:78,316–362](../../../../bsp-qemu-virt/src/gic.rs#L78) — `GIC_MAX_IRQ = 1020` (architectural max from IHI 0048B §4.3.2); `enable` and `disable` both `assert!(irq.0 < GIC_MAX_IRQ)`. PR #10 review-round-2's range-check requirement is satisfied. The bit offset `1u32 << ((n % 32) as u32)` cast carries a populated `reason` field for the `clippy::cast_possible_truncation` allow.

- [gic.rs:165–230](../../../../bsp-qemu-virt/src/gic.rs#L165) — `init` reads `GICD_TYPER` to learn the IT-line count and iterates only up to that count for ICENABLER / IPRIORITYR / ITARGETSR programming. SGI/PPI banked registers (n < FIRST_SPI = 32) are deliberately skipped per the comment at line 222. CPU-interface PMR + CTLR are written *after* the distributor is enabled. Initialisation order matches IHI 0048B §4.

- [gic.rs:364–376](../../../../bsp-qemu-virt/src/gic.rs#L364) — `acknowledge` reads `GICC_IAR`, masks with `GICC_IAR_INTID_MASK` (0x3FF, the bottom-10-bits INTID per spec), folds `GIC_SPURIOUS_INTID = 1023` to `None`. `end_of_interrupt` writes `GICC_EOIR` once with the IRQ ID. Trait-contract pairing (each successful `acknowledge` → exactly one `end_of_interrupt`) is upheld by every branch of `irq_entry` ([exceptions.rs:169–227](../../../../bsp-qemu-virt/src/exceptions.rs#L169)) — spurious returns early without EOI (correct per spec); timer branch acks + EOIs; "other IRQ" branch EOIs before `panic!`.

- [Cargo.toml:19–20](../../../../bsp-qemu-virt/Cargo.toml#L19) — `[lints] workspace = true` inherits the workspace lint set; no per-crate override silently relaxes a kernel-scoped deny. Crate posture: `#![no_std]`, `#![no_main]`, `#![allow(unreachable_pub, reason = "binary crate; pub items are for the linker")]` ([main.rs:22–26](../../../../bsp-qemu-virt/src/main.rs#L22)) — reason field is populated. `panic=abort` lives in `.cargo/config.toml`'s `[target.aarch64-unknown-none]` block, scoped to the bare-metal target so host tests are not affected.

- [build.rs:1–17](../../../../bsp-qemu-virt/build.rs#L1) — passes `-T<absolute-path>/linker.ld` to the linker, with `rerun-if-changed` for `linker.ld`, `build.rs`, `src/boot.s`, `src/vectors.s`. The absolute-path approach removes dependency on the linker's CWD. Matches ADR-0012's linker-script intent.

## Cross-track notes

- → Track B (HAL): trait contracts honoured by the BSP impls. `Console::write_bytes` ([console.rs:61](../../../../bsp-qemu-virt/src/console.rs#L61)) matches [hal/src/console.rs:38](../../../../hal/src/console.rs#L38). `Cpu` methods (`current_core_id`, `disable_irqs`, `restore_irq_state`, `wait_for_interrupt`, `instruction_barrier`) match [hal/src/cpu.rs:44–76](../../../../hal/src/cpu.rs#L44). `Timer` (`now_ns`, `arm_deadline`, `cancel_deadline`, `resolution_ns`) matches [hal/src/timer.rs:31–55](../../../../hal/src/timer.rs#L31). `IrqController` (`enable`, `disable`, `acknowledge`, `end_of_interrupt`) matches [hal/src/irq_controller.rs:37–59](../../../../hal/src/irq_controller.rs#L37). `ContextSwitch::context_switch` and `init_context` match the trait. No drift.

- → Track C (security): every `unsafe` block in BSP source carries a `// SAFETY:` comment with an audit-entry citation. UNSAFE-2026-0001/0002/0005 (PL011), 0003/0004 (Send/Sync), 0006/0007 (`QemuVirtCpu` markers + inline asm), 0008/0009 (`context_switch_asm` / `init_context`), 0010/0011 (StaticCell + TaskStack Sync), 0013 (`as_mut_ptr`), 0014 (raw-pointer bridge sites), 0015 (timer reads), 0016 (`CurrentEL` self-check, with T-013 Amendment), 0017 (boot.s DAIF + EL drop), 0019 (GIC MMIO), 0020 (vector table install + trampolines), 0021 (CNTV_CVAL/CTL writes) — all present and traced. No `unsafe` block in BSP source lacks an audit citation.

- → Track F (tests): no host-side tests live under `bsp-qemu-virt/` (the crate is `#![no_std, no_main]` and bare-metal-only). Smoke verification is the QEMU two-task demo run; the Pending QEMU smoke verification status notes on UNSAFE-2026-0019/0020/0021 are still active per the audit-log entries — flagging here for Track F's smoke-as-regression analysis.

- → Track J (hygiene): `\u{2014}` em-dashes appear in `task_a` / `task_b` / `idle_entry` writeln! strings ([main.rs:302, 338, 448](../../../../bsp-qemu-virt/src/main.rs#L302)) but plain `--` elsewhere. Phase-A review noted this was intentional given the QEMU-trace expectations; no change to flag.

## Sub-verdict

**Approve**

Boot ordering, vector-table layout, GIC v2 driver, EL drop, linker script, and crate posture all check out. The `wrapping_add` Phase-A non-blocker remains a code-style nit; nothing newer rises above non-blocking. Three cross-track notes routed to Tracks B, C, F.
