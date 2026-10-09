# Personal fork guide

## Principles

1. **Personal use only:** this fork is for Trung Ngo.
2. **Keep changes minimal, safe, and minor.** Prefer durable local fixes with few upstream differences or future conflicts. Do not reduce correctness, maintainability, or runtime performance to shorten a diff. Assume upstream tested unchanged code. Do narrow checks for fork changes, conflicts, and packages; upstream tests do not prove these work.
3. **Record current intent here:** each fork change's purpose and constraints, not a task log. Ask before contradicting existing intent. After approval, replace the old rule. Agents can then resolve mechanical conflicts without more approval.

This guide overrides conflicting repository instructions, including [AGENTS.md](../AGENTS.md). Its validation and platform limits replace upstream defaults. Other coding, formatting, test-authoring, and generator rules still apply. Recorded intent does not prove implementation, integration, or publication.

## Development workflow

### Start or resume

- Refresh Git status, branches, and worktrees before edits, including after resume.
- For syncs and pushes, inspect remotes and fork history. For overlap or unexplained changes, inspect relevant repo sessions. Recover context from Git and sessions, not owner memory or a task diary.
- For concurrent tasks, create a named `codex/` branch and separate worktree before edits. Coordinate shared files. Assign one agent to staging and commits per checkout. Integrate one task at a time into local `main`.
- Prefer a reusable task checkout: `python3 scripts/dev-worktree.py acquire codex/<task>`. Keep its path through builds and validation. Slots have an exclusive owner/branch lease. Owner defaults to `CODEX_THREAD_ID`; use `--owner <id>` if unset and `--slot <name>` for parallel tasks. Release with `python3 scripts/dev-worktree.py release codex/<task>` only after clean local `main` integration. Keep the detached checkout for reuse. Never clear another owner's lease or discard work; inspect and coordinate abandoned leases.
- Preserve existing work. If it blocks progress, use another worktree. Get permission before committing or stashing unrelated work. Keep authorized stashes until restoration is verified. Never overwrite other work to clean a checkout.

### Build the binary first

For runtime or UI edits, inspect all affected paths, including streaming and completed output. Make the smallest coherent change.

Before Cargo runs, ensure sufficient disk space. Local `just` Cargo recipes default to the main checkout's `codex-rs/target`; explicit `CARGO_TARGET_DIR` and Cargo target-directory options retain their meaning. Keep native host mode, toolchain, profile, selected features, flags, checkout, and cache stable. Do not add `--target` merely to name the native host. Fresh checkout timestamps, source/version changes, and changed inputs can still force rebuilds. Reuse matching verified fork [V8 artifacts](#v8-dependency-release); explicitly export the same verified absolute `RUSTY_V8_ARCHIVE` and `RUSTY_V8_SRC_BINDING_PATH` paths. Do not infer artifact validity from an old Cargo fingerprint. Also export `AWS_LC_SYS_NO_JITTER_ENTROPY=1` consistently; changing or unsetting it rebuilds AWS-LC and its dependents.

From `codex-rs`, build both binaries from the same source, native target, and `dev-small` profile:

```bash
just build-dev
```

Prioritize this pair over other Cargo jobs. Serialize jobs that share a target directory. Keep executable siblings in the resolved target directory's `dev-small`; verify which checkout produced them. During builds, inspect independent code and review affected tests or snapshots. Test executables are not these binaries.

The local Cargo wrapper uses the source checkout's absolute `scripts/local-rustc-workspace.sh` path as `RUSTC_WORKSPACE_WRAPPER`. Cargo namespaces workspace artifacts by this path while sharing registry dependencies. This prevents another checkout's older code from appearing fresh in the shared cache. Keep the checkout path stable for warm builds. Explicit wrapper overrides, including disabling it, remain unchanged; use a separate target directory unless the override also isolates checkouts. Clippy replaces this wrapper, so its default cache is private to the checkout under the main target directory's `clippy/<checkout-path-hash>`. An explicit Clippy target directory must also be private to that checkout.

