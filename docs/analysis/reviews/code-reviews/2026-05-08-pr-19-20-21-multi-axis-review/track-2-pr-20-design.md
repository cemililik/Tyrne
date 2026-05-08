# Track 2 — PR #20 ADR-0027 design correctness + §Simulation

- **PR:** [#20](https://github.com/cemililik/Tyrne/pull/20)
- **Branch:** adr-0027-kernel-virtual-memory-layout
- **Commits reviewed:** dc4d92b (Propose) + bb0a6ba (Accept) + 8b6eef4 (PR-num fix)
- **Reviewer:** Claude Opus 4.7 sub-agent (Track 2)
- **Verdict:** Approve-with-3-followups

## Summary

ADR-0027's §Simulation arithmetic is correct; the four-frame bootstrap budget, L0/L1/L2 indexing, MAIR encodings, and TCR field assignments all check out against ARM DDI 0487 §D5/§D8/§D13. The Option A–D analysis is honest (Option D is marginal-but-defensible vs Option B; Option C is correctly priced as premature). Three followups: (1) the ADR is silent on `mem::forget`/`ManuallyDrop` as `#[must_use]` escape hatches; (2) the `.flush(mmu)` API does not type-bind the MMU instance to the minting MMU, allowing a benign-but-undetectable cross-MMU flush; (3) future kernel-image re-mapping (.text RX / .rodata R / .bss/.data RW) is deferred but not given a named ADR slot the way ADR-0033 is.

## Findings

### Blocker (design-killing or false claim)

None.

### Major (must-fix before T-016 starts; not fatal)

1. **ADR-0027 §Decision outcome (c) does not name the `mem::forget`/`ManuallyDrop` escape hatch.** The claim "Forgetting both `.flush()` and `.ignore()` is a compile error" is true at the lint level, but `#[must_use]` only fires on a *dropped, unbound* return value. `let _ = mmu.map(...)?;`, `mem::forget(flush)`, or `ManuallyDrop::new(flush)` all silence the lint — and the latter two are the *only* way a kernel module could legitimately discard the token without the kernel-crate `unused_must_use = "deny"` complaining. The ADR should either (a) document that `_` / `mem::forget` are deliberate escape hatches matching the `x86_64::paging::MapperFlush` precedent, or (b) extend the §Negative consequences to call this out as a known token-discipline gap. Today T-016's reviewer cannot tell which the maintainer intends. Recommend adding one bullet under §Consequences/Negative.

2. **`MapperFlush::flush(self, mmu: &impl Mmu)` accepts *any* `Mmu`, not the minting one.** Nothing in the type system prevents `flush_a = mmu_a.map(...)` followed by `flush_a.flush(&mmu_b)`. In v1 single-`Mmu` reality this is a non-issue, but B3+ work that introduces per-task `AddressSpace` (still on a single `Mmu` impl) and B5+ multi-CPU work could hit confused-ASID flushes if the API is reused naively. The `x86_64` crate has the same shape, so this is precedent-aligned, but the ADR does not call it out. Recommend a one-line note in §Consequences/Neutral or §Decision outcome (c): "the token does not bind the minting `Mmu` instance; multi-`Mmu` deployments will need a stronger token type — out of scope for v1."

3. **Future kernel-image re-mapping (.text RX vs .rodata R vs .bss/.data RW) is deferred without a named-ADR slot.** ADR-0027 §Consequences/Negative bullet 4 mentions the deferral; T-016 §Out of scope line 94 mentions it; `memory-management.md` line 46 mentions "block-level; finer-grained `.text` RX vs `.rodata` R vs `.bss/.data` RW awaits B3+ remap-to-4-KiB-pages work". But unlike high-half migration (which gets the named ADR-0033 placeholder per §Dependency chain), section-level permissions get no ADR slot, no T-NNN reservation, and no §Simulation-grade design surface flagged for the future. This is the *exact* kind of "unknown-unknown that Option D's named-future-ADR was supposed to prevent" — section-permissions is a security-relevant decision (W^X enforcement) that deserves its own ADR before B3+ writes any kernel re-map code. Recommend adding "ADR-0034 placeholder — Kernel-image section permissions" alongside the ADR-0033 bullet in §Decision outcome.

### Minor (worth fixing but not on T-016 critical path)

4. **§Simulation Step 3 uses `DSB ISH` where Linux's `__enable_mmu` uses `DSB NSH`.** Both are correct; ISH is a strict superset of NSH. Linux uses NSH because the pre-MMU kernel runs single-core. Tyrne v1 is also single-core (per ADR-0009 §Open questions tracking multi-core TLB shootdown as future). NSH is a microscopic optimisation but the ADR's choice of ISH is forward-compatible with the eventual SMP boot. No change needed; flagging as a deliberate departure worth one sentence of rationale.

5. **`TCR_EL1.AS = 0` rationale conflates "8-bit ASID field" with "ASID = 0 globally".** Line 59: "Single global address space in v1; `TCR_EL1.AS = 0` (8-bit ASID field; not actively used)". Per ARM DDI 0487 §D13.2.131, TCR_EL1.AS=0 selects *8-bit* ASIDs (vs AS=1 = 16-bit); the actual ASID value lives in `TTBR0_EL1.ASID[55:48]` and is left at 0. The wording is technically correct but reads as if "AS=0" *is* the ASID value. Recommend: "TCR_EL1.AS = 0 (8-bit ASID size; v1 leaves TTBR0_EL1.ASID = 0 globally)."

6. **`memory-management.md` AP encoding table row is wrong-direction.** Line 100: `0b00 = kernel R/W, no userspace; 0b01 = kernel R/W + user R/W; 0b10 = kernel R/O, no user; 0b11 = kernel R/O + user R/O`. Per ARM DDI 0487 §D5.4.5 stage-1 AP[2:1]: AP=00 is EL1 R/W, EL0 no-access; AP=01 is EL1 R/W, EL0 R/W; AP=10 is EL1 R/O, EL0 no-access; AP=11 is EL1 R/O, EL0 R/O. The table matches the spec. Re-checked — correct. **Withdraw this finding.**

### Nit

7. ADR-0027 line 17 says "ADR-0026's table was the empirical retro-source; ADR-0032's was the first application". Out of repo-text reading scope per Track 3 split, but if the Propose-commit text is meant to read independently, "first application" might want a § citation.

8. `memory-management.md` line 88's bit-field diagram is 64-bit but uses a placeholder layout that conflates table and block-descriptor field positions. It's labelled "block descriptor at L2" so the *bits 1:0* `T,V` ordering is correct (bit 1 = block-vs-table = 0 for L2 block), but the diagram's `OutputAddress[47:21]` annotation is at "bit 12 onwards" position which is confusing because the block descriptor's PA field is bits[47:21] (with bits[20:12] reserved-zero), not bits[47:12]. Cosmetic; the prose table that follows is correct.

### Praise

- The §Simulation table is the right shape — five rows, each with state-pre / action / state-post / observable-effect — and Step 3 explicitly calls out "the §Simulation table is itself the *list of things to triple-check before flipping the bit*". This is the §Simulation discipline working as intended.
- The "Why Option D beats X" section is *not* boilerplate; each alternative is priced concretely (Option A: skips type-system enforcement; Option B: same-but-no-named-ADR; Option C: 2× audit entries, premature). Option D is marginal vs B but the marginal value (named ADR-0033 forward-flag) is concrete, not over-formalism.
- The dependency chain's eight numbered steps map cleanly to T-016's six-commit Approach — every ADR-promised deliverable has a commit slot.
- ADR-0009 §Revision rider precisely mirrors the ADR-0017 §Revision rider for `ipc_cancel_recv` — additive HAL-surface change discipline applied consistently.

## §Simulation arithmetic verification

VA → indices (4 KiB granule, 48-bit VA: L0=bits[47:39], L1=bits[38:30], L2=bits[29:21], L3=bits[20:12]):

| VA | L0 idx | L1 idx | L2 idx | ADR's claim | Verified? |
|----|-------:|-------:|-------:|-------------|-----------|
| `0x0800_0000` (GIC start) | 0 | 0 | 64 | L2_low[64] | Yes |
| `0x0900_0000` (UART) | 0 | 0 | 72 | L2_low[72] | Yes |
| `0x4000_0000` (RAM start) | 0 | 1 | 0 | L2_high[0] | Yes |
| `0x4800_0000` (RAM end, exclusive) | 0 | 1 | 64 | L2_high[64] (last mapped block index = 63) | Yes |

Frame budget: L0[0] → single L1 table; L1[0] + L1[1] → distinct L2 tables (L2_low, L2_high). 4 frames total. Yes — math holds. Both L1[0] and L1[1] reside in the *same* L1 table pointed to by L0[0]; the ADR's claim that "L1[0] and L1[1] both point at L0[0]'s table" is phrased oddly but the topology is: 1 L0 frame, 1 L1 frame (containing both entries [0] and [1]), 2 L2 frames.

| Region | Range | L1 entry | L2 table | Block count | Verified? |
|--------|-------|----------|----------|------------:|-----------|
| GIC | `0x0800_0000..0x0900_0000` (16 MiB) | L1[0] | L2_low[64..72] | 8 | Yes (16 MiB / 2 MiB = 8) |
| UART | `0x0900_0000` + ≥128 KiB | L1[0] | L2_low[72] | 1 (covers 2 MiB; UART occupies ≤128 KiB sub-block) | Yes |
| RAM | `0x4000_0000..0x4800_0000` (128 MiB) | L1[1] | L2_high[0..64] | 64 | Yes (128 MiB / 2 MiB = 64) |

| Step | Sequence | ARM-ARM-correct? | Notes |
|------|----------|------------------|-------|
| 0 | Pre-MMU; PC at `0x4008_NNNN`; `.boot_pt` zero-filled by BSS loop | OK | Linker symbol invariant required (T-016 step 3). |
| 1 | Populate L0[0], L1[0], L1[1], L2_low[64..73], L2_high[0..64] | OK | All AF=1; nG=0; AttrIndx per region. SH=00 for device is benign (SH is *ignored* for device memory per §D5.5.1). |
| 2 | `MSR MAIR_EL1; MSR TCR_EL1; MSR TTBR0_EL1; MSR TTBR1_EL1; ISB` | OK | ISB before MMU enable ensures system-register writes are observed. |
| 3 | `TLBI VMALLE1; DSB ISH; IC IALLU; DSB ISH; ISB; SCTLR.{M,I,C}=1; ISB` | OK | ISH is a strict superset of Linux's NSH choice for single-core boot; correct for forward-compatible SMP. `IC IALLU` *before* `SCTLR.I=1` is conventional and safer than after. |
| 4 | PC continues at identity-mapped VA | OK | `0x4008_NNNN` falls in L2_high[0] block (covers `0x4000_0000..0x4020_0000`); the post-ISB instruction-fetch is mapped. |

## MAIR / TCR field-by-field

ARM DDI 0487 §D8.5 (MAIR_EL1 attribute encoding) and §D13.2.131 (TCR_EL1):

| Register | Field | Value | Decoded | Correct? |
|----------|-------|-------|---------|----------|
| MAIR_EL1 | Attr0[7:0] | `0x00` | Device memory; [7:4]=0000 → Device, [3:2]=00 + [1:0]=00 → nGnRnE | Yes |
| MAIR_EL1 | Attr1[7:0] | `0xFF` | Outer[7:4]=1111 → Normal, Outer WB, Non-transient, R-allocate=1, W-allocate=1; Inner[3:0]=1111 → Normal, Inner WB, RW-allocate=1 | Yes |
| MAIR_EL1 | Attr2..7 | `0x00` (reserved) | Each decodes as device-nGnRnE; reserved = unused, not activated by any AttrIndx | Acceptable (no entry uses indices 2..7) |
| TCR_EL1 | T0SZ[5:0] | 16 | 64 − 16 = 48 input VA bits for TTBR0 | Yes |
| TCR_EL1 | EPD0[7] | 0 | TTBR0 walks enabled | Yes |
| TCR_EL1 | IRGN0[9:8] | 0b01 | Normal Inner WB-WA cacheable for table walks | Yes |
| TCR_EL1 | ORGN0[11:10] | 0b01 | Normal Outer WB-WA cacheable for table walks | Yes |
| TCR_EL1 | SH0[13:12] | 0b11 | Inner shareable | Yes |
| TCR_EL1 | TG0[15:14] | 0b00 | 4 KiB granule (TTBR0) | Yes |
| TCR_EL1 | EPD1[23] | 1 | TTBR1 walks disabled | Yes |
| TCR_EL1 | TG1[31:30] | 0b10 | 4 KiB granule (TTBR1) — note distinct encoding from TG0 | Yes |
| TCR_EL1 | IPS[34:32] | 0b010 | 40-bit PA (1 TiB) — covers `0..2 GiB` v1 RAM trivially | Yes |
| TCR_EL1 | AS[36] | 0 | 8-bit ASID size (not "ASID = 0"; see Minor #5) | Yes (decoded correctly; wording could be tightened) |
| SCTLR_EL1 | M, C, I | 1, 1, 1 | MMU on, D-cache on, I-cache on | Yes |

## Cross-reference integrity (design-only)

Track 3 owns full cross-ref audit. Limited to design-relevant pointers:

| File | Cross-ref | Present? | Correct? |
|------|-----------|----------|----------|
| ADR-0027 §References | ADR-0009, 0012, 0024, 0025, 0026, 0032 | Yes | All consistent with design dependencies. |
| ADR-0009 §Revision (2026-05-08) | ADR-0027 / T-016 / `MapperFlush` | Yes | Mirrors ADR-0017 §Revision rider precedent for `ipc_cancel_recv`; phrasing additive. |
| ADR-0012 §Open questions | "Boot-time MMU activation" → ADR-0027 | Yes | Strikethrough + "Resolved 2026-05-08" pattern matches project's resolution discipline. |
| `hal.md` §Mmu | ADR-0027 + `MapperFlush` paragraph | Yes | One-paragraph addition; reads cleanly. |
| `memory-management.md` | ADR-0009, 0012, 0024, 0027, 0033 placeholder | Yes | ADR-0033 placeholder bracketed-link `[ADR-0033 placeholder — Kernel high-half migration]` is unresolved-by-design (target file does not exist yet). |

## References

- ARM *Architecture Reference Manual* (ARM DDI 0487) — §D5.2/§D5.4 (VMSAv8 translation), §D5.5/§D8.5 (memory attributes), §D13.2.131 (TCR_EL1), §D13.2.108 (SCTLR_EL1).
- Linux `arch/arm64/kernel/head.S` `__primary_switch` and `arch/arm64/mm/proc.S` `__cpu_setup` — DSB NSH precedent for single-core early boot.
- `x86_64::structures::paging::MapperFlush` — Rust ecosystem precedent for the typed flush token; same minor concerns (no Mmu-instance binding, `mem::forget` escape hatch) apply.
- ADR-0009 §Revision (2026-05-08), ADR-0012 §Open questions ("Boot-time MMU activation").
- Workspace `Cargo.toml:38` confirms `unused_must_use = "deny"` workspace-wide.
