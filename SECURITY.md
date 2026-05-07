# Security policy

Tyrne is a security-oriented operating system project. Even while it is in pre-alpha, we want to handle security observations carefully.

## Project status and guarantees

Tyrne is **pre-alpha**. The kernel boots end-to-end on QEMU virt aarch64 (Phase A + B0/B1 closed) and runs a two-task IPC demo through to completion, but it is not yet a userspace-bearing OS — no production use is supported, and no security guarantees are made for the current tree. The formal threat model is documented in [`docs/architecture/security-model.md`](docs/architecture/security-model.md) (Accepted) and refined as Phase B progresses; both the model and the codebase will continue to evolve until the project reaches a stable release.

## Reporting a security issue

Until a dedicated disclosure channel is set up, please report security-relevant observations by opening a **private security advisory** on GitHub:

https://github.com/cemililik/Tyrne/security/advisories/new

Do not open a public issue for anything that looks like it might be security-sensitive, even in this early phase.

Where possible, include:

- A description of the observation and the affected file(s), commit(s), or ADR(s).
- Reasoning about why it is a risk — the threat, the assumed attacker capability, the affected assets.
- A suggested mitigation, if you have one.

## Scope

Everything in the `Tyrne` repository is in scope. Third-party dependencies are reviewed upstream; reporters are encouraged to also notify the upstream project when the root cause lives there.

## Disclosure

Because there are no production deployments yet, the current policy is **fix first, disclose later**. As the project matures, this policy will be revised and published here with explicit timelines and coordination expectations.

## For AI agents

If during any code or document review an AI agent notices something that plausibly weakens a security property (a removed capability check, a new ambient authority, a silenced security test, an undocumented `unsafe` block, or a proprietary blob entering the tree), the agent should **stop, flag the observation to the maintainer, and not proceed with the change**.
