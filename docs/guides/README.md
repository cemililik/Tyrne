# Guides

Task-oriented walkthroughs for contributors and users of Umbrix. Guides answer *"how do I do X?"* — they are step-by-step, they assume little, and they link out to architecture and ADRs when background is needed.

## Status

Umbrix is in the architecture phase. Guides will be added as the corresponding implementation work is done. The list below is a placeholder so that folder structure and naming are clear.

## Planned guides

| Guide | Audience | Status |
|-------|----------|--------|
| `toolchain-setup.md` | First-time contributor setting up the Rust cross-compiler and QEMU. | Planned — Phase 3 |
| [`run-under-qemu.md`](run-under-qemu.md) | Running the kernel under QEMU `virt` aarch64. | Accepted (v0.0.1) |
| `debug-with-gdb.md` | Attaching GDB to a QEMU-hosted kernel. | Planned — Phase 3 |
| `port-to-new-board.md` | Adding a new board support package to the HAL. | Planned — Phase 4 |
| `write-a-driver.md` | Implementing a userspace driver with capability grants. | Planned — Phase 4 |
| `write-an-adr.md` | Proposing, writing, and accepting an ADR. | Planned — Phase 2 |

## Conventions

- Every guide starts with **Goal** and **Prerequisites** sections.
- Commands are shown in fenced code blocks with the shell language tag.
- File paths and command output are verbatim; placeholders use `<angle-brackets>`.
- When a guide is long, split it into numbered sub-steps and include a final **Verifying it worked** section.
