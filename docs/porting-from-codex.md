# Fork purpose and upstream sync

## Principles

1. **Personal use only.** This fork exists solely for Trung Ngo's personal use.
2. **Keep changes minimal, safe, and minor.** Assume unchanged upstream code has already been tested upstream. Validate fork changes, conflict resolutions, and packaging with narrow, fast checks. Do not repeat full upstream suites by default. Upstream testing does not establish that our changes or packages work.
3. **Record the owner's current intent here.** Ensure each fork change's purpose and constraints are covered in the relevant section; update missing or changed intent, not a log of work already covered. If a proposed change conflicts with recorded intent, ask the owner first. Once accepted, replace the old intent rather than keeping contradictory rules. With intent clear, agents may triage changes and resolve merge or rebase conflicts within the authorized scope without asking about each conflict.

This file takes precedence over other repository instructions, including [AGENTS.md](../AGENTS.md), where they conflict. Its narrow validation policy replaces whole-crate and full-suite defaults; other implementation rules still apply. This file records intended behavior, not proof that a change has been committed, deployed, or included in a published binary.

## ASM branding

Use ASM in the README heading, GitHub repository description and release titles, introductory release prose, and descriptive workflow step labels, while clearly attributing the fork to OpenAI Codex. Keep the existing `codex` executable, crate, state and config names, archive filenames, tags, workflow names, job IDs, and CI check identities unchanged. Branding does not change what upstream links or installers provide.

## Upstream baseline