The local Cargo wrapper locks build/check/Clippy/nextest commands per effective cache and saves input details plus Cargo fingerprint reasons under the main checkout's `.agents/dev-build-cache/<date>/`. An explicit `CARGO_LOG` retains its filter. Inspect logs before blaming cache loss. Active wrapper commands also prevent reusable-checkout release or switching. `cargo run` keeps interactive I/O without holding the cache lock for a live app. CI and Windows behavior stay unchanged. Direct Cargo bypasses these protections and must finish before checkout release. Shared executable paths still belong to the last build; verify their producing checkout before handoff.

For ordinary local runtime or UI checks, run `"$(python3 ../scripts/local-cargo.py --print-target-dir)/dev-small/codex" --no-daemon` from `codex-rs`. This uses the current embedded backend without changing or stopping a shared daemon; the sibling `codex-code-mode-host` supports code mode. Test daemon or remote behavior separately when it matters: `codex agents` and `--remote` cannot use `--no-daemon`.

Before handoff, verify both executables exist, CLI `--version`, and `--help` for each. A help check does not prove model requests or tools work. When safe, hand off both before automated checks finish. Report their verified absolute paths, build time, a manual check, and pending checks. Rebuild both after runtime edits. Never present old binaries as current. Get permission before replacing installed `codex`.

Ordinary local `just` Cargo recipes, schema generators, source runners, package assembly, and VS Code Rust checks use `dev-small`. Keep its profile settings unchanged. Where a command accepts Cargo options, explicit `--profile`, nextest `--cargo-profile`, and `--release` selections take priority. Nextest's `--profile` selects test-runner settings, not a Cargo build profile. CI, benchmarks, and source-built Dylint keep their existing profiles. The npm hooks generator uses the same `just` recipe.

