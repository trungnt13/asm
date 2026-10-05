# Fork purpose and upstream sync

## Principles

1. **Personal use only:** this fork is for Trung Ngo.
2. **Keep changes minimal, safe, and minor.** Prefer durable local fixes with few upstream differences or future conflicts. Do not reduce correctness, maintainability, or runtime performance to shorten a diff. Assume upstream tested unchanged code. Do narrow checks for fork changes, conflicts, and packages; upstream tests do not prove these work.
3. **Record current intent here:** each fork change's purpose and constraints, not a task log. Ask before contradicting existing intent. After approval, replace the old rule. Agents can then resolve mechanical conflicts without more approval.

This guide overrides conflicting repository instructions, including [AGENTS.md](../AGENTS.md). Its validation and platform limits replace upstream defaults. Other coding, formatting, test-authoring, and generator rules still apply. Recorded intent does not prove implementation, integration, or publication.

## ASM branding

Use ASM in the README heading, GitHub repository description and release titles, introductory release prose, and descriptive workflow step labels, while clearly attributing the fork to OpenAI Codex. Keep the existing `codex` executable, crate, state and config names, archive filenames, tags, workflow names, job IDs, and CI check identities unchanged. Branding does not change what upstream links or installers provide.

- Refresh Git status, branches, and worktrees before edits, including after resume.
- For syncs and pushes, inspect remotes and fork history. For overlap or unexplained changes, inspect relevant repo sessions. Recover context from Git and sessions, not owner memory or a task diary.
- For concurrent tasks, create a named `codex/` branch and separate worktree before edits. Coordinate shared files. Assign one agent to staging and commits per checkout. Integrate one task at a time into local `main`.
- Preserve existing work. If it blocks progress, use another worktree. Get permission before committing or stashing unrelated work. Keep authorized stashes until restoration is verified. Never overwrite other work to clean a checkout.

### Build the binary first

For runtime or UI edits, inspect all affected paths, including streaming and completed output. Make the smallest coherent change.

