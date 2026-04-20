# Security policy

Umbrix is a security-oriented operating system project. Even while it is in pre-alpha, we want to handle security observations carefully.

## Project status and guarantees

Umbrix is **pre-alpha**. There is no runnable kernel yet. No production use is supported, and no security guarantees are made for the current tree. The formal threat model is a work in progress and will be documented in `docs/architecture/security-model.md` (planned, Phase 2).

## Reporting a security issue

Until a dedicated disclosure channel is set up, please report security-relevant observations by opening a **private security advisory** on GitHub:

https://github.com/cemililik/UmbrixOS/security/advisories/new

Do not open a public issue for anything that looks like it might be security-sensitive, even in this early phase.

Where possible, include:

- A description of the observation and the affected file(s), commit(s), or ADR(s).
- Reasoning about why it is a risk — the threat, the assumed attacker capability, the affected assets.
- A suggested mitigation, if you have one.

## Scope

Everything in the `UmbrixOS` repository is in scope. Third-party dependencies are reviewed upstream; reporters are encouraged to also notify the upstream project when the root cause lives there.

## Disclosure

Because there are no production deployments yet, the current policy is **fix first, disclose later**. As the project matures, this policy will be revised and published here with explicit timelines and coordination expectations.

## For AI agents

If during any code or document review an AI agent notices something that plausibly weakens a security property (a removed capability check, a new ambient authority, a silenced security test, an undocumented `unsafe` block, or a proprietary blob entering the tree), the agent should **stop, flag the observation to the maintainer, and not proceed with the change**.
