# Fork purpose and upstream sync

## Principles

1. **Personal use only.** This fork exists solely for Trung Ngo's personal use.
2. **Keep changes minimal, safe, and minor.** Assume unchanged upstream code has already been tested upstream. Run only narrow, fast checks for fork changes and conflict resolutions. Do not repeat upstream testing or run full suites by default. This assumption is not proof that a particular upstream commit passed CI. If a change needs broad testing to establish safety, stop and clarify its scope with the owner.
3. **Keep the owner's current intent in this file.** Whenever changing the fork, record the purpose and constraints here. If a proposed change conflicts with recorded intent, ask the owner before proceeding. Once accepted, replace the old intent rather than keeping contradictory rules or a change log. With intent clear, agents may triage changes, choose a sync method, and resolve merge or rebase conflicts within that scope without asking about each conflict.

The sections below record current intent. Update the relevant section when intent changes; do not add a duplicate policy. Resolving a conflict does not authorize changing requirements, discarding user work, rewriting published history, or publishing a release.

## Upstream baseline

- Fork (`origin`): [`trungnt13/codex`](https://github.com/trungnt13/codex).
- Parent (`upstream`): [`openai/codex`](https://github.com/openai/codex).
- Last incorporated upstream commit: `b334d5b3f2d9441b95286a8c2af8c2152737d977`.
- Commit date: 2026-09-26.
- Subject: Default to copying transcript selections in more terminals (#48469).

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

Fork releases use stable `v<major>.<minor>.<patch>` tags only. Do not create prereleases or suffixed tags. Upstream Rust releases use `rust-v*.*.*` and do not determine fork version numbers.

When the owner requests a release, choose the version automatically without asking for a number. Check the current tags on `trungnt13/codex`, compare versions numerically, and increment the patch number of the highest fork version: `v0.0.1` → `v0.0.2` → `v0.0.3`. Count a historical prerelease by its base version when choosing the next number. If no fork version exists, start at `v0.0.1`. Never reuse or move an existing tag; if another release takes the chosen number, check again and select the next patch version.

- A manual dispatch from a branch builds artifacts without publishing a GitHub Release.
- A `v*` tag publishes both archives and checksums.
- Every new fork release is a normal release and is marked Latest.

The release job checks the ref, not the event name. A manual dispatch on a `v*` tag can publish too. Do not push a release tag or dispatch on one without publication approval.

When release validation is requested, check the relevant existing run and artifacts first. Do not repeat builds merely to recheck unchanged packaging. After an authorized release, verify both archives and `SHA256SUMS` are present.

## CI intent

Keep the inherited [`blocking-ci.yml`](../.github/workflows/blocking-ci.yml) and [`postmerge-ci.yml`](../.github/workflows/postmerge-ci.yml) customized in place, rather than adding parallel fork CI files.

Blocking CI keeps formatting, `cargo shear --deny-warnings`, and `cargo clippy --target <target> --tests -- -D warnings` for the two release targets, plus a result collector. Clippy checks test code; it does not run the test suite.

Postmerge CI keeps `cargo build --release --target <target> --bin codex` for those same targets, plus a result collector. Both workflows use the release workflow's runners and target setup.

These retained CI checks do not require agents to repeat them locally for every change. Do not expand the custom workflows to upstream-wide testing, other platforms, Bazel, SDKs, remote executors, V8 source-build canaries, or OpenAI-only infrastructure. Other inherited workflow files may still exist; their presence does not make them required fork checks.

When syncing, port relevant action-version, toolchain, security, and build fixes. Do not replace the custom workflows wholesale with upstream versions.

## Local development: binary first

For runtime or UI changes, deliver a runnable development binary before automated testing is complete. Build speed takes priority over runtime performance for local testing.

Inspect affected paths, including streaming and completed output where relevant. Make the smallest coherent change, then build only the CLI from `codex-rs`:

```bash
cargo build -p codex-cli --bin codex --profile dev-small
```

- Use the native host target and the existing warm development cache. Keep the toolchain, profile, target directory, and compiler flags consistent. Do not clean caches or switch profiles merely to try to speed up one build.
- Use `dev-small` consistently for local CLI builds: no optimization or debug info. The first build may need to rebuild dependencies; later builds should reuse this profile's cache. Do not switch back to `dev` merely to reuse a different cache. Do not use release builds, cross-compilation, or packaging unless requested or needed to reproduce the behavior.
- Prioritize the CLI build over competing Cargo jobs. Test builds may reuse some dependencies, but they do not deliver the CLI binary or eliminate compilation and linking. Do not promise instant builds.
- As soon as the build succeeds, report the verified absolute binary path, build duration, a short manual check, and pending automated checks. Do not overwrite the installed `codex` or present an older executable as the new build.
- Hand off the binary before updating test expectations or running tests when those can safely follow. Then update affected assertions and snapshots together, verify test filters, and run the narrow checks below. Do not disable tests to hide changed behavior.
- If runtime code changes after handoff, rebuild and identify the replacement binary. Distinguish ready for manual testing from validated complete.

Documentation-only changes need no binary build. Authorized disk cleanup may remove the build cache; expect a slower next build rather than changing profiles to compensate.

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

## Transcript spacing

Keep Markdown paragraphs, code blocks, and bullet or numbered list items adjacent without renderer-added blank rows, both while streaming and after completion. Preserve blank lines inside code blocks, raw output, message boundaries, and user-message padding.
