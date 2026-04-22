# Release

How Tyrne is versioned, how changes accumulate into a release, what must be true for a release to happen, and how the release is cut and published. This standard is forward-looking: Tyrne is pre-alpha and has not yet cut its first release. The standard is written now so that when the first release approaches, the process is decided, not improvised.

## Scope

- Versioning scheme.
- Changelog discipline.
- Release gates — what must pass to cut a release.
- Release artifacts — what is produced and where it lives.
- Signing and verification.
- Rollback policy.

## Versioning

Tyrne uses **semantic versioning** with an Tyrne-specific convention during the pre-1.0 period:

- `0.x.y` means **pre-alpha / alpha / beta**. No stability guarantees. Any minor bump may break ABI, capability semantics, or behavior.
- `1.x.y` is the first release that commits to a stable kernel ABI for userspace. That boundary is cut only when the maintainer judges the kernel ABI to be worth committing to; it is not tied to a calendar.
- After 1.0:
  - `MAJOR` — breaking changes to the kernel-to-userspace ABI, to capability semantics, or to the boot protocol.
  - `MINOR` — additive changes that do not break existing callers.
  - `PATCH` — bug fixes, security fixes, internal changes invisible across the ABI.

A **build metadata** suffix may be appended per [semver §10](https://semver.org/#spec-item-10) for CI-built artifacts that are not intended for public consumption (e.g. `0.3.1+git.abc1234`).

## Release cadence

- No fixed cadence. Releases happen when a coherent body of work is ready.
- In pre-alpha, milestone releases (0.1, 0.2, …) correspond to architecturally meaningful checkpoints: "first kernel boot on QEMU", "first IPC round-trip", "first real-hardware boot on Pi 4", etc.
- Security releases cut as soon as a fix is ready; they do not wait for the next milestone.

## Changelog

- Maintained in [`CHANGELOG.md`](../../CHANGELOG.md) at the repository root (to be created with the first release).
- Follows **[Keep a Changelog](https://keepachangelog.com/en/1.1.0/)** v1.1.0 format.
- One section per released version, in reverse chronological order.
- An `[Unreleased]` section at the top accumulates entries between releases; contributors add to it as part of their PRs.

### Entry categories

- **Added** — new features, subsystems, targets.
- **Changed** — changes in existing behavior (including ABI breaks in 0.x).
- **Deprecated** — soon-to-be-removed features.
- **Removed** — features removed in this release.
- **Fixed** — bug fixes.
- **Security** — security-relevant changes or fixes; links to advisories if any.

### Entry quality

- Each entry is one sentence, in the imperative or declarative voice.
- References the PR / issue number: `[#42]`.
- For `Security` entries, links to any CVE or advisory; if the fix was pre-disclosure, the CVE is filled in after disclosure.

## Release gates

Before a version is tagged, **every one of these must be true**. A gate failure is a hard stop — the release is deferred or the blocker is fixed.

### Process gates

- [ ] All CI gates green on the commit being released (format, clippy, tests, build, QEMU smoke, `cargo-audit`, `cargo-vet`).
- [ ] Hardware smoke tests passed on every Tier 2+ target that this release claims to support.
- [ ] No `#[ignore]`d tests without a tracking issue.
- [ ] No open `Security` advisories that this release does not fix.
- [ ] `Cargo.lock` committed and clean (no uncommitted changes).
- [ ] Toolchain pinned; toolchain unchanged since the previous release or upgrade ADR linked.

### Content gates

- [ ] `CHANGELOG.md` `[Unreleased]` section moved to a version header with release date.
- [ ] Every non-trivial ADR added since the last release is Accepted (no `Proposed` or `Deprecated` in limbo).
- [ ] Every `Security` entry in the changelog has a linked advisory or a note explaining why one is not needed.
- [ ] Version number bumped in `Cargo.toml` workspace root and propagated to crate manifests.
- [ ] Documentation links that break at the version boundary are updated (`latest` pointer vs. version-pinned docs, when docs are hosted).

### Security gates

- [ ] `unsafe` audit log walked: every entry has a valid `SAFETY:` comment at the referenced location; no undocumented `unsafe`.
- [ ] `cargo-geiger` diff reviewed; unusual `unsafe` count deltas explained.
- [ ] SBOM generated and compared to the previous release's SBOM; diff reviewed.
- [ ] No proprietary binary blobs in the artifact tree (see [architectural-principles.md](architectural-principles.md), P7).

## Cutting a release

The release process is a single branch + tag sequence:

1. **Stabilize.** Ensure `main` is green, all gates pass.
2. **Bump version** in `Cargo.toml`.
3. **Update `CHANGELOG.md`** — move `[Unreleased]` to a version header, add release date, link headers.
4. **Commit** the version bump and changelog as one commit: `chore(release): 0.3.0`.
5. **Tag** the commit: `git tag -s v0.3.0 -m "Tyrne 0.3.0"`. Tags are signed.
6. **Push** the tag: `git push origin v0.3.0`.
7. **Publish artifacts** (see below).
8. **Open `[Unreleased]` section** on `main` for the next release with a follow-up commit.

## Release artifacts

For each Tier 1 and Tier 2 target, the release produces:

- `tyrne-<target>-<version>.elf` — kernel image.
- `tyrne-<target>-<version>.bin` — kernel binary for boards that boot from raw binaries.
- `tyrne-<target>-<version>.img` — full bootable image where applicable (e.g. SD card image for Raspberry Pi).
- `tyrne-<target>-<version>.sha256` — checksum manifest.
- `tyrne-<target>-<version>.sig` — detached signature.
- `tyrne-<version>-sbom.cdx.json` — CycloneDX SBOM for the release as a whole.
- `CHANGELOG-<version>.md` — changelog excerpt for this version.

Artifacts are attached to the GitHub release that corresponds to the signed tag.

## Signing

- Tags and release artifacts are signed with the maintainer's long-term key.
- The public key is published in `docs/release-signing.md` (to be added with the first release) with a fingerprint and an out-of-band verification path.
- Rotation: if the signing key must change, a dedicated release notes rotation and transition with both old and new keys announcing the next release.

## Rollback

- A shipped release is not unshipped. If a release turns out to contain a critical flaw, a **superseding patch release** is cut.
- A tag is never deleted or re-pointed. If the tagged commit is wrong, a new tag fixes it forward.
- Release artifacts are not deleted from the GitHub release page unless they expose a secret. In that case, the release page is edited to warn users and the replacement is published.

## Security releases

Security releases follow the normal release flow with modifications:

- The PR that carries the fix is reviewed per [security-review.md](security-review.md).
- If the fix is pre-disclosure, the PR is prepared on a private branch and merged at disclosure time.
- The release notes carry the `Security` entry with the CVE / advisory link.
- The release is announced on the project's public channels (once those exist) with a severity label.

## Anti-patterns to reject

- Releasing without all gates green, even for a "small fix".
- Releasing without a changelog entry.
- Re-tagging a release under the same version after a mistake.
- Mixing security and non-security fixes in the same release unless the security fix cannot be extracted.
- Unsigned tags or artifacts.
- Silent dependency bumps at release time.

## Tooling

- `git tag -s` for signed tags.
- `cargo-release` (optional) for version bumps and changelog moves — may be adopted; not required.
- SBOM generator via `cargo-cyclonedx`.
- GitHub Releases for artifact hosting.

## References

- Semantic Versioning 2.0.0: https://semver.org/
- Keep a Changelog 1.1.0: https://keepachangelog.com/en/1.1.0/
- CycloneDX: https://cyclonedx.org/
- Reproducible Builds: https://reproducible-builds.org/
