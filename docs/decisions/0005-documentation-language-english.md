# 0005 — English as the documentation and code language

- **Status:** Accepted
- **Date:** 2026-04-20
- **Deciders:** @cemililik

## Context

Umbrix is maintained by a native Turkish speaker and is being developed on a public GitHub repository. The maintainer is more fluent in Turkish for deep technical discussion and often uses AI assistance in Turkish. At the same time, the repository is open to the world, and the project aspires to attract external contributors and to remain readable to future maintainers (human or AI) who do not speak Turkish.

The question is: *in what language is the repository written?*

## Decision drivers

- **International discoverability.** Most open-source OS development happens in English — search results, Stack Overflow answers, Rust documentation, and research papers are predominantly English.
- **Contributor reach.** Limiting the repository to Turkish effectively limits contributions to Turkish speakers, which is a small fraction of potential OS-interested contributors worldwide.
- **Consistency within the repository.** Mixing languages inside a single codebase (some docs in Turkish, some in English) creates confusion, duplicates maintenance burden, and fragments search.
- **Maintainer fluency in chat.** The maintainer works fastest in Turkish when discussing design in real time. Forcing Turkish-to-English translation on every chat message would reduce design throughput.
- **AI agent interoperability.** Agents consume both chat and repository text. If the repository is English, agents reading the repo have unambiguous context; if chat is Turkish, the agent replies in Turkish, which is more natural for the maintainer.

## Considered options

1. **English only, everywhere** (chat, docs, code, commits).
2. **Turkish only, everywhere.**
3. **Bilingual documentation** (each doc written in both Turkish and English).
4. **Split: Turkish in chat, English in the repository** (all committed artifacts).

## Decision outcome

**Chosen: Turkish in chat, English in the repository.**

Everything committed to the repository — source code, comments, doc-comments, documentation (`docs/**`), ADRs, READMEs, commit messages, PR descriptions, issue text, and AI-agent guide files — is written in English. Conversation between the maintainer and AI agents (or other collaborators) is in Turkish by default and is not committed.

This split maximizes the maintainer's design throughput (fast Turkish discussion) while keeping the public artifacts accessible to the global open-source community and to future contributors whose native language may not be Turkish.

Bilingual documentation was rejected because it doubles maintenance cost, produces inconsistencies over time as the two versions drift, and because there is no third-language community that we should be paying that cost for.

## Consequences

### Positive

- Anyone on Earth with English literacy can read and contribute to Umbrix.
- Search discoverability on English-dominated platforms (GitHub, search engines, Stack Overflow) is preserved.
- AI agents working from the repository have unambiguous context in a well-supported language.
- The maintainer can still think and discuss in Turkish, which is faster for deep reasoning.

### Negative

- **The maintainer does more translation work than if the repo were in Turkish.** Mitigation: AI agents can assist with translation during drafting. The maintainer reviews the English output.
- **Some nuance may be lost.** Occasionally a Turkish idiom captures something English lacks; in those cases, prefer plain description over forced translation.
- **Commit messages in English demand discipline when committing quickly.** Mitigation: short commits are fine; detail can go in the ADR if needed.

### Neutral

- Onboarding Turkish-only contributors (if any) requires them to read English docs, which is the same expectation any other open-source project would make.
- Chat transcripts, if ever published as blog posts or case studies, will need translation. That cost is paid at publication time, not at development time.

## Pros and cons of the options

### English only, everywhere

- Pro: maximum simplicity, no language-switching.
- Con: slows the maintainer in chat; loses the natural-language advantage for deep discussion.

### Turkish only, everywhere

- Pro: no translation cost for the maintainer.
- Con: globally invisible and uncontributable; forecloses the possibility of external collaboration.

### Bilingual docs

- Pro: inclusive of both audiences.
- Con: double maintenance, drift, inconsistency.

### Split: Turkish chat, English repository

- Pro: fastest design velocity for the maintainer + international accessibility of the artifact.
- Con: one minor bit of context-switching discipline required.

## References

- Most major open-source operating systems (Linux, BSDs, seL4, Redox, Fuchsia, Tock, Hubris) use English throughout their repositories regardless of maintainer origin — pragmatic precedent.
- GitHub documentation language norms.