Plain Cargo does not inherit this local default. Pass `--profile dev-small` to direct local build/run/check/clippy commands; use `just test` for tests. VS Code run/test buttons and inherited Windows setup and sandbox smoke utilities retain their existing profiles. Do not delete an old cache while builds or editor processes use it. Published releases remain optimized `--release`.

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
- Last incorporated upstream commit: `2351d9e1b608e6f9d9a3699b71d7eb39ee41cfa4`.
- Commit date: 2026-10-09.
- Subject: Avoid an extra blank quote line when pasting text with a trailing newline (#52418).

Verify the upstream URL before fetching. Use the requested sync method. Otherwise, merge into `main` to preserve published history; rebase only unpublished branches. Use `git cherry-pick -x` for targeted ports.

A full sync includes the selected upstream `main` commit and all ancestors. Capture the old baseline and target before sync. Report that fixed range. Focus on fork differences and affected upstream changes. Apply the integration and validation rules above.

After each upstream rebase, give a short, high-level list of the main upstream changes in that range. Group related changes, not individual commits. Use one plain sentence per idea to explain the change and its user effect, with a source link. Keep rebase status and validation separate.

After successful full sync, advance the marker; cherry-picks do not advance it. Verify hash, date, and subject with `git show -s --format='%H%n%cs%n%s' <upstream-commit>`. Confirm the marker is an ancestor of the fork branch.

## Platforms and CI

Focus on macOS and Linux (Ubuntu). Preserve inherited code and workflows for other platforms unless removal is requested. Do not add them to fork CI or releases. Other inherited workflows also need approval for changes. Inspect triggers: [V8 canary](../.github/workflows/v8-canary.yml) runs on pull requests, with expensive builds conditional on relevant changes. [CLA](../.github/workflows/cla.yml) restricts its job to `openai`.

- **[Blocking CI](../.github/workflows/blocking-ci.yml):** manual-only `workflow_dispatch`, never automatic or a required branch check. Keep workspace formatting, production Clippy, and the result collector. Clippy covers `codex-cli`, `codex-tui`, `codex-core`, and `codex-config` on both release targets. Preserve `--lib --bin codex -- -D warnings`. Do not add a test-target matrix, `cargo shear`, or warning suppression. Add narrow checks for other affected packages as needed.
- **[Postmerge CI](../.github/workflows/postmerge-ci.yml):** automatic optimized builds, packaging, smoke checks, uploads, diagnostics, and results. Set push-level `paths-ignore` only for root `AGENTS.md` and this guide. Filtered pushes cannot cancel active builds. Other paths remain eligible; do not exclude all Markdown or `docs/`. Release publication does not require manual blocking CI. Report existing failures separately.

Port useful upstream action, toolchain, security, and build fixes without broad matrices. Do not repeat CI locally for each edit. Do not add unrelated platforms, Bazel suites, SDKs, remote executors, V8 source-build canaries, or OpenAI-only infrastructure to fork CI. The two-target V8 dependency build below is not a broad suite or canary.

## Releases

### Operate a release

Deliver the complete installable package before unrelated feature work. Reuse the existing release workflow, not a second pipeline. Keep installation separate; publication does not authorize installing binaries or restarting a shared server.

Inspect failed CI jobs before release. Fix relevant code or packaging errors. For runner allocation or communication failures, inspect annotations and available logs, then rerun failed jobs without repeating successful jobs. Do not infer a code defect or change build settings from a runner failure alone. When the owner requires CI recovery first, verify the retry succeeds before dispatching the release.

Before dispatch, run one bounded local live smoke turn with the default configuration and existing sign-in, without profile or model overrides. Use the paired release-source CLI/helper in an ephemeral session and temporary directory. Require a real model response and a successful read-only Code Mode command with verified output. Failure, timeout, or missing authentication blocks dispatch; do not silently skip it. Report source and result separately from package checks. Keep credentials out of CI.

1. Require an explicit release request. Select the repository explicitly, for example with `gh ... -R trungnt13/asm`. Before pushing source to `origin/main`, verify the V8 prerequisite below. Release the intended commit from `origin/main`, not unpushed local work. Candidates build or reuse their own archives; do not wait for postmerge builds.
2. Dispatch [`fork-rust-release.yml`](../.github/workflows/fork-rust-release.yml) on `main` with `publish_release=true`. The workflow owns version selection, candidate creation, builds, publication, verification, and remote `main` updates. Do not repeat these steps manually. Ordinary branch dispatch is build-only. A `main` push does not request publication.
3. After interruption, rerun failed jobs. For a new resume dispatch, use `main`, `publish_release=true`, and `resume_run_id=<original run ID>`. Keep the original version and commit even if upstream advances. Report failures that need owner decisions. Do not invent versions or replace tags.
4. A platform is available as soon as its checked archive and platform checksum manifest are published. The whole release is complete only when `release`, both `verify_native` jobs, and `sync_main` succeeded for that candidate. Legacy `v*` tag pushes and dispatches can publish. Require publication and both native checks. Skip `sync_main` for legacy releases. Inspect that run's recorded checks and current release metadata. Do not repeat successful checks on unchanged artifacts. A green build-only run, pushed tag, or version label does not prove publication.
5. Fetch the result. Fast-forward local `main` only when safe. Preserve local edits and concurrent commits. Report the released commit, upstream baseline, checks, and remote/local integration status, including blocked updates.

### Release notes

- Publish notes on GitHub for each new release. Include the same notes in the final report.
- Compare the release commit with the previous completed release. Include only changes present in the new package.
- Use separate `ASM` and `Codex` headings. State `No changes` for an empty section.
- Use ASD-STE100 style: short, direct sentences, active voice, and one term per concept.
- Use one flat list item per feature or fix: `**Name:** What changed. User effect or required action.` Keep each item on one source line.
- State relevant defaults, limits, and experimental status. Link supporting commits or files on the same line.
- State the version, platform readiness, and validation results outside the feature lists. Do not claim unverified effects.

### Workflow requirements

These requirements govern release code, not a second manual release procedure.

#### Version

- Select the highest upstream `rust-v` prerelease tag by Semantic Versioning order, not creation or publication time. An upstream release need not be published. Increment only its numeric patch by one and keep its suffix: `rust-v0.162.0-alpha.18` → `v0.162.1-alpha.18`.
- For repeat releases of that upstream version, append one positive numeric fork counter: `alpha.18.1`, then `alpha.18.2`. Use the highest used counter plus one; never fill gaps or change upstream identifiers. When upstream advances to `alpha.19`, start with `v0.162.1-alpha.19`. Put the counter before any build metadata. The installer accepts repeated numeric alpha/beta parts.
- Resume unfinished candidates or releases before selecting a new version. For the current version family and highest existing fork version, require complete published assets and successful release, both native checks, and main sync tied to the exact candidate; a successful resume counts. Older candidates still need complete published assets, but not expired historical Actions logs. Never overwrite tags or assets. If completion cannot be proved, no upstream prerelease exists, or the proposed version does not exceed every existing fork version, stop and ask the owner.
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
- Keep workflow shell commands compatible with macOS Bash 3.2, including array expansion under `set -u`.
- Build `codex` and `codex-code-mode-host` from one commit, target, and profile. Each `codex-<target>.tar.gz` must contain a complete canonical CLI package: `bin/codex`, `bin/codex-code-mode-host`, `codex-package.json`, `codex-path/rg`, and, on Linux, `codex-resources/bwrap`. Use the existing package layout and pinned ripgrep manifest; build Bubblewrap from the same source and release profile, and embed its final SHA-256 in the CLI before packaging. Include only required package resources, with regular executable files and matching version/target metadata. Shared-server startup must recognize and validate this package without relying on host helpers. The completed release has six assets: both archives, `SHA256SUMS-<target>` for each target, aggregate `SHA256SUMS`, and [`install.sh`](../scripts/install/install.sh). Do not publish source trees or diagnostics. Each platform manifest covers its archive and the pinned installer.
- Detect a candidate's complete package contract from its pinned smoke check's required `codex-package.json`, not optional resource flags. Pass `--no-zsh` only when that pinned CLI accepts it; old candidates retain their original contract.
- Pin builds, packaging, installer bytes, payload checks, and archive reuse to the immutable candidate commit SHA. Pin orchestration scripts to the dispatch SHA; preserve those bytes before checking out the candidate. Never fetch newer controls mid-run. For legacy releases, use the exact tagged commit SHA. Verify the release tag resolves to that commit. Candidate packaging can precede tag creation.
- Before upload, run [native package smoke checks](../.github/scripts/smoke-codex-archive.py). CLI `--version` must match Cargo. Both main binaries' `--help` commands must succeed with usage. Validate manifest fields and required resources, and run bundled ripgrep and Bubblewrap smoke commands. Do not require code-mode helper `--version`. Record binary and archive sizes.
- For GNU Linux, verify source-built ELF executables use the x86_64 GNU loader and no glibc symbols newer than 2.35. Pinned ripgrep may be statically linked; check its architecture and run it on the minimum OS too. Run version/help checks in a clean Ubuntu 22.04 container with only declared runtime libraries. The build runner alone does not prove compatibility.

Keep these limits:

- No Apple Developer signing/notarization, paid Apple membership, Azure Key Vault, release secrets, or self-hosted runners.
- Bundle only the ripgrep and Linux Bubblewrap required by the canonical package contract. Keep the system shell; do not bundle voice, patched zsh, or other optional helpers. Do not change sandbox/security defaults for packaging.
- For fork Linux builds, use the native GNU toolchain and [`install-gnu-build-tools.sh`](../.github/scripts/install-gnu-build-tools.sh). Do not use Zig or MUSL wrappers there. Retain inherited MUSL tooling for upstream workflows.
- Retain `AWS_LC_SYS_NO_JITTER_ENTROPY` and verified fork V8 through [`setup-rusty-v8`](../.github/actions/setup-rusty-v8/action.yml). Install Python 3.11+ explicitly on Ubuntu 22.04 for artifact verification.
- Preserve upstream GNU runtime choices, including the system allocator, locale handling, and PTY support. Do not change Rust platform conditions merely to mimic MUSL. GNU packages use system OpenSSL 3 and can use system liblzma. Match [`smoke-ubuntu-archive.sh`](../.github/scripts/smoke-ubuntu-archive.sh) runtime packages to ELF dependencies. Do not add ALSA or GStreamer requirements for the GNU migration.
- Get an agreed intent change before adding platforms, DMGs, optional bundled resources, npm, R2, WinGet, or website/OpenAI-only publishing.

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

### Installer

- Use only `trungnt13/asm` GitHub `v*` releases for macOS ARM64 and Linux x86_64 GNU. Verify GitHub SHA-256 digests and the host `SHA256SUMS-<target>` before installing the complete canonical package archive. Reject incomplete two-binary archives in the current installer; historical releases retain their pinned installers. Do not treat an existing two-binary installation as complete. Use legacy `SHA256SUMS` only when the host manifest is absent, never after an invalid digest or manifest.
- Reject unsupported targets and Linux without glibc 2.35+ before metadata downloads or install-state changes. Select only GNU Linux archives, without MUSL fallback. Historical MUSL-only releases require their original installer. GNU updates use separate target-qualified package directories. Preserve old MUSL packages and shared config/state.
- Preserve `--release`, `CODEX_HOME`, `CODEX_INSTALL_DIR`, install locking, and safe `current` selection. Store packages under `packages/asm-standalone`. Leave upstream packages and update markers unchanged. Keep the shared Codex config/state home and `codex` command.
- Only the owner installs updates externally. Do not fetch OpenAI/CDN or legacy npm packages. Do not support daemon-only installation. Do not write `auto-update-version`. Disable built-in update checks, prompts, commands, and daemon update loops regardless of upstream settings or markers. Neither CLI nor daemon can download or install upstream releases. Preserve ordinary daemon startup/restart independently of updates.

### Build experiment

Optional `macos_concurrency_experiment` compares cold builds of one commit on `macos-15` with 2 and 3 Cargo jobs. Keep other settings equal. Save separate timing, resource, size, and smoke results. Never reuse postmerge artifacts. Never publish, even on tags. Measure before changing defaults. One run does not prove a general speedup.

## Intentional behavior differences

### TUI automatic session names

`[auto_rename]` controls automatic naming in the TUI only. It does not change Desktop naming, the conversation model, stored history, or `/rename` suggestions. Defaults remain enabled, first user message, user-message context, one-time naming, 36 title characters, and a 960-byte initial prompt. Automatic naming prefers `gpt-6-luna` at low effort only with OpenAI, a ChatGPT account, and an available catalog entry; otherwise use the conversation model. Manual `/rename` suggestions retain their upstream model selection.

- `enabled` defaults to true. `model` and `reasoning_effort` override only title generation; omitted values retain the default selection. An explicit model must be preserved by the server; unavailable models fail rather than silently falling back. Custom provider models need not appear in the catalog.
- `first_trigger` accepts `first_user_message` (default) or `first_completed_turn`. Completed-turn naming ignores failed and interrupted turns. `context` accepts `user_message` (default) or `recent_conversation`, which includes the initial request and recent substantive user/assistant messages, excluding commentary and tool output.
- `max_context_bytes` accepts 128–8192 bytes of source text, separate from additional naming guidance. Omission retains the original source-text budget. `recent_message_limit` accepts 1–32 (default 8), including the initial request; a limit of 1 keeps only the latest message. Tight byte budgets drop earlier messages to preserve complete markup. `max_title_chars` accepts 1–128 (default 36). `max_title_words` is an optional positive integer used only in the prompt. Omission retains the soft “under five words where possible” guidance; an explicit value asks for at most that many words instead. Do not truncate output by word count. The captured prompt and character limit remain tied to the originating request.
- `additional_naming_guidance` appends naming guidance within 512 UTF-8 bytes; it does not replace the fixed prompt and has no `instructions` alias. Preserve fixed output rules and bound the complete prompt to 9500 bytes. **P0 manual review:** opt-in larger budgets can exceed 1000 tokens; never exceed the 10K-token item limit.

- `auto_update` defaults to false: generate only the initial name. Recurring refresh requires both `enabled` and `auto_update`. Include the captured previous title as bounded JSON-string data in recurring prompts. Ask the model to retain it unless the task meaningfully changes, preserving its core subject and wording when updating; routine progress alone is not a reason to rename. Keep initial naming and explicit `/autorename` prompts unchanged. Reserve room for this guidance within the existing total prompt cap without splitting conversation markup. Persist each successfully generated title in one client-local record under `CODEX_HOME`, scoped to the local or remote app server. Restore eligibility only when that record matches the saved name. Protect named sessions with missing, corrupt, or mismatched records; `/autorename` may opt them in. Persist initial automatic names even when updates are disabled, so a later resume may enable refresh. Ordinary view switches retain counters; resume resets the interval and excludes already-completed turns.
- `auto_update_interval_turns` is a positive integer, default 5. Count successful user-turn completions while that thread is displayed; ignore duplicate, failed, interrupted, background, helper, and replay events. Schedule at most one attempt every interval. Reset the counter when an attempt starts, not when it succeeds; failed generation retains ownership and waits for another interval. Count eligible completions during an outstanding request but do not start another until it settles. Track unnamed opt-in threads before their first title request, without granting naming ownership until a save succeeds. Retain only the latest user-turn/item identifiers, one ordering anchor, and at most 8192 UTF-8 bytes of user text so long turns and pending initial generation cannot lose user context when event buffers evict it. A first-user-message title saved before the assistant finishes counts that completion; a completed-turn initial title starts counting with later completions.

`/autorename` immediately generates and applies one name with the same title-generation settings, including context, model, effort, source budgets, title limits, and additional naming guidance. It ignores `enabled`, `first_trigger`, `auto_update`, and its interval because invocation is an explicit manual action. User-message context uses the latest substantive user text; recent-conversation context uses the shared bounded collector. The command may replace a pre-existing or resumed name. Preserve `/rename` suggestions and supplied names unchanged. Supersede pending background naming. A successful result restores persisted automatic-refresh eligibility, including when the generated title equals the current name. The global flags gate subsequent refresh, not this explicit generation. Failed generation leaves the current name and persisted eligibility unchanged. Deduplicate requests, retain the current name on failures, and report explicit failures. Discard stale results if the originating thread is no longer displayed or its saved name has changed. Reuse the existing app-server APIs; do not change Desktop generation.

Keep background requests, manual-name precedence, cancellation, and originating-thread checks. Before replacing a title, compare the server's current name with the request's captured name; identical generated titles require no name RPC. `/rename`, dashboard naming, and explicit `set_thread_title` tool intent remove persisted eligibility before their name RPC, including same-name renames. Abort the RPC and report an error if removal fails; a later RPC failure may conservatively leave refresh disabled. Generic cancellation only clears in-memory ownership. Observed external name changes cancel automatic work.

Bound records to 512 UTF-8 bytes and 128 characters, validate reads, and write atomically. Read storage only on attachment or a recurring attempt/save boundary, not on streaming deltas or rendering. Recheck persisted eligibility before background refresh so another TUI's manual opt-out is respected. Corrupt or unreadable records fail closed; report persistence failures. File and server writes cannot be atomic together, so same-name external renames and concurrent read/write races remain best-effort. Do not add shared APIs, rollout history records, or Desktop policy changes to solve those races.

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
- Store pairing and transcript-display boundaries in ASM client-local state, scoped to the app-server target. Resume only the selected session as an ordinary chat, whether it was a parent or parallel chat; never restore a saved pair or attach its counterpart. Keep saved transcript-display boundaries without file migration or history rewrites. Pair navigation exists only during active `/parallel` use. Closing a selection must not delete its saved conversation. Do not encode UI relationships through analytics source classification or false fork ancestry.
- Pairing metadata must not block ordinary history loading. Log when transcript-display metadata cannot be read. On confirmed deletion of a paired thread, clear the stale selection without deleting saved history or the child record; keep pairing on temporary connection or permission failures. A missing parent must not trap Ctrl+C.
- Stock clients see ordinary threads through unchanged app-server APIs; companion switching is not required there. Shared files and explicitly global config remain shared between chats.

### Recap session ID

End manual and automatic TUI recaps with dimmed `Session: <id>`, after any next action. Use the displayed conversation ID, not the recap-generation thread ID. Preserve recap generation and timing.

### Turn completion footer

Keep the completion footer as one dimmed logical line: `Worked for 5s • 11:05 AM • 12.4k in • 9.8k cc • 820|600 ou • 3ag • <session-id>`. Preserve existing timing, optional runtime metrics, and narrow-terminal wrapping. Show provider-reported totals for that conversation's turn, including compaction; cached input is part of input, and reasoning is part of output. Append the displayed conversation ID, not the turn ID. Omit unavailable token counts rather than substituting context estimates or session totals. Reuse existing saved usage where available; paginated stored history may omit counts without a database migration. Show `Nag` for distinct related subagents observed running at any point in a live turn, including descendants and agents already running when the turn starts. Retain the count across view switches in the same client; omit it for restored turns whose start was not observed. Token totals remain parent-only. Do not change model requests or token accounting.

### Copy session ID

`/copyid` copies only the displayed session ID through the existing clipboard path. Do not require `/status`, an assistant response, or a picker. Allow it during tasks, in side conversations, and while viewing parent-owned subagents. Report missing IDs or clipboard failures. Preserve `/copy` and clipboard platform support.

### ASM branding

Use ASM for the README heading, repository description, release titles/introduction, workflow step descriptions, fork TUI labels, and terminal title. Credit OpenAI Codex for origin. Preserve real upstream service/product names. Keep `codex` executable, CLI version prefix, crate, config/state, archive, tag, workflow, job, and check identifiers. Do not redirect upstream links or installers for branding.

### Background terminal waits

Allow top-level `background_terminal_min_timeout` / `background_terminal_max_timeout` in milliseconds, default 5000 / 300000. Reject zero, reversed, or unrepresentable bounds. Clamp empty `write_stdin` polls to these bounds. Show them in the tool description. End polls early on process exit.

Preserve initial command waits, nonempty stdin waits, and code-mode outer `exec`/`wait` limits. Process exit alone does not resume an idle model turn.

### Memory reasoning effort

Allow optional `[memories]` keys `extract_reasoning_effort` and `consolidation_reasoning_effort` for V1 and V2, with the existing effort type. Default to extraction `low` and consolidation `medium`, independently of parent effort. Preserve model selection and other memory behavior.

### Transcript spacing

Keep Markdown paragraphs, code blocks, and list items adjacent during streaming and after completion. Preserve blank lines inside code blocks, raw output, message boundaries, and user-message padding.

### Collapsed tool calls

`[tui] collapse_tool_calls = true` is opt-in; default `false`. In rich fullscreen transcripts, collapse consecutive shell, MCP, dynamic tool calls, routine V2 subagent interactions, normal agent starts/completions, and routine agent waits into one expandable summary. Show separate counts for tool calls, agent starts, and completions; lifecycle notifications are not tool calls or running tools. Show distinct command/tool names or target agent paths in first-use order. Count each successful subagent message or follow-up once. Count matching wait start/end previews once per contiguous displayed group; completed waits must not retain a running summary. Keep waits with agent results, failures, or interruption visible. Replace overflowing names with `+N more`. `[tui] collapse_tool_calls_max_lines` accepts positive integers and defaults to `1` when omitted. The budget covers each newly rendered summary header, including counts and names; larger values allow wrapping. Retained reading snapshots preserve their captured content on resize and may wrap further. Clip the header with an ellipsis if even counts cannot fit. Subagent interruptions, other activity types, and chat messages separate groups. Keep the active tool in a separate short row until committed history includes it.

Expansion restores original previews in order. Keep failures and approval requests visible. Preserve execution, model-visible history, raw output, and the full transcript. When disabled, preserve existing rendering.

### Compact status line

- Join segments with `·`, without spaces. Use `CtxN%` for used context, `CtxN%left` for remaining context, and `F:on` / `F:off` for fast mode.
- Show context-window capacity as only the compact token count, without the `window` suffix, including in `/statusline` previews.
- Lowercase model labels. Remove `gpt-`. Keep at most the first three letters. Append all version digits without separators: `GPT-6-Astra` → `ast6`, `GPT-6.1-Sol` → `sol61`, `Terra 5.6` → `ter56`. Apply this limit to custom aliases. Names without digits have no version suffix.
- Keep the first three reasoning-label letters, including model-with-reasoning items: `medium` → `med`, `high` → `hig`, `xhigh` → `xhi`.
- Apply labels to the footer, `/statusline` preview, and `/subagents` model info. Preserve model IDs, other picker labels, and terminal titles. Remove the space before the agent role only in the footer: `Main[default]`. Preserve paths, roles, colors, order, and single-row truncation.
- Offer opt-in `cache-hit-rate` in `/statusline` and `tui.status_line`, with unchanged defaults. Show `CchN.N%`: latest reported request's cached input / input tokens for the current thread, rounded to one decimal. Never use averages or cumulative totals. Use existing usage data, without new requests or API changes. Hide unknown or zero-input usage. Missing cache details remain indistinguishable from reported zero.

### Subagent context windows, service tiers, and picker

Allow optional top-level `subagent_model_context_windows = { "gpt-6-astra" = 272000, "gpt-6.1-sol" = 272000 }`. Values must be positive integers fitting `i64`. Apply exact model-ID matches only to V2 spawned children, using their final selected model throughout startup, model changes, and reload. Missing entries retain the existing scalar/model fallback. Keep the original `model_context_window` unchanged so a matched override cannot spill into unmatched grandchildren. Preserve model maximum clamping, usable-context headroom, and existing compaction settings. Keep root, review, and guardian sessions unchanged; add no app-server API or rollout fields.

Allow `[subagent_service_tiers]`, for example `gpt-6-sol = { high = "fast" }`. Match the child's final model and effective effort after overrides and role settings. Matching rules override root tier for the child's lifetime, including later root-tier changes. Unmatched children retain upstream behavior. Keep root requests unchanged. Reject unsupported matched tiers. When fast mode is disabled, reject fast overrides. Do not expose this policy through spawn arguments.

In `/subagents`, append known model/effort to main and child titles with compact labels: `gpt-6.1-sol-high-fast` → `sol61-hig-fast`, `gpt-6-astra-xhigh` → `ast6-xhi`. Add `-fast` only for matching fast rules or inherited fast tier. A matching `default` rule suppresses inherited fast. Do not guess missing settings, remove thread ID descriptions, or change navigation. Labels show configuration, not confirmed backend routing. Keep this TUI-only, without new app-server APIs or request tracking.

Append a known positive configured context capacity, for example `sol61-hig-fast-272k` or `sol61-max-272k`. Use exact model-map matches only for V2 spawned children identified by canonical agent paths; never apply the map to main or V1 children. Otherwise use the configured scalar `model_context_window`, including for main. Omit unknown or nonpositive capacities rather than guessing catalog defaults. Labels show the configured value before model-maximum clamping and usable-context headroom, not runtime-confirmed capacity, usage, or remaining context. Preserve other picker metadata and navigation.
