# 0003 — Apache-2.0 license

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

Tyrne is public on GitHub and may attract external contributors. The project also intends to be deployed on the maintainer's own devices — smart-home hardware, SBCs, and eventually mobile devices. The license choice determines:

- Who can redistribute the code and under what terms.
- Whether the project is protected against patent litigation by contributors.
- Whether contributors can embed Tyrne in proprietary products (theirs or the maintainer's).
- How the license interacts with the Rust ecosystem (most of which is dual MIT OR Apache-2.0).

## Decision drivers

- **Patent protection.** Any contributor could hold patents on the code they contribute. The license should include an explicit patent grant to users.
- **Contributor friendliness.** The license should be well-known, legally battle-tested, and not require legal review for typical corporate contributors.
- **Ecosystem alignment.** The bulk of the Rust ecosystem is Apache-2.0 (or dual MIT/Apache-2.0). Compatibility at the license boundary matters when Tyrne takes dependencies.
- **Commercial flexibility.** The maintainer wants to be able to deploy Tyrne on their own products without licensing constraints on the downstream application.
- **Simplicity.** A single-license project is simpler to reason about than a dual-license project for both contributors and consumers, unless there is a specific need for dual licensing.

## Considered options

1. **MIT.** Minimalist permissive license; no patent grant.
2. **Apache-2.0.** Permissive with an explicit patent grant and termination clause.
3. **Dual MIT OR Apache-2.0.** Rust ecosystem convention; the consumer chooses.
4. **GPL v3.** Strong copyleft; derivative works must remain open.
5. **MPL-2.0.** File-level copyleft; allows combination with closed code at the module boundary.
6. **BSD-3-Clause.** Permissive, similar to MIT, no patent grant.

## Decision outcome

**Chosen: Apache-2.0, single-license.**

Apache-2.0 gives us the explicit patent grant that MIT and BSD lack, which matters for a security-oriented project that may accumulate contributions from industry participants with patent portfolios. It is the dominant license in the modern cloud and systems ecosystem, which minimizes contributor friction.

We considered the dual MIT-or-Apache-2.0 Rust convention. The convention exists largely because some downstream projects (particularly older Linux components) have historic reasons to prefer MIT. Tyrne is a new project with no such entanglement, and single-licensing simplifies the contributor agreement and the NOTICE accounting.

GPL-v3 was rejected because it is incompatible with the maintainer's goal of being able to ship Tyrne inside proprietary products they may build later. MPL-2.0 was considered as a middle ground but rejected: its file-level copyleft creates awkward boundary questions for a kernel that may be extended with proprietary userspace services. BSD-3 offers no meaningful advantage over Apache-2.0 for this project.

## Consequences

### Positive

- Explicit patent grant from every contributor, with a clear termination clause if a contributor later files patent infringement litigation related to the work.
- Well-understood by corporate legal teams; contributors from large organizations can submit without bespoke CLA negotiation.
- Aligns with the Rust ecosystem: most dependencies can be consumed without license-compatibility concerns.
- The maintainer retains the right to incorporate Tyrne into proprietary products without relicensing.

### Negative

- **Not copyleft.** Someone can fork Tyrne and ship a closed-source derivative without contributing changes back. We accept this trade-off because the goal is broad adoption on the maintainer's own devices, not forced openness of downstream.
- **`NOTICE` file maintenance.** Apache-2.0 requires that attribution notices be preserved. We maintain a `NOTICE` at the repository root and expect contributors to respect its content.
- **Slightly longer license file than MIT.** Negligible.

### Neutral

- Some developers prefer MIT on philosophical grounds. This is a preference, not a technical consideration.

## Pros and cons of the options

### MIT

- Pro: shortest possible license; widely known.
- Con: no patent grant; weaker protection against patent trolls among contributors.

### Apache-2.0

- Pro: patent grant with termination; widely understood; well-tested legally.
- Con: longer than MIT; requires `NOTICE` discipline.

### Dual MIT OR Apache-2.0

- Pro: Rust convention; maximum downstream choice.
- Con: adds a small amount of legal surface area for no concrete benefit on a new project.

### GPL v3

- Pro: strongest forced-openness of derivatives.
- Con: incompatible with proprietary downstream deployment by the maintainer.

### MPL-2.0

- Pro: file-level copyleft — middle ground.
- Con: boundary questions get confusing in OS-sized codebases.

### BSD-3-Clause

- Pro: simple permissive.
- Con: no patent grant (same weakness as MIT).

## References

- Apache License, Version 2.0: https://www.apache.org/licenses/LICENSE-2.0
- Rust API Guidelines on licensing: https://rust-lang.github.io/api-guidelines/necessities.html#crate-has-a-permissive-license
- Why Apache 2 (GitHub's choosealicense.com): https://choosealicense.com/licenses/apache-2.0/
