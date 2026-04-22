# Phase F — Smart-home deployment

**Exit bar:** A real physical device in the maintainer's smart-home setup runs Tyrne as its firmware, communicating with a hub (Matter controller or MQTT broker) and reacting to real events.

**Scope:** The project's reason to exist. The plumbing from Phase E gets specialized drivers; one or more smart-home protocols are selected; the first deployable device is built.

**Out of scope:** A generic smart-home platform for others to use (possible long-term but not in this phase); voice assistants; cloud connectivity.

---

## Milestone F1 — GPIO driver on Pi 4

GPIO control on BCM2711. Fundamental because most smart-home peripherals (sensors, relays, LEDs) sit on GPIO pins.

### Sub-breakdown

1. **ADR-0043 — GPIO service interface.** Pin granularity, capability per pin vs. per bank; direction / pull-up / drive-strength configuration.
2. **`tyrne-driver-gpio-bcm2711`** driver task.
3. **Client library** `tyrne-gpio` with typed pin handles.

### Acceptance criteria

- ADR-0043 Accepted.
- Driver toggles a GPIO pin observable externally (an LED, a scope).

## Milestone F2 — I2C and SPI drivers

Most smart-home sensors use one of these. Covers the BCM2711 peripherals.

### Sub-breakdown

1. **ADR-0044 — I2C service interface.**
2. **ADR-0045 — SPI service interface.** (Separate ADR because of different capability semantics — SPI has chip-select per device, I2C has addresses.)
3. **Drivers** `tyrne-driver-i2c-bcm2711`, `tyrne-driver-spi-bcm2711`.
4. **Test clients** that read a known sensor (e.g., BME280 on I2C, an MCP SPI flash) to verify end-to-end.

### Acceptance criteria

- ADRs Accepted.
- One real I2C sensor read returns plausible values.
- One real SPI device read returns plausible values.

## Milestone F3 — Protocol choice (Matter / MQTT / both)

The smart-home communication protocol. Matter is the modern open standard; MQTT is the lightweight alternative.

### Sub-breakdown

1. **ADR-0046 — Smart-home protocol.** Weighed by: open-source library availability, power profile, interop with the maintainer's existing hub, security posture.
2. **Implementation** — either a port of an existing Rust crate (preferred) or a minimal subset implementation from scratch (accepted cost).
3. **Security review** of the protocol implementation per [`analysis/reviews/security-reviews/`](../../analysis/reviews/security-reviews/).

### Acceptance criteria

- ADR-0046 Accepted.
- End-to-end: Tyrne device sends a heartbeat / state update to a real hub.

## Milestone F4 — First smart-home device

A chosen device — e.g., a temperature sensor node, a smart plug, an environmental monitor — running Tyrne as its full firmware.

### Sub-breakdown

1. **Device choice** — specific hardware with power and mechanical suitability.
2. **Integration** — wiring F1–F3 together into a coherent application running on Pi 4 hardware.
3. **Reliability test** — 7-day uptime under realistic load without crashes or memory growth.
4. **Guide** `docs/guides/first-smart-home-device.md`.
5. **Business review** — the first real "production" deployment.

### Acceptance criteria

- Device runs 7 days uninterrupted.
- Its state is reflected in the hub and reacts to commands.
- Guide reproducible.

### Phase F closure

Milestone F4 is a genuine milestone: Tyrne becomes real when this ships. Subsequent phases tighten the security story (Phase G) and expand the platform base (Phase H).

## ADR ledger for Phase F

| ADR | Purpose | Expected state |
|-----|---------|----------------|
| ADR-0043 | GPIO service interface | F1 |
| ADR-0044 | I2C service interface | F2 |
| ADR-0045 | SPI service interface | F2 |
| ADR-0046 | Smart-home protocol | F3 |

## Open questions carried into Phase F

- **Wi-Fi on Pi 4.** The Broadcom Wi-Fi chip requires proprietary firmware; Tyrne's policy rejects blobs. Options: use Ethernet instead on Pi 4 (simplest), use USB Wi-Fi dongles with open-source firmware, or accept a documented exception for firmware that lives outside the kernel (in-scope for an ADR).
- **Battery operation.** Power-management is substantial; may belong in Phase I alongside mobile.
- **Encryption at rest** on device storage — crosses into Phase G.
