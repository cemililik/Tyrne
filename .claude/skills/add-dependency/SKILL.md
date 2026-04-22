---
name: add-dependency
description: Add a new Rust crate to the Tyrne workspace following the dependency policy in `infrastructure.md`.
when-to-use: Whenever a PR needs to introduce a new dependency in `Cargo.toml`, or upgrade an existing dependency across a semver-major boundary.
---

# Add dependency

## Inputs

- The **crate name and version** to add.
- The **reason** it is needed — what functionality it provides that cannot reasonably be implemented in-tree.
- The **intended target** — kernel-linked, HAL-linked, userspace-linked, or dev-only (tests, build scripts, tools).

## Procedure

1. **Search for a substitute.** Before adding a dependency:
   - Check whether the standard library (when applicable) or an existing dependency already covers the need.
   - For small tasks (a few hundred lines of utility code), consider inlining instead of depending.
   - If you cannot articulate why in-tree code is worse than the dependency, write the in-tree code instead.

2. **License check.**
   - Acceptable: Apache-2.0, MIT, MIT/Apache-2.0 dual, BSD-2-Clause, BSD-3-Clause, ISC, MPL-2.0.
   - Rejected for kernel/HAL/userspace targets: GPL-family (GPLv2, GPLv3, LGPL), AGPL, any "source available" non-OSI license, unlicensed code.
   - Dev-only / build-only dependencies have more latitude but are still reviewed.
   - If the license is unclear (no `LICENSE` file, contradictory metadata), stop and raise the question with the maintainer.

3. **`no_std` check for kernel / HAL / userspace dependencies.**
   - The crate must support `no_std`. Verify by reading its `Cargo.toml` for a `default-features = false` no-std path, or by inspecting its source for `#![no_std]` at the crate root (possibly conditional).
   - If the crate requires `std`, it cannot be used in kernel / HAL / userspace targets. It may still be acceptable for build scripts or tests.

4. **Classify the trust category** per [infrastructure.md — Trust categories](../../../docs/standards/infrastructure.md):
   - **Foundational** — core building blocks (e.g. `aarch64-cpu`, `volatile-register`, `bitflags`). Full audit.
   - **Recognized groups** — crates maintained by `rust-lang`, `oxidecomputer`, `rust-embedded`, `google`, `mozilla`. `cargo-vet` import + delta audit on upgrade.
   - **Individuals** — most of crates.io. Scrutinize; prefer to inline if small.
   - **Dev-only / build-only** — ordinary review.

5. **Graph impact.**
   - Run `cargo tree -p <candidate-crate>` (or equivalent) to list transitive dependencies.
   - Summarize: number of new transitive crates, rough lines of Rust added (use `tokei` or a reasonable estimate).
   - If the transitive graph is disproportionate to the benefit, reconsider.

6. **`cargo-vet` certification.**
   - If a trusted peer (rust-lang, oxide, google, etc.) has an existing audit for the exact version, import it:
     ```sh
     cargo vet certify-import <peer>
     ```
   - If no trusted audit exists and the crate is small, create a local audit entry:
     ```sh
     cargo vet certify <crate> <version>
     ```
     The certification records what level of review you performed (`safe-to-run`, `safe-to-deploy`, etc.).
   - If the crate is large and unaudited, escalate — a large unaudited kernel dependency is a decision requiring an ADR.

7. **Pin the version.**
   - In `Cargo.toml`, use a caret range reflecting the actual compatibility tested: `"^1.4"` or exact `"=1.4.2"` for reproducibility-critical dependencies.
   - Never use `"*"`, `"latest"`, or unspecified git branches.

8. **Add to the workspace** `Cargo.toml`:
   - Prefer the workspace-level `[workspace.dependencies]` so version is defined once and inherited.
   - Individual crate `Cargo.toml` then does `serde = { workspace = true }`.

9. **Build and test** to confirm the dependency resolves and the workspace compiles cleanly on all Tier 1 targets.

10. **Write the PR description** covering:
    - What the crate does and why it is needed.
    - Rejected alternatives (in-tree, other crates, the standard library).
    - License and trust category.
    - Graph impact (transitive count, approximate size).
    - `cargo-vet` certification (imported or newly authored).

11. **Commit** per [commit-style.md](../../../docs/standards/commit-style.md):
    - Message: `build(deps): add <crate> <version>` (or `build(deps): upgrade <crate> <old> → <new>`).
    - Body: the justification and trust category.
    - Trailer: `Security-Review:` if the dependency is security-sensitive (touches capabilities, crypto, network, parsing, or kernel linkage).

## Acceptance criteria

- [ ] License confirmed acceptable.
- [ ] `no_std` compatibility confirmed for non-dev targets.
- [ ] Trust category assigned and appropriate review depth applied.
- [ ] Graph impact summarized in the PR description.
- [ ] `cargo-vet` entry exists (imported or authored).
- [ ] Version pinned with a caret range, not `"*"`.
- [ ] Workspace compiles on Tier 1 targets.
- [ ] PR description has the full justification.
- [ ] Commit trailer correct; `Security-Review:` present if security-sensitive.

## Anti-patterns

- Adding a dependency "because it's easier" without the substitute search.
- `version = "*"` anywhere.
- Adding a GPL-licensed crate to a kernel / userspace target.
- Skipping `cargo-vet` because the crate "looks fine".
- Upgrading a major version in a PR whose primary purpose is something else.
- Pulling in a 500-crate transitive graph for a 20-line utility.
- Adding a dependency in a PR that does not mention it in the description.

## References

- [infrastructure.md — Dependency policy](../../../docs/standards/infrastructure.md).
- `cargo-vet` documentation: https://mozilla.github.io/cargo-vet/.
- `cargo-audit`: https://rustsec.org/.
- [commit-style.md](../../../docs/standards/commit-style.md).