Before Cargo runs in a new worktree, ensure sufficient disk space. Select a compatible existing cache with `CARGO_TARGET_DIR`, not an empty worktree-local cache. Keep the native target, toolchain, profile, flags, and cache stable. Cache reuse can still require recompilation. Reuse matching verified fork [V8 artifacts](#v8-dependency-release); keep `RUSTY_V8_ARCHIVE` and `RUSTY_V8_SRC_BINDING_PATH` overrides stable with that cache.

From `codex-rs`, build both binaries from the same source, native target, and `dev-small` profile:

```bash
cargo build -p codex-cli -p codex-code-mode-host --bin codex --bin codex-code-mode-host --profile dev-small
```

Prioritize this pair over other Cargo jobs. Serialize jobs that share a target directory. Keep executable siblings in `${CARGO_TARGET_DIR:-target}/dev-small`; verify which checkout produced them. During builds, inspect independent code and review affected tests or snapshots. Test executables are not these binaries.

For ordinary local runtime or UI checks, run `"${CARGO_TARGET_DIR:-target}/dev-small/codex" --no-daemon` from `codex-rs`. This uses the current embedded backend without changing or stopping a shared daemon; the sibling `codex-code-mode-host` supports code mode. Test daemon or remote behavior separately when it matters: `codex agents` and `--remote` cannot use `--no-daemon`.

Before handoff, verify both executables exist, CLI `--version`, and `--help` for each. A help check does not prove model requests or tools work. When safe, hand off both before automated checks finish. Report their verified absolute paths, build time, a manual check, and pending checks. Rebuild both after runtime edits. Never present old binaries as current. Get permission before replacing installed `codex`.

Use optimized or cross-platform local builds only on request or to reproduce affected behavior.

### Validate narrowly

Select checks for changed behavior and conflict resolutions, not sync size.

- **Documentation:** review wording and local links. Run `git diff --check`. Do not build or test.
- **Rust:** use `just test -p <crate> <test-filter>`, not direct `cargo test`. Format changed code. Use targeted Clippy when needed. Update affected assertions and snapshots together before checks. Exclude unrelated stale expectations. Never disable tests to hide intended output changes.
- **Workflows/scripts:** run `actionlint` on changed workflows and small fixtures for script logic. Build natively only for affected packaging or approved experiments.

New tests or snapshot coverage require an explicit user request; implementation requests alone are insufficient. By default, update existing tests and snapshots only as needed for changed behavior. This overrides requirements to add integration or snapshot tests. Retain existing checks. Report gaps and ask before adding tests.

Run required generators for changed inputs. Keep schemas, dependency locks, and Bazel data correct, even without Bazel CI. Reuse correct upstream outputs.

Do not default to whole-crate or workspace suites. If narrow checks cannot establish safety, explain the gap and ask before expanding. A clean merge is not validation. Claim upstream CI passed only after verification.

### Commit, integrate, report

Implementation requests, including documentation edits, authorize commits and local `main` integration unless the owner says otherwise. The lead agent owns completion, including delegated work. After narrow checks pass and owner decisions are resolved, commit immediately. Then integrate. Wait for manual testing or separate approval only on request. **Done means committed and verified in local `main`**, not another worktree or a binary handoff. Releases follow separate completion rules below.

- Inspect the staged diff. Include only finished task changes, their tests, generated files, and intent.
- Use `Trung Ngo <1390402+trungnt13@users.noreply.github.com>`. Verify `git var GIT_AUTHOR_IDENT` and `git var GIT_COMMITTER_IDENT`. Correct local configuration if needed. Get approval before rewriting published history solely for attribution.
- Recheck the target branch before integration. Compare base, fork, and upstream when resolving conflicts against this guide. Verify affected behavior. For owner decisions, inspect all exposed conflicts, then ask one question with recommendations. Later commits can expose more conflicts.
- Reconcile task-owned duplicate edits without losing other work. If integration is blocked, commit finished work and report **ready, integration blocked**, with the reason. Identify unfinished work separately.
- Report changes, exact checks/results, skipped checks, uncertainty, commit, integration, and push status. Give the branch or worktree for remaining work.

### Push permissions

`origin/main` pushes, including `--force-with-lease`, have standing approval but are optional for local completion. Other force pushes, tag moves, discarding work, and releases require explicit approval. Implementation alone does not authorize publication.

Push branches with an explicit ref and `--no-follow-tags`, never all local tags. Release-triggering tag pushes require publication approval, including inherited workflow tags. Before force pushes, fetch and account for remote-only work, including workflow-created release commits. Preserve that work. Use an explicit lease against the inspected remote tip. Never refresh a failed lease and retry blindly.

## Upstream sync

- Fork (`origin`): [`trungnt13/asm`](https://github.com/trungnt13/asm).
- Parent (`upstream`): [`openai/codex`](https://github.com/openai/codex).
- Last incorporated upstream commit: `7f892275e31002f0422477c6219189284560e689`.
- Commit date: 2026-10-04.
- Subject: Isolate tracing in the strict third-party tool deferral test (#50977).

Verify the upstream URL before fetching. Use the requested sync method. Otherwise, merge into `main` to preserve published history; rebase only unpublished branches. Use `git cherry-pick -x` for targeted ports.

A full sync includes the selected upstream `main` commit and all ancestors. Capture the old baseline and target before sync. Report that fixed range. Focus on fork differences and affected upstream changes. Apply the integration and validation rules above.

After successful full sync, advance the marker; cherry-picks do not advance it. Verify hash, date, and subject with `git show -s --format='%H%n%cs%n%s' <upstream-commit>`. Confirm the marker is an ancestor of the fork branch.

Follow the requested sync method. Without a specified method, merge to preserve published history; rebase unpublished branches when useful. A requested rebase permits local history rewriting, not a force push unless that is also authorized. Use `git cherry-pick -x` for targeted ports and `codex/` for new branch names. Do not discard work, rewrite remote history, move existing release tags, or publish releases without explicit authorization.

Focus on macOS and Linux (Ubuntu). Preserve inherited code and workflows for other platforms unless removal is requested. Do not add them to fork CI or releases. Other inherited workflows also need approval for changes. Inspect triggers: [V8 canary](../.github/workflows/v8-canary.yml) runs on pull requests, with expensive builds conditional on relevant changes. [CLA](../.github/workflows/cla.yml) restricts its job to `openai`.

- **[Blocking CI](../.github/workflows/blocking-ci.yml):** manual-only `workflow_dispatch`, never automatic or a required branch check. Keep workspace formatting, production Clippy, and the result collector. Clippy covers `codex-cli`, `codex-tui`, `codex-core`, and `codex-config` on both release targets. Preserve `--lib --bin codex -- -D warnings`. Do not add a test-target matrix, `cargo shear`, or warning suppression. Add narrow checks for other affected packages as needed.
- **[Postmerge CI](../.github/workflows/postmerge-ci.yml):** automatic optimized builds, packaging, smoke checks, uploads, diagnostics, and results. Set push-level `paths-ignore` only for root `AGENTS.md` and this guide. Filtered pushes cannot cancel active builds. Other paths remain eligible; do not exclude all Markdown or `docs/`. Release publication does not require manual blocking CI. Report existing failures separately.

Port useful upstream action, toolchain, security, and build fixes without broad matrices. Do not repeat CI locally for each edit. Do not add unrelated platforms, Bazel suites, SDKs, remote executors, V8 source-build canaries, or OpenAI-only infrastructure to fork CI. The two-target V8 dependency build below is not a broad suite or canary.

Leave other inherited workflows unchanged unless the task authorizes changes. Check their own triggers and repository guards: [V8 canary](../.github/workflows/v8-canary.yml) still triggers on pull requests, with expensive builds conditional on relevant changes; [CLA](../.github/workflows/cla.yml) restricts its job to the `openai` owner. The custom CI scope does not disable these workflows.

### Operate a release

1. Require an explicit release request. Select the repository explicitly, for example with `gh ... -R trungnt13/asm`. Before pushing source to `origin/main`, verify the V8 prerequisite below. Release the intended commit from `origin/main`, not unpushed local work. Candidates build or reuse their own archives; do not wait for postmerge builds.
2. Dispatch [`fork-rust-release.yml`](../.github/workflows/fork-rust-release.yml) on `main` with `publish_release=true`. The workflow owns version selection, candidate creation, builds, publication, verification, and remote `main` updates. Do not repeat these steps manually. Ordinary branch dispatch is build-only. A `main` push does not request publication.
3. After interruption, rerun failed jobs. For a new resume dispatch, use `main`, `publish_release=true`, and `resume_run_id=<original run ID>`. Keep the original version and commit even if upstream advances. Report failures that need owner decisions. Do not invent versions or replace tags.
4. A platform is available as soon as its checked archive and platform checksum manifest are published. The whole release is complete only when `release`, both `verify_native` jobs, and `sync_main` succeeded for that candidate. Legacy `v*` tag pushes and dispatches can publish. Require publication and both native checks. Skip `sync_main` for legacy releases. Inspect that run's recorded checks and current release metadata. Do not repeat successful checks on unchanged artifacts. A green build-only run, pushed tag, or version label does not prove publication.
5. Fetch the result. Fast-forward local `main` only when safe. Preserve local edits and concurrent commits. Report the released commit, upstream baseline, checks, and remote/local integration status, including blocked updates.

### Workflow requirements

These requirements govern release code, not a second manual release procedure.

#### Version

- Select the latest published, non-draft upstream `rust-v` prerelease by publication time. Increment only its numeric patch by one. Keep the suffix: `rust-v0.159.0-alpha.6` → tag `v0.159.1-alpha.6`. Never increment the previous fork version or suffix. If the derived remote tag exists or no upstream prerelease is available, stop. Ask the owner how to proceed.
- Create immutable candidate branch `agent/release-<run-id>`, not a version commit on `main`. Set `[workspace.package].version` in [`Cargo.toml`](../codex-rs/Cargo.toml) and matching workspace versions in [`Cargo.lock`](../codex-rs/Cargo.lock). Do not change dependencies.
- Tag, Cargo, and CLI `--version` must agree, except name prefixes. A tag rename cannot change binary contents. Never move candidates or existing tags during retries. Keep candidate branches for recovery. Retain pre-policy version bumps until the next successful release.

#### Build and package

Keep the fork release workflow separate from upstream. Use GitHub-hosted runners:

| Platform | Target | Runner | Build timeout |
| --- | --- | --- | --- |
| macOS ARM64 | `aarch64-apple-darwin` | `macos-15` | 180 minutes |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | 90 minutes |

Legacy artifact lookup has a 190-minute step limit. Its wait guard is 150 minutes for macOS and 185 minutes for Linux. A timeout fails rather than starting a duplicate build. Only legacy tag jobs allow 360 minutes total for lookup, build, and checks. Candidate jobs allow the build limit plus 15 minutes for publication; the actual build keeps the limits above. Fresh candidates do not wait for postmerge builds.

Ubuntu 22.04 / glibc 2.35 is the Linux minimum. Keep Linux builds and native smoke jobs on that baseline. Changing only the target triple on a newer runner is insufficient. Before runner retirement in April 2027, move this environment into an Ubuntu 22.04 container on a supported runner. Do not raise the runtime minimum.

- Use optimized `--release`, never `dev-small`. Set `CARGO_PROFILE_RELEASE_STRIP=debuginfo`. Retain function symbols and optimization settings. Fix pipeline timeouts without weakening the profile. Upstream tests do not justify more symbol or dependency removal.
- Build `codex` and `codex-code-mode-host` from one commit, target, and profile. Each `codex-<target>.tar.gz` must contain exactly these two regular executable siblings. The completed release has six assets: both archives, `SHA256SUMS-<target>` for each target, aggregate `SHA256SUMS`, and [`install.sh`](../scripts/install/install.sh). Do not publish source trees or diagnostics. Each platform manifest covers its archive and the pinned installer.
- Pin builds, packaging, installer bytes, payload checks, and archive reuse to the immutable candidate commit SHA. Pin orchestration scripts to the dispatch SHA; preserve those bytes before checking out the candidate. Never fetch newer controls mid-run. For legacy releases, use the exact tagged commit SHA. Verify the release tag resolves to that commit. Candidate packaging can precede tag creation.
- Before upload, run [native package smoke checks](../.github/scripts/smoke-codex-archive.py). CLI `--version` must match Cargo. Both `--help` commands must succeed with usage. Do not require helper `--version`. Record binary and archive sizes.
- For GNU Linux, verify both ELF executables use the x86_64 GNU loader and no glibc symbols newer than 2.35. Run version/help checks in a clean Ubuntu 22.04 container with only declared runtime libraries. The build runner alone does not prove compatibility.

Keep these limits:

- No Apple Developer signing/notarization, paid Apple membership, Azure Key Vault, release secrets, or self-hosted runners.
- Use host `bwrap`, `rg`, and the system shell as needed. Do not bundle Bubblewrap, voice, patched zsh, or other helpers. Do not change sandbox/security defaults for packaging.
- For fork Linux builds, use the native GNU toolchain and [`install-gnu-build-tools.sh`](../.github/scripts/install-gnu-build-tools.sh). Do not use Zig or MUSL wrappers there. Retain inherited MUSL tooling for upstream workflows.
- Retain `AWS_LC_SYS_NO_JITTER_ENTROPY` and verified fork V8 through [`setup-rusty-v8`](../.github/actions/setup-rusty-v8/action.yml). Install Python 3.11+ explicitly on Ubuntu 22.04 for artifact verification.
- Preserve upstream GNU runtime choices, including the system allocator, locale handling, and PTY support. Do not change Rust platform conditions merely to mimic MUSL. GNU packages use system OpenSSL 3 and can use system liblzma. Match [`smoke-ubuntu-archive.sh`](../.github/scripts/smoke-ubuntu-archive.sh) runtime packages to ELF dependencies. Do not add ALSA or GStreamer requirements for the GNU migration.
- Get an agreed intent change before adding platforms, DMGs, bundled resources, npm, R2, WinGet, or website/OpenAI-only publishing.

#### V8 dependency release

Build V8 separately only when its crate version, target, or baseline lacks a verified fork release. Use [`fork-v8-release.yml`](../.github/workflows/fork-v8-release.yml), tag `asm-v8-v<exact resolved v8 crate version>-glibc2.35`. This suffix identifies the GNU-baseline generation and matching macOS pair. Do not move or replace the old MUSL release.

- Manual branch dispatch is build-only by default. Explicit `publish=true` builds and smokes, then creates the tag and release at that commit. Tag pushes also publish. Tag moves or published asset replacement require approval.
- Reuse verified V8 when inputs are unchanged. If source, patches, flags, or bindings change under the same crate version, ask how to version the dependency.
- Build only optimized sandbox + pointer-compression pairs for macOS ARM64 and Linux x86_64 GNU. Use the existing Bazel source pair and staging helper locally on GitHub-hosted runners. No BuildBuddy, remote execution, paid infrastructure, or broad suites. Retain required static-library symbols.
- Require native `codex-v8-poc` sandbox and JavaScript smoke tests to pass on both targets, including GNU on Ubuntu 22.04. Reuse the GNU Bazel platform and older glibc sysroot.
- Publish each target's archive, Rust binding, and two-file checksum manifest only. Publish a normal release, **not Latest**, without an installer. Keep it separate from CLI release discovery.
- For a new V8 version or artifact generation, publish both target pairs before the first consuming `main` push. Verify both pairs before that push. Fork CI and CLI releases consume only matching verified fork V8. Verify downloaded GitHub SHA-256 digests and exact manifest target names/hashes. Fail on missing or bad assets; no upstream fallback. Retain the upstream action path for workflows outside fork CI.

#### Reuse and publish

Serialize release runs. The workflow must finish without later chat actions or workflows triggered by token-created pushes. Legacy tags must already have the correct Cargo version.

Use [artifact lookup](../.github/scripts/find-postmerge-artifacts.sh) per target for unexpired, smoke-checked archives from the current dispatch or original candidate run. Do not wait for the other platform or rebuild a published, ready platform. Legacy tags can reuse successful target jobs from same-repository push-to-`main` archives at the exact tagged commit SHA. Wait for a matching active run instead of concurrent builds. Fail if the wait expires. Build only without usable archives. Treat API errors as failures, not cache misses.

1. Create the pinned tag, draft release, and common installer once. Never move the tag, replace assets, or change the candidate. Resume existing uploads only when names, sizes, and digests agree.
2. Each platform builds or reuses its own archive, passes native package checks, and uploads it independently. Verify the uploaded archive and installer against GitHub digests, then smoke the downloaded archive. Upload `SHA256SUMS-<target>` last; it is the readiness marker.
3. Publish a normal, non-draft release when the first platform is ready, including `alpha` versions. Keep it **not Latest** while incomplete. Append the other platform's checked assets to the same release. Published payloads are immutable by fork policy; GitHub's immutable-release feature must not be enabled for this append-only flow. Never change that repository setting automatically.
4. After both platforms are verified, append the aggregate manifest for old installers, verify all six assets, and mark the release Latest. Keep both final native checks. Only then merge the candidate into `origin/main`; preserve concurrent changes and stop on version conflicts. Do not force-push.
5. On resume, verify ready platforms and finish only missing work. If an archive was uploaded before its readiness marker, recover and verify those exact release bytes rather than rebuild, even if Actions artifacts expired. A partial public release is not complete. Completed legacy four-asset releases remain valid; candidates whose pinned installer lacks platform manifests keep the old all-platform publication flow.

Deploy platform-aware `tngo-workflow/setup.py` selection before the first partial release. It selects the highest published version ready for the host; macOS must not select a Linux-only release. The installer itself keeps default Latest selection; explicit `--release` can install a ready platform before completion. Existing in-flight runs keep their frozen publication policy.

Preserve these build constraints:

- Use only `trungnt13/asm` GitHub `v*` releases for macOS ARM64 and Linux x86_64 GNU. Verify GitHub SHA-256 digests and the host `SHA256SUMS-<target>` before installing the two-binary archive. Use legacy `SHA256SUMS` only when the host manifest is absent, never after an invalid digest or manifest.
- Reject unsupported targets and Linux without glibc 2.35+ before metadata downloads or install-state changes. Select only GNU Linux archives, without MUSL fallback. Historical MUSL-only releases require their original installer. GNU updates use separate target-qualified package directories. Preserve old MUSL packages and shared config/state.
- Preserve `--release`, `CODEX_HOME`, `CODEX_INSTALL_DIR`, install locking, and safe `current` selection. Store packages under `packages/asm-standalone`. Leave upstream packages and update markers unchanged. Keep the shared Codex config/state home and `codex` command.
- Only the owner installs updates externally. Do not fetch OpenAI/CDN or legacy npm packages. Do not support daemon-only installation. Do not write `auto-update-version`. Disable built-in update checks, prompts, commands, and daemon update loops regardless of upstream settings or markers. Neither CLI nor daemon can download or install upstream releases. Preserve ordinary daemon startup/restart independently of updates.

- Pushing a `v*` tag starts the release workflow; pushing `main` alone does not publish.
- Normal manual dispatch from a branch builds artifacts only. Normal dispatch on a `v*` tag can publish too, because publication checks the ref.
- Every new fork release is a normal GitHub release marked Latest. Retaining an upstream `alpha` suffix in the version does not set the GitHub prerelease flag.
- The concurrency experiment described below never publishes, including when dispatched on a tag.

Optional `macos_concurrency_experiment` compares cold builds of one commit on `macos-15` with 2 and 3 Cargo jobs. Keep other settings equal. Save separate timing, resource, size, and smoke results. Never reuse postmerge artifacts. Never publish, even on tags. Measure before changing defaults. One run does not prove a general speedup.

### Installer boundaries

### Account-security setup reminder

Do not fetch or display the optional Daybreak account-security setup reminder in ASM, including after reconnect or account refresh. Suppress loaded reminder events too. Keep authentication, account-security enforcement, approvals, account email, and backend usage banners unchanged. Retain upstream reminder rendering and its checks for easier syncs. Do not weaken security requirements.

### Standalone request audit

Keep `debug prompt-input` unchanged. `debug prompt-request` exports versioned JSON with effective base instructions and the production request representation, including ordered input, full advertised tool definitions, Responses Lite prefixes, and optional output schema. Reuse the app-server extension installer and request builder; do not add audit work to normal inference paths.

Default to ephemeral startup. Require `--allow-session-state` to create session records and initialize persistent state-backed tools; setup may opt in inside its isolated temporary home. Do not dispatch queued input. Label the export as a fresh standalone debug turn, not a live-session or exact transport capture. Report omitted history, turn hooks, input-triggered skill/plugin injections, uncaptured tools, authentication-dependent request metadata, and server-side additions. Do not send an inference request or claim startup is free of network or local-state effects. Audit output can contain sensitive instructions and tool content.

### Model catalog ownership

- Treat [`codex-rs/models-manager/models.json`](../codex-rs/models-manager/models.json) as the upstream reference. Do not make personal or fork-specific edits.
- Accept upstream catalog changes during upstream syncs; do not freeze the reference.
- Make personal catalog changes only in `/home/trungnt13/codes/tngo-workflow/configs/model-catalogs/models.json`. Never copy them back into the built-in catalog.

### Model catalog instruction visibility

Allow top-level `include_model_catalog_in_spawn_agent_list`, default `true`. When false, omit the generated model, reasoning-effort, and service-tier listing from both spawn-tool descriptions and `<model_catalog>` context for V1 and V2. Omit guidance pointing to the suppressed catalog. Keep custom descriptions, tool arguments, model loading, and override validation unchanged. This controls inclusion independently of `model_catalog_in_context`, which controls placement, and `expose_spawn_agent_model_overrides`, which controls argument exposure. Preserve existing history and use the normal catalog invalidation path when a retained listing stops applying. Do not update setup-managed profiles before a supporting CLI is released.

### MCP resource tools

Allow top-level `mcp_resource_tools_enabled`, default `true`. When false, do not register the built-in `list_mcp_resources`, `list_mcp_resource_templates`, or `read_mcp_resource` helpers. Remove their callable entries and generated declarations from direct tools, Code Mode, and `ALL_TOOLS`. Preserve existing registration conditions when enabled. Keep MCP connections, other server tools, apps, plugins, and shared TypeScript preamble settings independent. Apply on config loading without new live-reload behavior or history rewrites. Do not update setup-managed profiles before a supporting CLI is released.

### Web search copyright instruction visibility

Allow top-level `include_web_search_copyright_compliance`, default `true`. When false, omit only the terminal Copyright compliance subsection from the local `web.run` tool description. Keep preceding quotation and word limits, tool arguments, execution, search modes, and history unchanged. If the expected subsection changes or is no longer terminal, warn and retain the full description rather than risk removing unrelated instructions. Apply to standalone web search in cached, indexed, and live modes; do not change hosted `web_search` instructions or model safeguards. Do not update setup-managed profiles before a supporting CLI is released.

### Saved forks, upstream sides, and parallel conversations

- Preserve the parent's effective prompt-cache routing key for saved and temporary root forks, independently of thread/session identity. Persist it for resume and further forks. Keep old rollouts readable without ancestor lookups. Preserve upstream guardian/subagent routing. Shared routing does not guarantee backend cache hits.
- Keep upstream behavior for `/side` and its `/btw` alias: temporary forks, command restrictions, reference-only instructions, no subagents, no saved pairing metadata. Ctrl+/ switches views. Ctrl+C returns to the parent and discards the side. Ordinary picker navigation also discards temporary sides. Preserve upstream helper names, constants, and tests where practical. Avoid unnecessary upstream-path rewrites for parallel behavior.
- `/parallel` creates a saved ordinary user fork. Normal permissions, feature flags, platform support, and busy-state checks control commands. Inherited history is reference context, not a request to continue parent tasks or control parent agents. Parallel chats can own goals and subagents. Keep the initial transcript clean without rewriting stored or model-visible history.
- Keep one main/companion pair without nesting. `/parallel` from a parallel chat returns to its parent and replaces the selected companion; old parallel chats stay saved. Block `/side` and `/btw` while a parallel pair is open, whether viewing main or parallel, so they cannot interrupt or replace it. Closing the pair restores these commands; saved closed chats do not block them. Temporary sides retain upstream restrictions, including rejection of another companion command.
- Ctrl+/ switches without stopping work. With an empty composer and no modal, Ctrl+C stops the parallel chat. It pauses the active goal and returns to the parent without deleting history. Selecting its subagents must not stop it. Ordinary `/new`, `/clear`, `/resume`, `/fork`, `/cd`, and `/worktree` navigation exits parallel mode. Archive/delete affect the displayed parallel chat and return to its parent.
- Store pairing and transcript-display boundaries in ASM client-local state, scoped to the app-server target. Read saved-side records as parallel chats without file migration or history rewrites. Restore open pairs on resume. Closing a selection must not delete its saved conversation. Do not encode UI relationships through analytics source classification or false fork ancestry.
- Pairing metadata must not block ordinary history loading. Warn when it cannot be read. On confirmed deletion of a paired thread, clear the stale selection without deleting saved history or the child record; keep pairing on temporary connection or permission failures. A missing parent must not trap Ctrl+C.
- Stock clients see ordinary threads through unchanged app-server APIs; companion switching is not required there. Shared files and explicitly global config remain shared between chats.

### Recap session ID

End manual and automatic TUI recaps with dimmed `Session: <id>`, after any next action. Use the displayed conversation ID, not the recap-generation thread ID. Preserve recap generation and timing.

### Turn completion footer

Keep the completion footer as one dimmed logical line: `Worked for 5s • 11:05 AM • 12.4k in • 9.8k cc • 820|600 ou • 3ag • <session-id>`. Preserve existing timing, optional runtime metrics, and narrow-terminal wrapping. Show provider-reported totals for that conversation's turn, including compaction; cached input is part of input, and reasoning is part of output. Append the displayed conversation ID, not the turn ID. Omit unavailable token counts rather than substituting context estimates or session totals. Reuse existing saved usage where available; paginated stored history may omit counts without a database migration. Show `Nag` for distinct related subagents observed running at any point in a live turn, including descendants and agents already running when the turn starts. Retain the count across view switches in the same client; omit it for restored turns whose start was not observed. Token totals remain parent-only. Do not change model requests or token accounting.

### Copy session ID

`/copyid` copies only the displayed session ID through the existing clipboard path. Do not require `/status`, an assistant response, or a picker. Allow it during tasks, in side conversations, and while viewing parent-owned subagents. Report missing IDs or clipboard failures. Preserve `/copy` and clipboard platform support.

### ASM branding

Use ASM for the README heading, repository description, release titles/introduction, workflow step descriptions, fork TUI labels, and terminal title. Credit OpenAI Codex for origin. Preserve real upstream service/product names. Keep `codex` executable, CLI version prefix, crate, config/state, archive, tag, workflow, job, and check identifiers. Do not redirect upstream links or installers for branding.

Keep [`blocking-ci.yml`](../.github/workflows/blocking-ci.yml) and [`postmerge-ci.yml`](../.github/workflows/postmerge-ci.yml) customized in place. Port useful upstream action, toolchain, security, and build fixes without restoring upstream-wide matrices.

Allow top-level `background_terminal_min_timeout` / `background_terminal_max_timeout` in milliseconds, default 5000 / 300000. Reject zero, reversed, or unrepresentable bounds. Clamp empty `write_stdin` polls to these bounds. Show them in the tool description. End polls early on process exit.

Preserve initial command waits, nonempty stdin waits, and code-mode outer `exec`/`wait` limits. Process exit alone does not resume an idle model turn.

Do not repeat these checks locally for every edit or expand the custom workflows to unrelated platforms, Bazel suites, SDKs, remote executors, V8 source-build canaries, or OpenAI-only infrastructure.

Allow optional `[memories]` keys `extract_reasoning_effort` and `consolidation_reasoning_effort` for V1 and V2, with the existing effort type. Default to extraction `low` and consolidation `medium`, independently of parent effort. Preserve model selection and other memory behavior.

## Background terminal waits

Keep Markdown paragraphs, code blocks, and list items adjacent during streaming and after completion. Preserve blank lines inside code blocks, raw output, message boundaries, and user-message padding.

### Collapsed tool calls

`[tui] collapse_tool_calls = true` is opt-in; default `false`. In rich fullscreen transcripts, collapse consecutive shell, MCP, and dynamic tool calls into one summary row. Show the count and distinct command/tool names in first-use order. Replace overflowing names with `+N more`. Other activity types and chat messages separate groups. Keep the active tool in a separate short row until committed history includes it.

Expansion restores original previews in order. Keep failures and approval requests visible. Preserve execution, model-visible history, raw output, and the full transcript. When disabled, preserve existing rendering.

### Compact status line

- Join segments with `·`, without spaces. Use `CtxN%` for used context, `CtxN%left` for remaining context, and `F:on` / `F:off` for fast mode.
- Lowercase model labels. Remove `gpt-`. Keep at most the first three letters. Append all version digits without separators: `GPT-6-Astra` → `ast6`, `GPT-6.1-Sol` → `sol61`, `Terra 5.6` → `ter56`. Apply this limit to custom aliases. Names without digits have no version suffix.
- Keep the first three reasoning-label letters, including model-with-reasoning items: `medium` → `med`, `high` → `hig`, `xhigh` → `xhi`.
- Apply labels to the footer, `/statusline` preview, and `/subagents` model info. Preserve model IDs, other picker labels, and terminal titles. Remove the space before the agent role only in the footer: `Main[default]`. Preserve paths, roles, colors, order, and single-row truncation.
- Offer opt-in `cache-hit-rate` in `/statusline` and `tui.status_line`, with unchanged defaults. Show `CchN.N%`: latest reported request's cached input / input tokens for the current thread, rounded to one decimal. Never use averages or cumulative totals. Use existing usage data, without new requests or API changes. Hide unknown or zero-input usage. Missing cache details remain indistinguishable from reported zero.

### Subagent context windows, service tiers, and picker

Allow optional top-level `subagent_model_context_windows = { "gpt-6-astra" = 272000, "gpt-6.1-sol" = 272000 }`. Values must be positive integers fitting `i64`. Apply exact model-ID matches only to V2 spawned children, using their final selected model throughout startup, model changes, and reload. Missing entries retain the existing scalar/model fallback. Keep the original `model_context_window` unchanged so a matched override cannot spill into unmatched grandchildren. Preserve model maximum clamping, usable-context headroom, and existing compaction settings. Keep root, review, and guardian sessions unchanged; add no app-server API or rollout fields.

Allow `[subagent_service_tiers]`, for example `gpt-6-sol = { high = "fast" }`. Match the child's final model and effective effort after overrides and role settings. Matching rules override root tier for the child's lifetime, including later root-tier changes. Unmatched children retain upstream behavior. Keep root requests unchanged. Reject unsupported matched tiers. When fast mode is disabled, reject fast overrides. Do not expose this policy through spawn arguments.

In `/subagents`, append known model/effort to main and child titles with compact labels: `gpt-6.1-sol-high-fast` → `sol61-hig-fast`, `gpt-6-astra-xhigh` → `ast6-xhi`. Add `-fast` only for matching fast rules or inherited fast tier. A matching `default` rule suppresses inherited fast. Do not guess missing settings, remove thread ID descriptions, or change navigation. Labels show configuration, not confirmed backend routing. Keep this TUI-only, without new app-server APIs or request tracking.
