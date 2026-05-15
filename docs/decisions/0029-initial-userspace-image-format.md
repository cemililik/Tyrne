# 0029 — Initial userspace image format

- **Status:** Accepted
- **Date:** 2026-05-14
- **Deciders:** @cemililik

## Context

[Phase B § B4 — Task loader](../roadmap/phases/phase-b.md#milestone-b4--task-loader) opens once the [AddressSpace kernel object](0028-address-space-data-structure.md) lands (B3, 2026-05-14). B4 must define **how a userspace binary is represented inside the kernel image** before the loader (T-019) can read it: the byte-level layout the loader walks, the entry-point convention it follows, and how the loader knows where the code/data segments live in the embedded blob.

For v1 the binary is statically embedded into the kernel via `include_bytes!` (per phase-b plan §B4 — "the binary is statically embedded in the kernel image; the filesystem / dynamic loading comes later"). No filesystem, no dynamic linking, no module loader. The format decision settles **what the bytes inside the `include_bytes!` literal mean** and how the loader interprets them.

The stakes of getting this wrong are bounded but real. A maximalist choice (ELF subset) front-loads parser complexity into v1's loader before any userspace exists; a minimalist choice (raw flat) ships v1 quickly but locks B5+'s richer needs (per-section permissions, symbol info, debug data, runtime relocation) into a *second* format the loader will also have to support. The pattern matches the [ADR-0027 §"Bounded bootstrap frame budget"](0027-kernel-virtual-memory-layout.md) trade-off from B2 — keep v1 small, defer richness until a concrete need surfaces.

## Decision drivers

- **Loader simplicity.** The B4 loader runs in kernel mode at boot, with no allocator beyond [`Pmm`](../../kernel/src/mm/pmm.rs) and no parser primitives. Every parser line is kernel `.text` that must be panic-free per `kernel/src/lib.rs`'s `#![deny(clippy::panic)]` discipline. A simpler format means a smaller, easier-to-audit loader.
- **Boot footprint.** The embedded binary's bytes live in the kernel's `.rodata` section (the only RX section in v1's kernel-image map per ADR-0027). Each byte costs kernel image size on every boot, even for the tiny v1 "hello" binary B6 will produce. Format overhead (ELF headers, section tables, symbol tables) is multiplicative across every future userspace image.
- **Future extensibility.** v1's "hello" binary is ~100 bytes of `mov w0, #42; ret` shape. B5 brings syscalls; B6 brings the first real userspace. Eventually userspace will want symbol info for debugger attach, section permissions for `.rodata` R-only / `.text` RX-only enforcement, and possibly runtime relocation. Each of those needs the format to carry richer metadata; raw flat does not.
- **Toolchain alignment.** Rust's `cargo build` produces ELF binaries by default. Going from ELF → raw flat is a single `objcopy -O binary` step (or a `cargo-binutils` step); going from raw flat → ELF requires a custom toolchain step the project does not have today. The default toolchain output is ELF; v1 should not fight this further than necessary.
- **Test surface.** The format choice affects how host-side loader tests work. A raw flat blob is `&[u8]` the test can construct directly; an ELF blob requires either a real `cargo build` artefact or a synthetic ELF builder. The B3 closure trio §Adjustments item 5 already flagged that `bsp-qemu-virt` has no host-test infrastructure for fake page tables; adding ELF parsing on top would compound the testing-infra debt.
- **Inspectability.** A raw flat blob produces no `objdump -d` symbols, no `addr2line` mapping, no DWARF info. Debugging a crashed userspace task in v1 means single-stepping in QEMU and matching addresses against the linker-script-defined layout. ELF would give symbols immediately. v1's "userspace" is a single 100-byte binary; the inspectability cost is small.

## Considered options

1. **Raw flat binary.** The embedded blob is the literal byte stream of the userspace `.text` (plus `.rodata` and `.data` if any), concatenated with no metadata. The loader treats offset 0 as the entry point and lays the blob out in memory at a fixed VA. A linker script controls the userspace VA / segment boundaries.
2. **Minimal ELF subset.** The embedded blob is a valid ELF64 file with a small subset of headers honoured: `e_entry` (entry-point VA), `e_phoff` + `e_phnum` (program headers for `PT_LOAD` segments), plus the `PT_LOAD` headers themselves (offset, vaddr, filesz, memsz, flags). Other ELF features (sections, dynamic linking, relocations, symbols, notes) ignored.
3. **Custom packed format.** A Tyrne-specific format defined in `docs/architecture/userspace-image.md`: a fixed-size header (magic, version, entry-point offset, segment count) followed by per-segment descriptors and the raw bytes. Designed to be exactly as rich as v1 needs and no richer.

## Decision outcome

Chosen option: **Option 1 — Raw flat binary** for v1.

The minimalist option wins on the four most load-bearing decision drivers (loader simplicity, boot footprint, toolchain alignment, test surface) and pays the future-extensibility / inspectability costs as the deliberate v1 trade-off. The pattern matches how T-016 chose identity-mapping for B2 (ADR-0027) and how T-017 chose a bitmap allocator for B3 (ADR-0035) — pick the smallest shape that satisfies the v1 use case, and defer richness to a successor ADR when a concrete B5+ need surfaces.

Specifically:

- **Layout.** The loader treats the embedded blob as a single `PT_LOAD`-style segment: the bytes at offset 0 are the userspace entry point's first instruction; subsequent bytes are read-only data and (eventually) writable data in linker-script-defined order. The loader does **not** distinguish `.text` / `.rodata` / `.data` in v1.
- **Mapping flags choice** is owned by **T-019**, not this ADR — this ADR settles the **format** (raw flat bytes), not how those bytes get mapped. T-019 §Approach pins the v1 flags as `MappingFlags::USER | MappingFlags::EXECUTE` for the image region and `MappingFlags::USER | MappingFlags::WRITE` for the stack region; the per-section R-only / RX-only / RW-only discipline is the future [ADR-0034 (kernel-image section permissions)][adr-0034-placeholder] placeholder's responsibility, gated on the first attacker-observable execution context (B5+).
- **Entry point.** Always at the start of the embedded blob (offset 0 ↔ VA = `<userspace-base>`). No `e_entry`-style indirection.

[adr-0034-placeholder]: 0027-kernel-virtual-memory-layout.md
- **VA placement.** A fixed userspace base VA is implementation-detail of T-019, not this ADR — the VA range scoping decision is owned by [T-019's Approach](../analysis/tasks/phase-b/T-019-task-loader.md) and bounded by [ADR-0027 §Decision outcome (a)](0027-kernel-virtual-memory-layout.md)'s `TTBR0_EL1` range. The loader maps the blob at a single contiguous VA range determined at compile time per the userspace linker script.
- **Build pipeline (B4 / T-019 — placeholder blob).** T-019 ships with a **hand-coded** placeholder blob: a small `&[u8]` literal (`[0x40, 0x05, 0x80, 0x52, 0xc0, 0x03, 0x5f, 0xd6]` — LE word 0 = `0x52800540` (`MOVZ w0, #42`) + LE word 1 = `0xd65f03c0` (`RET`); together `mov w0, #42; ret`) embedded into the BSP at compile time — sufficient to exercise the loader's `cap_create_address_space` + `cap_map` + `LoadedImage`-return path under host tests + the smoke trace without depending on a userspace toolchain. **No `cargo build`-to-`objcopy` pipeline lands with T-019.**
- **Build pipeline (B6 — real userspace crate).** B6's `userland/hello/` crate (separate, future task — not opened in B4) is the first **real** userspace binary: a `no_std, no_main` aarch64 crate built via `cargo build --target aarch64-unknown-none` (`cargo build` default for the userland workspace member) and stripped to raw bytes via `objcopy -O binary` (or the equivalent `cargo-binutils` invocation) as a userland-crate build-script step. The kernel embeds the resulting `.bin` via `include_bytes!("../../userland/hello/target/.../hello.bin")` (or similar — the exact path lands with B6). Until B6 lands, T-019's placeholder blob is the only userspace image in tree.

### Simulation

**Not applicable** — this ADR settles a single-shape format decision; no state machine to simulate. (T-019's loader implementation **is** multi-step state-machine work and its task user-story will carry the §Simulation table required by [`write-adr` skill §Procedure step 5 sub-bullet "Decision outcome → Simulation row-to-verification mapping"](../../.agents/skills/write-adr/SKILL.md) — but ADR-0029's subject is the format choice, not the loader sequence.)

### Dependency chain

For this decision to be fully in effect:

```text
1. Userspace binary build pipeline — userland/hello/ crate + objcopy -O binary
   build step  — opens with B6 (separate task; B4's T-019 ships with a
   placeholder blob)                                                — Phase B6 (deferred)
2. Loader implementation that consumes the raw flat blob              — T-019 (Draft, opens with this ADR)
3. Address-space scaffold the loader writes mappings into             — ADR-0028 + T-018 (Done 2026-05-14)
4. Physical-frame allocator backing the loader's frame requests       — ADR-0035 + T-017 (Done 2026-05-10)
5. Per-section userspace permissions (RX `.text` / R `.rodata` / RW
   `.data` + NX/PXN bits)                                             — ADR-0034 (slot reserved; opens with
                                                                        the first attacker-observable
                                                                        execution context, likely B5+)
6. Symbol info / debug data for userspace crashes                     — successor ADR if + when this v1
                                                                        decision is revisited (no slot reserved)
```

Step 1 is the natural Phase B6 work and is **not** opened today; B4's T-019 ships with a placeholder embedded blob (a minimal hand-written sequence of bytes the loader can verify it maps correctly). Steps 3 + 4 are already grounded. Step 2 (T-019) opens at `Draft` in the same commit that lands this ADR per [ADR-0025 §Rule 1](0025-adr-governance-amendments.md). Steps 5 + 6 are explicit forward-flags, consistent with v1's "smallest shape that works now" discipline.

## Consequences

### Positive

- **Loader complexity is bounded by the page-loop + intermediate-frame-budget pattern**, not by a parser. The actual loader sequence (per [T-019 §Approach][t-019-approach]) is: (1) preflight cap kind + image size + total PMM-frame budget (1 root AS frame + image pages + stack pages + worst-case intermediate page-table frames per [ADR-0027 §VMSAv8 4-level translation][adr-0027-vmsav8]); (2) `cap_create_address_space` for the new AS; (3) loop `pmm.alloc_frame` + `core::ptr::copy_nonoverlapping` byte-copy + `cap_map` per image page (with tail zeroing on the partial page); (4) loop the same shape per stack page; (5) return `LoadedImage` metadata. The per-page mechanics are real (`Pmm::alloc_frame` is per-frame, not contiguous; `cap_map` is per-page, not range-mapped — see T-019 §Approach for the explicit loop) but the parser surface is *zero*. Compared to Option 2 (ELF subset) which adds ~80–120 lines of header / program-header / segment-table validation that **all** must be panic-free per `kernel/src/lib.rs`'s `#![deny(clippy::panic)]` discipline, Option 1's complexity sits in the *loop* (which can be table-driven, easily Miri-tested) rather than in *parsing* (which is an attacker-controllable input surface in B5+ when filesystem-loaded modules eventually land).
- **Boot footprint stays bounded.** No ELF header overhead (54 bytes header + 56 per program header in ELF64 = ~110 bytes minimum, doubling a 100-byte v1 binary). The kernel `.rodata` cost is exactly `blob.len()`.
- **Host-side loader tests are trivial.** Tests construct a `&'static [u8]` directly (e.g., `static TEST_BLOB: &[u8] = &[0x40, 0x05, 0x80, 0x52, 0xc0, 0x03, 0x5f, 0xd6]` = a real `mov w0, #42; ret` sequence). No fake-ELF builder; no toolchain-dependent test fixtures.
- **Toolchain alignment.** Every aarch64 Rust crate's `cargo build` output is one `objcopy -O binary` away from a working raw flat binary.

[t-019-approach]: ../analysis/tasks/phase-b/T-019-task-loader.md#approach
[adr-0027-vmsav8]: 0027-kernel-virtual-memory-layout.md

### Negative

- **No per-section permissions for v1 — `.data` writes will permission-fault.** Every userspace image page maps with the same flags. T-019 §Approach pins the v1 choice as `MappingFlags::USER | MappingFlags::EXECUTE` (no `WRITE`) for the image region, which means **v1 userspace is effectively restricted to code + read-only data**: a real binary's `.data` section (writable globals) mapped under the image region would fault on the first write. The stack region is separately mapped `USER | WRITE` so userspace can use the stack normally, but heap / mutable globals outside the stack are not reachable in v1. The loader has no information about section boundaries to discriminate; raw flat carries no `.text` / `.rodata` / `.data` metadata. *Mitigation:* v1's hand-coded placeholder (`mov w0, #42; ret`) and B6's planned minimal "hello" binary are code-only with no `.data` section, so the constraint is non-blocking for the v1 demo trajectory. Per-section RX/.text + R/.rodata + RW/.data discipline lands with ADR-0034 (kernel-image section permissions, slot reserved in [ADR-0027 §Decision outcome (a)](0027-kernel-virtual-memory-layout.md)) when B5+ surfaces the first attacker-observable execution context. Today's threat model has no userspace, so the "all pages have the same flags" property is non-exploitable.
- **No symbols, no DWARF.** Crashing a userspace task in v1 produces a register dump and an architectural PC. Mapping that PC back to a Rust source line requires manually matching against the userspace linker script + the userspace crate's `objdump -d` output. *Mitigation:* v1's userspace is ~100 bytes; the cost is small. When symbols become valuable (B5+), the successor ADR that introduces ELF (or a richer custom format) lands the debug-data discipline alongside.
- **Format-second-time tax.** When B5+ eventually needs richer metadata, the loader will support **two** formats during the transition: raw flat (legacy) + the chosen successor. This is a known cost of "ship v1 fast" vs "settle once". *Mitigation:* explicit cut-over in the successor ADR — old format deprecated at the same time the new format lands, with a migration window of one Phase-B milestone.
- **Linker-script awareness leaks into the loader.** The userspace VA layout (where the blob is mapped, where the stack lives, where future heap goes) is encoded in the userspace linker script, **not** in the binary. The loader must hard-code or read-from-build-script the same VAs the linker script uses. *Mitigation:* T-019's Approach pins this with a `userland-layout.rs`-or-similar source-of-truth that both the userspace linker script and the kernel loader read.

### Neutral

- **The choice is reversible per the §Negative "Format-second-time tax" item.** Switching to ELF or a custom format in B5+ requires a new ADR + loader work, but does not invalidate any v1 code that this ADR enables.
- **The format spec is *short*** — five sentences in §Decision outcome above + one diagram in the future `docs/architecture/task-loader.md` chapter (lands with T-019; file does not yet exist). The shortness is itself a feature: spec drift becomes less likely when the spec fits on one page.

## Pros and cons of the options

### Option 1 — Raw flat binary (chosen)

- **Pro:** Smallest loader, smallest binary footprint, simplest tests, aligns with `objcopy` default toolchain step.
- **Pro:** Reversible — successor ADR can introduce a richer format and run both in parallel during a one-milestone transition.
- **Con:** No per-section permissions in v1.
- **Con:** No symbols / no DWARF for userspace debugging.
- **Con:** Loader must hard-code or build-script-derive the same VAs the userspace linker script uses; spec drift potential.

### Option 2 — Minimal ELF subset

- **Pro:** Native Rust toolchain output; no `objcopy` step needed.
- **Pro:** `e_entry` + program headers give the loader the metadata to support per-section permissions, multiple `PT_LOAD` segments, and explicit `vaddr` placement without changing the format spec.
- **Pro:** Standard format → standard tooling (`readelf`, `objdump`, GDB) works on the embedded blob.
- **Con:** Loader complexity multiplies: parser must validate magic, endianness, machine class, segment table integrity, segment-bound checks, before laying out pages. Each validation step is kernel `.text` that must be panic-free.
- **Con:** Boot footprint grows by ~110 bytes minimum (ELF64 header + 1 program header) plus per-segment overhead. Multiplies across every embedded userspace binary.
- **Con:** Host-side loader tests need a real ELF builder or a baked test ELF; the latter creates a toolchain dependency in the test crate.
- **Con:** v1 has no use for `PT_DYNAMIC`, relocations, symbols, sections, or notes — the ignored 95 % of ELF is unused-attack-surface in the parser.

### Option 3 — Custom packed format

- **Pro:** Sized exactly for v1's needs; no unused ELF surface, no missing-feature gap.
- **Pro:** Format spec is one architecture doc the project fully owns; no external standard to diverge from.
- **Con:** New format documented in *one* place is one cross-link drift away from being a project-only mystery to future contributors.
- **Con:** No off-the-shelf tooling (`readelf`-equivalent) for inspection.
- **Con:** Loader complexity sits between Options 1 and 2 — small parser, but the parser exists and must be panic-free.
- **Con:** Test surface still needs a synthetic builder (the format isn't `objcopy`-producible).

## References

- [Phase B §B4 — Task loader](../roadmap/phases/phase-b.md#milestone-b4--task-loader) — milestone scope.
- [ADR-0027 — Kernel virtual memory layout](0027-kernel-virtual-memory-layout.md) — VA range constraints for userspace per `TTBR0_EL1`.
- [ADR-0028 — Address-space data structure](0028-address-space-data-structure.md) — the cap-gated `Mmu::map` surface the loader uses.
- [ADR-0035 — Physical Memory Manager](0035-physical-memory-manager.md) — frame allocation primitive the loader consumes.
- ADR-0034 — Kernel-image section permissions (placeholder; named-but-unallocated forward-flag in [ADR-0027 §Decision outcome (a)](0027-kernel-virtual-memory-layout.md), no ADR-0034 file yet). Successor ADR for per-section permissions; slot reserved.
- [ARM ARM §D5.2 "Translation regimes"](https://developer.arm.com/documentation/ddi0487/latest) — `TTBR0_EL1` VA range bounds.
- [ELF-64 Object File Format (System V ABI)](https://refspecs.linuxfoundation.org/elf/gabi4+/contents.html) — the format Option 2 would adopt a subset of.
