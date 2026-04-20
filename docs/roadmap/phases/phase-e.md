# Phase E — Driver model and essential services

**Exit bar:** A set of real userspace services composes into a working system — log service, service supervisor, storage driver, simple filesystem, userspace network stack — with the driver template documented so new drivers can be written consistently.

**Scope:** Establishes "userspace drivers as first-class tasks" as a working pattern, not just an architectural claim. Lands the minimum set of services any real-world deployment will need.

**Out of scope:** Specific end-user applications (Phase F); cryptographic services (Phase G); wireless (blob-dependent).

---

## Milestone E1 — Userspace driver template

A template crate and guide for writing a userspace driver task. A driver holds a `MemoryRegionCap` for its device's MMIO, an `IrqCap` for its interrupt line, and an `EndpointCap` pair for its service interface.

### Sub-breakdown

1. **ADR-0036 — Driver task structure.** Single-threaded vs. multi-threaded; how does a driver receive IRQ notifications (endpoint + notify); error / restart semantics.
2. **Template crate** `umbrix-driver-template/` — a skeleton a new driver copies from.
3. **Guide** `docs/guides/write-a-driver.md`.

### Acceptance criteria

- ADR-0036 Accepted.
- Template compiles and documents the driver's service interface.

## Milestone E2 — Log service

A userspace service that receives log records from kernel and other userspace tasks via a capability-gated endpoint and emits them to the console (and later, to persistent storage).

### Sub-breakdown

1. **ADR-0037 — Log wire format.** Binary (postcard / custom TLV); versioned; structured key-value per [logging-and-observability.md](../../standards/logging-and-observability.md).
2. **`umbrix-log` facade** in the kernel — the `log!` / `info!` / `warn!` macros encoded in the facade.
3. **Log service task** — listens on its endpoint, reads records, renders to the console.

### Acceptance criteria

- ADR-0037 Accepted.
- Kernel logs route through the service rather than direct UART writes (the boot console remains as emergency fallback).

## Milestone E3 — Service manager / supervisor

A task that starts, watches, and restarts other tasks per a config. The foundation of the init-task concept.

### Sub-breakdown

1. **ADR-0038 — Supervision strategy.** Always-restart / N-failures-then-give-up / operator-controlled.
2. **Supervisor task** that reads a config (compile-time initial, filesystem-based later).
3. **Fault-endpoint plumbing** — each supervised task has its fault endpoint held by the supervisor.

### Acceptance criteria

- ADR-0038 Accepted.
- A deliberately-crashing test task is restarted by the supervisor per the configured policy.

## Milestone E4 — Storage driver

QEMU: virtio-blk. Pi 4: SD card via the SDHCI-like controller on BCM2711. The driver exposes a block-device service interface.

### Sub-breakdown

1. **ADR-0039 — Block-device service interface.** Synchronous / asynchronous read-write; sector size; capability model.
2. **`umbrix-driver-virtio-blk`** — the first real non-trivial driver.
3. **`umbrix-driver-sdhci-bcm2711`** — the Pi 4 counterpart (may be stubbed until later).

### Acceptance criteria

- ADR-0039 Accepted.
- A userspace client can read and write sectors through the storage service.

## Milestone E5 — Simple filesystem

A read-mostly filesystem service on top of E4. Initial choice may be read-only (e.g., something like BootFS or a custom simple layout) with write support added later.

### Sub-breakdown

1. **ADR-0040 — Filesystem choice.** Build a simple one, port an existing crate (`littlefs`, `ext4`-via-crate, a log-structured FS like F2FS-style for flash-friendly wear-levelling), or start with a read-only block layout and add write support incrementally. Weighed against portability, `no_std + alloc` compatibility, crash-consistency guarantees, and the smart-home target's preference for flash-friendly wear-levelling.
2. **Filesystem service task** implementing the chosen approach.
3. **Storage capability flow** — the filesystem service has the block-device capability; it grants named-file capabilities to clients.

### Acceptance criteria

- ADR-0040 Accepted.
- A userspace client can open, read, and (at minimum) list files through the filesystem service.

## Milestone E6 — Network stack integration

`smoltcp` or similar, in a userspace network service, using virtio-net on QEMU.

### Sub-breakdown

1. **ADR-0041 — Network stack choice.** smoltcp is the probable answer; this ADR commits to it or to an alternative, covering `no_std + alloc` compatibility, license, and maintenance.
2. **`umbrix-driver-virtio-net`** driver.
3. **Network service task** wrapping the stack with a capability-gated interface.

### Acceptance criteria

- ADR-0041 Accepted.
- Loopback works; a test client completes a TCP three-way handshake with a server on the host.

### Phase E closure

Business review. The system now has enough plumbing to support a real end-user deployment, which is Phase F.

## ADR ledger for Phase E

| ADR | Purpose | Expected state |
|-----|---------|----------------|
| ADR-0036 | Driver task structure | E1 |
| ADR-0037 | Log wire format | E2 |
| ADR-0038 | Supervision strategy | E3 |
| ADR-0039 | Block-device service interface | E4 |
| ADR-0040 | Filesystem choice | E5 |
| ADR-0041 | Network stack choice | E6 |

## Open questions carried into Phase E

- Whether a unified "service interface" pattern emerges that multiple services share, or each service designs its own interface.
- Sync vs. async driver model.
- Where smoltcp fits in licensing and `cargo-vet` posture.
