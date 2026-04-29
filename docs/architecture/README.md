# Architecture documentation

This folder describes **how Tyrne is designed**. The **why** behind each design choice lives in [../decisions/](../decisions/); the documents here focus on the *how* — the structure, the interactions, and the data flow.

## Status

The architecture is being written in phases. Many documents listed below are planned but not yet written. They will be added as the ADRs for the corresponding subsystems are accepted.

## Index

| Document | Purpose | Status |
|----------|---------|--------|
| [`overview.md`](overview.md) | Top-level structure: kernel, userspace, HAL, boot flow; with Mermaid diagrams. | Accepted |
| [`security-model.md`](security-model.md) | Capability system, trust boundaries, threat model. | Accepted |
| [`hal.md`](hal.md) | Hardware Abstraction Layer: trait surfaces, board support packages, portability. | Accepted |
| [`boot.md`](boot.md) | Boot flow from reset vector through kernel init to first userspace task. | Accepted (v0.0.1 — QEMU virt; T-013 EL drop landed) |
| [`scheduler.md`](scheduler.md) | Cooperative FIFO scheduler: ready queue, idle task, raw-pointer IPC bridge, ContextSwitch trait. | Accepted (v0.0.1 — single-core, no preemption) |
| [`ipc.md`](ipc.md) | Inter-process communication: synchronous send/recv, endpoint state machine, capability transfer, scheduler-bridge wrappers. | Accepted (v0.0.1 — depth-1 endpoints) |
| [`exceptions.md`](exceptions.md) | Exception vector table, IRQ dispatch, GIC v2 driver, generic-timer IRQ wiring, idle WFI activation. | Accepted (v0.0.1 — T-012 Done 2026-04-28 via PR #10 merge; design + implementation match; maintainer-side QEMU smoke verification of the deliberate-deadline path remains pre-B1-closure work) |
| `memory-management.md` | Physical + virtual memory, MMU/paging, allocators. | Planned — B2 |
| `drivers.md` | Userspace driver model, capability grants, driver API. | Planned |
| `userspace.md` | Init process, system services, shell, root of trust. | Planned |

## Reading order

Start with `overview.md`, then follow the subsystem that interests you. The security model cross-cuts everything and is worth reading early.

## Conventions

- Architectural diagrams use Mermaid (see [../standards/documentation-style.md](../standards/documentation-style.md)).
- Every subsystem document begins with a one-paragraph summary suitable for someone skimming the tree.
- Code snippets in architecture documents are illustrative pseudocode unless explicitly marked as real source.
- Every architectural claim of the form *"Tyrne does X because Y"* should link to the ADR that made that choice.
