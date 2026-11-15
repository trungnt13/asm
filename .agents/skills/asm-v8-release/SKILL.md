---
name: asm-v8-release
description: Build or publish the ASM V8 dependency when no verified matching release exists. Use for V8 prerequisite preparation, not CLI publication.
---

# V8 Release

Read the [fork guide](../../../docs/porting-from-codex.md#v8-dependency-release) first. It remains authoritative.

Read the guide’s V8 contracts before dependency work. They control reuse, target coverage, ABI proof, asset verification, and publication. Skill use grants no publication authority.

Follow this procedure and the guide’s gates. Skill use does not expand task scope or authorization.

## Procedure

Build V8 separately only when the exact version/target lacks a verified fork pair. Use [`fork-v8-release.yml`](../../../.github/workflows/fork-v8-release.yml), selecting one target per run:

- Linux x86_64 MUSL (default): `asm-v8-v<exact resolved v8 crate version>-musl.1`.
- macOS ARM64: retain `asm-v8-v<exact resolved v8 crate version>-glibc2.35.2`. This historical generation name identifies the verified macOS artifacts; it does not build or publish GNU assets.

Reuse the existing verified macOS pair when its inputs remain compatible. Preserve historical GNU/MUSL assets. Never reuse a GNU archive for MUSL or substitute an upstream pair without the fork guide's compatibility proof.

- Manual branch dispatch defaults to build-only. Explicit `publish=true` builds/smokes, then tags/publishes only the selected target's archive, binding, and manifest. Tag pushes infer the target from the generation. Tag moves or published-asset replacement need approval.
