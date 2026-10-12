# Personal fork guide

## Principles

1. **Personal use only:** this fork is for Trung Ngo.
2. **Keep changes minimal, safe, and minor.** Prefer durable local fixes with few upstream differences and conflicts. Do not sacrifice correctness, maintainability, or runtime performance. Assume upstream tested unchanged code. Check fork changes, conflicts, and packages narrowly; upstream tests do not prove these work.
3. **Record current intent here:** purpose and constraints for each fork change, not a task log. Ask before contradicting intent. After approval, replace the old rule. Then resolve mechanical conflicts without more approval.

This guide overrides conflicting instructions, including [AGENTS.md](../AGENTS.md). Its validation/platform limits replace upstream defaults. Other coding, formatting, test-authoring, and generator rules apply. Recorded intent does not prove implementation, integration, or publication.

## Development workflow

### Start or resume

- Before edits or after resume, refresh Git status, branches, and worktrees.
- Before syncs or pushes, inspect remotes and fork history. For overlap or unexplained changes, inspect relevant repo sessions. Recover context from Git and sessions, not owner memory or a task diary.
- Before edits or syncs, create a named `codex/` task branch. Isolate concurrent tasks in separate worktrees; these are not deliverables. Coordinate shared files. Assign one staging/commit owner per checkout.
- Prefer reusable checkouts:
  - Acquire: `python3 scripts/dev-worktree.py acquire codex/<task>`. Keep the path through builds and validation.
  - Each slot has an exclusive owner/branch lease. Owner defaults to `CODEX_THREAD_ID`; use `--owner <id>` if unset. Use `--slot <name>` for parallel tasks.
  - Keep the checkout and lease while its PR is pending. After approved integration includes its branch tip in clean local `main`, release: `python3 scripts/dev-worktree.py release codex/<task>`.
  - Keep the detached checkout for reuse. Never clear another owner's lease or discard work. Inspect and coordinate abandoned leases.
- Preserve existing work. If blocked, use another worktree. Get permission before committing or stashing unrelated work. Keep authorized stashes until verified restoration. Never overwrite other work to clean a checkout.

### Build the binary first

For runtime/UI edits, inspect affected paths, including streaming and completed output. Make the smallest coherent change.

