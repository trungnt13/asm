# Fork purpose and upstream sync

## Principles

1. **Personal use only.** This fork is for Trung Ngo.
2. **Keep changes minimal, safe, and minor.** Prefer durable, localized solutions that minimize upstream divergence and future merge or rebase conflicts. Do not sacrifice correctness, maintainability, or runtime performance merely to reduce the diff. Assume unchanged code was tested upstream. Check our changes, conflicts, and packages narrowly; upstream testing does not prove they work.
3. **Record current intent here.** Cover each fork change's purpose and constraints; update new or changed intent, not a task log. Ask before contradicting existing intent. Once approved, replace the old rule. Agents may then resolve mechanical conflicts without further approval.

This file takes precedence over other repository instructions, including [AGENTS.md](../AGENTS.md), where they conflict. Its narrow validation policy replaces whole-crate and full-suite defaults; other implementation rules still apply. This file records intended behavior, not proof that a change has been committed, deployed, or included in a published binary.

## ASM branding

Use ASM in the README heading, GitHub repository description and release titles, introductory release prose, and descriptive workflow step labels, while clearly attributing the fork to OpenAI Codex. Keep the existing `codex` executable, crate, state and config names, archive filenames, tags, workflow names, job IDs, and CI check identities unchanged. Branding does not change what upstream links or installers provide.

Check Git status, branches, worktrees, remotes, fork differences, and relevant repo sessions for overlapping work and unfinished handoffs. Recover context from Git and sessions, not the owner's memory or a separate task diary.

For concurrent tasks, create a named `codex/` branch and separate worktree before editing; coordinate shared files. One agent owns staging and commits in each checkout. Integrate into local `main` one task at a time.

Preserve existing work. Use a separate worktree if it blocks progress. Committing or stashing unrelated work needs permission; keep any authorized stash until restoration is verified. Never overwrite other work to make a checkout clean.

### Build the binary first

For runtime or UI changes, inspect all affected paths, including streaming and completed output. Make the smallest coherent change.

Before running Cargo in a new worktree, check available disk space and resolve insufficient space before starting. Explicitly select a compatible existing cache with `CARGO_TARGET_DIR`; do not accidentally start with an empty worktree-local cache. Keep the native target, toolchain, profile, flags, and cache stable. Cache reuse can still require recompilation.

Build from `codex-rs`:

```bash
cargo build -p codex-cli --bin codex --profile dev-small
```

Prioritize the CLI build over competing Cargo jobs. When sharing a target directory, coordinate and serialize Cargo jobs, and verify which checkout produced the binary before handing it off. Use build time for independent inspection and affected-test or snapshot review. Test executables are not the CLI.

Hand off the binary before automated checks finish when safe. Report its verified absolute path, build time, a manual check, and pending checks. Rebuild after runtime edits; never present an old binary as current or replace the installed `codex` without permission. Binary handoff is progress, not completion.

Use optimized or cross-platform builds locally only when requested or needed to reproduce the affected behavior.

### Validate narrowly

Choose checks from changed behavior and conflict resolutions, not the size of an upstream sync:

- **Documentation:** review wording and local links; run `git diff --check`. No builds or tests.
- **Rust:** use `just test -p <crate> <test-filter>`, not direct `cargo test`. Format changed code and use targeted Clippy when needed. Group affected assertion and snapshot updates before validation; keep unrelated stale expectations out of the change, and never disable tests to hide intentional output changes.
- **Workflows and scripts:** run `actionlint` for changed workflows and small fixtures for script logic. Build natively only for affected packaging or an approved experiment.

Run required generators for changed inputs; keep schemas, dependency lockfiles, and Bazel data correct. Reuse correct upstream outputs; skipping Bazel CI does not waive Bazel correctness.

Do not run whole-crate or workspace suites by default. If narrow checks cannot establish safety, explain the gap and ask before expanding. A clean merge is not validation; claim upstream CI passed only when verified.

