# Infrastructure

How Tyrne is built, how its dependencies are managed, what its CI gates are, and what promises the build system makes about the output. This standard is aspirational in places — not all of it is wired up yet — but it establishes the bar so that each piece of infrastructure is built to the standard rather than evolved ad-hoc.

## Scope

- Toolchain (Rust version pinning).
- Dependency policy (adding, upgrading, auditing).
- Continuous integration (what runs, what gates merges).
- Supply-chain security (`cargo-vet`, `cargo-audit`, SBOM).
- Reproducibility of builds.
- Branch protection and merge rules.
- Secrets management.

## Toolchain

- **Pinned nightly Rust via `rust-toolchain.toml`** at the repository root. The file specifies the exact nightly date and the components required (`rust-src`, `rustfmt`, `clippy`, `llvm-tools-preview` as needed).
- The pinned nightly is bumped deliberately, via a dedicated PR, with a commit message explaining the upgrade. Do not update the toolchain as a side effect of other changes.
- CI runs against the pinned toolchain only. Multiple-toolchain matrices are not currently useful for a `no_std` kernel.
- Cross-compile targets are installed with `rustup target add` per CI job:
  - `aarch64-unknown-none` (primary kernel target).
  - `aarch64-unknown-none-softfloat` (variants where needed).
  - Additional targets added as tiers 2+ come online.

## Dependency policy

Every crate added to the workspace is a trust decision. The policy makes the decision explicit.

### Adding a new dependency

A PR that adds a dependency must:

1. **Justify the dependency** in the PR description. What does it do, why is it needed, what is the alternative of writing it ourselves?
2. **Record size and graph impact.** Lines of Rust added; number of transitive dependencies pulled in.
3. **Run `cargo-vet` certification** — either consume an existing audit from a trusted peer, or produce an audit entry (small crates) or a delta audit (incremental change).
4. **Confirm `no_std` compatibility** for kernel-linked crates.
5. **Confirm license compatibility** — Apache-2.0, MIT, MIT/Apache-2.0 dual, BSD-2/3, ISC, MPL-2.0 are acceptable. GPL-licensed crates are rejected unless the dependency is build-time-only and the output is not linked.
6. **Pin the version** in `Cargo.toml` with a caret range that reflects the actual compatibility tested. Do not use `"*"`.

### Trust categories

We classify dependencies into four categories:

| Category | Examples | Review depth |
|----------|----------|--------------|
| **Foundational** | `aarch64-cpu`, `volatile-register`, `bitflags` | Full audit. Changes reviewed like kernel code. |
| **Maintained by recognized groups** | `rust-lang` crates, `oxidecomputer` crates, `rust-embedded` crates | `cargo-vet` import, delta audits on upgrades. |
| **Maintained by individuals** | Most of crates.io | Scrutinize; prefer to inline or vendor if small. |
| **Dev-only / build-only** | `cargo-geiger`, test harnesses | Ordinary review; not linked into shipped binaries. |

### Upgrades

- Patch and minor upgrades reviewed for changelog highlights.
- Major upgrades reviewed as if the dependency were being added new. `cargo-vet delta` is the primary tool.
- **Do not run `cargo update` as a routine housekeeping commit.** `Cargo.lock` updates are deliberate, scoped, and reviewed.

### Removal

Removing a dependency (replacing with in-tree code or dropping the feature it enabled) is encouraged whenever the dependency is thin or unmaintained. Deletion PRs are low-ceremony.

## Continuous integration

CI is expected to be set up early in Phase 4 (Rust toolchain + workspace skeleton). The gates below define the bar.

