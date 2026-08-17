# Porting From Codex: A Practical Upstream Sync Guide

This guide is a repeatable checklist for porting changes from [`openai/codex`](https://github.com/openai/codex) into this repo.

Use it for any merge: single file, feature branch, or full release sync.

## Last Sync Point (historical upstream marker)

- **Commit:** `7093e8c480715667a5a75b602fd8c9ca2cad1780`
- **Date:** 2026-08-12
- **Upstream subject:** `Start required cached MCP servers lazily for subagents (#38217)`

This is the last upstream commit incorporated before the fork-specific release and CI work described below.

After every completed upstream sync, replace the commit, date, and subject with the exact upstream tip incorporated. Do not reuse the old range for the next sync. Verify the marker with:

```bash
git show -s --format='%H%n%cs%n%s' <upstream-commit>
```

## 0) Know the remotes and protect local work

`origin` is the fork, `trungnt13/codex`. Add the parent as `upstream` once:

```bash
git remote add upstream https://github.com/openai/codex.git
git remote -v
git fetch upstream --tags --prune
```

If `upstream` already exists, verify its URL before fetching:

```bash
git remote get-url upstream
```

Before any sync:

```bash
git status --short
git branch --show-current
git log -1 --oneline
```

- Treat every pre-existing modification as user-owned.
- Do not reset, clean, stash, overwrite, or revert unrelated work.
- Do not assume a diff proves who authored it.
- Work from a clean branch or a separate worktree when the shared checkout is dirty.

A separate worktree avoids disturbing uncommitted files:

```bash
git worktree add ../codex-upstream-sync -b sync/openai-codex HEAD
cd ../codex-upstream-sync
git fetch upstream --tags --prune
```

## 1) Define the sync scope

Record before editing:

- The upstream commit, tag, PR, or range.
- Whether this is a full sync or a targeted port.
- The crates, APIs, workflows, and generated files affected.
- Which fork divergences may conflict.
- Which upstream platforms or publishing paths remain intentionally skipped.

Inspect the range from the recorded marker:

```bash
git log --oneline --decorate \
  7093e8c480715667a5a75b602fd8c9ca2cad1780..upstream/main
git diff --stat \
  7093e8c480715667a5a75b602fd8c9ca2cad1780..upstream/main
```

Do not import a large range blindly. Review upstream changes to the conflict hotspots in section 6 first.

## 2) Choose the integration method

### Full upstream sync

Prefer merging into a dedicated sync branch when preserving published fork history matters:

```bash
git switch -c sync/openai-codex
git merge --no-ff upstream/main
```

Resolve conflicts semantically, validate, then merge the reviewed sync branch.

Rebase is acceptable for an unpublished sync branch:

```bash
git switch -c sync/openai-codex
git rebase upstream/main
```

Do not rebase or force-push shared `main` without explicit approval. It rewrites the fork commit and release-tag ancestry.

### Targeted commit or PR

Use cherry-pick and retain upstream provenance:

```bash
git switch -c port/<short-description>
git cherry-pick -x <upstream-commit>
```

For a multi-commit change, preserve order. Do not squash first if intermediate commits explain migrations or generated-file changes.

After either method, inspect all resolutions:

```bash
git status --short
git diff --check
git diff --name-status upstream/main...HEAD
```

## 3) Apply Codex Rust rules, not generic Rust assumptions

Read [the repository instructions](../AGENTS.md) before every port. In particular:

- Rust lives under `codex-rs`; crate names use the `codex-` prefix.
- Reuse existing code and crates before adding abstractions or growing `codex-core`.
- Keep public crate APIs small and modules focused.
- Preserve exhaustive matches where practical.
- Inline `format!` arguments, collapse nested `if` statements, and prefer method references over redundant closures.
- Avoid opaque Boolean or `Option` positional arguments; follow the repository's exact argument-comment convention when an API cannot be improved.
- New traits need role and implementation guidance in doc comments. Prefer native RPITIT methods with explicit `Send` futures over `async_trait`.
- Never add or alter logic related to `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` or `CODEX_SANDBOX_ENV_VAR`.
- Preserve cross-platform behavior unless a feature is explicitly platform-specific.

Watch repository-specific generated or locked outputs:

- A Rust dependency change requires `just bazel-lock-update` from the repository root and the resulting `MODULE.bazel.lock` update.
- A `ConfigToml` shape change requires `just write-config-schema`.
- App-server protocol changes require the matching schema generation and app-server documentation updates.
- New compile-time file reads such as `include_str!` also require the crate's Bazel data declarations.
- User-visible TUI changes require reviewed `insta` snapshot coverage.

Do not run `cargo test` directly. Use the validation rules in section 8.

## 4) Preserve the fork release contract

The committed fork workflow is [`fork-rust-release.yml`](../.github/workflows/fork-rust-release.yml). It is deliberately separate from upstream's [`rust-release.yml`](../.github/workflows/rust-release.yml) to minimize sync conflicts.

The supported release matrix is exactly:

| Platform | Rust target | GitHub-hosted runner | Asset |
| --- | --- | --- | --- |
| macOS ARM64 | `aarch64-apple-darwin` | `macos-15` | `codex-aarch64-apple-darwin.tar.gz` |
| Linux x86_64 MUSL | `x86_64-unknown-linux-musl` | `ubuntu-24.04` | `codex-x86_64-unknown-linux-musl.tar.gz` |

Each archive contains only the raw `codex` executable. The release also contains `SHA256SUMS` for both archives.

Preserve these decisions:

- macOS is unsigned and unnotarized.
- No paid Apple Developer membership or Apple credentials are used.
- Azure Key Vault is not used.
- No repository release secrets or self-hosted runners are required.
- Linux targets MUSL for portability.
- Linux archives do not bundle Bubblewrap; install `bwrap` on the host when sandboxing requires it.
- Zig and [`install-musl-build-tools.sh`](../.github/scripts/install-musl-build-tools.sh) provide the Linux C/C++ MUSL toolchain.
- [`setup-rusty-v8`](../.github/actions/setup-rusty-v8/action.yml) supplies the verified prebuilt `rusty_v8` artifact for each target.
- The workspace package version remains `0.0.0` in [`codex-rs/Cargo.toml`](../codex-rs/Cargo.toml); a release tag does not rewrite Cargo files.

Do not silently add Windows, macOS x86_64, Linux ARM64, Linux GNU, DMGs, bundled resources, npm, R2, WinGet, website publication, Apple signing, or OpenAI-only publishing infrastructure.

## 5) Preserve tag and release behavior

Fork releases use `v*`; upstream Rust releases use `rust-v*.*.*`. The distinct prefixes avoid a direct trigger collision.

- A manual dispatch from a branch builds both archives and uploads workflow-run artifacts; it does not create a GitHub Release.
- Pushing a `v*` tag builds both targets, then publishes all archives and `SHA256SUMS` on the GitHub Releases page.
- A tag with a hyphenated suffix, such as `v0.0.1-alpha.0`, is a prerelease and is not marked Latest.
- A plain tag, such as `v0.0.1`, is a normal release and is marked Latest.

Do not select a `v*` tag as the ref for a manual dispatch unless publication is intended: the release job tests the ref, not the event name.

The first verified fork release is [`v0.0.1-alpha.0`](https://github.com/trungnt13/codex/releases/tag/v0.0.1-alpha.0), published as a prerelease with exactly:

- `codex-aarch64-apple-darwin.tar.gz`
- `codex-x86_64-unknown-linux-musl.tar.gz`
- `SHA256SUMS`

## 6) Treat fork CI workflows as conflict hotspots

The owner chose to modify the inherited workflows in place rather than add parallel fork CI files:

- [`blocking-ci.yml`](../.github/workflows/blocking-ci.yml)
- [`postmerge-ci.yml`](../.github/workflows/postmerge-ci.yml)

At the time this guide was created, those two changes were selected and present locally but were not part of the already committed release-workflow change. Check their actual commit status before every sync; do not assume this historical state is still current.

The intended minimal blocking CI is:

- Rust formatting.
- `cargo shear --deny-warnings`.
- `cargo clippy --tests -- -D warnings` for macOS ARM64.
- The same Clippy check for Linux x86_64 MUSL.
- One required result collector.

The intended minimal postmerge CI is:

- `cargo build --release --bin codex` for macOS ARM64.
- The same release build for Linux x86_64 MUSL.
- One result collector.

Both target jobs use the same standard runners and target setup as the release workflow. Preserve `setup-rusty-v8`; preserve Zig, the MUSL setup script, and the AWS-LC no-jitter environment for the MUSL target.

The fork intentionally omits upstream CI for Windows, macOS x86_64, Linux ARM64, Linux GNU, Bazel, SDKs, remote executors, V8 source-build canaries, and OpenAI-specific runner groups, environments, credentials, and result collectors.

When upstream changes either workflow, compare and port useful action-version, toolchain, security, or build fixes into the minimal jobs. Do not replace the fork files wholesale with upstream matrices.

## 7) Check divergence and regression traps

Before resolving a conflict, compare all three relevant versions:

```bash
git show upstream/main:.github/workflows/blocking-ci.yml > /tmp/upstream-blocking.yml
git diff upstream/main -- .github/workflows/blocking-ci.yml
git diff upstream/main -- .github/workflows/postmerge-ci.yml
git diff upstream/main -- .github/workflows/fork-rust-release.yml
```

Check for these common regressions:

- `v*` accidentally changed back to `fork-v*` or upstream `rust-v*.*.*`.
- The release job runs during an ordinary branch dispatch.
- Stable tags are marked prerelease, or suffixed tags become Latest.
- One target is omitted from the checksum or release file list.
- An archive contains a package tree instead of only `codex`.
- macOS signing, notarization, Azure, or secret references return.
- Linux changes from MUSL to GNU or loses Zig/native dependency setup.
- OpenAI self-hosted runner labels or protected environments return.
- Cargo versions are rewritten from `0.0.0` during release.
- Blocking CI expands into unrelated platforms or upstream-wide validation.
- Postmerge builds stop matching the two release targets.

For Rust code, also inspect external behavior that upstream changes can break:

- App-server v2 API and generated TypeScript schemas.
- CLI flags and configuration loading.
- Raw response item events.
- Session resume compatibility.
- Model-visible context size, boundedness, and incremental construction.
- Connected app-server and exec-server operation across different OSes.

## 8) Validate the port

Run only checks relevant to the files changed, and report exact failures.

For workflow or documentation-only changes:

```bash
actionlint .github/workflows/fork-rust-release.yml \
  .github/workflows/blocking-ci.yml \
  .github/workflows/postmerge-ci.yml
git diff --check
```

After Rust changes, from `codex-rs`:

```bash
just fmt
just test -p <changed-crate>
```

- Never substitute direct `cargo test` for `just test`.
- For a large Rust change, run `just fix -p <changed-crate>` before finalizing.
- If `common`, `core`, or `protocol` changed, ask before running the complete `just test` suite.
- Follow any crate-specific schema, snapshot, remote-test, or Bazel-lock rules triggered by the diff.
- Per repository convention, do not rerun tests after the final `just fix` or `just fmt` pass.

For a live release check, first dispatch the workflow from `main` and inspect its two run artifacts. Only then create and push the intended `v*` tag. Verify the Releases page contains both archives and `SHA256SUMS`.

## 9) Verify Git attribution before committing

The correct GitHub no-reply identity for this fork owner is:

```text
1390402+trungnt13@users.noreply.github.com
```

Configure and verify it before committing:

```bash
git config user.email "1390402+trungnt13@users.noreply.github.com"
git var GIT_AUTHOR_IDENT
git var GIT_COMMITTER_IDENT
```

The numeric prefix controls GitHub account attribution. The incorrect prefix `390402+trungnt13@users.noreply.github.com` is associated with another account; the username text after `+` does not correct that attribution.

Do not amend published commits, force-push `main`, or move release tags merely to fix attribution without explicit approval.

## 10) Quick audit checklist

- [ ] The upstream remote URL and intended commit range were verified.
- [ ] User-owned working-tree changes remained untouched.
- [ ] The recorded sync marker was updated to the exact incorporated upstream tip.
- [ ] Rust changes follow `AGENTS.md` and affected crate boundaries.
- [ ] Only macOS ARM64 and Linux x86_64 MUSL are release targets.
- [ ] CI uses `macos-15` and `ubuntu-24.04`, not self-hosted runners.
- [ ] Linux still uses Zig, the MUSL setup script, and `setup-rusty-v8`.
- [ ] macOS remains unsigned and unnotarized; no Apple or Azure credentials exist.
- [ ] Each archive contains only `codex`; Linux does not bundle `bwrap`.
- [ ] `v*` tags publish; upstream `rust-v*.*.*` behavior remains separate.
- [ ] Suffixed tags are prereleases; plain tags are normal Latest releases.
- [ ] Both archives and `SHA256SUMS` are uploaded.
- [ ] The Cargo workspace version remains `0.0.0`.
- [ ] Minimal blocking and postmerge coverage was preserved intentionally.
- [ ] No skipped platform, Bazel, SDK, canary, or OpenAI publishing jobs returned.
- [ ] Relevant validation ran, or each blocked check is stated exactly.
- [ ] Git author and committer identities use the correct numeric no-reply prefix.
