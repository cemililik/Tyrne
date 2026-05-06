# Track J — Localization & hygiene

- **Agent run by:** Claude general-purpose agent, 2026-05-06
- **Scope:** Umbrix→tyrne residue, English-only enforcement, naming, phantom symbols, TODO/FIXME audit.
- **HEAD reviewed:** 214052d

## Umbrix residue scan

Commands run:

```sh
git grep -i 'umbrix' -- ':!docs/analysis/technical-analysis/'
grep -ri 'umbrix' /Users/dev/Documents/Projects/OS-Project/ \
  --exclude-dir=target --exclude-dir=.git --exclude-dir=technical-analysis
find . -name '*umbrix*' -not -path '*/.git/*' -not -path '*/target/*'
```

Files with `umbrix` (case-insensitive, excluding `.git/`, `target/`, `docs/analysis/technical-analysis/`):

- [`docs/analysis/reviews/business-reviews/2026-04-28-B1-closure.md`](../../business-reviews/2026-04-28-B1-closure.md), lines 139 + 152 — both occurrences are inside prose narrating the *history* of the 2026-04-22 Umbrix → Tyrne rename and the 2026-04-28 follow-up sweep. Legitimate historical references; not residue.
- [`docs/analysis/reviews/code-reviews/2026-05-06-full-tree-comprehensive-review-plan.md`](../2026-05-06-full-tree-comprehensive-review-plan.md) — four mentions, all describing this Track J's own scope (and the glossary track's "no orphaned entries from the umbrix→tyrne rename" check). Self-referential / scoping prose; not residue.
- One filename match — `docs/analysis/technical-analysis/WOSR/12-comparison-with-umbrix.md` — sits inside the explicitly out-of-scope `technical-analysis/` subtree (per the review plan's §2 carve-out).

**Verdict on the residue scan: clean.** Commit [`10e3351`](https://github.com/cemililik/Tyrne/commit/10e3351) closed the file-name half of the rename. The four lines that still mention "Umbrix" are all narrative, not link or identifier residue.

## Repository-URL drift (scope cousin to umbrix residue)

While verifying the umbrix scan I noticed a **second, distinct** rename inconsistency that the `10e3351` "stale-link rename" sweep did *not* address: the `cemililik/<repo-name>` URL form embedded in 64 source/doc cross-references is `TyrneOS`, while four newer references (all written after PR #10) use `Tyrne` instead, and the local git remote points at a third stale form, `UmbrixOS`. Concretely:

| Form | Count (excluding `target/`, `.git/`, `technical-analysis/`) | Where |
|---|---|---|
| `https://github.com/cemililik/TyrneOS` | 66 (across 27 files) | `Cargo.toml`, `SECURITY.md`, every `tyrne-hal` / `tyrne-kernel` / `tyrne-bsp-qemu-virt` rustdoc cross-reference, `docs/guides/run-under-qemu.md`'s clone command |
| `https://github.com/cemililik/Tyrne` | 4 | recent docs only — `T-012-exception-and-irq-infrastructure.md`, `2026-04-28-B1-closure.md` (business + security), `current.md` |
| `git@github.com:cemililik/UmbrixOS.git` (origin) | 1 | local `git remote -v` (not in working tree) |

The 64 `TyrneOS` URLs all 404 against the actual remote (whatever the canonical name is — `git remote -v` and the four newest references disagree). Track A's cross-track note (`track-a-kernel.md` line 85) already observed "every link points at `cemililik/TyrneOS`" but flagged it as a clean-state observation rather than a finding; in fact none of those URLs resolve. Cross-track note coordinates with Track G (root-doc link integrity) and Track H (rustdoc cross-references) on whether to fix in this review window or hand off as a follow-up commit. See *Findings → Non-blocking → J-NB1* below.

Not in scope to fix here (Auto-mode rules + CLAUDE.md non-negotiable rule #1 "respect the pace" + "NEVER update the git config" both argue for surfacing rather than batch-rewriting).

## Phantom-symbol scan

Workspace-level lint policy in `Cargo.toml:36–39` already addresses phantom `pub` items:

```toml
unreachable_pub = "warn"
unused_must_use = "deny"
missing_docs = "warn"
```

Track A's verdict (kernel-build, kernel-clippy, host-clippy, fmt all clean at HEAD `214052d`) implicitly confirms zero `unreachable_pub` warnings across the kernel and HAL crates. No further phantom-`pub` audit needed for this track.

`#[allow(dead_code, ...)]` reason fields — six occurrences across [kernel/src/obj/notification.rs](../../../../kernel/src/obj/notification.rs#L65), [kernel/src/obj/endpoint.rs](../../../../kernel/src/obj/endpoint.rs#L53), [kernel/src/obj/task.rs](../../../../kernel/src/obj/task.rs#L60), and the seven `#[cfg(test)]` test-module blocks each with `reason = "tests may use pragmas forbidden in production kernel code"` — all checked. The two "symmetric with `TaskHandle::test_handle`" reasons are truthful: `TaskHandle::test_handle` exists at [kernel/src/obj/task.rs:60](../../../../kernel/src/obj/task.rs#L60) and is referenced from `kernel/src/cap/table.rs:622` and `kernel/src/ipc/mod.rs:541`. The `EndpointHandle::test_handle` and `NotificationHandle::test_handle` siblings are intentionally unreferenced for symmetry — defensible scaffolding, reason field accurate.

## TODO/FIXME/HACK inventory

Command run:

```sh
grep -rn -E '// (TODO|FIXME|HACK|XXX)' \
  kernel/src hal/src test-hal/src bsp-qemu-virt/src
grep -rn -i -E '(TODO|FIXME|HACK|XXX)' \
  kernel hal test-hal bsp-qemu-virt
```

**Result: zero hits** across both passes (Rust line comments, doc comments, `.s`, `.ld`, `.toml`).

| File:line | Tag | Has reference (task/ADR)? | Note |
|---|---|---|---|
| *(none)* | — | — | The kernel crate explicitly bans them via [`#![deny(clippy::todo)]`](../../../../kernel/src/lib.rs#L41); the absence of any `TODO` / `FIXME` / `HACK` is therefore a positive lint-enforced property, not just author discipline. |

Empty TODO/FIXME slate is unusually clean for a 1.5-month-old kernel codebase and a healthy signal.

## English-only spot-check (per [docs/standards/localization.md](../../../../docs/standards/localization.md))

Source / `Cargo.toml` / `.s` / `.ld` files: clean. Console output strings (`bsp-qemu-virt/src/main.rs:302/338/394/448/451/468/526/611/711/734`) use lowercase `tyrne:` as a deliberate program-banner convention, not a noun in prose — fine.

Sole non-ASCII character in source/manifests: `authors = ["Cemil İlik"]` in [Cargo.toml:27](../../../../Cargo.toml#L27). Author name spelling — out of scope of localization.md (rule #2 covers kernel-produced strings, not author identifiers).

`docs/standards/localization.md:88` contains the Turkish anti-pattern example `"işlem başarısız"` — pedagogical use, expressly inside the *Anti-patterns to reject* section. Fine.

**Possible policy deviation:** the Turkish severity adjective `Yüksek` ("High") is used as a defined severity label in seven committed English documents:

| File:line | Use |
|---|---|
| [`docs/analysis/tasks/phase-b/T-009-timer-init-cntvct.md:109`](../../../../docs/analysis/tasks/phase-b/T-009-timer-init-cntvct.md) | "Second-read review surfaced three Yüksek findings…" |
| [`docs/analysis/tasks/phase-b/T-009-timer-init-cntvct.md:110`](../../../../docs/analysis/tasks/phase-b/T-009-timer-init-cntvct.md) | "Review 1's Yüksek #1 was only half addressed…" |
| [`docs/analysis/reviews/business-reviews/2026-04-27-B0-closure.md:140`](../../business-reviews/2026-04-27-B0-closure.md) | "Verdict clean — no Yüksek findings…" |
| [`docs/analysis/reviews/security-reviews/2026-04-27-B0-closure.md:109`](../../security-reviews/2026-04-27-B0-closure.md) | "no Yüksek findings…" |
| [`docs/analysis/reviews/security-reviews/README.md:35`](../../security-reviews/README.md) | "Clean — no Yüksek findings…" |
| [`docs/audits/unsafe-log.md:263`](../../../../docs/audits/unsafe-log.md) | "Closes the runtime-check half of Review 1's Yüksek #1…" |
| [`docs/audits/unsafe-log.md:271`](../../../../docs/audits/unsafe-log.md) | "the second-read review's Yüksek #1 explicitly asked for…" |

(One additional hit — [`docs/analysis/reviews/business-reviews/2026-04-27-T-009-mini-retro.md:29`](../../business-reviews/2026-04-27-T-009-mini-retro.md) — quotes a historical commit-message subject literally, so it's an immutable artefact and out of scope.)

`Yüksek` is being used as a current, repeated severity term for security-review findings, while [`docs/standards/security-review.md`](../../../../docs/standards/security-review.md) and [`docs/standards/code-review.md`](../../../../docs/standards/code-review.md) use only English severity labels (Critical / High / Medium / Low). Localization.md rule #2 ("Kernel-produced strings are English"), ADR-0005, and the localization.md §6 restatement ("Committed artifacts: English") collectively suggest English replacements (`High`, `Yüksek finding` → `High finding` / `Critical finding`) would be more compliant. See *Findings → Non-blocking → J-NB2*.

Recent commit messages spot-checked (last 50): all English except commit `db3a4c7` ("fix(bsp,docs): close R1 Yüksek #1 — runtime EL check missing from 39fb66c") — same `Yüksek` appearing in an immutable commit subject. Cannot be retroactively fixed without history rewrite; flagged as cause-and-effect of the standing convention rather than as a separate finding.

## Naming consistency

Convention observed across the tree:

- Proper-noun prose: **`Tyrne`** (capital T, never `TyrneOS`, `tyrne`, or `TYRNE` in prose).
- Crate names: **`tyrne-kernel`**, **`tyrne-hal`**, **`tyrne-test-hal`**, **`tyrne-bsp-qemu-virt`** (kebab-case lowercase per Cargo convention).
- Module-path identifier: `tyrne_hal::timer::ticks_to_ns` (snake-case) — appears once at [`docs/analysis/tasks/phase-b/T-009-timer-init-cntvct.md:109`](../../../../docs/analysis/tasks/phase-b/T-009-timer-init-cntvct.md#L109); correct Rust path form for the `tyrne-hal` crate.
- Kernel banner output: `b"tyrne: ..."` (lowercase, as a fixed banner prefix in serial output) — deliberate, parallels the Linux `linux: ...` printk banner convention; not a prose use.

No `TyrneOS` anywhere as a prose name — the only `TyrneOS` strings are the stale repo-URL captured under *Repository-URL drift* above. Naming convention itself is uniform.

## Spelling spot-check

Common typo dictionary (`teh|recieve|seperate|occured|dependant|definately|accomodate|priviledge|priviliege|comitted|committment|untill|seperately|begining|writting`) checked against `README.md`, `CLAUDE.md`, `AGENTS.md`, `SECURITY.md`, `CONTRIBUTING.md`, `docs/decisions/0024-el-drop-policy.md`, `docs/decisions/0025-adr-governance-amendments.md`. Zero hits.

## Findings

### Blocker

*(none)*

### Non-blocking

#### J-NB1 — 64 stale `TyrneOS` rustdoc / manifest URLs

Severity: **Non-blocking** — the URLs do not affect compilation or test outcomes; they only break when a reader clicks an `intra-doc` rustdoc link. But every cross-reference from `tyrne-kernel` → ADR-0014 / ADR-0016 / standards / etc. is broken at the moment the docs render.

Recommended action: a follow-up `docs(refs):` commit (single mechanical sweep, very similar in shape to `10e3351`) replacing `cemililik/TyrneOS` with the canonical form. Resolving the canonicality first is a precondition: `git remote -v` says `UmbrixOS`, four newest docs say `Tyrne`, 64 older references say `TyrneOS`. Whichever the maintainer picks needs to be applied to the remote name *and* the in-tree references, then captured in an ADR-style note (e.g., the same retro section as the `10e3351` rename, or an Amendment to that retro).

Files with the old form (27 of them) are listed in §"Repository-URL drift" above. Hand-off to follow-up commit, not in scope to fix here.

#### J-NB2 — Turkish severity term `Yüksek` in eight committed English docs

Severity: **Non-blocking** — the term is a single word, semantically clear from context, and `docs/standards/localization.md` does not call it out by name. But:

- ADR-0005 + localization.md rules #2 and #6 make English the committed-artefact language without exception.
- `docs/standards/security-review.md` and `docs/standards/code-review.md` use the English vocabulary (`Critical` / `High` / `Medium` / `Low`).
- A reader using the project as a reference for *their own* security-review template will see two competing severity vocabularies in the same review tree.

Recommended action: pick one English replacement (`High` is the natural carry-over) and apply it across the seven affected non-quoted references. The two commit-message-quoted instances ([`db3a4c7`](https://github.com/cemililik/Tyrne/commit/db3a4c7) and the [T-009 mini-retro line 29 quote](../../business-reviews/2026-04-27-T-009-mini-retro.md)) cannot be retroactively edited and should be left as-is. A small follow-up `docs(localization):` commit is the most compact framing.

### Observation

#### J-OBS1 — TODO/FIXME-free codebase enforced by lint, not just discipline

The kernel crate's [`#![deny(clippy::todo)]`](../../../../kernel/src/lib.rs#L41) at HEAD `214052d` is an active deny-lint, meaning a `// TODO` comment in any kernel module would fail the `kernel-clippy` gate. Combined with the zero hits from the recursive `(TODO|FIXME|HACK|XXX)` scan, the project's hygiene posture here is structurally maintained, not author-by-author. Worth preserving when the lint policy is migrated into a workspace-level `[workspace.lints]` block (currently only `kernel/src/lib.rs` carries it; `hal`, `test-hal`, and `bsp-qemu-virt` do not — minor gap, not raised as a non-blocking finding because the lint is genuinely targeted at the kernel proper).

#### J-OBS2 — `#[allow(dead_code)]` reason fields are uniformly accurate

Six suppressions checked; reason text matches the underlying invariant in every case. The "symmetric with `TaskHandle::test_handle`" reason is verifiable (`TaskHandle::test_handle` exists; the matching test-only constructors are deliberate scaffolding, not dead leftovers). No drift between reason fields and reality.

## Cross-track notes

- → Track A: confirms (and partially extends) Track A's line 85 cross-track note — every kernel-crate rustdoc link points at `cemililik/TyrneOS`, but 64 of them, not "spot-checked", and they are *broken* against the actual remote rather than benign. Together with this report's *J-NB1*, the two tracks form a complete picture of the URL-drift surface.
- → Track G (root-doc link integrity): the same `cemililik/TyrneOS` URL appears in `Cargo.toml` and `SECURITY.md`, both of which Track G covers. Track G should likely escalate the URL drift one notch above what it would on its own merit, since it's not a single broken link but a 27-file fan-out.
- → Track H (rustdoc cross-references): every `tyrne-kernel` / `tyrne-hal` / `tyrne-test-hal` / `tyrne-bsp-qemu-virt` rustdoc footer link is part of the broken set; if Track H runs `cargo doc --no-deps` and tries to follow links, it'll find the same 64-hit count.
- → Tracks B (HAL crate) and C (BSP crate): the broken URLs are concentrated here too — flagging early so neither track raises the issue independently.
- → Track I (workspace-level standards): `#![deny(clippy::todo)]` is only set on the kernel crate. If the standards track examines workspace-level lint policy completeness, lifting it to `[workspace.lints.clippy]` would remove a small surface-area gap. Not flagging here as a non-blocking on its own.

## Sub-verdict

**Comment.** No blockers; two non-blocking items (J-NB1 = 64 stale `TyrneOS` URLs, J-NB2 = `Yüksek` in committed English docs) merit a follow-up `docs(refs):` and `docs(localization):` commit pair. The umbrix→tyrne residue itself is *clean* — the four `Umbrix` mentions left in the tree are all legitimate historical narrative. Naming, phantom symbols, TODO/FIXME hygiene, spelling, and code-comment language are all clean as-is.
