# Phase I — Mobile

**Exit bar:** First prototype boot on phone-class hardware with a display and minimal input, demonstrating that Umbrix can host a phone-style device shell.

**Scope:** The long-horizon stretch. Years away. This file is a sketch; detail grows as Phase I approaches.

**Out of scope:** A shippable phone OS (that is a project in itself); app ecosystem; cellular (heavily blob-dependent).

---

## Milestone I1 — ARM64 mobile SoC evaluation and target choice

Pick a target SoC that is approachable (community-supported, documented) and commit to a specific device model.

### Candidates

- **PinePhone / PinePhone Pro** — Allwinner / Rockchip SoCs; good Linux community support; well-documented.
- **Volla / Murena** phones — generally Qualcomm Snapdragon; blob-heavy for Wi-Fi / modem.
- **Experimental / developer boards** with mobile-class CPUs (Rockchip RK3588, MediaTek MT8195).

### Sub-breakdown

1. **ADR-0055 — Mobile target.** Specific device, specific SoC, exhaustive list of required peripherals, policy on required blobs.
2. **Availability survey** — prices, lead times, reliability of the supply.
3. **Prior-art survey** — who else has built non-Linux kernels on this SoC family; what they learned.

## Milestone I2 — Display + touch stack

A display panel writes pixels; a touch panel reports events; these compose into a minimal interactive surface.

### Sub-breakdown

1. **ADR-0056 — Display stack architecture.** Framebuffer vs. compositor vs. direct-panel. Probably direct-panel + a tiny software compositor for Phase I.
2. **Display driver** for the chosen panel.
3. **Touch driver** (often I2C-attached; reuses F2 work).
4. **Input service** mapping raw touch events to typed events.

## Milestone I3 — Power management

Mobile requires battery-aware CPU scaling, suspend / resume, screen blanking, aggressive idle. This is large and cross-cutting.

### Sub-breakdown

1. **ADR-0057 — Power management scope.** What levels (idle, suspend-to-RAM, hibernate) and what invariants.
2. **Scheduler integration** — CPU-frequency hints, big.LITTLE awareness if the SoC has it.
3. **Battery service** — SoC-specific PMIC driver + SOC / voltage monitoring.
4. **Wake sources** — timer, touch, modem (if applicable).

## Milestone I4 — First prototype boot

The device runs Umbrix, shows a screen, responds to touch, reports battery status. It is not a product; it is a demonstration that the kernel can host such a product.

### Sub-breakdown

1. **Integration** of I1–I3 into one image.
2. **Demonstration application** — something simple (clock, battery monitor).
3. **Photographs / video** for record.
4. **Business review** — honest assessment of what remains to go from prototype to device.

### Phase I closure

The mobile milestone is explicitly a stretch goal. Reaching I4 makes Umbrix a credible mobile-capable kernel, not a consumer product.

## ADR ledger for Phase I

| ADR | Purpose | Expected state |
|-----|---------|----------------|
| ADR-0055 | Mobile target | I1 |
| ADR-0056 | Display stack architecture | I2 |
| ADR-0057 | Power management scope | I3 |

## Open questions carried into Phase I

- Whether "mobile" for Umbrix is a phone, a tablet, or both.
- Cellular modem support (likely never in this project; use devices without built-in modems or with open modems).
- Audio stack (not covered in I1–I4; a separate ADR or a future phase).
- Long-term positioning: is Umbrix a security-first *phone OS*, or does the smart-home deployment remain the primary face?