### Commit, integrate, and report

Implementation requests, including documentation edits, authorize committing finished task changes and integrating them into local `main`, unless the owner says otherwise.

The lead agent owns completion, including delegated work. Once narrow checks pass and owner decisions are resolved, commit and integrate immediately. Wait for manual testing or a separate approval only if requested. **Done means committed and verified in local `main`**, not just finished in another worktree.

Inspect the staged diff; include only finished task changes and their tests, generated files, and intent. Use `Trung Ngo <1390402+trungnt13@users.noreply.github.com>`; verify `git var GIT_AUTHOR_IDENT` and `git var GIT_COMMITTER_IDENT`. Correct local configuration when needed, but do not rewrite published history just to fix attribution without approval.

Recheck the target branch before integration. Resolve conflicts against this guide, comparing base, fork, and upstream. Check affected behavior. For owner decisions, inspect all exposed conflicts, then ask one consolidated question with recommendations; later commits may expose more conflicts.

Reconcile task-owned duplicate edits only when other work is preserved. If integration is blocked, commit completed work and report **ready, integration blocked**, with the reason. Identify unfinished work separately.

Finish with the change, exact checks and results, commit, integration and push status, and remaining work's branch or worktree. Report skipped checks and uncertainty, not just successes.

### Push permissions

Pushing to `origin/main`, including `--force-with-lease`, has standing owner approval but is not required for local task completion. Other force pushes, moving existing tags, discarding work, and publishing releases need explicit approval. An implementation request alone does not authorize a release.

## Upstream sync