### Required gates (block merge)

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace` — host-runnable unit and integration tests.
- `cargo build --workspace --target aarch64-unknown-none` — kernel builds clean.
- QEMU smoke — kernel boots under `qemu-system-aarch64 -machine virt` and reaches the success marker. *(As of 2026-05: maintainer-launched only; no `qemu-smoke` CI job yet — tracked as a B2-or-later roadmap follow-up.)*
- `cargo audit` — fails on known advisories. `cargo-audit` database is updated weekly in CI. *(Conditional — currently dormant: `Cargo.lock` carries zero external dependencies, so the gate would be a no-op. The job is wired in once the first external dependency lands per [add-dependency](../../.claude/skills/add-dependency/SKILL.md).)*
- `cargo vet check` — fails if any dependency is not audited. *(Same conditional — see `cargo audit` above.)*

### Advisory gates (warn, do not block)

- `cargo-geiger` report — records `unsafe` counts, compared against the audit log.
- Coverage delta (via `cargo llvm-cov`) — not a gate yet; informational.
- Binary size delta (`cargo bloat`) — informational; large increases prompt a question.

### CI platform

- GitHub Actions is the default. Workflows live under `.github/workflows/`.
- Jobs are reusable — shared setup (install toolchain, cache cargo registry) is a composite action.
- CI caches `~/.cargo/registry` and `target/` keyed by the toolchain hash.
- Secrets never enter CI. If a future workflow needs a secret (e.g. publishing artifacts), it is scoped and rotated.

### Runners

- `ubuntu-latest` for the standard toolchain matrix.
- `qemu-system-aarch64` on the Linux runner for smoke tests.
- Real-hardware jobs (Raspberry Pi lab, when it exists) are self-hosted runners, off the PR hot path, running on release cadence.

## Performance harness

`tools/perf-harness.sh` is the canonical source for boot-to-end timing claims. It wraps `tools/run-qemu.sh` in an iteration loop with a per-run watchdog, parses the kernel's `boot-to-end elapsed = X ns` emission out of each run's serial output, and prints `min / p10 / p50 / p90 / p99 / max / mean / stddev` in both ns and ms. Maintainer-launched only; not yet wired into CI (matches the QEMU smoke convention above — promotion to a CI gate is a B2-or-later follow-up alongside the smoke job).

### Usage

```text
tools/perf-harness.sh                                           # 20 iterations, debug build
tools/perf-harness.sh --iterations=K --timeout=SECONDS          # tune iteration count + per-run watchdog
tools/perf-harness.sh --release                                 # use the release ELF (forwarded to run-qemu.sh)
tools/perf-harness.sh --quiet                                   # suppress per-iteration progress
tools/perf-harness.sh --report=CONTEXT                          # also emit a markdown report under
                                                                # docs/analysis/reports/perf-baseline-YYYY-MM-DD-CONTEXT.md