- Before Cargo, ensure sufficient disk space. Local `just` recipes default to main's `codex-rs/target`. Explicit `CARGO_TARGET_DIR` and Cargo target-directory options retain their meaning.
- Keep native host mode, toolchain, profile, features, flags, checkout, and cache stable. Do not add `--target` just to name the host. Fresh timestamps, source/version changes, and other changed inputs can force rebuilds.
- Reuse matching verified [fork V8](#v8-dependency-release). Export the same verified absolute `RUSTY_V8_ARCHIVE` and `RUSTY_V8_SRC_BINDING_PATH` paths. An old Cargo fingerprint does not prove validity.
- Export `AWS_LC_SYS_NO_JITTER_ENTROPY=1` consistently. Changing or unsetting it rebuilds AWS-LC and dependents.
- From `codex-rs`, build both binaries with the same source, native target, and `dev-small` profile:

  ```bash
  just build-dev
  ```

- Prioritize this pair. Serialize Cargo jobs sharing a target directory. Keep executable siblings in its resolved `dev-small` directory. Verify their producing checkout; test executables are not this pair. During builds, inspect independent code and affected tests/snapshots.

The local Cargo wrapper:

- Sets `RUSTC_WORKSPACE_WRAPPER` to the source checkout's absolute `scripts/local-rustc-workspace.sh` path. This namespaces workspace artifacts by checkout while sharing registry dependencies. Older code from another checkout cannot appear fresh. Keep paths stable for warm builds.
- Preserves explicit wrapper overrides, including disabling it. Use a separate target directory unless the override also isolates checkouts.
- Gives Clippy a private default cache at main's `clippy/<checkout-path-hash>` target subdirectory because Clippy replaces the wrapper. Explicit Clippy target directories must also be checkout-private.
- Locks build/check/Clippy/nextest commands per effective cache. Saves inputs and Cargo fingerprint reasons under main's `.agents/dev-build-cache/<date>/`. Explicit `CARGO_LOG` retains its filter. Inspect logs before blaming cache loss.
- Cleans stale artifacts only on Linux with Python 3.11+, complete process visibility, and no cache users or Rust build/editor processes:
  - Hold each profile's native Cargo lock during deletion.
  - Use descriptor-relative traversal. Reject symlink directories and artifacts outside the physical cache.
  - Log removal counts and skip reasons. Skip other hosts or uncertain activity.
  - Keep the three-day rule: `incremental` directory modification time, or latest `deps` file access/modification time. Age does not prove checkout inactivity. Removal can force even active checkouts to rebuild.
- Blocks reusable-checkout release/switching during active wrapper commands. Direct Cargo bypasses protection and must finish before checkout release.
- Keeps interactive I/O for `cargo run` without holding the cache lock for the live app. CI and Windows stay unchanged.

For ordinary runtime/UI checks, from `codex-rs` run:

```bash
"$(python3 ../scripts/local-cargo.py --print-target-dir)/dev-small/codex" --no-daemon
```

Follow [real-session isolation](#real-session-checks). This uses the current embedded backend and sibling `codex-code-mode-host`, without changing/stopping the shared daemon. Check daemon/remote behavior separately when needed; `codex agents` and `--remote` cannot use `--no-daemon`.

Before handoff, verify both binaries exist, CLI `--version`, and both `--help` commands. Help does not prove model/tool operation. When safe, hand off before automated checks finish. Report verified absolute paths, build time, a manual check, and pending checks. Shared executables belong to the last build: verify their checkout. Rebuild both after runtime edits; never hand off old binaries as current. Get permission before replacing installed `codex`.

Local profile rules:

- Ordinary `just` Cargo recipes, schema generators, source runners, package assembly, and VS Code Rust checks use `dev-small`. Keep its settings. The npm hooks generator uses the same recipe.
- Explicit `--profile`, nextest `--cargo-profile`, and `--release` take priority where supported. Nextest `--profile` selects runner settings, not Cargo settings.
- CI, benchmarks, and source-built Dylint retain their profiles. VS Code run/test buttons, inherited Windows setup, and sandbox smoke utilities also retain theirs.
- Plain Cargo has no local default. Pass `--profile dev-small` for direct build/run/check/clippy; use `just test` for tests.
- Do not delete caches used by builds/editors. Use optimized/cross-platform local builds only on request or to reproduce affected behavior. Published releases use optimized `--release`.

### Validate narrowly

Verify the intended outcome with suitable evidence. Select checks from changed inputs, fork interactions, and conflicts, not sync size. Do not default to retesting upstream behavior untouched by the fork. Use focused checks when fork interactions, failures, or unresolved concerns require them. Record the selected checks, reused evidence, and uncertainty. Never label unchecked behavior as verified. Narrow scope does not reduce required evidence.

- **Documentation:** review wording/links; run `git diff --check`. No builds or tests.
- **Rust:** use `just test -p <crate> <test-filter>`, not `cargo test`. Use scoped formatting below and targeted Clippy when needed. Update affected assertions/snapshots together. Exclude unrelated stale expectations. Never disable tests to hide intended output changes.
- **Workflows/scripts:** run `actionlint` on changed workflows and small logic fixtures. Build natively only for affected packaging or approved experiments.
- New tests/snapshot coverage require an explicit user request. Implementation alone does not authorize them. Update existing tests/snapshots only as needed; retain checks. This overrides new integration/snapshot test requirements. Report gaps and ask before additions.
- **Formatting:** pass affected checkout-relative or absolute files: `just fmt --files scripts/format.py justfile`. Use `--files -- <paths>` for leading hyphens. Select changed Rust child modules explicitly; selected parents do not format unselected children. Select repository files; this local tool trusts the supplied paths. Unsupported file types fail. Formatters apply their project exclusions and report normal file errors. `just fmt --rust` formats the whole Rust workspace only; use it only when that scope is needed. Unscoped `just fmt` and CI `just fmt-check` remain full-repository commands.
- Run required generators for changed inputs. Keep schemas, locks, and Bazel data correct without Bazel CI. Reuse upstream outputs only when verified source, generator inputs, output bytes, and provenance match the applied changes, and fork-specific input effects are unchanged. Record that evidence. Regenerate when fork edits or conflict resolution changes generator inputs/outputs, or evidence is uncertain.
- Do not default to whole-crate/workspace suites. If focused checks cannot verify outcome or safety, explain the gap and ask before expansion. A clean merge is not validation. Verify upstream CI before claiming it passed.

After approved integration, reuse PR checks and the verified binary pair only when effective source, generated files, dependencies, toolchain, features, flags, and pair provenance still match. Preserve checkout-wrapper isolation; never copy Cargo fingerprints to fake freshness. For integration changes or version bumps, rebuild affected binaries and rerun affected checks. Release live/package checks remain required; releases still use optimized `--release`.

### Real-session checks

- Decide whether changed behavior needs a real session. State behavior and expected result. Use sufficient existing focused checks; do not replay every feature/session. The release smoke turn remains required.
- Use private temporary `CODEX_HOME` with `diagnostic_log_capture = true`, not ephemeral mode. Retain history for debugging outside personal history.
- Securely provision only needed config, authentication, and resources. Keep the intended profile/model. Put session, database, and log paths inside the test home. Do not copy personal history or symlink shared mutable state.
- Use a temporary working directory unless behavior needs a specific checkout. Use `--no-daemon`; for daemon/remote checks, use an isolated server, never the personal shared daemon.
- Record created session IDs, commands, binary/source identity, expected/actual results. Before cleanup, retain rollouts, output, and diagnostic logs in private task evidence. Never report or commit credentials.
- Keep failed test homes until triage finishes. Remove only task-owned state after retaining required evidence. Titles alone do not make personal sessions disposable.

### Commit, open a PR, report

Use PRs for implementation, documentation, and syncs. The lead owns delegated work too. After validation and owner decisions, commit the task branch, push, and open a PR against `main`.

**Done means validated changes are committed, pushed, and ready for PR review.** A worktree or binary handoff alone is not completion.

- Stage only finished task changes, tests, generated files, and intent. Inspect the staged diff.
- Use `Trung Ngo <1390402+trungnt13@users.noreply.github.com>`. Verify `git var GIT_AUTHOR_IDENT` and `git var GIT_COMMITTER_IDENT`; fix local config if needed. Ask before attribution-only published-history rewrites.
- Before branch updates, recheck the PR base. Resolve against base, fork, upstream, and this guide; verify affected behavior. For owner decisions, inspect all exposed conflicts and ask one question with recommendations. Later commits can expose more.
- Reconcile task-owned duplicate edits without losing other work. If validation/push/PR creation is blocked, commit finished work and report the blocker. Identify unfinished work separately.
- Report changes, exact checks/results, skipped checks, uncertainty, commit, branch, PR URL, and push status. State ready, blocked, or merged. Keep one evidence record; reference it in later handoffs and integration reports instead of repeating unchanged checks. Refresh changed results/status. PR readiness does not authorize merging.

### Push permissions

- Implementation requests authorize task-branch commits, pushes, and PR creation, not merges or direct implementation on `main`. Do not commit directly to `main` or merge PRs without explicit approval.
- An authorized rebase or history reconciliation may update `main` with `--force-with-lease` without separate force-push approval. Other force pushes, tag moves, discarding work, and releases need explicit approval.
- An explicit release request authorizes the existing workflow's candidate publication, verified remote `main` update, and safe local fast-forward. Source fixes still require PRs.
- Push explicit refs with `--no-follow-tags`, never all tags. Release-triggering tags, including inherited workflow tags, require publication approval.
- Before authorized force pushes, fetch and preserve remote-only work, including workflow release commits. Use a lease against the inspected tip. Never refresh a failed lease and retry blindly.

## Upstream sync

- Fork (`origin`): [`trungnt13/asm`](https://github.com/trungnt13/asm).
- Parent (`upstream`): [`openai/codex`](https://github.com/openai/codex).
- Last incorporated commit: `f60516235442c99cb14490b2758e5f7a882b029d`.
- Date: 2026-10-12.
- Subject: Preserve Windows runtime registration failure diagnostics (#53426).

- Verify the upstream URL before fetching. Sync on a task branch; submit a PR. Use the requested method there. Otherwise, merge upstream to preserve published history; rebase only unpublished branches. Follow push permissions for authorized history rewrites. Use `git cherry-pick -x` for targeted ports.
- Enable reviewed conflict reuse for each sync command, without changing shared Git config. Keep the chosen strategy/history options, such as `--rebase-merges`; these flags only enable resolution reuse:
  - Merge: `git -c rerere.enabled=true -c rerere.autoupdate=false merge <upstream-target>`.
  - Rebase: `git -c rerere.enabled=true -c rerere.autoupdate=false rebase <upstream-target>`.
  - Review reused resolutions before staging; verify affected behavior under the validation rules. Rerere is not evidence of correctness.
  - After manual resolution, record it with `git -c rerere.enabled=true -c rerere.autoupdate=false rerere`.
  - Continue with the same flags: `git -c rerere.enabled=true -c rerere.autoupdate=false rebase --continue` or `git -c rerere.enabled=true -c rerere.autoupdate=false merge --continue`.
- A full sync includes the chosen upstream `main` commit and all ancestors. Capture baseline and target first; report that fixed range. Focus on fork differences and affected upstream changes. Apply development validation/integration rules.
- After each rebase, report a short high-level list of upstream changes. Group related changes, not commits. Give one plain sentence per idea, user effect, and source link. Keep status/validation separate.
- After full sync on the branch, update the marker in that PR. Cherry-picks do not advance it. Verify with `git show -s --format='%H%n%cs%n%s' <upstream-commit>` and confirm ancestry to the fork branch.

## Platforms and CI

Focus on macOS and Linux (Ubuntu). Preserve other inherited code/workflows unless removal is requested; do not add them to fork CI/releases. Other inherited workflow changes need approval. Inspect triggers: [V8 canary](../.github/workflows/v8-canary.yml) runs on PRs with expensive builds conditional on relevant changes; [CLA](../.github/workflows/cla.yml) restricts its job to `openai`.

- **[Blocking CI](../.github/workflows/blocking-ci.yml):** manual-only `workflow_dispatch`; never automatic or required. Keep workspace formatting, production Clippy, and result collector. Cover `codex-cli`, `codex-tui`, `codex-core`, `codex-config` on both release targets. Preserve `--lib --bin codex -- -D warnings`. No test-target matrix, `cargo shear`, or warning suppression. Check other affected packages narrowly as needed.
- **[Postmerge CI](../.github/workflows/postmerge-ci.yml):** automatic optimized builds, packaging, smoke checks, uploads, diagnostics, and results. Push `paths-ignore` covers only root `AGENTS.md` and this guide. Filtered pushes cannot cancel active builds. Other paths remain eligible; never exclude all Markdown or `docs/`. Publication does not require blocking CI. Report existing failures separately.

Port useful upstream action/toolchain/security/build fixes without broad matrices. Do not repeat CI locally for every edit. No unrelated platforms, Bazel suites, SDKs, remote executors, V8 source-build canaries, or OpenAI-only infrastructure in fork CI. The two-target V8 build below is not a broad suite/canary.

## Releases

### Operate a release

1. **Prepare.**
   - Require an explicit release request. Deliver the complete installable package before unrelated features. Use the existing workflow, not another pipeline.
   - Select the repo explicitly: `gh ... -R trungnt13/asm`.
   - Resume an unfinished candidate first; follow **Recover**. Do not rebase or replace its pinned source during recovery.
   - Before a new release, fetch upstream with tags and prune. Capture the latest `upstream/main` commit and rebase the fork onto that fixed target. Follow the sync, PR, validation, and push rules; push the intended `origin/main` before dispatch.
   - Before source pushes to `origin/main`, verify the V8 prerequisite below. Release the intended `origin/main` commit, not unpushed work.
   - Publication does not authorize installation or shared-server restart.
2. **Check CI.**
   - Inspect failures; fix relevant code/package errors.
   - For runner allocation/communication failures, inspect annotations/logs and rerun only failed jobs. Do not infer code defects or change build settings from runner failure alone.
   - If the owner requires recovery first, verify successful retry before dispatch.
3. **Check live use.**
   - Before dispatch, run one time-limited local live turn with default config/existing sign-in. Do not override profile/model.
   - Use the release-source CLI/helper pair and [real-session isolation](#real-session-checks).
   - Require a real model response and verified successful read-only Code Mode output.
   - Failure, timeout, or missing authentication blocks dispatch; never skip this check. Report source/result separately from package checks. Keep credentials out of CI.
4. **Dispatch.**
   - Dispatch [`fork-rust-release.yml`](../.github/workflows/fork-rust-release.yml) on `main` with `publish_release=true`.
   - Let it select version, create candidate, build, publish, verify, and update remote `main`. Do not repeat manually or wait for postmerge builds; candidates build/reuse archives.
   - Ordinary branch dispatch is build-only. A `main` push does not request publication.
5. **Recover.**
   - After interruption, rerun failed jobs. A new resume dispatch uses `main`, `publish_release=true`, `resume_run_id=<original run ID>`.
   - Keep original version/commit despite upstream changes. Never invent versions or replace tags. Report failures needing owner decisions.
6. **Verify.**
   - A platform is ready when its checked archive and platform checksum manifest are published.
   - Candidate completion requires `release`, both `verify_native` jobs, and `sync_main` success for that candidate.
   - Legacy `v*` pushes/dispatches can publish. Require publication and both native checks, not `sync_main`.
   - Inspect recorded checks and current metadata. Do not repeat successful checks on unchanged artifacts. Build-only success, tag pushes, or version labels do not prove publication.
7. **Finish.**
   - Fetch; fast-forward local `main` only safely. Preserve local edits/concurrent commits.
   - Report release commit, upstream baseline, checks, remote/local integration, and blocked updates.

### Release notes

- Publish each release's notes on GitHub and in the final report. Compare with the previous completed release; include only packaged changes.
- Separate `ASM` and `Codex` headings; use `No changes` for empty sections.
- Use ASD-STE100: short direct active sentences, one term per concept.
- One flat item per feature/fix, on one source line: `**Name:** What changed. User effect or required action.`
- Include relevant defaults, limits, experimental status, and supporting commit/file links on that line.
- Keep version, platform readiness, and validation outside feature lists. Do not claim unverified effects.

### Workflow requirements

These govern release code, not another manual procedure.

#### Version

- Select the highest upstream `rust-v` prerelease tag by Semantic Versioning, not creation/publication time. Publication is unnecessary. Increment only its numeric patch by one; retain suffix: `rust-v0.162.0-alpha.18` → `v0.162.1-alpha.18`.
- For repeats, append one positive numeric fork counter: `alpha.18.1`, then `alpha.18.2`. Use highest used counter plus one, never gaps. Preserve upstream identifiers; put counter before build metadata. New upstream `alpha.19` starts `v0.162.1-alpha.19`. Installer accepts repeated numeric alpha/beta parts.
- Resume unfinished releases/candidates first. Prove complete published assets:
  - Current family and highest fork version also require release, both native checks, and main sync tied to the exact candidate. Successful resumes count.
  - Older candidates do not require expired Actions logs.
  - Never overwrite tags/assets. If completion is unproved, no upstream prerelease exists, or proposed version is not above every fork version, stop and ask.
- Create immutable `agent/release-<run-id>`, not a `main` version commit. Set `[workspace.package].version` in [`Cargo.toml`](../codex-rs/Cargo.toml) and matching [`Cargo.lock`](../codex-rs/Cargo.lock) workspace versions. Do not change dependencies.
- Tag, Cargo, and CLI `--version` must agree except prefixes. Renaming a tag cannot change binary bytes. Never move candidates/tags on retries. Keep candidate branches for recovery and pre-policy bumps until the next successful release.

#### Build and package

Keep fork/upstream release workflows separate. Use GitHub-hosted runners:

| Platform | Target | Runner | Build timeout |
| --- | --- | --- | --- |
| macOS ARM64 | `aarch64-apple-darwin` | `macos-15` | 180 minutes |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | 90 minutes |

- Legacy lookup step limit: 190 minutes; wait guards: macOS 150, Linux 185. Timeout fails, not duplicate builds. Only legacy tag jobs allow 360 minutes total for lookup/build/checks. Candidates allow build limit plus 15 publication minutes. Actual builds keep table limits; fresh candidates do not wait for postmerge.
- Linux minimum: Ubuntu 22.04 / glibc 2.35. Keep builds/native smoke there; a GNU triple on newer runners is insufficient. Before April 2027 retirement, use an Ubuntu 22.04 container on supported runners. Never raise the runtime minimum.
- Use optimized `--release`, not `dev-small`; `CARGO_PROFILE_RELEASE_STRIP=debuginfo`. Retain function symbols and optimization. Fix timeouts without weakening the profile. Upstream tests do not justify further symbol/dependency removal.
- Keep shell commands compatible with macOS Bash 3.2, including `set -u` array expansion.
- Build `codex` and `codex-code-mode-host` from one commit/target/profile. Each `codex-<target>.tar.gz` contains:
  - `bin/codex`, `bin/codex-code-mode-host`, `codex-package.json`, `codex-path/rg`.
  - Linux: `codex-resources/bwrap`, built from the same source/release profile. Embed its final SHA-256 in the CLI before packaging.
  - Use existing canonical layout/pinned ripgrep manifest. Include only required resources, with regular executable files and matching version/target metadata. Shared-server startup must validate/recognize the package without host helpers.
- Final six assets: two archives, two `SHA256SUMS-<target>`, aggregate `SHA256SUMS`, and [`install.sh`](../scripts/install/install.sh). Each platform manifest covers its archive and pinned installer. No source trees/diagnostics.
- Detect candidate contract from pinned smoke's required `codex-package.json`, not optional resource flags. Pass `--no-zsh` only if its CLI accepts it. Older candidates keep original contracts.
- Pin builds, packaging, installer bytes, checks, and reuse to candidate SHA. Pin orchestration to dispatch SHA; save controls before candidate checkout. Never fetch newer controls mid-run. Legacy uses exact tagged SHA; verify tag resolution. Candidate packaging may precede tagging.
- Before upload, run [native smoke](../.github/scripts/smoke-codex-archive.py): CLI version matches Cargo; both binaries' help succeeds with usage; manifest/resources valid; bundled ripgrep/Bubblewrap smoke passes. Helper `--version` is not required. Record binary/archive sizes.
- GNU source-built ELF executables require x86_64 GNU loader and glibc symbols no newer than 2.35. Pinned ripgrep may be static; check architecture and minimum-OS execution. Run version/help in a clean Ubuntu 22.04 container with only declared runtime libraries; build runners alone do not prove compatibility.

Package limits:

- No Apple Developer signing/notarization, paid Apple membership, Azure Key Vault, release secrets, or self-hosted runners.
- Bundle only required ripgrep/Linux Bubblewrap. Use system shell; no voice, patched zsh, or optional helpers. Do not change sandbox/security defaults.
- Fork Linux uses native GNU and [`install-gnu-build-tools.sh`](../.github/scripts/install-gnu-build-tools.sh), not Zig/MUSL wrappers. Keep inherited MUSL tooling for upstream workflows.
- Retain `AWS_LC_SYS_NO_JITTER_ENTROPY` and verified V8 through [`setup-rusty-v8`](../.github/actions/setup-rusty-v8/action.yml). Explicitly install Python 3.11+ on Ubuntu 22.04 for verification.
- Preserve upstream GNU system allocator, locale, and PTY choices. Do not change Rust platform conditions to mimic MUSL. Use system OpenSSL 3; system liblzma is permitted. Match [`smoke-ubuntu-archive.sh`](../.github/scripts/smoke-ubuntu-archive.sh) libraries to ELF dependencies. Do not add ALSA/GStreamer requirements for GNU migration.
- Agree intent before adding platforms, DMGs, optional resources, npm, R2, WinGet, or website/OpenAI-only publishing.

#### V8 dependency release

Build V8 separately only if version/target/baseline lacks a verified fork release. Use [`fork-v8-release.yml`](../.github/workflows/fork-v8-release.yml), tag `asm-v8-v<exact resolved v8 crate version>-glibc2.35`. The suffix identifies GNU-baseline generation and matching macOS pair. Do not replace/move the old MUSL release.

- Manual branch dispatch defaults to build-only. Explicit `publish=true` builds/smokes, then tags/publishes at that commit. Tag pushes publish too. Tag moves or published asset replacement need approval.
- Reuse unchanged verified inputs. For changed source/patches/flags/bindings under the same version, ask how to version V8.
- Build only optimized sandbox + pointer-compression pairs for macOS ARM64/Linux x86_64 GNU. Use existing Bazel source pair/staging helper locally on GitHub-hosted runners. No BuildBuddy, remote execution, paid infrastructure, or broad suites. Retain required static-library symbols.
- Require both targets' native `codex-v8-poc` sandbox/JavaScript smoke, including Ubuntu 22.04 GNU. Reuse GNU Bazel platform and older glibc sysroot.
- Publish only each target's archive, Rust binding, and two-file manifest. Normal release, **not Latest**, no installer; separate from CLI discovery.
- Before the first consuming `main` push, publish and verify both new version/generation pairs. CI/CLI releases consume only matching verified fork V8. Verify GitHub SHA-256 digests and exact manifest names/hashes. Missing/bad assets fail; no upstream fallback. Keep upstream action paths for other workflows.

#### Reuse and publish

Serialize releases. Finish without later chat actions or token-push-triggered workflows. Legacy tags must already have correct Cargo versions.

Use [artifact lookup](../.github/scripts/find-postmerge-artifacts.sh) per target for unexpired smoke-checked archives from current dispatch/original candidate run. Do not wait for other platforms or rebuild ready published ones. Legacy may reuse successful same-repo push-to-`main` target jobs at exact tagged SHA. Wait for matching active runs instead of duplicate builds; timeout fails. Build only without usable archives. API errors are failures, not cache misses.

1. Create pinned tag, draft, and common installer once. Never change candidates, move tags, or replace assets. Resume uploads only with matching names/sizes/digests.
2. Independently build/reuse/check/upload each platform. Verify archive/installer GitHub digests, then smoke the downloaded archive. Upload `SHA256SUMS-<target>` last as readiness marker.
3. On first readiness, publish normal non-draft, even `alpha`, **not Latest** while incomplete. Append other checked assets. Payloads are immutable by policy. GitHub's immutable-release setting must remain disabled for append-only publication; never change it automatically.
4. After both platforms pass, append aggregate manifest for old installers, verify six assets, mark Latest, and retain both final native checks. Only then merge candidate into `origin/main`. Preserve concurrent changes; stop on version conflicts. Never force-push.
5. Resume only missing work after verifying ready platforms. For archives uploaded before readiness markers, recover/verify exact public bytes, not rebuild, even after Actions expiry. Partial releases are incomplete. Legacy completed four-asset releases remain valid. Candidates whose pinned installer lacks platform manifests keep all-platform publication.

Before first partial release, deploy platform-aware `tngo-workflow/setup.py`. It selects highest published host-ready version; macOS cannot select Linux-only. Installer defaults to Latest; explicit `--release` may install a ready platform earlier. In-flight runs retain frozen policy.

### Installer

- Use only `trungnt13/asm` GitHub `v*` releases for macOS ARM64/Linux x86_64 GNU. Verify GitHub digests and host `SHA256SUMS-<target>` before complete-package installation. Reject incomplete two-binary archives in the current installer. Do not treat existing two-binary installations as complete; historical releases keep pinned installers. Use legacy `SHA256SUMS` only when host manifest is absent, never after invalid digests/manifests.
- Reject unsupported targets/Linux without glibc 2.35+ before metadata downloads/state changes. GNU only, no MUSL fallback. Historical MUSL requires original installer. Use separate target-qualified GNU package directories; preserve MUSL packages and shared config/state.
- Preserve `--release`, `CODEX_HOME`, `CODEX_INSTALL_DIR`, locking, and safe `current` selection. Store under `packages/asm-standalone`; keep upstream packages/update markers and shared Codex config/state and `codex` command unchanged.
- Only the owner installs externally. No OpenAI/CDN/legacy npm downloads, daemon-only installs, or `auto-update-version` writes.
- Disable built-in update checks/prompts/commands/daemon loops regardless of settings/markers. CLI/daemon cannot install/download upstream releases. Preserve ordinary daemon startup/restart independently.

### Build experiment

Optional `macos_concurrency_experiment`: compare cold same-commit `macos-15` builds with 2/3 Cargo jobs; other settings equal. Save separate timing/resource/size/smoke results. No postmerge reuse or publication, even on tags. Measure before defaults change; one run does not prove general speedup.

## Intentional behavior differences

### Minimal diagnostics

- Default `diagnostic_log_capture = false`. This startup policy controls Rust stderr/file logs, persistent diagnostics, feedback ring/continuous tags, optional TUI replay, and rollout-trace recorders. Disabled capture cannot be enabled by `RUST_LOG`, `log_dir`, or recorder environment variables.
- Opt in with `diagnostic_log_capture = true`; restart affected processes. Existing filters then apply. Propagate policy to code-mode helper; inherited helper settings cannot override it. Standalone helpers without user config need explicit opt-in.
- When disabled, do not allocate feedback capture, install its tracing layers, open/migrate diagnostic databases, or run insertion/pruning/reclamation workers. Ordinary startup leaves historical files unchanged.
- Explicit history inspection/feedback export may read old logs without capture. Thread deletion removes historical diagnostic records through existing-only on-demand access; never create missing files. Preserve feedback submission/upload errors; never invent evidence.
- Preserve direct errors/warnings, exit status, protocol events, saved conversation/tool history, subagent relationships, memory, and operational state. Detached-daemon startup/fatal stderr and Windows sandbox security-audit logs remain separate. Do not suppress them or weaken security. Keep product analytics/account usage unchanged.
- Disable the built-in Statsig metrics exporter, including optimized releases/account reloads. Keep its config spelling; default `otel.metrics_exporter` to `none`. Preserve explicit OTLP collectors. Opt-in `runtime_metrics` uses a manual reader without timer/network exporter when none is configured. No unsolicited session-storage size telemetry scan. Check fork-disabled paths with existing focused tests, not broad upstream retesting.

### Adaptive reasoning

Use opt-in `[adaptive_reasoning]` in the shared backend, not the TUI. Require `features.step_model_switching` and `features.reasoning_effort_override`.

| Setting | Default | Validation |
| --- | --- | --- |
| `enabled` | `false` | Opt-in only |
| `decision_model` | `"gpt-6-luna"` | Nonblank |
| `decision_model_api_env` | `"DECISION_API_KEY"` | ASCII environment-variable identifier, not a secret |
| `min_effort` / `max_effort` | `"low"` / `"max"` | Ordinary levels; `min_effort <= max_effort` |
| `decision_timeout_ms` | `200` | 1–5000 ms |
| `max_context_bytes` | `8192` | 128–32768 UTF-8 bytes across all Decisions input items |
| `update_on` | `["turn_start", "tool_result"]` | Only these trigger types |
| `rubric_instructions` | Built-in rubric when absent | Nonblank; at most 4096 UTF-8 bytes |

- Adapt ordinary root conversations and spawned subagents. Keep internal workers unchanged. Use the Decisions API to select supported effort within bounds. Keep Ultra/Persistent selections, models, service tiers, execution modes, permissions, and saved defaults unchanged.
- If fewer than two efforts remain, skip Decisions and warm-up; keep current effort.
- Read credentials only from the named backend environment variable, default `DECISION_API_KEY`. Do not fall back to `OPENAI_API_KEY`; explicit variable-name overrides remain supported. Keep main-model authentication separate. Do not provision credentials or log keys or evidence. Do not route Decisions requests through Guardian.
- If set, use `rubric_instructions` instead of the built-in rubric, without a suffix. Enforce choices and runtime limits in code. Keep instructions and results outside the main conversation.
- Collect genuine user, assistant, and tool evidence, newest first. Exclude injected user instructions, compaction output, images, and encrypted reasoning. Scale scan limits with the byte budget: at least 128 history/input items and 32 content parts, up to 512 items and 128 parts at 32768 bytes. These scan limits and the byte budget can still omit older history; they do not promise a complete transcript.
- **P0 manual review:** bound total evidence and rubric separately using the table limits. Split larger evidence into ordered Decisions input messages of at most 8192 UTF-8 bytes each, preferring line boundaries. Preserve text and the default single-message request. Rubric instructions remain at most 4096 bytes. Each item can exceed 1000 tokens, but its byte cap keeps it below the 10K-token item limit. Keep this evidence outside the main conversation.
- At eligible session start, send one tiny synthetic warm-up in the background. Without triggers or supported effort within bounds, skip it. Reuse its pooled Decisions client. Keep separate minimal instructions. Discard its answer. Keep history and settings unchanged.
- Limit warm-up to two seconds. At shutdown, abort and join it. Do not retry, ping periodically, or add warm-up settings. Warm-up can add one billable request. It does not guarantee a warm backend or connection.
- Before each main step with an enabled trigger, collect current evidence and await Decisions and validation in the turn task, without a separate step job or completion channel. Then capture immutable step settings and run the model. Do not retry or wait for a busy settings-publication permit. Keep only warm-up and log writing in the background.
- Publish a current result within its original deadline through live next-step settings. Keep captured requests unchanged. Later evidence invalidates pending decisions, not already applied settings.
- For missing credentials, unsupported models, cancellation, refusals, invalid answers, request failure, or timeout, keep current effort. A Decisions failure must not stop the main step.
- On accepted user, assistant, or tool evidence, invalidate old decisions. Reject results after cancellation, task replacement, shutdown, settings changes, or deadline expiry.
- Reuse live step-settings validation and publication. Preserve the original request-effort pin, trusted append-only updates, reconnect behavior, and explicit compaction handling.
- Turn adaptation off after a successful manual model or effort change. Log the transition. Ordinary turn-start and spawn settings do not turn it off. Use `/adapt` to toggle it for the current thread, without changing saved defaults. Keep runtime state separate from captured step settings.
- Report runtime mode and its revision at thread attachment and on each transition. Ignore older revisions so delayed notifications cannot restore old mode. Show `<effort>·auto` when adaptation is on without an outstanding error, including startup and the wait before a decision. Show `<effort>·err` after a Decisions error; keep adaptation on and retain current effort. When off, show only effort. Use the baseline before the first captured step. The marker reports runtime health, not a promised effort change. Unsupported steps keep their effort without a Decisions call. Before each eligible main request, report captured effort through `turn/reasoningEffortUpdated`, including fallback steps. Keep mode and captured effort separate; toggles do not change already-running steps.
- A spawned subagent copies its parent’s runtime mode once at spawn. Later parent toggles do not change that child. Preserve the child’s own mode across live eviction and reload, not backend restarts. Do not apply this rule to internal workers. If the assigned task is encrypted and no readable task is available, log the fallback and keep current effort; never forward ciphertext or guess the task.
- Retain captured effort and turn identity in live TUI thread snapshots, outside saved history. Restore them after switching threads without accepting stale history settings. Clear them for a new turn or manual settings change.
- Deliver transient effort notifications directly to clients, without entering rollout persistence before the main request.
- Clients must use this backend. The feature does not replace Desktop's backend. Use HTTP for Decisions, not the Responses WebSocket.
- Report errors through backend runtime state, independent of diagnostics. Include health at attachment and after live child reload. New children inherit mode, not parent errors. Deduplicate visible warnings by failure episode. Clear errors only after a current main-step decision passes acceptance guards, including unchanged effort. Warm-up errors count; warm-up success, cancellation, and supersession do not clear errors.
- With `diagnostic_log_capture = true`, append sanitized attempt metadata to a separate per-session `decision.jsonl`. Include credential failures, API outcome, and acceptance outcome. Do not log evidence, rubric text, bodies, or authentication headers. With capture off, create no log file or writer. Bound buffering and storage; never wait for disk on the decision path. Report dropped records or writer failures separately. Crash-interrupted attempts can lack an end record.
- Report decision latency and fallbacks without sensitive input. The deadline bounds result eligibility and local publication, not API latency. Preserved prefixes do not guarantee cache hits. Verify live update order and backend acceptance separately from mock checks before making claims.

### TUI automatic session names

`[auto_rename]` is TUI-only. Preserve Desktop naming, conversation model/history, and `/rename` suggestions. Defaults: enabled, first user message, user-message context, one-time naming, 36 characters, 960-byte initial prompt.

Prefer `gpt-6-luna`/low only with OpenAI, ChatGPT account, and available catalog entry; otherwise conversation model. Manual `/rename` retains upstream model selection.

- `enabled`: default true. `model`/`reasoning_effort` affect only titles; omissions keep defaults. Server must preserve explicit models. Unavailable models fail, not fall back. Custom-provider models need no catalog entry.
- `first_trigger`: `first_user_message` (default) or `first_completed_turn`; ignore failed/interrupted completions.
- `context`: `user_message` (default) or `recent_conversation`. Recent context includes initial request/substantive user/assistant messages, not commentary/tool output.
- `max_context_bytes`: 128–8192 source bytes, excluding added guidance. Omission keeps original budget.
- `recent_message_limit`: 1–32, default 8, including initial request; 1 keeps only latest. Tight byte budgets drop earlier messages to keep markup complete.
- `max_title_chars`: 1–128, default 36. Optional positive integer `max_title_words` is prompt-only. Omission keeps “under five words where possible”; explicit values request at most that many. Never truncate by words. Captured prompt/character limit belong to the originating request.
- `additional_naming_guidance`: append at most 512 UTF-8 bytes; never replace fixed rules/prompt. No `instructions` alias. Complete prompt cap: 9500 bytes. **P0 manual review:** larger opt-in budgets can exceed 1000 tokens; keep the 10K-token item limit.
- `auto_update`: default false, initial-only. Refresh requires `enabled` and `auto_update`:
  - Include captured previous title as bounded JSON-string data. Ask the model to retain it unless the task meaningfully changes. When updating, preserve its core subject and wording. Routine progress does not justify renaming.
  - Keep initial/manual prompts unchanged. Fit guidance in existing cap without splitting markup.
  - Persist each successfully generated title in one client-local `CODEX_HOME` record, scoped to local/remote app server. Restore eligibility only if record matches saved name.
  - Protect named sessions with missing/corrupt/mismatched records; `/autorename` can opt in. Persist initial automatic names even with updates disabled, allowing refresh on later resume.
  - View switches retain counters. Resume resets interval and excludes completed turns.
- `auto_update_interval_turns`: positive integer, default 5:
  - Count successful user-turn completions while displayed. Ignore duplicate/failed/interrupted/background/helper/replay events.
  - At most one attempt per interval. Reset on start, not success. Failures retain ownership and wait another interval.
  - Count during pending requests, but start no new request until settlement.
  - Track unnamed opt-in threads before initial requests; ownership requires successful save.
  - Retain only latest user-turn/item IDs, one ordering anchor, and at most 8192 UTF-8 user bytes. Long turns/pending initial generation must retain context after event-buffer eviction.
  - First-user-message titles saved before assistant completion count that completion. Completed-turn initial titles count later completions.

`/autorename`:

- Generate/apply one title immediately with the same context/model/effort/budgets/limits/guidance. Ignore `enabled`, `first_trigger`, `auto_update`, and interval for this explicit action; flags gate later refresh.
- Use latest substantive user text or shared bounded recent collector. May replace existing/resumed names. Preserve `/rename` suggestions/supplied names.
- Supersede pending background naming. Successful generation restores persisted eligibility, even with identical title. Failure leaves name/eligibility unchanged.
- Deduplicate; retain titles on failure and report explicit failures. Discard results for undisplayed threads or changed saved names. Reuse app-server APIs; no Desktop changes.

Naming safety:

- Keep background requests, manual-name precedence, cancellation, and originating-thread checks. Compare current server name with captured name before replacement. Identical generated names need no RPC.
- `/rename`, dashboard naming, and explicit `set_thread_title` intent remove eligibility before RPC, including same-name requests. Removal failure aborts RPC with an error. Later RPC failure may leave refresh disabled.
- Generic cancellation clears only in-memory ownership. External name changes cancel automatic work.
- Records: at most 512 UTF-8 bytes/128 characters; validate reads and write atomically. Read only on attachment or recurring attempt/save, not streaming/rendering.
- Recheck persisted eligibility before refresh to respect another TUI's opt-out. Corrupt/unreadable records fail closed; report persistence errors.
- File/server writes cannot be jointly atomic. Same-name external renames and concurrent read/write races remain best-effort. No shared APIs, rollout records, or Desktop policy changes to solve them.

### Account-security setup reminder

Do not fetch/display optional Daybreak setup reminders, including reconnect/account refresh. Suppress loaded events too. Preserve authentication, security enforcement, approvals, account email, backend usage banners, and upstream reminder rendering/checks. Never weaken security requirements.

### Standalone request audit

- Keep `debug prompt-input`. `debug prompt-request` exports versioned JSON: effective base instructions, production request, ordered input, full advertised tool definitions, Responses Lite prefixes, optional output schema.
- Reuse app-server extension installer/request builder. No audit work in ordinary inference paths.
- Default ephemeral. Session records/persistent state-backed tools require `--allow-session-state`; setup may opt in within isolated temporary homes. Never dispatch queued input.
- Label fresh standalone debug turn, not live-session/exact transport capture. Report omitted history, hooks, input-triggered skill/plugin injections, uncaptured tools, auth-dependent metadata, and server additions.
- Never send inference or claim startup lacks network/state effects. Exports can contain sensitive instructions/tool content.

### Model catalog ownership

- [`codex-rs/models-manager/models.json`](../codex-rs/models-manager/models.json) is upstream-owned. Accept upstream sync changes, not personal/fork edits; never freeze it.
- Personal edits belong only in `/home/trungnt13/codes/tngo-workflow/configs/model-catalogs/models.json`. Never copy them into the built-in catalog.

### Model catalog instruction visibility

Top-level `include_model_catalog_in_spawn_agent_list`, default `true`:

- False removes generated model/effort/tier lists from V1/V2 spawn descriptions and `<model_catalog>`, plus guidance pointing there.
- Preserve custom descriptions, arguments, loading, validation, and history. Use normal invalidation when retained lists stop applying.
- Inclusion is independent of `model_catalog_in_context` placement and `expose_spawn_agent_model_overrides` argument exposure.

### MCP resource tools

Top-level `mcp_resource_tools_enabled`, default `true`:

- False removes `list_mcp_resources`, `list_mcp_resource_templates`, `read_mcp_resource` registration, callable entries, and declarations from direct tools, Code Mode, and `ALL_TOOLS`.
- Enabled keeps existing registration conditions. Keep MCP connections, other server tools, apps/plugins, and shared TypeScript preamble independent.
- Apply on config load; no new live reload/history rewrites.

### Web search copyright instruction visibility

Top-level `include_web_search_copyright_compliance`, default `true`:

- False removes only terminal Copyright compliance from local `web.run` description.
- Preserve preceding quotation/word limits, arguments, execution, search modes, history, hosted `web_search` instructions, and model safeguards.
- If subsection changes or is not terminal, warn and retain full description.
- Apply to standalone cached/indexed/live modes.

For all three visibility/tool settings above, do not update setup-managed profiles before a supporting CLI release.

### Saved forks, upstream sides, and parallel conversations

- Saved/temporary root forks keep the parent's effective prompt-cache routing key independently of thread/session ID. Persist for resume/further forks. Old rollouts need no ancestor lookups. Keep upstream guardian/subagent routing. Shared routing does not guarantee cache hits.
- `/side` and `/btw` retain upstream temporary lifecycle: restricted commands, reference-only instructions, no subagents/saved pairing. Ctrl+/ switches; Ctrl+C returns/discards. Ordinary picker navigation discards sides too. Preserve helper names/constants/tests where practical; avoid needless upstream rewrites.
- `/chat [message]` shares Side lifecycle/restrictions:
  - Top-level `chat_model`/`chat_reasoning` select initial settings; empty/omitted values inherit current parent selection.
  - Require model Fast tier/feature permission. Reject unavailable startup/submission, not downgrade. Reject automatic fallback without Fast.
  - Keep explicit Fast across view switches/catalog refresh/sync. Block Fast-toggle shortcut. Preserve Side/Btw/Parallel settings.
- In temporary Side/Chat, `/sendlast [extra message]` forwards raw last completed assistant reply plus optional text as main user input. No prefix/navigation/settings change; atomically start or steer main. Reject complete items above 10,000 UTF-8 bytes, never truncate. Explicit item can exceed 1K tokens; require manual review when changed.
- `/sync` replaces idle temporary Side/Chat with main's latest context, retaining model/effort/tier. Discard old history only after replacement setup/selection succeeds. Keep Side pairing/boundaries. Context refresh does not guarantee cache hits.
- In idle temporary Side/Chat, `/fork [name]` saves an independent ordinary conversation:
  - No navigation, pairing changes, or automatic work. Copy retained inherited replay/temporary history. After paginated main compacts, do not recover older raw entries outside model context.
  - Persist current model/effort/tier through resume. Append durable ordinary-role context; never rewrite history.
  - Default `<main title> (fork)` or `Untitled (fork)`; keep explicit names.
  - Keep raw replay in memory only for temporary root forks. Normally append only new items; copy retained history only on explicit save. Temporary chats stay pathless/unlisted until saved.
- `/parallel` creates a saved ordinary user fork. Normal permissions/flags/platform/busy checks control commands. Inherited history is reference, not instructions to continue parent work/control its agents. Parallel may own goals/subagents. Keep initial transcript clean without stored/model-history rewrites.
- Keep one non-nested main/companion pair:
  - `/parallel` from parallel returns to parent/replaces selected companion; older chats stay saved.
  - Block `/side`/`/btw` while viewing either open-pair member. Restore after closure; saved closed chats do not block.
  - Temporary sides retain upstream restrictions, including rejection of another companion. Only added exceptions: `/sendlast`, `/sync`, non-navigating `/fork`.
- Ctrl+/ switches without stopping. Empty-composer/no-modal Ctrl+C stops parallel, pauses active goal, returns to parent, and preserves history. Selecting subagents must not stop it.
- Ordinary `/new`, `/clear`, `/resume`, `/fork`, `/cd`, `/worktree` navigation exits parallel. Archive/delete affects displayed parallel and returns to parent.
- Persist pairing/transcript-display boundaries in client-local ASM state per app-server target:
  - Resume only selected session as ordinary chat, parent or parallel. Never restore pair/attach counterpart. Keep saved boundaries without file migration/history rewrites. Pair navigation exists only during active `/parallel`.
  - Closing selections must not delete conversations. Do not encode UI relationships with analytics source classification or false ancestry.
  - Pair metadata cannot block history. Log unreadable transcript-display metadata. Confirmed paired-thread deletion clears stale selection, not saved history/child record. Temporary connection/permission failures retain pairing. Missing parents cannot trap Ctrl+C.
- Stock clients see ordinary threads through unchanged APIs; companion switching is unnecessary there. Files/explicit global config remain shared.

### Recap session ID

End manual/automatic TUI recaps after any next action with dimmed `Session: <id>`. Use displayed conversation ID, not recap-generation thread. Preserve generation/timing.

### Turn completion footer

Use one dimmed logical line: `Worked for 5s • 11:05 AM • 12.4k in • 9.8k cc • 820|600 ou • 3ag • <session-id>`.

- Preserve timing, optional runtime metrics, and narrow wrapping. Use displayed conversation ID, not turn ID.
- Show provider-reported conversation-turn totals, including compaction. Cached input belongs to input; reasoning to output. Omit unknown counts; never substitute context estimates/session totals. Reuse saved usage; paginated history may omit counts without migration.
- `Nag` counts distinct related subagents seen running during a live turn, including descendants/already-running agents. Retain across client view switches. Omit restored-turn counts if start was not observed. Tokens remain parent-only. No request/accounting changes.

### Copy session ID

`/copyid` copies only displayed session ID via existing clipboard. No `/status`, assistant reply, or picker. Allow during tasks, sides, and parent-owned subagent views. Report missing IDs/clipboard failures. Preserve `/copy` and platform support.

### ASM branding

Use ASM in README heading, repo description, release titles/introduction, workflow step descriptions, fork TUI labels, and terminal title. Credit OpenAI Codex. Preserve actual upstream service/product names.

Local CLI help descriptions and TUI messages use ASM; displayed command hints use `asm`, including resume, agents, login, and MCP setup. Keep underlying executable names, CLI parsing names, upstream product names, and installer commands unchanged. Command hints require a user-provided `asm` alias or wrapper.

Keep `codex` executable/version prefix, crate/config/state/archive/tag/workflow/job/check identifiers. Never redirect upstream links/installers for branding.

### Background terminal waits

Top-level `background_terminal_min_timeout` / `background_terminal_max_timeout`, milliseconds, defaults 5000 / 300000. Reject zero/reversed/unrepresentable bounds. Clamp empty `write_stdin` polls; show bounds in description and finish early on exit.

Preserve initial/nonempty waits and Code Mode outer `exec`/`wait` limits. Exit alone does not resume idle model turns.

### Memory reasoning effort

Optional `[memories]` `extract_reasoning_effort` / `consolidation_reasoning_effort` for V1/V2 use existing effort type. Defaults `low` / `medium`, independent of parent. Preserve models/other memory behavior.

### Transcript spacing

Keep Markdown paragraphs/code blocks/list items adjacent while streaming and completed. Preserve blanks inside code blocks, raw output, message boundaries, and user padding.

### Collapsed tool calls

`[tui] collapse_tool_calls = true` is opt-in; default `false`. Rich fullscreen behavior:

- Collapse consecutive shell/MCP/dynamic calls, routine V2 subagent interactions, normal agent starts/completions, and routine waits into one expandable summary.
- Separate counts for calls/starts/completions. Lifecycle notifications are not calls/running tools. Show distinct command/tool names or agent paths in first-use order.
- Count each successful subagent message/follow-up once; matching wait start/end previews once per contiguous displayed group. Completed waits cannot stay running. Keep waits with results/failures/interruption visible.
- Replace overflowing names with `+N more`. Positive integer `[tui] collapse_tool_calls_max_lines` defaults to `1`. Budget applies to each new header's counts/names; larger values wrap. Retained reading snapshots keep captured content on resize and may wrap further. Ellipsize if even counts cannot fit.
- Interruptions, other activity, and chat messages split groups. Keep active tool in a separate short row until committed history includes it.
- Expansion restores original previews/order. Keep failures/approvals visible. Preserve execution/model history/raw output/full transcript. Disabled keeps existing rendering.

### Compact status line

- Join with `·`, no spaces. Use `CtxN%` for used context, `CtxN%left` for remaining context, and `F:on` / `F:off` for fast mode.
- Context capacity: compact token count only, no `window` suffix, including `/statusline` previews.
- Lowercase model labels; remove `gpt-`; retain at most first three letters, including custom aliases. Append all version digits without separators: `GPT-6-Astra` → `ast6`, `GPT-6.1-Sol` → `sol61`, `Terra 5.6` → `ter56`. No digits means no suffix.
- Reasoning: first three letters, including model-with-reasoning: `medium` → `med`, `high` → `hig`, `xhigh` → `xhi`.
- Apply to footer, `/statusline` preview, `/subagents` model info. Preserve IDs, other picker labels, terminal titles. Remove role-leading space only in footer: `Main[default]`. Preserve paths/roles/colors/order/single-row truncation.
- Opt-in `cache-hit-rate` in `/statusline`/`tui.status_line`; defaults unchanged. Show current thread's latest reported request cached/input ratio as `CchN.N%`, rounded to one decimal. Use existing usage; no averages/totals/new requests/APIs. Hide unknown/zero-input usage. Missing cache detail remains indistinguishable from reported zero.
- Offer opt-in `output-tps` with unchanged defaults. Show `Ntps`, rounded to the nearest whole number, for the latest successful ordinary Responses request: provider-reported output tokens, including reasoning, divided by that successful attempt's duration from send to completion. Exclude failed attempts, retry waits, compaction, warmup, tools, and other threads. This is observed request throughput, not pure decoding speed. Calculate once at live response completion and carry the result on the existing token-usage event. Cache the formatted value in the TUI and use its existing refresh without another notification, redraw request, timer, or streaming estimate. Retain the latest valid result across failed requests; omit unavailable timing or usage and do not reconstruct it from replayed history.

### Subagent context windows, service tiers, and picker

Optional top-level `subagent_model_context_windows = { "gpt-6-astra" = 272000, "gpt-6.1-sol" = 272000 }`:

- Positive `i64` integers; exact final model-ID matches only for V2 spawned children through startup/model changes/reload. Missing entries retain scalar/model fallback.
- Keep original `model_context_window` so matches cannot spill into unmatched grandchildren. Preserve maximum clamping, usable headroom, and compaction settings. Root/review/guardian unchanged; no API/rollout fields.

`[subagent_service_tiers]`, e.g. `gpt-6-sol = { high = "fast" }`:

- Match final child model/effective effort after overrides/roles. Matches override root tier throughout child lifetime, including later root-tier changes. Unmatched follows upstream; root requests unchanged.
- Reject unsupported matched tiers and Fast when disabled. No spawn-argument exposure.

`/subagents` labels:

- Append known compact model/effort to main/child titles: `gpt-6.1-sol-high-fast` → `sol61-hig-fast`, `gpt-6-astra-xhigh` → `ast6-xhi`.
- `-fast` only for matched Fast or inherited Fast; matched `default` suppresses inheritance. Do not guess settings/remove thread-ID descriptions/change navigation.
- Append known positive configured capacity: `sol61-hig-fast-272k` / `sol61-max-272k`. Exact map applies only to canonical-path V2 spawned children, never main/V1. Otherwise use configured scalar, including main. Omit unknown/nonpositive values, not catalog guesses.
- Labels show config, not confirmed routing/runtime capacity/usage/remaining context. Capacity precedes model clamping/headroom. Keep TUI-only with other metadata/navigation unchanged; no new APIs/request tracking.
