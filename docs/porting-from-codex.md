# Personal fork guide

## Principles

1. **Personal use only.** This fork is for Trung Ngo.
2. **Keep changes minimal, safe, and minor.** Prefer durable, localized solutions that minimize upstream divergence and future merge or rebase conflicts. Do not sacrifice correctness, maintainability, or runtime performance merely to reduce the diff. Assume unchanged code was tested upstream. Check our changes, conflicts, and packages narrowly; upstream testing does not prove they work.
3. **Record current intent here.** Cover each fork change's purpose and constraints; update new or changed intent, not a task log. Ask before contradicting existing intent. Once approved, replace the old rule. Agents may then resolve mechanical conflicts without further approval.

This guide overrides other repository instructions, including [AGENTS.md](../AGENTS.md), where they conflict. Its validation and platform limits replace upstream defaults; other coding, formatting, test-authoring, and generator rules still apply. Intent recorded here is not proof of implementation, integration, or publication.

## Development workflow

### Start or resume

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
- Last incorporated upstream commit: `e7ea5f4a8658ebe49e879be933effed2340fa276`.
- Commit date: 2026-10-02.
- Subject: Add managed worktree tools to the TUI (#50148).

Verify the upstream URL before fetching. Follow the requested sync method; otherwise merge into `main` to preserve published history and rebase only unpublished branches. Use `git cherry-pick -x` for targeted ports.

A full sync includes the selected upstream `main` commit and all ancestors. Capture the old baseline and selected commit before syncing; use that fixed range for the report. Focus on fork differences and affected upstream changes. Apply the integration and validation rules above.

After success, advance the marker to the incorporated commit; a cherry-pick does not advance it. Verify its hash, date, and subject with `git show -s --format='%H%n%cs%n%s' <upstream-commit>` and confirm it is an ancestor of the fork branch.

## Platforms and CI

Focus on macOS and Linux (Ubuntu). Preserve inherited code and workflows for other platforms unless removal is requested; do not wire them into fork CI or releases. Other inherited workflows stay unchanged unless authorized. Check their triggers: [V8 canary](../.github/workflows/v8-canary.yml) still triggers on pull requests, with expensive builds conditional on relevant changes; [CLA](../.github/workflows/cla.yml) restricts its job to `openai`.

Keep [`blocking-ci.yml`](../.github/workflows/blocking-ci.yml) manual-only (`workflow_dispatch`), not automatic or a required branch check. A manual run checks workspace formatting and production Clippy for `codex-cli`, `codex-tui`, `codex-core`, and `codex-config` on both release targets, plus a result collector. Preserve `--lib --bin codex -- -D warnings`; no test-target matrix, `cargo shear`, or suppressed warnings. Add narrow checks for other affected packages when needed.

Keep [postmerge CI](../.github/workflows/postmerge-ci.yml) automatic: optimized builds, packaging, smoke checks, artifact uploads, diagnostics, and results. Release publication does not depend on manual blocking CI; report existing failures separately.

Port useful upstream action, toolchain, security, and build fixes without restoring broad matrices. Do not add unrelated platforms, Bazel suites, SDKs, remote executors, V8 source-build canaries, or OpenAI-only infrastructure to fork CI, or repeat CI locally for every edit. The separate two-target V8 dependency build below is not a broad Bazel suite or canary.

## Releases

### Version

On a release request, find the latest published, non-draft upstream prerelease with a `rust-v` tag on `openai/codex`, ordered by publication time. Increment only its numeric patch component by one and preserve the suffix: `rust-v0.159.0-alpha.6` becomes `0.159.1-alpha.6`, tagged `v0.159.1-alpha.6`.

Do not increment the previous fork version or suffix. Check remote fork tags first. If the derived tag exists or no upstream prerelease can be determined, ask; do not invent a version or move a tag.

Prepare the version on an immutable release-candidate branch, not on `main`. Set `[workspace.package].version` in [`Cargo.toml`](../codex-rs/Cargo.toml) and the matching workspace package versions in [`Cargo.lock`](../codex-rs/Cargo.lock), without changing dependencies. The tag, Cargo version, and CLI `--version` must agree, ignoring their name prefixes. Renaming a tag cannot change an existing binary. Advance `main` only after publication and verification succeed; preserve unrelated concurrent changes and stop on version conflicts rather than force-pushing. Leave existing pre-policy version bumps intact until the next successful release.

### Build and package

Keep [`fork-rust-release.yml`](../.github/workflows/fork-rust-release.yml) separate from upstream's release workflow. Use GitHub-hosted runners:

- macOS ARM64: `aarch64-apple-darwin`, `macos-15`, 180-minute timeout.
- Linux x86_64 glibc: `x86_64-unknown-linux-gnu`, `ubuntu-22.04`, 90-minute timeout. Ubuntu 22.04 / glibc 2.35 is the minimum supported Linux baseline. Keep Linux build and native smoke jobs on that baseline; changing only the target triple on a newer runner is not sufficient. Before GitHub retires this runner in April 2027, move the same build environment into an Ubuntu 22.04 container on a supported runner rather than raising the runtime requirement.

Published binaries use optimized `--release`, never `dev-small`. Set `CARGO_PROFILE_RELEASE_STRIP=debuginfo`; retain function symbols and optimization settings. Fix pipeline timeouts rather than weakening the profile. Upstream tests do not justify stripping more symbols or dependencies.

Build `codex` and `codex-code-mode-host` from the same commit, target, and profile. Each `codex-<target>.tar.gz` contains exactly those two regular executable siblings. Publish both target archives, `SHA256SUMS`, and [`install.sh`](../scripts/install/install.sh), not source trees or diagnostics. Check out the exact tag when adding the installer to the checksum manifest, even when reusing archives.

Before archive upload, run [the smoke check](../.github/scripts/smoke-codex-archive.py) on the packaged binaries on their native runner: CLI `--version` matches Cargo, and both `--help` commands succeed with usage. Do not require the helper to support `--version`. Record binary and archive sizes. For GNU Linux archives, verify both ELF executables use the x86_64 GNU loader and require no glibc symbol newer than 2.35. Also run version/help checks in a clean Ubuntu 22.04 container with only the declared runtime libraries; a dependency-rich build runner does not establish runtime compatibility.

Keep these boundaries:

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

Use explicit repository selection, such as `gh ... -R trungnt13/asm`. Verify the published tag's commit, release flags, both archives, installer, downloaded checksums, and version/help smoke results. Report the source commit and upstream baseline separately. A pushed tag or version label is not proof of a successful release or incorporated code.

### Installer

The published installer uses only `trungnt13/asm` GitHub `v*` releases for macOS ARM64 and Linux x86_64 GNU. Verify GitHub SHA-256 digests and `SHA256SUMS` before installing the two-binary archive. Reject unsupported targets and Linux hosts without glibc 2.35+ before downloading release metadata or changing install state. Select only GNU Linux archives; never silently fall back to MUSL. A MUSL-only historical release requires its original installer. A GNU update uses a separate target-qualified package directory and preserves old MUSL packages and shared config/state.

Preserve `--release`, `CODEX_HOME`, `CODEX_INSTALL_DIR`, install locking, and safe `current` selection. Store packages under `packages/asm-standalone`; leave upstream packages and update markers untouched. Keep the shared Codex config/state home and `codex` command.

Updates are manual. Do not fetch OpenAI/CDN or legacy npm packages, support daemon-only installation, or write `auto-update-version`. Built-in update checks, prompts, commands, and daemon update loops are disabled in ASM, regardless of upstream settings or markers. Updates are installed externally by the owner; the CLI and daemon must not download or install upstream releases. Keep ordinary daemon startup and restart separate from updating.

### Build experiment

The optional `macos_concurrency_experiment` compares cold builds of one commit on `macos-15` with Cargo job limits 2 and 3. Keep other settings equal and save separate timing, resource, size, and smoke results. Never reuse postmerge artifacts or publish, even on a tag. Measure before changing defaults; one run is not proof of a general speedup.

## Intentional behavior differences

### Saved forks, upstream sides, and parallel conversations

Preserve the parent's effective prompt-cache routing key for saved and temporary root forks, independently of thread/session identity. Persist that routing choice for resume and further forks; keep old rollouts readable without ancestor lookups. Preserve upstream guardian and subagent routing. Sharing routing is an optimization, not a promise of backend cache hits.

Keep `/side` and its `/btw` alias aligned with upstream: temporary forks, upstream command restrictions and reference-only instructions, no subagents, and no saved pairing metadata. Ctrl+/ switches views; Ctrl+C returns to the parent and discards the temporary side. Ordinary picker navigation discards temporary sides as upstream does. Preserve upstream `/side` helper names, constants, and tests where practical; add parallel-specific behavior without unnecessary rewrites of the upstream path.

Use `/parallel` for the saved ordinary user fork with normal command availability under permissions, feature flags, platform support, and busy-state checks. Inherited history is reference context, not a request to continue the parent's task or control its agents. Parallel chats may own goals and subagents. Keep the initial transcript clean without rewriting stored or model-visible history.

Share one main/companion pair without nesting. Starting a companion from a parallel chat returns to its parent and replaces the selected companion; old parallel chats remain saved. Temporary sides keep upstream restrictions, including rejecting another companion command inside them. Ctrl+/ switches without stopping work. With an empty composer and no modal, Ctrl+C stops the parallel chat, pauses its active goal, and returns to its parent without deleting history. Selecting its subagents must not stop it. Ordinary `/new`, `/clear`, `/resume`, `/fork`, `/cd`, and `/worktree` navigation leaves parallel mode. Archive/delete act on the displayed parallel chat and return to its parent.

Keep parallel pairing and transcript-display boundaries in ASM client-local state scoped to the app-server target. Read existing saved-side records as parallel chats without migrating files or rewriting history. Restore an open pair on resume; closing the selection must not delete its saved conversation. Do not use analytics source classification or false fork ancestry to encode UI relationships. Stock clients see ordinary threads through unchanged app-server APIs; their UI need not implement companion switching. Shared files and explicitly global configuration changes are not isolated between chats.

### Recap session ID

Append `Session: <id>` as the final dimmed line of manual and automatic TUI recaps, after the optional next action. Use the displayed conversation’s session ID, not the temporary recap-generation thread ID. Keep recap generation and timing unchanged.

### Copy session ID

Offer `/copyid` to copy only the currently displayed session's ID directly through the existing clipboard path, without requiring `/status`, an assistant response, or a picker. Allow it while a task is running, in side conversations, and when viewing parent-owned subagents. Report missing IDs and clipboard failures; preserve `/copy` behavior and clipboard platform support.

### ASM branding

Use ASM in the README heading, repository description, release titles and introductory prose, workflow step descriptions, and the fork's own TUI-visible labels and terminal title. Credit OpenAI Codex where origin is described. Keep real upstream service and product names, and keep `codex` executable, CLI version prefix, crate, config/state, archive, tag, workflow, job, and check identifiers unchanged. Branding does not redirect upstream links or installers.

### Background terminal waits

Allow top-level `background_terminal_min_timeout` and `background_terminal_max_timeout` in milliseconds, defaulting to 5000 and 300000. Reject zero, reversed, or unrepresentable bounds. Clamp empty `write_stdin` polls to that range and show it in the tool description. Polls end early when the process exits.

Do not change initial command waits, nonempty stdin waits, or code-mode's outer `exec`/`wait` limits. Process exit alone does not resume an idle model turn.

### Memory reasoning effort

Under `[memories]`, allow optional `extract_reasoning_effort` and `consolidation_reasoning_effort` for both V1 and V2, using the existing effort type. When omitted, retain extraction's `low` and consolidation's `medium`, independently of parent effort. Preserve model selection and other memory behavior.

### Transcript spacing

Keep Markdown paragraphs, code blocks, and list items adjacent while streaming and after completion. Preserve blank lines inside code blocks, raw output, message boundaries, and user-message padding.

### Collapsed tool calls

Offer opt-in `[tui] collapse_tool_calls = true`, defaulting to `false`. In the rich fullscreen transcript, group consecutive shell, MCP, and dynamic tool calls into a single-row summary with the call count and distinct command or tool names in first-use order. Shorten overflowing name lists with `+N more`. Other activity types and chat messages separate groups. Keep the active tool in its own short row until it joins committed history. Expanding the group restores the original previews in order. Keep failures and approval requests visible. Do not change tool execution, model-visible history, raw output, or the full transcript. When disabled, preserve existing rendering.

### Compact status line

Join segments with `·` without spaces. Use `CtxN%` for context used, `CtxN%left` for remaining context, and `F:on` / `F:off` for fast mode. Lowercase model labels, remove `gpt-`, keep at most the first three letters, and append all version digits without separators: `GPT-6-Astra`, `GPT-6.1-Sol`, and `Terra 5.6` become `ast6`, `sol61`, and `ter56`. Apply the same limit to custom aliases; names without digits have no version suffix. Show only the first three letters of reasoning labels, including model-with-reasoning items: `medium`, `high`, and `xhigh` become `med`, `hig`, and `xhi`. Apply these labels to the footer, `/statusline` preview, and `/subagents` model info; preserve model identifiers, other picker labels, and terminal titles. Remove the space before the agent role only in the footer, such as `Main[default]`. Preserve paths, roles, colors, order, and single-row truncation.

Offer opt-in `cache-hit-rate` in `/statusline` and `tui.status_line`. Show `CchN.N%`: cached input tokens divided by input tokens for the latest reported request in the current thread, rounded to one decimal. Never average across requests or use cumulative thread totals. Use existing usage data without new requests or API changes. Hide unknown or zero-input usage; keep defaults unchanged. Missing cache details remain indistinguishable from reported zero.

### Subagent service tiers and picker

Allow `[subagent_service_tiers]`, such as `gpt-6-sol = { high = "fast" }`, keyed by the child's final model and effective effort after overrides and role settings. Matching rules override the root tier throughout the child's lifetime, including root tier changes; unmatched children inherit upstream behavior. Keep root requests unchanged. Reject unsupported matched tiers and fast overrides when fast mode is disabled. Do not add this policy to spawn arguments.

In `/subagents`, append known model and effort to both main and child titles using the compact status-line labels: `gpt-6.1-sol-high-fast` becomes `sol61-hig-fast`, and `gpt-6-astra-xhigh` becomes `ast6-xhi`. Add `-fast` only for a matching fast rule or inherited fast tier; a matching `default` rule suppresses inherited fast. Do not guess missing settings, remove thread ID descriptions, or change navigation. Labels describe configuration, not confirmed backend routing. Keep this TUI-only, without new app-server APIs or request tracking.