```

A run aborts non-zero if fewer than 50 % of iterations produced a valid sample — that threshold is treated as environmental (kernel image missing, QEMU not in PATH, host under heavy load) rather than a measurement worth aggregating.

### Reporting discipline

- **Cite the band, not a single sample.** When a PR's commentary needs a boot-to-end figure, run the harness and quote the `p10 / p50 / p90` triple plus the iteration count. A solitary `boot-to-end elapsed = X ns` from a single QEMU launch is not a load-bearing measurement; QEMU TCG's translation-cache behaviour gives ~15-30 % run-to-run variance and a single sample can fall anywhere in the band.
- **Single-run anecdotes from before this harness landed are preserved as historical record.** The 2026-04-21 / 2026-04-28 / 2026-05-07 perf reviews quote single-run figures; those numbers are not retroactively replaced — but every *new* perf claim cites a harness band.
- **Baseline reports under `docs/analysis/reports/perf-baseline-*.md`** are append-only artefacts. Re-baselines after a perf-relevant change land as fresh reports with a new context slug; old reports stay in place as the historical record.

### Counter caveat

The harness measures the kernel's `now_ns()` delta. Under QEMU TCG that counter advances based on emulated instructions, so the band reflects translation-cache variance plus host-scheduler jitter rather than wall-clock time on real hardware. The numbers are useful for *relative* regression detection across a tight window of commits on the same host; they are not predictive of boot-time on real ARM silicon. When that question becomes load-bearing the harness gains a `--hardware` mode or the measurement moves to a self-hosted Pi runner — neither is in scope for v1.

## Supply-chain security

### `cargo-vet`

- Tracks, per dependency, whether it has been audited and by whom.
- Imports trusted audits from:
  - `rust-lang` — standard library adjacent crates.
  - `google` / `mozilla` — broad-use crate audits.
  - `bytecodealliance` — wasm/runtime crates.
  - `oxidecomputer` — embedded and kernel-adjacent crates.
- Local audits (our own) are stored in `supply-chain/audits.toml` and signed by the maintainer.

### `cargo-audit`

- Matches the dependency graph against RustSec advisories.
- A live advisory blocks merge. The PR author either upgrades past the advisory or vendors in a fix.
- False positives are rare; when they occur, the advisory is annotated with the reason for exception and a link to upstream discussion.

### SBOM

- A software bill of materials is generated per release (planned, Phase 5).
- Format: CycloneDX JSON.
- Published alongside the release artifacts.

### Reproducibility

- Builds are reproducible given the same source tree, toolchain, and target.
- Build artifacts do not bake in timestamps, absolute host paths, or user names.
- Rust build flags avoid `*-cpu=native` in release builds; the target triple fully defines the ISA baseline.
- Binary outputs are compared across CI runs; an unexpected delta is investigated.

## Branch protection and merge rules

When the project moves out of solo phase:

- `main` is protected.
- PRs to `main` require at least one approval (two for security-sensitive changes — see [security-review.md](security-review.md)).
- Required status checks: the CI gates listed under "Required gates" above.
- Force-push to `main` disabled.
- Force-push to protected `release/*` branches disabled.

## Secrets management

- The repository contains **no secrets**, ever.
- Keys, tokens, credentials, and development certificates are stored outside the repository in the maintainer's keyring or a secrets manager.
- CI secrets (if any) are GitHub Actions secrets, scoped to specific workflows, with periodic rotation.
- A leaked secret is treated as an incident: rotate, force re-auth everywhere the key was used, update the affected CI configurations, and record the incident for the review in [release.md](release.md).

## Configuration files

The lint set is canonical at [`code-style.md` §Lints](code-style.md#lints); every entry below either references that policy directly or layers narrowly on top of it. Keep both standards in sync when either changes.

### Present at HEAD

| File | Purpose |
|------|---------|
| `rust-toolchain.toml` | Pinned toolchain + required components. **Note:** `miri` is not listed in the components array; the Miri CI job adds it on-demand (`rustup toolchain install $NIGHTLY_PIN --component miri`), and a workspace-local `cargo +nightly miri test` invocation requires `rustup component add miri` once. |
| `rustfmt.toml` | Formatter config. |
| `clippy.toml` | Linter thresholds and allowed lints. |
| `.cargo/config.toml` | Target triples, linker flags per target. |
| `.github/workflows/*.yml` | CI pipelines. Active jobs at HEAD: `lint-and-host-test`, `kernel-build`, `miri`, `coverage`. |

### Planned (when first external dependency lands)

| File | Purpose |
|------|---------|
| `supply-chain/config.toml` | `cargo-vet` trust imports and thresholds. |
| `supply-chain/audits.toml` | Local audits. |
| `.github/dependabot.yml` | Dependency PR automation (to be enabled once standards are enforced in CI). |

The `supply-chain/` directory does not exist at HEAD — see [add-dependency](../../.claude/skills/add-dependency/SKILL.md) for the trigger that creates it.

## Anti-patterns to reject

- Upgrading the toolchain silently in an unrelated PR.
- Using `cargo update` as routine housekeeping.
- Adding a dependency without justification or audit.
- Copy-pasting a `rustfmt::skip` without an explanation.
- Running CI with relaxed gates just to unblock a merge.
- Storing secrets in the repo, even encrypted.
- Self-hosted runners running code from untrusted PRs.

## References

- `cargo-vet`: https://mozilla.github.io/cargo-vet/
- RustSec Advisory Database: https://rustsec.org/
- Reproducible Builds project: https://reproducible-builds.org/
- CycloneDX SBOM: https://cyclonedx.org/
- Rust `rust-toolchain.toml`: https://rust-lang.github.io/rustup/overrides.html
