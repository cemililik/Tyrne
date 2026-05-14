# T-019 — Task loader: embedded raw-flat userspace image → AS-mapped task

- **Phase:** B
- **Milestone:** B4 — Task loader (this task is B4's implementation; ADR-0029 settles the format choice)
- **Status:** Draft
- **Created:** 2026-05-14
- **Author:** @cemililik (+ Claude Sonnet 4.6 agent)
- **Dependencies:** [ADR-0029](../../../decisions/0029-initial-userspace-image-format.md) — must be `Accepted` before code lands. Also depends on [T-017](T-017-physical-memory-manager.md) (Done 2026-05-10 — PMM provides the frames the loader allocates) and [T-018](T-018-address-space-kernel-object.md) (Done 2026-05-11; live 2026-05-14 via PR #28 — provides `cap_create_address_space` + `cap_map` the loader invokes).
- **Informs:** Closes B4 milestone implementation. Unblocks B5 (syscall ABI — the loader produces the first task whose `TaskCap` a future userspace process could hold) and B6 (first userspace "hello" — actually *running* the task produced here). First runtime caller of [UNSAFE-2026-0025](../../../audits/unsafe-log.md)'s per-call `Mmu::map` path post-bootstrap; lifts its `Pending QEMU smoke verification` status note via Amendment if the smoke trace exercises a real mapping.
- **ADRs required:** [ADR-0029](../../../decisions/0029-initial-userspace-image-format.md). Touches no prior ADR's §Revision notes — the [`cap_create_address_space`](../../../../kernel/src/mm/address_space.rs) / [`cap_map`](../../../../kernel/src/mm/address_space.rs) surface from [ADR-0028](../../../decisions/0028-address-space-data-structure.md) is consumed unchanged; the [`FrameProvider`](../../../../hal/src/mmu/mod.rs) trait from [ADR-0009](../../../decisions/0009-mmu-trait.md) is consumed unchanged. No supersession.

---

## User story

As the Tyrne kernel, I want a **task loader** that consumes an embedded raw-flat userspace binary (per [ADR-0029](../../../decisions/0029-initial-userspace-image-format.md)), allocates physical frames via [`Pmm`](../../../../kernel/src/mm/pmm.rs), creates a fresh [`AddressSpace`](../../../../kernel/src/mm/address_space.rs) via [`cap_create_address_space`](../../../../kernel/src/mm/address_space.rs), copies the binary's bytes into the new frames, maps them into the new AS via [`cap_map`](../../../../kernel/src/mm/address_space.rs), sets up a userspace stack, and produces a [`TaskCap`](../../../../kernel/src/obj/task.rs) carrying the entry-point VA + initial SP, so that B5 can teach the kernel to schedule this task at EL0 and B6 can demonstrate the first userspace "hello" trace.

## Context

[Phase B §B4](../../../roadmap/phases/phase-b.md#milestone-b4--task-loader) defines the loader as the first runtime consumer of the AddressSpace + PMM scaffolds that B3 landed. [ADR-0029](../../../decisions/0029-initial-userspace-image-format.md) (lands with this task at Draft, Accepts before code) settles the byte-level format the loader walks — a raw flat binary embedded into the kernel image via `include_bytes!`. The loader's job is to turn that byte stream into a *task-ready* state: a fresh AS with the binary's pages mapped, a stack mapped, and a `TaskCap` whose context register file points at the entry point.

The work touches three subsystems already in tree (PMM, AddressSpace, Task), one new BSP-side glue layer (the embedded blob + the userspace linker layout it implicitly assumes), and zero new ADRs beyond ADR-0029. The complexity sits in **the sequence**: PMM-allocate frames → copy bytes → cap_create_address_space → cap_map (×N pages) → allocate stack frames → cap_map stack → mint TaskCap with the right context. Every step is fallible; the leak-path-closure discipline from T-018's review-round arc (preflight every rejectable check before committing PMM frames) applies here too.

The task user-story below is intentionally Draft-shape: the §Approach is sparse, the §Acceptance criteria are concrete, and the §Out of scope deliberately defers running the task to B6. Implementation detail (linker-script layout, blob-content choice, stack size) lands in the Approach section as the work progresses, per the [start-task](../../../../.agents/skills/start-task/SKILL.md) skill's "Draft is OK with placeholders" convention.

## Acceptance criteria

A checklist of items that must be true for the task to move from `In Review` to `Done`.

- [ ] **ADR-0029 Accepted** before code lands. Same-day Accept after careful re-read is permitted per [ADR-0025 §Revision notes](../../../decisions/0025-adr-governance-amendments.md); Propose commit is separate from the Accept commit per [`write-adr` skill §10](../../../../.agents/skills/write-adr/SKILL.md).
- [ ] **`task_create_from_image(image: &[u8], pmm: &mut Pmm, table: &mut CapabilityTable, parent_as_cap: CapHandle) -> Result<TaskHandle, TaskLoaderError>`** lands in `kernel/src/obj/task.rs` (or a new `kernel/src/obj/task_loader.rs` module — Approach decides). Signature must follow the leak-path-closure discipline: every rejectable preflight (cap lookup, cap kind, image size sanity, frame-availability) runs **before** the first `pmm.alloc_frame()` call, mirroring [T-018's `cap_create_address_space` step ordering](../../../../kernel/src/mm/address_space.rs).
- [ ] **`TaskLoaderError`** enum covers: `ImageTooLarge` (image size + stack > available PMM frames), `InvalidParentCap` (cap lookup fail or wrong kind), `AddressSpaceCreationFailed(AddressSpaceError)`, `MapFailed(AddressSpaceError)`, `ArenaFull` (TaskArena exhausted). All variants are typed errors, no `panic!` on any reachable path.
- [ ] **Loader copies the embedded image into the new AS's mapped pages** verbatim — no transformation, no relocation, no validation beyond size sanity. The first byte of the image is at VA `<userspace-base>` (the userspace linker-script-defined base, hardcoded per the §Approach below); subsequent bytes follow contiguously.
- [ ] **Initial userspace stack** is allocated (size: implementation-detail, see §Approach) and mapped above the image at the standard `TTBR0_EL1` userspace range; the resulting `TaskHandle`'s context register file is initialised with `PC = <userspace-base>` and `SP = <stack-top>`.
- [ ] **The task is created but not scheduled.** Per [phase-b plan §B4 §4](../../../roadmap/phases/phase-b.md#milestone-b4--task-loader): "QEMU-side task creation without yet running the task (that's B6)". The smoke trace shows the loader returning a `TaskHandle` + a new banner line; no userspace execution.
- [ ] **Host tests in `kernel/src/obj/task_loader.rs::tests` (or wherever the loader lands) per the ADR-0029 §Simulation discipline — wait, ADR-0029 has no §Simulation table (single-shape format decision). The loader's *own* §Simulation lives in this task user-story's §Approach §Simulation table below; each row maps to a host test per the [`write-adr` skill §Procedure step 5 sub-bullet](../../../../.agents/skills/write-adr/SKILL.md) (codified in commit `3ec94b0`).** Concretely: every row of T-019's §Approach §Simulation table is pinned by a host test or audit-log entry that this task lands.
- [ ] **Smoke trace gains exactly one new line** (e.g. `tyrne: task-loader ready (hello task at VA 0x..., sp 0x...; image bytes N, stack bytes M)`) inserted in a stable position in the boot sequence. Full demo through `tyrne: all tasks complete` still passes; `-d int,unimp,guest_errors` reports no new event classes beyond the pre-existing PL011-disabled-UART noise.
- [ ] **No new `unsafe` audit entries unless the loader introduces a sanctioned site.** The loader's frame-copy and cap-map calls should compose existing audited primitives. If a new `unsafe` block is unavoidable, follow the [`justify-unsafe`](../../../../.agents/skills/justify-unsafe/SKILL.md) skill.
- [ ] **`cargo fmt --check`, `cargo host-clippy -D warnings`, `cargo kernel-clippy -D warnings`, `cargo host-test`, `cargo kernel-build`** all clean.
- [ ] **Documentation:** a new short chapter [`docs/architecture/task-loader.md`](../../../architecture/) describing the loader sequence + the userspace linker layout this task assumes; cross-link from [`memory-management.md` §"Address-space objects"](../../../architecture/memory-management.md) and from [`boot.md` §Stage 3](../../../architecture/boot.md).

## Out of scope

- **Running the userspace task.** B6's job. T-019 produces a `TaskHandle` whose context is ready; scheduling it at EL0 + handling the first userspace fault sit behind B5's syscall ABI work.
- **Per-section permissions.** Every mapped page in v1 gets the same flags (RW + NX initially per ADR-0029 §Decision outcome). Per-section RX/.text + R/.rodata + RW/.data discipline is ADR-0034's job (slot reserved; B5+ trigger).
- **Symbols / debug data.** Raw flat carries none. ADR-0029 §Negative consequences accepts this v1 cost.
- **Dynamic loading / filesystem.** The image is `include_bytes!`-embedded at compile time. Filesystem-loaded modules are Phase C / D work.
- **`MemoryRegionCap` for frame ownership.** B5+ ADR per [B3 closure retro §Adjustments](../../analysis/reviews/business-reviews/2026-05-14-B3-closure.md#adjustments). T-019 holds frames informally through the AS cap; revocation cascades correctly but per-frame ownership tracking arrives with the future MemoryRegionCap ADR.
- **A real `hello` userspace binary.** B6's `userland/hello/` crate produces the real binary; T-019 ships with a *placeholder* embedded blob (a hand-written sequence of bytes — e.g. `[0x40, 0x00, 0x80, 0xd2, 0xc0, 0x03, 0x5f, 0xd6]` for `mov w0, #42; ret` — sufficient to verify the loader-produced mapping behaves correctly).

## Approach

To be filled out during implementation; this Draft sketches the load-bearing decisions only.

### Module placement

Two candidates: (a) extend `kernel/src/obj/task.rs` with a `pub fn task_create_from_image(...)` free function next to the existing `create_task`; (b) introduce a new `kernel/src/obj/task_loader.rs` module that owns the loader sequence and re-exports the entry point. Decision deferred to the first implementation commit; the leaning is **(b)** for separation of concerns (the loader's preflight chain is non-trivial; bundling with `create_task` risks bloating `task.rs`).

### Userspace linker layout

Hardcoded to a fixed VA per [ADR-0027 §Decision outcome (a)](../../../decisions/0027-kernel-virtual-memory-layout.md)'s `TTBR0_EL1` range. Candidate base: `0x0000_0000_0080_0000` (8 MiB from the start of the userspace VA range; matches `bsp-qemu-virt/linker.ld`'s kernel-image base offset shape so the layout is mirror-recognisable). Stack at top of a fixed 64 KiB region above the image. Final values land in [`userland/layout.rs`](../../../../userland/) or equivalent shared-source-of-truth (file location decided at first commit).

### Simulation

Per [`write-adr` skill §Procedure step 5 — "Decision outcome → Simulation row-to-verification mapping"](../../../../.agents/skills/write-adr/SKILL.md), every state-machine multi-step task must walk its worst-case interaction in a §Simulation table and pin each row to a host test or audit-log entry. T-019's loader IS a multi-step state machine. Initial table (to be refined during implementation):

| Step | State pre | Action | State post | Verification artefact |
|------|-----------|--------|------------|----------------------|
| 0 | Kernel-init holds bootstrap-AS cap (full rights); PMM has frames; `TaskArena` has slots; image bytes embedded in `.rodata`. | `task_create_from_image(image, ...)` is called. | Same; no state change. | n/a (entry-point row). |
| 1 | Parent-cap lookup + kind check + image-size sanity preflight pass. | Continue to PMM allocation phase. | Same; ready to allocate. | `task_loader::tests::rejects_wrong_parent_cap_kind` host test. |
| 2 | `pmm.alloc_frame` × N for image pages + M for stack pages. | Frames allocated; bytes copied via `write_volatile` (or equivalent audited primitive). | PMM `free_count - (N+M)`; frames now own the image bytes. | `task_loader::tests::copies_image_bytes_into_frames` host test; PMM `Stats::Δ` check. |
| 3 | `cap_create_address_space` mints fresh AS cap. | `cap_map` × (N+M) maps frames into the new AS. | New AS has image + stack mappings; TaskCap not yet minted. | `task_loader::tests::maps_image_and_stack_into_new_as` host test. |
| 4 | `TaskArena.alloc` for the new TaskHandle. | Initialise context register file: `PC = base, SP = stack-top`. | TaskHandle live; ready to schedule (not actually scheduled per §Out of scope). | `task_loader::tests::returns_task_handle_with_correct_pc_sp` host test. |
| 5 (rollback path) | Any preflight fails (kind, size, PMM exhausted, arena full). | Return `TaskLoaderError::*` with all preflighted state untouched. | PMM frame count unchanged; arena slot count unchanged. | `task_loader::tests::rollback_on_*` host test per error variant. |

The table will be refined as implementation surfaces edge cases (block-mapped VAs, `MmuError::BlockMapped` propagation, etc.).

### Embedded image content

For v1 / Draft: a hand-coded 8-byte aarch64 sequence `0x40 0x00 0x80 0xd2 0xc0 0x03 0x5f 0xd6` (`mov w0, #42; ret`). Stored as a `static USERSPACE_IMAGE: &[u8] = include_bytes!(...)` or inlined as a `&[u8]` literal in the BSP (which form depends on whether `include_bytes!` is wired up at this point — if not, an inline literal is acceptable as a Draft).

### Error handling

`TaskLoaderError` enum + `cargo kernel-clippy`'s `#![deny(clippy::panic)]` discipline. Every fallible step propagates a typed error; no `unwrap` / `expect` on the kernel-reachable path. Mirrors [T-018's `AddressSpaceError`](../../../../kernel/src/mm/address_space.rs) shape.

## Review history

| Date | Reviewer | Notes |
|------|----------|-------|
| 2026-05-14 | @cemililik (+ Claude Sonnet 4.6 agent) | Task opened at `Draft` paired with ADR-0029 propose commit (Phase B / Milestone B4). Gates on ADR-0029 `Accepted` before implementation begins. Will move to `In Progress` after ADR Accept; status flips to `In Review` after the full acceptance-criteria checklist passes locally + bot-review-round arc settles. |