- Fork (`origin`): [`trungnt13/asm`](https://github.com/trungnt13/asm).
- Parent (`upstream`): [`openai/codex`](https://github.com/openai/codex).
- Last incorporated upstream commit: `e53e932dc855927afb1cafab5f1ae6edf2695e81`.
- Commit date: 2026-10-01.
- Subject: Allow microphone channel selection for voice conversations (#49836).

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

Prepare the version on an immutable release-candidate branch, not on `main`. Set `[workspace.package].version` in [`Cargo.toml`](../codex-rs/Cargo.toml) and the matching workspace package versions in [`Cargo.lock`](../codex-rs/Cargo.lock), without changing dependencies. The tag, Cargo version, and CLI `--version` must agree, ignoring their name prefixes. Renaming a tag cannot change an existing binary. Advance `main` only after publication and verification succeed; preserve unrelated concurrent changes and stop on version conflicts rather than force-pushing. Leave existing pre-policy version bumps intact until the next successful release.

For runtime or UI changes, make the smallest coherent change and deliver a runnable development binary before automated testing is complete. Inspect all affected paths, including streaming and completed output where relevant. From `codex-rs`, build the affected binaries; include `codex-code-mode-host` when changing its behavior:

```bash
cargo build -p codex-cli --bin codex --profile dev-small
```

- macOS ARM64: `aarch64-apple-darwin`, `macos-15`, 180-minute timeout.
- Linux x86_64 glibc: `x86_64-unknown-linux-gnu`, `ubuntu-22.04`, 90-minute timeout. Ubuntu 22.04 / glibc 2.35 is the minimum supported Linux baseline. Keep Linux build and native smoke jobs on that baseline; changing only the target triple on a newer runner is not sufficient. Before GitHub retires this runner in April 2027, move the same build environment into an Ubuntu 22.04 container on a supported runner rather than raising the runtime requirement.

Documentation-only changes need no binary build. Use release builds, cross-compilation, or packaging during local work only when requested or needed to reproduce the affected behavior.

## Release versions

Before archive upload, run [the smoke check](../.github/scripts/smoke-codex-archive.py) on the packaged binaries on their native runner: CLI `--version` matches Cargo, and both `--help` commands succeed with usage. Do not require the helper to support `--version`. Record binary and archive sizes. For GNU Linux archives, verify both ELF executables use the x86_64 GNU loader and require no glibc symbol newer than 2.35. Also run version/help checks in a clean Ubuntu 22.04 container with only the declared runtime libraries; a dependency-rich build runner does not establish runtime compatibility.

Choose this version automatically; do not increment the previous fork version or the suffix number. Check the fork's remote tags before changing versions. If the derived tag already exists or no upstream prerelease can be determined, stop and ask for a decision. Do not invent another number or move an existing tag. The current rule therefore requires a decision for another release based on the same upstream prerelease.

- No Apple Developer signing or notarization, paid Apple membership, Azure Key Vault, release secrets, or self-hosted runners.
- Use host `bwrap`, `rg`, and the system shell where needed. Do not bundle Bubblewrap, voice, patched zsh, or other helpers, or change runtime sandbox/security defaults for packaging.
- Use the native GNU toolchain and [`install-gnu-build-tools.sh`](../.github/scripts/install-gnu-build-tools.sh) for fork Linux builds. Do not use Zig or MUSL build wrappers there; keep inherited MUSL tooling for upstream workflows. Retain `AWS_LC_SYS_NO_JITTER_ENTROPY` and verified fork-built V8 via [`setup-rusty-v8`](../.github/actions/setup-rusty-v8/action.yml). Install Python 3.11+ explicitly on Ubuntu 22.04 for artifact verification.
- Preserve upstream GNU runtime choices, including the system allocator, locale handling, and PTY support; do not change Rust platform conditionals merely to mimic MUSL. GNU packages use system OpenSSL 3 and may use system liblzma. Keep the runtime package list in [`smoke-ubuntu-archive.sh`](../.github/scripts/smoke-ubuntu-archive.sh) aligned with actual ELF dependencies. Voice remains unbundled; this migration does not add an ALSA or GStreamer requirement.
- Do not add platforms, DMGs, bundled resources, npm, R2, WinGet, or website/OpenAI-only publishing without an agreed intent change.

### V8 dependency release

Build V8 separately from the CLI only when the required crate version, target, or baseline lacks a verified fork release. Use [`fork-v8-release.yml`](../.github/workflows/fork-v8-release.yml) with tag `asm-v8-v<exact resolved v8 crate version>-glibc2.35`. The suffix identifies the GNU-baseline artifact generation, including its matching macOS pair, without moving or replacing the old MUSL release. A manual branch run builds only by default; explicit `publish=true` builds and smokes first, then creates the tag and release at that commit. Tag pushes also publish. Never move a tag or replace published assets without approval. Reuse a verified fork V8 release when V8 inputs have not changed; if source, patches, build flags, or bindings change under the same crate version, ask how to version the dependency.

Build only sandbox + pointer-compression optimized pairs for macOS ARM64 and Linux x86_64 GNU. Use the existing Bazel source pair and staging helper locally on GitHub-hosted runners, without BuildBuddy, remote execution, paid infrastructure, or broad suites. Keep the static library's required symbols. Run the native `codex-v8-poc` sandbox and JavaScript smoke tests on both targets, including GNU on Ubuntu 22.04. Reuse the existing GNU Bazel platform and its older glibc sysroot; the native Ubuntu 22.04 tests must still pass. Publish only each target's archive, Rust binding, and two-file checksum manifest. The dependency release is normal but **not Latest**; it has no installer and does not affect CLI release discovery.

Fork CI and CLI releases consume only the matching verified fork V8 release. For a new V8 version or artifact generation, publish and verify both target pairs before the first `main` push that consumes it. Check each downloaded asset's GitHub SHA-256 digest and the manifest's exact target names and hashes. Missing or bad assets fail without upstream fallback. The inherited upstream V8 action path remains for workflows outside fork CI.

### Reuse and publish

Start an explicitly requested release by dispatching [`fork-rust-release.yml`](../.github/workflows/fork-rust-release.yml) on `main` with `publish_release=true`. This single durable workflow owns version selection, candidate creation, builds, publication, verification, and updating `main`; no later chat action or workflow triggered by a token-created push is required. Serialize release runs. Pin every build, installer, and verification checkout to the candidate commit. Ordinary branch dispatch remains build-only, and a `main` push never requests publication.

The candidate lives at `agent/release-<run-id>`. Rerun failed jobs to continue after interruption; to resume from a new dispatch, set `publish_release=true` and `resume_run_id` to the original run ID. Reuse the frozen version and commit even if upstream has advanced. Never move a candidate or existing tag. A new candidate must use a free release tag; conflicts require owner input. Keep candidate branches for recovery. Existing `v*` tag pushes and tag dispatches can still publish without modifying `main`.

Use [the artifact lookup](../.github/scripts/find-postmerge-artifacts.sh) to reuse both unexpired, smoke-checked archives from the current dispatch or original candidate run. Legacy tag releases may reuse a successful same-repository push-to-`main` run at the exact tagged commit; wait for a matching active run rather than building concurrently, and fail if the wait expires. Build only when no usable archives remain. API errors are failures, not cache misses.

Upload all four assets to a draft before publishing. Resume partial uploads only when existing names, sizes, and digests agree; never replace assets. Verify the tag commit, GitHub digests, checksum manifest, installer bytes, and both downloaded archives before publication. Publish normal and Latest even with an `alpha` suffix, then repeat metadata and native binary smoke checks on both release platforms. Only then merge the candidate into `main`. If publication already succeeded, retries verify the existing release and finish updating `main` without rebuilding or republishing.

Before archive upload, run [the smoke check](../.github/scripts/smoke-codex-archive.py) on both packaged binaries on their native runner. The CLI's `--version` must match Cargo; both executables' `--help` must succeed and print usage. Do not demand `--version` from the helper. Record binary and archive sizes. These checks validate our packaging; they do not replace functional tests of changed code.

Preserve these build constraints:

The published installer uses only `trungnt13/asm` GitHub `v*` releases for macOS ARM64 and Linux x86_64 GNU. Verify GitHub SHA-256 digests and `SHA256SUMS` before installing the two-binary archive. Reject unsupported targets and Linux hosts without glibc 2.35+ before downloading release metadata or changing install state. Select only GNU Linux archives; never silently fall back to MUSL. A MUSL-only historical release requires its original installer. A GNU update uses a separate target-qualified package directory and preserves old MUSL packages and shared config/state.

[Postmerge CI](../.github/workflows/postmerge-ci.yml) saves release archives. A tag release uses [the artifact lookup](../.github/scripts/find-postmerge-artifacts.sh) to reuse both unexpired archives from a successful same-repository push-to-`main` run at the exact tagged commit. It waits for a matching active run. If that run stays active beyond the wait limit, stop rather than build concurrently. If no usable completed run remains, build the archives in the release workflow. API errors are failures, not cache misses.

Updates are manual. Do not fetch OpenAI/CDN or legacy npm packages, support daemon-only installation, or write `auto-update-version`. Built-in update checks, prompts, commands, and daemon update loops are disabled in ASM, regardless of upstream settings or markers. Updates are installed externally by the owner; the CLI and daemon must not download or install upstream releases. Keep ordinary daemon startup and restart separate from updating.

- Pushing a `v*` tag starts the release workflow; pushing `main` alone does not publish.
- Normal manual dispatch from a branch builds artifacts only. Normal dispatch on a `v*` tag can publish too, because publication checks the ref.
- Every new fork release is a normal GitHub release marked Latest. Retaining an upstream `alpha` suffix in the version does not set the GitHub prerelease flag.
- The concurrency experiment described below never publishes, including when dispatched on a tag.

Use explicit repository selection for GitHub operations, such as `gh ... -R trungnt13/asm`; do not rely on inferred upstream defaults. After publication, verify the tag's commit, release flags, both archives, the installer asset, and downloaded checksums. Confirm version/help checks passed for the published artifacts. Report failures and limits instead of treating a pushed tag as a completed release.

### Installer boundaries

### Saved forks, upstream sides, and parallel conversations

Preserve the parent's effective prompt-cache routing key for saved and temporary root forks, independently of thread/session identity. Persist that routing choice for resume and further forks; keep old rollouts readable without ancestor lookups. Preserve upstream guardian and subagent routing. Sharing routing is an optimization, not a promise of backend cache hits.

Keep `/side` and its `/btw` alias aligned with upstream: temporary forks, upstream command restrictions and reference-only instructions, no subagents, and no saved pairing metadata. Ctrl+/ switches views; Ctrl+C returns to the parent and discards the temporary side. Ordinary picker navigation discards temporary sides as upstream does. Preserve upstream `/side` helper names, constants, and tests where practical; add parallel-specific behavior without unnecessary rewrites of the upstream path.

Use `/parallel` for the saved ordinary user fork with normal command availability under permissions, feature flags, platform support, and busy-state checks. Inherited history is reference context, not a request to continue the parent's task or control its agents. Parallel chats may own goals and subagents. Keep the initial transcript clean without rewriting stored or model-visible history.

Share one main/companion pair without nesting. Starting a companion from a parallel chat returns to its parent and replaces the selected companion; old parallel chats remain saved. Temporary sides keep upstream restrictions, including rejecting another companion command inside them. Ctrl+/ switches without stopping work. With an empty composer and no modal, Ctrl+C stops the parallel chat, pauses its active goal, and returns to its parent without deleting history. Selecting its subagents must not stop it. Ordinary `/new`, `/clear`, `/resume`, `/fork`, `/cd`, and `/worktree` navigation leaves parallel mode. Archive/delete act on the displayed parallel chat and return to its parent.

Keep parallel pairing and transcript-display boundaries in ASM client-local state scoped to the app-server target. Read existing saved-side records as parallel chats without migrating files or rewriting history. Restore an open pair on resume; closing the selection must not delete its saved conversation. Do not use analytics source classification or false fork ancestry to encode UI relationships. Stock clients see ordinary threads through unchanged app-server APIs; their UI need not implement companion switching. Shared files and explicitly global configuration changes are not isolated between chats.

### Copy session ID

Offer `/copyid` to copy only the currently displayed session's ID directly through the existing clipboard path, without requiring `/status`, an assistant response, or a picker. Allow it while a task is running, in side conversations, and when viewing parent-owned subagents. Report missing IDs and clipboard failures; preserve `/copy` behavior and clipboard platform support.

### ASM branding

Use ASM in the README heading, repository description, release titles and introductory prose, workflow step descriptions, and the fork's own TUI-visible labels and terminal title. Credit OpenAI Codex where origin is described. Keep real upstream service and product names, and keep `codex` executable, CLI version prefix, crate, config/state, archive, tag, workflow, job, and check identifiers unchanged. Branding does not redirect upstream links or installers.

Keep [`blocking-ci.yml`](../.github/workflows/blocking-ci.yml) and [`postmerge-ci.yml`](../.github/workflows/postmerge-ci.yml) customized in place. Port useful upstream action, toolchain, security, and build fixes without restoring upstream-wide matrices.

Keep `blocking-ci` manual-only (`workflow_dispatch`), not automatic on pushes or pull requests and not a required branch check. Run local formatting and targeted Clippy when fork-owned Rust changes need them. A manual CI run still checks workspace formatting and production Clippy for `codex-cli`, `codex-tui`, `codex-core`, and `codex-config`, on both release targets, plus a result collector. Preserve `--lib --bin codex -- -D warnings`; do not suppress warnings or assertions. Add narrow checks if future fork changes affect other packages.

Keep release builds, packaged-binary smoke checks, archive and checksum validation automatic. Postmerge CI builds optimized binaries with the release target setup, packages and smoke-checks them, uploads archives and diagnostics, and collects results. The tag-release workflow does not depend on manual blocking CI; report any existing CI failures separately rather than claiming publication proves all CI passed.

Do not repeat these checks locally for every edit or expand the custom workflows to unrelated platforms, Bazel suites, SDKs, remote executors, V8 source-build canaries, or OpenAI-only infrastructure.

The optional `macos_concurrency_experiment` dispatch compares cold builds of the same commit on `macos-15` with Cargo job limits 2 and 3. Keep every other build setting equal. Save separate timing, resource, size, and smoke-check results. It does not reuse postmerge artifacts or publish. Compare results before changing the default job count; treat one comparison as evidence, not proof of a general speedup.

## Background terminal waits

Allow top-level `background_terminal_min_timeout` and `background_terminal_max_timeout` in milliseconds. Keep omitted bounds at 5000 and 300000. Reject zero, reversed, or unrepresentable bounds rather than silently changing them. Clamp empty `write_stdin` polls to the configured range and show that range in the model-facing tool description. A poll returns early when the process ends. Do not change initial command waits, nonempty stdin waits, or code-mode's outer `exec`/`wait` limits. A background process exit event alone does not resume an idle model turn.

### Collapsed tool calls

Offer opt-in `[tui] collapse_tool_calls = true`, defaulting to `false`. In the rich fullscreen transcript, group consecutive shell, MCP, and dynamic tool calls into a single-row summary with the call count and distinct command or tool names in first-use order. Shorten overflowing name lists with `+N more`. Other activity types and chat messages separate groups. Keep the active tool in its own short row until it joins committed history. Expanding the group restores the original previews in order. Keep failures and approval requests visible. Do not change tool execution, model-visible history, raw output, or the full transcript. When disabled, preserve existing rendering.

### Compact status line

Join segments with `·` without spaces. Use `CtxN%` for context used, `CtxN%left` for remaining context, and `F:on` / `F:off` for fast mode. Lowercase model labels, remove `gpt-`, keep at most the first three letters, and append all version digits without separators: `GPT-6-Astra`, `GPT-6.1-Sol`, and `Terra 5.6` become `ast6`, `sol61`, and `ter56`. Apply the same limit to custom aliases; names without digits have no version suffix. Show only the first three letters of reasoning labels, including model-with-reasoning items: `medium`, `high`, and `xhigh` become `med`, `hig`, and `xhi`. Apply these labels to the footer, `/statusline` preview, and `/subagents` model info; preserve model identifiers, other picker labels, and terminal titles. Remove the space before the agent role only in the footer, such as `Main[default]`. Preserve paths, roles, colors, order, and single-row truncation.

Offer opt-in `cache-hit-rate` in `/statusline` and `tui.status_line`. Show `CchN.N%`: cached input tokens divided by input tokens for the latest reported request in the current thread, rounded to one decimal. Never average across requests or use cumulative thread totals. Use existing usage data without new requests or API changes. Hide unknown or zero-input usage; keep defaults unchanged. Missing cache details remain indistinguishable from reported zero.

If narrow checks cannot establish safety, explain the gap and ask before expanding scope. Report exact checks, results, skipped or blocked work, and remaining uncertainty. Do not claim upstream CI passed unless verified.

## Commit attribution

In `/subagents`, append known model and effort to both main and child titles using the compact status-line labels: `gpt-6.1-sol-high-fast` becomes `sol61-hig-fast`, and `gpt-6-astra-xhigh` becomes `ast6-xhi`. Add `-fast` only for a matching fast rule or inherited fast tier; a matching `default` rule suppresses inherited fast. Do not guess missing settings, remove thread ID descriptions, or change navigation. Labels describe configuration, not confirmed backend routing. Keep this TUI-only, without new app-server APIs or request tracking.
