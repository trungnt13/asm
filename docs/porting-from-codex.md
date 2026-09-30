# Personal fork guide

## Principles

1. **Personal use only.** This fork is for Trung Ngo.
2. **Keep changes minimal, safe, and minor.** Assume unchanged code was tested upstream. Check our changes, conflicts, and packages narrowly; upstream testing does not prove they work.
3. **Record current intent here.** Cover each fork change's purpose and constraints; update new or changed intent, not a task log. Ask before contradicting existing intent. Once approved, replace the old rule. Agents may then resolve mechanical conflicts without further approval.

This guide overrides other repository instructions, including [AGENTS.md](../AGENTS.md), where they conflict. Its validation and platform limits replace upstream defaults; other coding, formatting, test-authoring, and generator rules still apply. Intent recorded here is not proof of implementation, integration, or publication.

## Development workflow

### Start or resume

Check Git status, branches, worktrees, remotes, fork differences, and relevant repo sessions for overlapping work and unfinished handoffs. Recover context from Git and sessions, not the owner's memory or a separate task diary.

For concurrent tasks, create a named `codex/` branch and separate worktree before editing; coordinate shared files. One agent owns staging and commits in each checkout. Integrate into local `main` one task at a time.

Preserve existing work. Use a separate worktree if it blocks progress. Committing or stashing unrelated work needs permission; keep any authorized stash until restoration is verified. Never overwrite other work to make a checkout clean.

### Build the binary first

For runtime or UI changes, inspect all affected paths, including streaming and completed output. Make the smallest coherent change, then build from `codex-rs`:

```bash
cargo build -p codex-cli --bin codex --profile dev-small
```

Prioritize this build over competing Cargo jobs. Keep the native target, toolchain, profile, flags, and cache stable; switching profiles or cleaning caches can trigger rebuilds. Test executables are not the CLI; instant builds are not guaranteed.

Hand off the binary before automated checks finish when safe. Report its verified absolute path, build time, a manual check, and pending checks. Rebuild after runtime edits; never present an old binary as current or replace the installed `codex` without permission. Binary handoff is progress, not completion.

Use optimized or cross-platform builds locally only when requested or needed to reproduce the affected behavior.

### Validate narrowly

Choose checks from changed behavior and conflict resolutions, not the size of an upstream sync:

- **Documentation:** review wording and local links; run `git diff --check`. No builds or tests.
- **Rust:** use `just test -p <crate> <test-filter>`, not direct `cargo test`. Format changed code and use targeted Clippy when needed. Group affected assertion and snapshot updates; never disable tests to hide intentional output changes.
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
- Last incorporated upstream commit: `d42056091aded7feb1d88ac7e83972108b2aa478`.
- Commit date: 2026-09-30.
- Subject: Add a fork shortcut to the TUI command center (#49517).

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

Set `[workspace.package].version` in [`Cargo.toml`](../codex-rs/Cargo.toml), refresh [`Cargo.lock`](../codex-rs/Cargo.lock), and commit both before building or tagging. Keep that version on `main` until the next release. The tag, Cargo version, and CLI `--version` must agree, ignoring their name prefixes. Renaming a tag cannot change an existing binary.

### Build and package

Keep [`fork-rust-release.yml`](../.github/workflows/fork-rust-release.yml) separate from upstream's release workflow. Use GitHub-hosted runners:

- macOS ARM64: `aarch64-apple-darwin`, `macos-15`, 180-minute timeout.
- Linux x86_64 MUSL: `x86_64-unknown-linux-musl`, `ubuntu-24.04`, 90-minute timeout.

Published binaries use optimized `--release`, never `dev-small`. Set `CARGO_PROFILE_RELEASE_STRIP=debuginfo`; retain function symbols and optimization settings. Fix pipeline timeouts rather than weakening the profile. Upstream tests do not justify stripping more symbols or dependencies.

Build `codex` and `codex-code-mode-host` from the same commit, target, and profile. Each `codex-<target>.tar.gz` contains exactly those two regular executable siblings. Publish both target archives, `SHA256SUMS`, and [`install.sh`](../scripts/install/install.sh), not source trees or diagnostics. Check out the exact tag when adding the installer to the checksum manifest, even when reusing archives.

Before archive upload, run [the smoke check](../.github/scripts/smoke-codex-archive.py) on the packaged binaries on their native runner: CLI `--version` matches Cargo, and both `--help` commands succeed with usage. Do not require the helper to support `--version`. Record binary and archive sizes.

Keep these boundaries:

- No Apple Developer signing or notarization, paid Apple membership, Azure Key Vault, release secrets, or self-hosted runners.
- Use host `bwrap`, `rg`, and the system shell where needed. Do not bundle Bubblewrap, voice, patched zsh, or other helpers, or change runtime sandbox/security defaults for packaging.
- Retain Zig, [`install-musl-build-tools.sh`](../.github/scripts/install-musl-build-tools.sh), `AWS_LC_SYS_NO_JITTER_ENTROPY`, and verified fork-built V8 via [`setup-rusty-v8`](../.github/actions/setup-rusty-v8/action.yml).
- Do not add platforms, DMGs, bundled resources, npm, R2, WinGet, or website/OpenAI-only publishing without an agreed intent change.

### V8 dependency release

Build V8 separately from the CLI only when the resolved `v8` crate version lacks a fork release. Use [`fork-v8-release.yml`](../.github/workflows/fork-v8-release.yml) with tag `asm-v8-v<exact resolved v8 crate version>`. A manual branch run builds only by default; explicit `publish=true` builds and smokes first, then creates the tag and release at that commit. Tag pushes also publish. Never move a tag or replace published assets without approval. Reuse a verified fork V8 release when V8 inputs have not changed; if source, patches, build flags, or bindings change under the same crate version, ask how to version the dependency.

Build only sandbox + pointer-compression optimized pairs for macOS ARM64 and Linux x86_64 MUSL. Use the existing Bazel source pair and staging helper locally on GitHub-hosted runners, without BuildBuddy, remote execution, paid infrastructure, or broad suites. Keep the static library's required symbols. Run the native `codex-v8-poc` sandbox and JavaScript smoke tests on both targets, including MUSL. Publish only each target's archive, Rust binding, and two-file checksum manifest. The dependency release is normal but **not Latest**; it has no installer and does not affect CLI release discovery.

Fork CI and CLI releases consume only the matching verified fork V8 release. For a new V8 version, publish and verify both target pairs before the first `main` push that consumes it. Check each downloaded asset's GitHub SHA-256 digest and the manifest's exact target names and hashes. Missing or bad assets fail without upstream fallback. The inherited upstream V8 action path remains for workflows outside fork CI.

### Reuse and publish

Use [the artifact lookup](../.github/scripts/find-postmerge-artifacts.sh) to reuse both unexpired archives from a successful same-repository push-to-`main` run at the exact tagged, version-bumped commit. Wait for a matching active run; if it exceeds the wait limit, stop instead of building concurrently. Build in the release workflow only when no usable completed run remains. API errors are failures, not cache misses.

A `v*` tag push starts release publication; a `main` push does not. Normal manual dispatch publishes only on a `v*` tag; branch dispatch builds artifacts only. Require both archives. Mark releases normal and Latest, even with an `alpha` suffix.

Use explicit repository selection, such as `gh ... -R trungnt13/asm`. Verify the published tag's commit, release flags, both archives, installer, downloaded checksums, and version/help smoke results. Report the source commit and upstream baseline separately. A pushed tag or version label is not proof of a successful release or incorporated code.

### Installer

The published installer uses only `trungnt13/asm` GitHub `v*` releases for macOS ARM64 and Linux x86_64 MUSL. Verify GitHub SHA-256 digests and `SHA256SUMS` before installing the two-binary archive. Reject unsupported targets before changing install state.

Preserve `--release`, `CODEX_HOME`, `CODEX_INSTALL_DIR`, install locking, and safe `current` selection. Store packages under `packages/asm-standalone`; leave upstream packages and update markers untouched. Keep the shared Codex config/state home and `codex` command.

Updates are manual. Do not fetch OpenAI/CDN or legacy npm packages, support daemon-only installation, or write `auto-update-version`. Built-in update checks, prompts, commands, and daemon update loops are disabled in ASM, regardless of upstream settings or markers. Updates are installed externally by the owner; the CLI and daemon must not download or install upstream releases. Keep ordinary daemon startup and restart separate from updating.

### Build experiment

The optional `macos_concurrency_experiment` compares cold builds of one commit on `macos-15` with Cargo job limits 2 and 3. Keep other settings equal and save separate timing, resource, size, and smoke results. Never reuse postmerge artifacts or publish, even on a tag. Measure before changing defaults; one run is not proof of a general speedup.

## Intentional behavior differences

### ASM branding

Use ASM in the README heading, repository description, release titles and introductory prose, workflow step descriptions, and the fork's own TUI-visible labels and terminal title. Credit OpenAI Codex where origin is described. Keep real upstream service and product names, and keep `codex` executable, CLI version prefix, crate, config/state, archive, tag, workflow, job, and check identifiers unchanged. Branding does not redirect upstream links or installers.

### Background terminal waits

Allow top-level `background_terminal_min_timeout` and `background_terminal_max_timeout` in milliseconds, defaulting to 5000 and 300000. Reject zero, reversed, or unrepresentable bounds. Clamp empty `write_stdin` polls to that range and show it in the tool description. Polls end early when the process exits.

Do not change initial command waits, nonempty stdin waits, or code-mode's outer `exec`/`wait` limits. Process exit alone does not resume an idle model turn.

### Memory reasoning effort

Under `[memories]`, allow optional `extract_reasoning_effort` and `consolidation_reasoning_effort` for both V1 and V2, using the existing effort type. When omitted, retain extraction's `low` and consolidation's `medium`, independently of parent effort. Preserve model selection and other memory behavior.

### Transcript spacing

Keep Markdown paragraphs, code blocks, and list items adjacent while streaming and after completion. Preserve blank lines inside code blocks, raw output, message boundaries, and user-message padding.

### Compact status line

Join segments with `·` without spaces. Use `CtxN%` for context used, `CtxN%left` for remaining context, and `F:on` / `F:off` for fast mode. Lowercase model labels, remove `gpt-` and dots, and put numeric versions after the family: `GPT-6.1-Sol` becomes `sol61`. Show only the first three letters of reasoning labels, including model-with-reasoning items: `medium`, `high`, and `xhigh` become `med`, `hig`, and `xhi`. Apply these labels to the footer and `/statusline` preview only; preserve model identifiers, picker labels, and terminal titles. Remove the space before the agent role only in the footer, such as `Main[default]`. Preserve paths, roles, colors, order, and single-row truncation.

Offer opt-in `cache-hit-rate` in `/statusline` and `tui.status_line`. Show `CchN.N%`: cached input tokens divided by input tokens for the latest reported request in the current thread, rounded to one decimal. Never average across requests or use cumulative thread totals. Use existing usage data without new requests or API changes. Hide unknown or zero-input usage; keep defaults unchanged. Missing cache details remain indistinguishable from reported zero.

### Subagent service tiers and picker

Allow `[subagent_service_tiers]`, such as `gpt-6-sol = { high = "fast" }`, keyed by the child's final model and effective effort after overrides and role settings. Matching rules override the root tier throughout the child's lifetime, including root tier changes; unmatched children inherit upstream behavior. Keep root requests unchanged. Reject unsupported matched tiers and fast overrides when fast mode is disabled. Do not add this policy to spawn arguments.

In `/subagents`, append known model and effort to the title, such as `/root/sol_hello gpt-6-sol-high`. Add `-fast` only for a matching fast rule or inherited fast tier; a matching `default` rule suppresses inherited fast. Do not guess missing settings, remove thread ID descriptions, or change navigation. Labels describe configuration, not confirmed backend routing. Keep this TUI-only, without new app-server APIs or request tracking.
