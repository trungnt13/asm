# Fork purpose and upstream sync

## Principles

1. **Personal use only.** This fork exists solely for Trung Ngo's personal use.
2. **Keep changes minimal, safe, and minor.** Assume unchanged upstream code has already been tested upstream. Run only narrow, fast checks for fork changes and conflict resolutions. Do not repeat upstream testing or run full suites by default. This assumption is not proof that a particular upstream commit passed CI. If a change needs broad testing to establish safety, stop and clarify its scope with the owner.
3. **Keep the owner's current intent in this file.** Whenever changing the fork, record the purpose and constraints here. If a proposed change conflicts with recorded intent, ask the owner before proceeding. Once accepted, replace the old intent rather than keeping contradictory rules or a change log. With intent clear, agents may triage changes, choose a sync method, and resolve merge or rebase conflicts within that scope without asking about each conflict.

The sections below record current intent. Update the relevant section when intent changes; do not add a duplicate policy. Resolving a conflict does not authorize changing requirements, discarding user work, rewriting published history, or publishing a release.

## Upstream baseline

- Fork (`origin`): [`trungnt13/codex`](https://github.com/trungnt13/codex).
- Parent (`upstream`): [`openai/codex`](https://github.com/openai/codex).
- Last incorporated upstream commit: `63eb71c9da95a6e4deaf34ee9340c88a7cac9a65`.
- Commit date: 2026-09-25.
- Subject: Preserve late result metadata for truncated code-mode calls (#48222).

After a full sync, replace this marker with the exact upstream commit incorporated. A targeted cherry-pick does not advance the full-sync baseline. Verify the marker with `git show -s --format='%H%n%cs%n%s' <upstream-commit>`.

## Sync and conflict decisions

Before editing, inspect Git status, the branch, remotes, and the fork diff. Treat existing changes as user-owned. Do not reset, clean, stash, overwrite, or revert unrelated work. Use a separate worktree if needed to keep it untouched.

Fetch the intended upstream refs after checking the remote URL. Compare the recorded baseline with the selected upstream commit, not a stale example range. Focus on fork differences and the upstream changes that affect them; do not audit unchanged upstream code again.

Use a merge when preserving published history matters. A rebase is acceptable for an unpublished branch. For targeted ports, use `git cherry-pick -x` to retain the source commit. Use `codex/` for new branch names. Ask before rebasing or force-pushing published `main`, amending published commits, or moving release tags.

Resolve conflicts against the current intent in this file. Compare the common base, fork, and upstream versions when needed. Keep useful upstream fixes without restoring deliberately omitted platforms or infrastructure. Ask only when the resolution would change intent or the evidence does not settle the choice.

This file takes precedence over all other repository instructions, including [AGENTS.md](../AGENTS.md), where they conflict. Follow their remaining implementation rules. Keep fork-specific logic small and avoid unrelated refactors. Update generated schemas, snapshots, lockfiles, or Bazel data declarations only when the fork change requires them. Imported upstream changes alone do not justify regenerating or retesting everything.

## Platform focus

This fork targets only macOS and Linux (Ubuntu). Keep fork changes and validation focused on these platforms. Retain inherited code and workflow files for other platforms, but do not add those platforms to the fork CI or release workflows. The release targets below remain macOS ARM64 and Linux x86_64 MUSL.

## Release intent

Keep the fork release workflow separate from upstream's release workflow to reduce conflicts. See [`fork-rust-release.yml`](../.github/workflows/fork-rust-release.yml).

Build only these targets on GitHub-hosted runners:

- macOS ARM64: `aarch64-apple-darwin` on `macos-15`.
- Linux x86_64 MUSL: `x86_64-unknown-linux-musl` on `ubuntu-24.04`.

Each `codex-<target>.tar.gz` contains only the `codex` executable. Publish both archives and `SHA256SUMS`.

Preserve these constraints:

- macOS stays unsigned and unnotarized. No paid Apple membership, Apple credentials, or Azure Key Vault.
- No repository release secrets or self-hosted runners.
- Linux stays on MUSL. Do not bundle Bubblewrap; install `bwrap` on the host when needed for sandboxing.
- Keep Zig and [`install-musl-build-tools.sh`](../.github/scripts/install-musl-build-tools.sh) for Linux native dependencies, including the AWS-LC no-jitter settings in the workflows.
- Keep [`setup-rusty-v8`](../.github/actions/setup-rusty-v8/action.yml) for the verified prebuilt V8 artifacts.
- Keep the workspace version at `0.0.0` in [`Cargo.toml`](../codex-rs/Cargo.toml). Release tags do not rewrite Cargo files.

Do not add other platforms, DMGs, bundled resources, npm, R2, WinGet, website publishing, signing, or OpenAI-only publishing infrastructure without an agreed change of intent.

### Tags and publication

Fork releases use `v*`; upstream Rust releases use `rust-v*.*.*`.

- A manual dispatch from a branch builds artifacts without publishing a GitHub Release.
- A `v*` tag publishes both archives and checksums.
- Tags with a hyphenated suffix are prereleases and are not Latest.
- Plain tags are normal releases and are marked Latest.

The release job checks the ref, not the event name. A manual dispatch on a `v*` tag can publish too. Do not push a release tag or dispatch on one without publication approval.

When release validation is requested, check the relevant existing run and artifacts first. Do not repeat builds merely to recheck unchanged packaging. After an authorized release, verify both archives and `SHA256SUMS` are present.

## CI intent

Keep the inherited [`blocking-ci.yml`](../.github/workflows/blocking-ci.yml) and [`postmerge-ci.yml`](../.github/workflows/postmerge-ci.yml) customized in place, rather than adding parallel fork CI files.

Blocking CI keeps formatting, `cargo shear --deny-warnings`, and `cargo clippy --target <target> --tests -- -D warnings` for the two release targets, plus a result collector. Clippy checks test code; it does not run the test suite.

Postmerge CI keeps `cargo build --release --target <target> --bin codex` for those same targets, plus a result collector. Both workflows use the release workflow's runners and target setup.

These retained CI checks do not require agents to repeat them locally for every change. Do not expand the custom workflows to upstream-wide testing, other platforms, Bazel, SDKs, remote executors, V8 source-build canaries, or OpenAI-only infrastructure. Other inherited workflow files may still exist; their presence does not make them required fork checks.

When syncing, port relevant action-version, toolchain, security, and build fixes. Do not replace the custom workflows wholesale with upstream versions.

## Narrow validation

Choose checks from the fork-specific diff and conflict resolutions, not the size of the imported upstream range:

- **Documentation:** inspect the wording and local links; run `git diff --check`. No builds or tests.
- **Workflows:** lint only changed workflow files with `actionlint`; inspect affected triggers, permissions, targets, and packaging. Do not run release builds for unrelated edits.
- **Rust:** use `just test -p <crate> <test-filter>` for the changed behavior. Use `just test`, not direct `cargo test`. Keep required formatting and affected generated outputs current. Do not run whole-crate or workspace suites by default.
- **Conflict resolutions:** check the behavior or build setup that the resolution changed. A clean merge alone does not prove correctness, but it does not call for full upstream validation either.

For affected release logic, check only the relevant constraints above: tag handling, target setup, archive contents, checksums, credentials, and version preservation. For affected runtime logic, target the changed API, CLI, configuration, session, context, or cross-OS behavior rather than testing all of them.

If a fast, narrow check cannot establish safety, explain the gap and ask before expanding the change or validation. Report exact checks and results, skipped or blocked checks, and remaining uncertainty. Do not claim upstream CI passed unless verified.

## Commit attribution

Before committing, verify the author and committer identities with `git var GIT_AUTHOR_IDENT` and `git var GIT_COMMITTER_IDENT`. The owner's GitHub no-reply email is:

```text
1390402+trungnt13@users.noreply.github.com
```

Do not use `390402+trungnt13@users.noreply.github.com`. Do not rewrite published history merely to fix attribution without approval.
