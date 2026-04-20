# Architecture documentation

This folder describes **how Umbrix is designed**. The **why** behind each design choice lives in [../decisions/](../decisions/); the documents here focus on the *how* — the structure, the interactions, and the data flow.

## Status

The architecture is being written in phases. Many documents listed below are planned but not yet written. They will be added as the ADRs for the corresponding subsystems are accepted.

## Index

| Document | Purpose | Status |
|----------|---------|--------|
| [`overview.md`](overview.md) | Top-level structure: kernel, userspace, HAL, boot flow; with Mermaid diagrams. | Accepted |
| `kernel-core.md` | Core kernel responsibilities: scheduler, IPC, memory, capabilities. | Planned — Phase 2 |
| [`security-model.md`](security-model.md) | Capability system, trust boundaries, threat model. | Accepted |
| `memory-management.md` | Physical + virtual memory, MMU/paging, allocators. | Planned |
| `scheduling.md` | Scheduler design, priorities, real-time considerations. | Planned |
| `ipc.md` | Inter-process communication, message passing, endpoints, capability transfer. | Planned |
| [`hal.md`](hal.md) | Hardware Abstraction Layer: trait surfaces, board support packages, portability. | Accepted |
| [`boot.md`](boot.md) | Boot flow from reset vector through kernel init to first userspace task. | Accepted (v0.0.1 — QEMU virt only) |
| `drivers.md` | Userspace driver model, capability grants, driver API. | Planned |
| `userspace.md` | Init process, system services, shell, root of trust. | Planned |

## Reading order

Start with `overview.md`, then follow the subsystem that interests you. The security model cross-cuts everything and is worth reading early.

## Conventions

- Architectural diagrams use Mermaid (see [../standards/documentation-style.md](../standards/documentation-style.md)).
- Every subsystem document begins with a one-paragraph summary suitable for someone skimming the tree.
- Code snippets in architecture documents are illustrative pseudocode unless explicitly marked as real source.
- Every architectural claim of the form *"Umbrix does X because Y"* should link to the ADR that made that choice.