- Fork (`origin`): [`trungnt13/asm`](https://github.com/trungnt13/asm).
- Parent (`upstream`): [`openai/codex`](https://github.com/openai/codex).
- Last incorporated upstream commit: `69f7140559180269e2eb8f5be6e0c20eb37b0c85`.
- Commit date: 2026-09-28.
- Subject: Isolate the memory startup metadata test from Git enrichment (#49000).

A full sync incorporates a selected upstream `main` commit and all its ancestors. Capture the old baseline and selected upstream commit before syncing; use that fixed range for the report. After success, update the marker to the incorporated commit. A targeted cherry-pick does not advance it. Verify the marker with `git show -s --format='%H%n%cs%n%s' <upstream-commit>` and check that it is an ancestor of the fork branch.

## Sync and local work

Inspect Git status, the branch, remotes, and fork differences before editing. Preserve existing work. If local changes would block a sync, use a separate worktree unless the task authorizes committing or stashing them. When stashing is authorized, keep the named stash until restoration is verified; keep unfinished work out of published commits.

Follow the requested sync method. Without a specified method, merge to preserve published history; rebase unpublished branches when useful. A requested rebase permits local history rewriting, not a force push unless that is also authorized. Use `git cherry-pick -x` for targeted ports and `codex/` for new branch names. Do not discard work, rewrite remote history, move existing release tags, or publish releases without explicit authorization.

Verify the upstream URL before fetching. Focus on fork differences and the upstream changes that affect them, not unchanged upstream code. Resolve conflicts against this guide's intent, comparing the base, fork, and upstream versions when needed. Inspect all currently exposed conflicts before asking one consolidated question with recommended resolutions. Later rebase commits may expose more conflicts.

Keep related generated files correct when fork changes require them: schemas, snapshots, dependency lockfiles, and Bazel data declarations. Not running Bazel CI does not authorize breaking its files. Reuse already-correct upstream outputs rather than regenerating everything after a sync.

## Platforms and build profiles

Port useful upstream action, toolchain, security, and build fixes without restoring broad matrices. Do not add unrelated platforms, Bazel suites, SDKs, remote executors, V8 source-build canaries, or OpenAI-only infrastructure to fork CI, or repeat CI locally for every edit. The separate two-target V8 dependency build below is not a broad Bazel suite or canary.

Leave other inherited workflows unchanged unless the task authorizes changes. Check their own triggers and repository guards: [V8 canary](../.github/workflows/v8-canary.yml) still triggers on pull requests, with expensive builds conditional on relevant changes; [CLA](../.github/workflows/cla.yml) restricts its job to the `openai` owner. The custom CI scope does not disable these workflows.

Keep these release targets and GitHub-hosted runners:

- macOS ARM64: `aarch64-apple-darwin` on `macos-15`; build timeout 180 minutes. The installer treats an x86_64 process under Rosetta on Apple Silicon as ARM64, but rejects an actual Intel Mac.
- Linux x86_64 MUSL: `x86_64-unknown-linux-musl` on `ubuntu-24.04`; build timeout 90 minutes.

Local iteration uses `dev-small` for build speed. Published binaries always use optimized `--release`. Fix release build timeouts in the pipeline, not by switching to a development profile. In release workflows, use `CARGO_PROFILE_RELEASE_STRIP=debuginfo` to remove debug information while retaining function symbols and existing optimization settings. Do not remove runtime dependencies or additional symbols merely because upstream tests passed.

## Local development: binary first

For runtime or UI changes, make the smallest coherent change and deliver a runnable development binary before automated testing is complete. Inspect all affected paths, including streaming and completed output where relevant. From `codex-rs`, build the affected binaries; include `codex-code-mode-host` when changing its behavior:

```bash
cargo build -p codex-cli --bin codex --profile dev-small
```

- Keep the native target, toolchain, `dev-small` profile, compiler flags, and target directory consistent. Do not clean caches or switch profiles to try to speed up one build. Authorized cache cleanup makes the next build slower.
- Prioritize the CLI build over competing Cargo jobs. Tests may reuse some dependencies, but test executables are not the CLI. Do not promise instant builds.
- After a successful build, report the verified absolute binary path, build duration, a short manual check, and pending automated checks. Do not overwrite the installed `codex` unless asked, or present an old executable as the new build.
- When safe, hand off the binary before updating test expectations or running focused tests. Group related assertion and snapshot updates; do not disable tests to hide changed behavior.
- If runtime code changes after handoff, rebuild and identify the replacement binary. Distinguish ready for manual testing from validated complete.

Documentation-only changes need no binary build. Use release builds, cross-compilation, or packaging during local work only when requested or needed to reproduce the affected behavior.

## Release versions

When the owner requests a release, select the latest published, non-draft upstream Codex prerelease with a `rust-v` tag on `openai/codex`, ordered by publication time. Increment only its numeric patch component by one and preserve its suffix. For example, `rust-v0.159.0-alpha.6` gives fork version `0.159.1-alpha.6` and tag `v0.159.1-alpha.6`.

Choose this version automatically; do not increment the previous fork version or the suffix number. Check the fork's remote tags before changing versions. If the derived tag already exists or no upstream prerelease can be determined, stop and ask for a decision. Do not invent another number or move an existing tag. The current rule therefore requires a decision for another release based on the same upstream prerelease.

- No Apple Developer signing or notarization, paid Apple membership, Azure Key Vault, release secrets, or self-hosted runners.
- Use host `bwrap`, `rg`, and the system shell where needed. Do not bundle Bubblewrap, voice, patched zsh, or other helpers, or change runtime sandbox/security defaults for packaging.
- Retain Zig, [`install-musl-build-tools.sh`](../.github/scripts/install-musl-build-tools.sh), `AWS_LC_SYS_NO_JITTER_ENTROPY`, and verified fork-built V8 via [`setup-rusty-v8`](../.github/actions/setup-rusty-v8/action.yml).
- Do not add platforms, DMGs, bundled resources, npm, R2, WinGet, or website/OpenAI-only publishing without an agreed intent change.

### V8 dependency release

Build V8 separately from the CLI only when the resolved `v8` crate version lacks a fork release. Use [`fork-v8-release.yml`](../.github/workflows/fork-v8-release.yml) with tag `asm-v8-v<exact resolved v8 crate version>`. Before the first `main` push that consumes fork V8, or a CLI version push or tag, publish and verify that V8 dependency release; otherwise fork postmerge and CLI release builds fail. Local integration does not require publication. Reuse an existing verified fork V8 release when V8 inputs have not changed. If source, patches, build flags, or bindings change under the same crate version, ask how to version the dependency; never silently reuse or replace its old assets. A manual branch run builds but does not publish.

Build only sandbox + pointer-compression optimized pairs for macOS ARM64 and Linux x86_64 MUSL. Use the existing Bazel source pair and staging helper locally on GitHub-hosted runners, without BuildBuddy, remote execution, paid infrastructure, or broad suites. Keep the static library's required symbols. Run the native `codex-v8-poc` sandbox and JavaScript smoke tests on both targets, including MUSL. Publish only each target's archive, Rust binding, and two-file checksum manifest. The dependency release is normal but **not Latest**; it has no installer and does not affect CLI release discovery. Never move its tag or replace published assets without approval.

Fork CI and releases consume only the matching fork V8 release. Verify the GitHub SHA-256 digest of all three downloaded assets, then verify that the manifest names and hashes exactly match the target archive and binding. Missing or bad assets fail; there is no upstream V8 fallback. The inherited upstream V8 action path remains for inherited workflows outside fork CI.

### Reuse and publish

## Release packaging and publication

Keep [`fork-rust-release.yml`](../.github/workflows/fork-rust-release.yml) separate from upstream's release workflow. Build `codex` and `codex-code-mode-host` together from the same commit, target, and optimized release profile. Each `codex-<target>.tar.gz` contains exactly these two regular executable siblings. Upload both target archives, `SHA256SUMS`, and [`install.sh`](../scripts/install/install.sh), not source trees or diagnostic artifacts. The release job checks out the exact tag before adding the installer to the checksum manifest, including when it reuses postmerge archives.

Before archive upload, run [the smoke check](../.github/scripts/smoke-codex-archive.py) on both packaged binaries on their native runner. The CLI's `--version` must match Cargo; both executables' `--help` must succeed and print usage. Do not demand `--version` from the helper. Record binary and archive sizes. These checks validate our packaging; they do not replace functional tests of changed code.

Preserve these build constraints:

- No Apple Developer signing or notarization, paid Apple membership, Azure Key Vault, repository release secrets, or self-hosted runners.
- Linux stays on MUSL. Do not bundle Bubblewrap; install `bwrap` on the host when sandboxing needs it. Rely on the host's `rg` and system shell where relevant; do not bundle voice, patched zsh, or other auxiliary binaries. Do not silently disable sandboxing.
- Keep Zig, [`install-musl-build-tools.sh`](../.github/scripts/install-musl-build-tools.sh), and `AWS_LC_SYS_NO_JITTER_ENTROPY` settings for Linux native dependencies.
- Keep [`setup-rusty-v8`](../.github/actions/setup-rusty-v8/action.yml) for verified prebuilt V8 artifacts.
- Do not add platforms, DMGs, bundled resources, npm, R2, WinGet, website publishing, or OpenAI-only publishing infrastructure without an agreed change of intent.

[Postmerge CI](../.github/workflows/postmerge-ci.yml) saves release archives. A tag release uses [the artifact lookup](../.github/scripts/find-postmerge-artifacts.sh) to reuse both unexpired archives from a successful same-repository push-to-`main` run at the exact tagged commit. It waits for a matching active run. If that run stays active beyond the wait limit, stop rather than build concurrently. If no usable completed run remains, build the archives in the release workflow. API errors are failures, not cache misses.

Updates are manual. Do not fetch OpenAI/CDN or legacy npm packages, support daemon-only installation, or write `auto-update-version`. Built-in update checks, prompts, commands, and daemon update loops are disabled in ASM, regardless of upstream settings or markers. Updates are installed externally by the owner; the CLI and daemon must not download or install upstream releases. Keep ordinary daemon startup and restart separate from updating.

- Pushing a `v*` tag starts the release workflow; pushing `main` alone does not publish.
- Normal manual dispatch from a branch builds artifacts only. Normal dispatch on a `v*` tag can publish too, because publication checks the ref.
- Every new fork release is a normal GitHub release marked Latest. Retaining an upstream `alpha` suffix in the version does not set the GitHub prerelease flag.
- The concurrency experiment described below never publishes, including when dispatched on a tag.

Use explicit repository selection for GitHub operations, such as `gh ... -R trungnt13/asm`; do not rely on inferred upstream defaults. After publication, verify the tag's commit, release flags, both archives, the installer asset, and downloaded checksums. Confirm version/help checks passed for the published artifacts. Report failures and limits instead of treating a pushed tag as a completed release.

### Installer boundaries

The published `install.sh` installs only from `trungnt13/asm` GitHub releases (`v*`) on macOS ARM64 and Linux x86_64 MUSL. It verifies the release assets against GitHub's SHA-256 digests and `SHA256SUMS` before installing the flat two-binary archive. Keep `--release`, `CODEX_HOME`, `CODEX_INSTALL_DIR`, install locking, and safe `current` selection. Its `packages/asm-standalone` directory leaves upstream standalone packages and their update markers untouched without changing the shared Codex config/state home or the `codex` command. Reject unsupported targets before touching install state. Do not fetch OpenAI/CDN or legacy npm packages. Keep daemon-only installer mode unsupported; manual updates only. Do not write `auto-update-version`, because the inherited daemon updater fetches OpenAI's installer. The inherited in-app update action also points upstream; use this fork's installer for ASM updates until explicitly redesigned. Do not alter runtime sandbox/security defaults to accommodate packaging.

## CI and build experiments

Keep [`blocking-ci.yml`](../.github/workflows/blocking-ci.yml) and [`postmerge-ci.yml`](../.github/workflows/postmerge-ci.yml) customized in place. Port useful upstream action, toolchain, security, and build fixes without restoring upstream-wide matrices.

Keep `blocking-ci` manual-only (`workflow_dispatch`), not automatic on pushes or pull requests and not a required branch check. Run local formatting and targeted Clippy when fork-owned Rust changes need them. A manual CI run still checks workspace formatting and production Clippy for `codex-cli`, `codex-tui`, `codex-core`, and `codex-config`, on both release targets, plus a result collector. Preserve `--lib --bin codex -- -D warnings`; do not suppress warnings or assertions. Add narrow checks if future fork changes affect other packages.

Keep release builds, packaged-binary smoke checks, archive and checksum validation automatic. Postmerge CI builds optimized binaries with the release target setup, packages and smoke-checks them, uploads archives and diagnostics, and collects results. The tag-release workflow does not depend on manual blocking CI; report any existing CI failures separately rather than claiming publication proves all CI passed.

Do not repeat these checks locally for every edit or expand the custom workflows to unrelated platforms, Bazel suites, SDKs, remote executors, V8 source-build canaries, or OpenAI-only infrastructure.

The optional `macos_concurrency_experiment` dispatch compares cold builds of the same commit on `macos-15` with Cargo job limits 2 and 3. Keep every other build setting equal. Save separate timing, resource, size, and smoke-check results. It does not reuse postmerge artifacts or publish. Compare results before changing the default job count; treat one comparison as evidence, not proof of a general speedup.

## Background terminal waits

Allow top-level `background_terminal_min_timeout` and `background_terminal_max_timeout` in milliseconds. Keep omitted bounds at 5000 and 300000. Reject zero, reversed, or unrepresentable bounds rather than silently changing them. Clamp empty `write_stdin` polls to the configured range and show that range in the model-facing tool description. A poll returns early when the process ends. Do not change initial command waits, nonempty stdin waits, or code-mode's outer `exec`/`wait` limits. A background process exit event alone does not resume an idle model turn.

## Narrow validation

Choose checks from fork changes and conflict resolutions, not the size of the imported upstream range:

- **Documentation:** read the changed guidance, check local links, and run `git diff --check`. No builds or tests.
- **Workflows and scripts:** lint changed workflows with `actionlint` and check affected script logic with small fixtures. Run native builds only when needed to validate changed packaging or an authorized experiment.
- **Rust:** use `just test -p <crate> <test-filter>` for changed behavior, not direct `cargo test`. Keep formatting and affected generated outputs current. Do not run whole-crate or workspace suites by default.
- **Conflicts:** check the behavior or build setup changed by the resolution. A clean merge alone proves neither correctness nor a need for full upstream testing.

If narrow checks cannot establish safety, explain the gap and ask before expanding scope. Report exact checks, results, skipped or blocked work, and remaining uncertainty. Do not claim upstream CI passed unless verified.

## Commit attribution

Use the configured owner identity: `Trung Ngo <1390402+trungnt13@users.noreply.github.com>`. Verify both identities with `git var GIT_AUTHOR_IDENT` and `git var GIT_COMMITTER_IDENT` before committing. Correct local configuration when needed; do not rewrite published commits merely to fix attribution without approval.

## Memory reasoning effort

Allow optional `extract_reasoning_effort` and `consolidation_reasoning_effort` under `[memories]` to control the respective memory model requests for both V1 and V2. Use the existing reasoning-effort type. Preserve extraction's `low` and consolidation's `medium` when omitted, independently of the parent thread's effort. Do not change model selection or other memory behavior.

## Transcript spacing

Keep Markdown paragraphs, code blocks, and bullet or numbered list items adjacent without renderer-added blank rows, both while streaming and after completion. Preserve blank lines inside code blocks, raw output, message boundaries, and user-message padding.

## Compact status line

Join status-line segments with `·` without surrounding spaces. Use `CtxN%` for context used, `CtxN%left` for context remaining, and `F:on` / `F:off` for fast mode. Remove the space before the active agent role, such as `Main[default]`, only in the footer; leave picker labels unchanged. Preserve thread titles, paths, model names, agent roles, colors, item order, and the existing single-row truncation behavior.

Offer an opt-in `cache-hit-rate` item in `/statusline` and `tui.status_line`. Show `CchN.N%`, the current thread's cumulative cached input tokens divided by its total input tokens, rounded to one decimal place. Use the existing backend-reported usage without new requests or API changes. Hide the item while usage is unknown or input tokens are zero; keep existing status-line defaults unchanged. Providers that omit cache details remain indistinguishable from a reported zero.

## Subagent service tiers

Allow per-model, per-reasoning-effort service tiers for spawned subagents through `[subagent_service_tiers]`, for example `gpt-6-sol = { high = "fast" }`. Match the child's final model and effective effort after overrides and role settings. A matching rule overrides the root tier throughout the child's lifetime, including root tier changes; unmatched children keep upstream inheritance. Root requests are unchanged. Reject unsupported matched tiers and fast overrides when fast mode is disabled. Keep this policy out of the spawn tool arguments.

In the TUI `/subagents` picker, append each known agent model and reasoning effort to its title, such as `/root/sol_hello gpt-6-sol-high`. Append `-fast` only when the configured child rule or inherited root tier selects fast routing. A child rule selecting `default` suppresses the inherited fast label. Do not guess missing model or effort, remove the thread ID description, or alter picker navigation. This is a configured-settings label, not confirmation of the service tier used by a backend request. Keep it TUI-only: no new app-server API or request tracking.
