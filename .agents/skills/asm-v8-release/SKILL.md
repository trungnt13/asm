---
name: asm-v8-release
description: Build or publish the ASM V8 dependency when no verified matching release exists. Use for V8 prerequisite preparation, not CLI publication.
---

# V8 Release

Read the [fork guide](../../../docs/porting-from-codex.md#v8-dependency-release) first. It remains authoritative.

Read the guide’s V8 contracts before dependency work. They control reuse, target coverage, ABI proof, asset verification, and publication. Skill use grants no publication authority.

Follow this procedure and the guide’s gates. Skill use does not expand task scope or authorization.

## Procedure

Build V8 separately only when version/target/baseline lacks a verified fork release. Use [`fork-v8-release.yml`](../../../.github/workflows/fork-v8-release.yml) and `asm-v8-v<exact resolved v8 crate version>-glibc2.35.2`. The suffix identifies the GNU baseline generation and matching macOS pair. Preserve the old MUSL release.

- Manual branch dispatch defaults to build-only. Explicit `publish=true` builds/smokes, then tags/publishes at that commit. Tag pushes also publish. Tag moves or published-asset replacement need approval.
