# Personal fork guide

## Principles

- This fork is for Trung Ngo's personal use only.
- Keep changes minimal, safe, and minor. Prefer durable local fixes with little upstream drift or conflict. Preserve correctness, maintainability, and runtime performance.
- Assume upstream tested unchanged code. Check fork changes, conflicts, and packages narrowly; upstream tests do not prove them.
- Record current fork purposes and constraints here, not task history. Ask before contradicting intent. After approval, replace the rule; resolve mechanical conflicts without further approval.

This guide overrides conflicting instructions, including [AGENTS.md](../AGENTS.md). Its validation and platform limits replace upstream defaults. Other coding, formatting, test-authoring, and generator rules still apply. Recorded intent does not prove implementation, integration, or publication.

## Development workflow

### Start or resume

- Before edits or after resume, refresh Git status, branches, and worktrees. Before syncs or pushes, inspect remotes and fork history. For overlaps or unexplained changes, inspect relevant repo sessions. Recover context from Git and sessions, not owner memory or task diaries.
- Before edits or syncs, create a named `codex/` task branch. Use separate worktrees for concurrent tasks. Coordinate shared files; assign one staging/commit owner per checkout. Worktrees are not deliverables.
- Prefer reusable checkouts: `python3 scripts/dev-worktree.py acquire codex/<task>`. Keep the path through builds and validation. Each slot has an exclusive owner/branch lease. The owner defaults to `CODEX_THREAD_ID`; if unset, use `--owner <id>`. Use `--slot <name>` for parallel tasks.
- While the PR is pending, keep its checkout and lease. After approved integration includes its branch tip in clean local `main`, run `python3 scripts/dev-worktree.py release codex/<task>`. Keep the detached checkout for reuse. Never clear another owner's lease or discard work. Inspect and coordinate abandoned leases.
- Preserve existing work. If blocked, use another worktree. Get permission before committing or stashing unrelated work. Keep authorized stashes until restoration is verified. Never overwrite others' work to clean a checkout.

### Build the binary first

For runtime/UI edits, inspect affected paths, including streaming and completed output. Make the smallest coherent change.

- Before Cargo, verify disk space. Rebase/release maintenance may remove verified inactive local build caches under existing path, lock, and process checks. Record measurements and removals. Preserve active/uncertain artifacts, source, user state, and verified binaries. If safe cleanup cannot provide space, ask.
- Local `just` recipes default to main's `codex-rs/target`. Preserve explicit `CARGO_TARGET_DIR` and Cargo target-directory options.
- Keep native host mode, toolchain, profile, features, flags, checkout, and cache stable. Do not add `--target` merely to name the host. Timestamp, source, version, and other input changes can force rebuilds.
- Reuse matching verified [fork V8](#v8-dependency-release). Export the same verified absolute `RUSTY_V8_ARCHIVE` and `RUSTY_V8_SRC_BINDING_PATH`. Old Cargo fingerprints do not prove validity.
- Export `AWS_LC_SYS_NO_JITTER_ENTROPY=1` consistently; changes rebuild AWS-LC and dependents.
- From `codex-rs`, run `just build-dev`. Prioritize `codex` and `codex-code-mode-host` with matching source, native target, and `dev-small` profile. Serialize Cargo jobs sharing a target directory. Verify the producing checkout; test executables are not this pair. During builds, inspect independent code and affected tests/snapshots.
- Before releasing shared-cache ownership, verify real copies of both executable siblings in one task-owned directory under the checkout, e.g. `.agents/test-bin/<build-id>/`. Do not use symlinks or hard links. Record source commit, build inputs, absolute paths, and SHA-256 hashes. Test and hand off this unchanged pair with its evidence, not overwriteable shared-target executables.

The local Cargo wrapper must:

- Set `RUSTC_WORKSPACE_WRAPPER` to the checkout's absolute `scripts/local-rustc-workspace.sh`. Isolate workspace artifacts by checkout while sharing registry dependencies. Do not treat another checkout's old code as fresh. Keep paths stable for warm builds.
- Preserve explicit wrapper overrides, including disabling. Unless an override isolates checkouts, use a separate target directory.
- Give Clippy a checkout-private cache, defaulting to main's `clippy/<checkout-path-hash>` subdirectory; Clippy replaces the wrapper. Explicit Clippy target directories must also be private.
- Lock build/check/Clippy/nextest per effective cache. Save inputs and fingerprint reasons under main's `.agents/dev-build-cache/<date>/`. Preserve explicit `CARGO_LOG` filters. Inspect logs before blaming cache loss.
- Clean stale artifacts only on Linux with Python 3.11+, complete process visibility, and no cache users or Rust build/editor processes. Hold each profile's native Cargo lock. Use descriptor-relative traversal; reject symlink directories and artifacts outside the physical cache. Log removal counts and skip reasons. Skip other hosts or uncertain activity.
- Keep the two-day cleanup rule: `incremental` directory modification time, or latest `deps` file access/modification time. Age does not establish checkout inactivity; removal can force active checkouts to rebuild.
- Block reusable-checkout release/switching during active wrapper commands. Direct Cargo bypasses protection; finish it before releasing the checkout.
- Keep interactive `cargo run` I/O without holding the cache lock for the live app. Leave CI and Windows unchanged.

For ordinary runtime/UI checks, use the verified absolute copied CLI with `--no-daemon` and [real-session isolation](#real-session-checks). This uses the embedded backend and sibling helper without changing/stopping the shared daemon. Check daemon/remote behavior separately when needed; `codex agents` and `--remote` cannot use `--no-daemon`.

Before handoff, verify both files, CLI `--version`, both `--help` commands, and pair source identity. Help does not prove model/tool operation. When safe, hand off before automated checks finish. Report verified absolute paths, build time, a manual check, and pending checks. After runtime edits, rebuild and copy both; never present old binaries as current. Get permission before replacing installed `codex`.

Local profile rules:

- Use unchanged `dev-small` settings for ordinary `just` Cargo recipes, schema generators, source runners, package assembly, and VS Code Rust checks. The npm hooks generator uses the same recipe.
- Preserve explicit supported `--profile`, nextest `--cargo-profile`, and `--release` overrides. Nextest `--profile` selects runner settings, not Cargo settings.
- Preserve profiles for CI, benchmarks, source-built Dylint, VS Code run/test buttons, inherited Windows setup, and sandbox smoke utilities.
- Plain Cargo has no local default. For direct build/run/check/clippy, pass `--profile dev-small`. For tests, use `just test`.
- Never delete caches used by builds/editors. Use optimized/cross-platform local builds only on request or to reproduce affected behavior. Releases use optimized `--release`.

### Validate narrowly

Select checks from changed inputs, fork interactions, conflicts, failures, and unresolved concerns—not sync size. Do not retest untouched upstream behavior by default. Record checks, reused evidence, and uncertainty. Narrow scope does not reduce required evidence. Never call unchecked behavior verified; a clean merge is not validation. Verify upstream CI before claiming it passed.

- **Documentation:** review wording/links; run `git diff --check`. No rebuild is required. Every PR still needs the live check below.
- **Behavior:** accept changed fork behavior with the built binary in a small real user scenario. Mocked/simulated responses are not acceptance evidence. Do not add mock tests or run them by default.
- **Rust:** when needed, run existing non-mock checks with `just test -p <crate> <test-filter>`, not `cargo test`. Use scoped formatting and targeted Clippy. Update affected assertions/snapshots together; exclude unrelated stale expectations.
- **Workflows/scripts:** run `actionlint` on changed workflows and small checks of the actual script. Build natively only for affected packaging or approved experiments.
- Without an explicit user request, do not add tests or extend coverage. Update existing tests/snapshots only for compile fixes or production changes' required output changes. Preserve inherited checks; never delete, ignore, weaken, or disable them to hide output changes or avoid mock tests. This overrides upstream integration/snapshot requirements. Report coverage gaps before asking for new tests or broader checks.
- Limit fork-added test files to 200 lines. In inherited files, limit fork-added behavioral coverage to 200 lines per file. Preserve upstream coverage and necessary compile adaptations. Keep meaningful user-visible/safety checks; remove lower-value cases rather than split files or compress formatting.
- **Formatting:** select affected repository files with checkout-relative or absolute paths: `just fmt --files scripts/format.py justfile`. For leading hyphens, use `--files -- <paths>`. Select changed Rust child modules explicitly; parents do not format unselected children. The tool trusts paths, rejects unsupported types, honors formatter exclusions, and reports normal file errors. Use workspace-only `just fmt --rust` only when needed. Unscoped `just fmt` and CI `just fmt-check` remain full-repository commands.
- Run required generators for changed inputs. Keep schemas, locks, and Bazel data correct without Bazel CI. Reuse upstream outputs only with matching source, generator inputs, output bytes, provenance, and unchanged fork-specific effects. Record proof; regenerate for fork/conflict changes or uncertain evidence.
- Do not default to whole-crate/workspace suites. If focused checks cannot establish outcome or safety, explain the gap and ask before expansion.

After approved integration, reuse checks and the verified binary pair only with matching effective source, generated files, dependencies, toolchain, features, flags, and pair provenance. Preserve checkout-wrapper isolation; never copy fingerprints to fake freshness. For integration changes or version bumps, rebuild affected binaries and repeat affected checks. Release live/package checks remain separate requirements.

### Real-session checks

- Every PR needs a successful real-provider live check before review or merge readiness. Select the smallest changed-behavior/fork-interaction scenario; state the expected result. Builds, offline checks, and mocks are not substitutes. Missing credentials, budget, or compatible setup block readiness. Do not replay unrelated upstream features or whole sessions. Release smoke remains separately required.
- Before calls, set request/turn counts and a time limit. Minimize input/requested output; set supported token limits. Stop at proof or first failure, then inspect evidence. Do not default to broad replays, model matrices, benchmarks, or repeated cold builds.
- Use existing sign-in only as authorized. New paid API/provider/model tests require approval for endpoint, model, request/token limits, and spend cap. Never invent a dollar budget.
- Do not trigger unrelated billable retries, warmups, Decisions calls, or extra subagents. Budget required runtime helpers; preserve release defaults. Record calls, elapsed time, and reported tokens. Report cost only when known; otherwise mark unknown. Missing prerequisites mean blocked, not verified.
- Use private temporary `CODEX_HOME` with `diagnostic_log_capture = true`, not ephemeral mode. Retain diagnostic history outside personal history. Securely provision only needed config, authentication, and resources. Preserve intended profile/model. Put sessions, databases, and logs inside that home. Never copy personal history or symlink shared mutable state.
- Unless behavior needs a specific checkout, use a temporary working directory. Use `--no-daemon`; for daemon/remote checks, use an isolated server, never the personal shared daemon.
- Record session IDs, commands, binary/source identity, and expected/actual results. Before cleanup, retain private rollouts, output, and diagnostics. Never report or commit credentials.
- Keep failed homes until triage ends. Remove only task-owned state after retaining evidence. Titles do not make personal sessions disposable.

### Desktop SSH tests

For isolated Desktop SSH tests, use [asm-desktop-ssh-test](../.agents/skills/asm-desktop-ssh-test/SKILL.md): setup, reuse, candidate updates, and Desktop checks. Preserve the normal backend. Follow the build and real-session rules.

### Commit, open a PR, report

Use PRs against `main` for implementation, documentation, and syncs. The lead also owns delegated work. After validation and owner decisions, commit the task branch, push, and open the PR.

**Done means validated changes are committed, pushed, and ready for review. Finish authorized merges before reporting done.** A worktree or binary handoff is insufficient.

- Stage only finished task changes, tests, generated files, and intent. Inspect the staged diff.
- Use `Trung Ngo <1390402+trungnt13@users.noreply.github.com>`. Verify `git var GIT_AUTHOR_IDENT` and `git var GIT_COMMITTER_IDENT`; fix local config if needed. Ask before attribution-only published-history rewrites.
- Before branch updates, recheck the PR base. Resolve mechanical conflicts without new approval when they preserve this guide. Verify affected behavior against base, fork, and upstream. For unsafe changes, ambiguous intent, or owner decisions, inspect all exposed conflicts; ask one question with recommendations. Later commits can expose more conflicts.
- Reconcile task-owned duplicate edits without losing others' work. If validation/push/PR creation is blocked, commit finished work and report the blocker. Identify unfinished work separately.
- Report changes, exact check results, skipped checks, uncertainty, commit, branch, PR URL, and push status. State ready, blocked, or merged. Keep one evidence record; reference unchanged results in later handoffs/integration reports. Refresh changed results/status. Readiness does not authorize merge.

### Push permissions

- Implementation requests authorize task-branch commits, pushes, and PR creation—not direct implementation on `main`.
- An upstream rebase request also authorizes merging its sync PR after required checks. Do not ask again. Other PRs, including features, require explicit merge approval.
- Within the task, allow `--force-with-lease` on fork branches, including `main`, without separate approval. Plain `--force`, tag moves, discarded work, and releases still require approval.
- A release request authorizes existing-workflow candidate publication, verified remote `main` update, and safe local fast-forward. Source fixes require PRs.
- Push explicit refs with `--no-follow-tags`, never all tags. Release-triggering tags, including inherited ones, require publication approval.
- Before lease-protected pushes, fetch and preserve remote-only work, including workflow release commits. Use an explicit lease against the inspected tip. Never refresh a failed lease and retry blindly.

## Upstream sync

### Shared compatibility gate

For syncs or saved-cache/wire-format changes, apply this gate even after conflict-free rebases. Follow narrow validation and real-session rules; this gate grants no new provider, paid-matrix, broader-check, or test-authoring authority.

1. Map all affected writers and readers, including stock consumers. Trace caches, migrations, serialization, app-server responses, raw events, and live notifications. Review consumers, not only producers. For new mandatory handshakes, inspect existing clients, fixtures, and reconnects.
2. Keep fork-only formats/migrations in ASM-owned versioned caches, never stock caches. Preserve native canonical rollouts, thread IDs, and shared metadata. For incompatible formats or migration-number/checksum collisions, change the ASM namespace. Never rewrite applied migration checksums.
3. Gate fork wire extensions with explicit versioned feature capabilities—not `experimentalApi`, client names, or versions. Verify ordinary behavior without opt-in.
4. Before resume or warming, check cold timeline, search, turn, and item reads in separate private homes. Hand bounded saved history to an independent supported stock reader. Then check read/resume and warm-cache reuse. Cover affected paging directions, hidden-only rows, and cursor contracts. Where affected, check opted-in/out ASM clients concurrently.
5. Record actual backend/client executables, versions, hashes, hosts, and `CODEX_HOME`. Launcher versions do not establish backend identity. Resume-assisted reads do not prove cold reads or UI rendering. Record unchecked platforms/consumer boundaries. Provider turns and packaging/version checks do not prove interoperability.
6. Before merge, refresh the actual base and verify the candidate against it. For base advances, inspect interactions. Rebuild the pair and repeat focused checks when relevant inputs change. Reuse evidence only while inputs match; do not merge first and rely on an old build.
7. Keep remaining findings unresolved and missing boundary evidence blocked. Do not claim readiness or silently expand scope. Ask for out-of-scope decisions.
8. For stock-cache contamination by older ASM, recover manually only. Verify host, actual reader executable/version/hash, and home. Stop all writers. Back up the derived database and WAL/SHM. Quarantine/rebuild only that cache; preserve canonical history/shared state. Never reuse the older writer against the recovered home.

#### Known upstream cold-history limitation

Stock `rust-v0.168.0-alpha.7` returns an empty cold child timeline with canonical rollouts but no derived history database. Resume builds the projection and exposes messages. This missing catch-up persists through `748ab55ca7`. Independent stock `rust-v0.169.0-alpha.2` also returned an empty first cold timeline for valid real history; resume restored its projection and saved permissions. ASM already rebuilds before querying; this sync did not introduce the difference. Isolated checks found no history loss.

The owner accepts this limitation only for the sync through `748ab55ca7` and its ensuing release. Keep cold-stock reading unresolved, not passed. Report resume-assisted handoff separately. This acceptance waives no other checks and permits no personal history/cache changes.

### Live backend model catalog

Every upstream rebase/sync must fetch the live authenticated backend catalog. Source, cached, and bundled catalogs cannot establish current availability; backend models can change without source changes.

- Use the configured backend and authorized sign-in. Follow client path `models?client_version=<client version>`, authentication, account routing, and discovery settings. Never send credentials to an unverified endpoint. This read-only request is not inference.
- Save successful raw JSON as timestamped private task-owned `models.json` outside tracked source. Validate model-list shape. Record fetch time, backend URL, client version, and returned IDs. Keep credentials/authentication headers out of evidence, logs, and commits.
- Compare with the bundled catalog. Report new models, visibility, and relevant capabilities; link saved JSON in the rebase report. Catalog presence does not prove execution. Do not automatically change model/config defaults.
- If authentication, connectivity, or discovery policy blocks a fresh fetch, report the reason. Do not substitute cached data or claim live availability was checked.

Fork (`origin`): [`trungnt13/asm`](https://github.com/trungnt13/asm). Parent (`upstream`): [`openai/codex`](https://github.com/openai/codex).

Last incorporated commit: `748ab55ca7de0b647d9a7a9d9a8e70e00f28666d`, 2026-11-08. Subject: Increase Bazel core unit test shards from 8 to 16 (#60206).

- Before fetching, verify the upstream URL. Sync on a task branch and submit a PR. Use the requested method. Otherwise, merge to preserve published history; rebase only unpublished branches. Follow push permissions for authorized rewrites. Use `git cherry-pick -x` for targeted ports.
- For each sync command, enable reviewed conflict reuse without shared Git config changes. Preserve chosen strategy/history options, e.g. `--rebase-merges`. Use `git -c rerere.enabled=true -c rerere.autoupdate=false merge <upstream-target>` or `git -c rerere.enabled=true -c rerere.autoupdate=false rebase <upstream-target>`. Review reused resolutions before staging; validate affected behavior. Rerere does not prove correctness.
- After manual resolution, run `git -c rerere.enabled=true -c rerere.autoupdate=false rerere`. Continue with `git -c rerere.enabled=true -c rerere.autoupdate=false rebase --continue` or `git -c rerere.enabled=true -c rerere.autoupdate=false merge --continue`.
- For full syncs, capture baseline and target first; include the chosen upstream `main` commit and all ancestors. Report that fixed range. Focus on fork differences and affected upstream changes. Follow development validation/integration rules.
- After each rebase, group upstream changes by idea, not commit. Give one short plain sentence per idea, user effect, and source link. Keep status/validation separate.
- After full sync, update the marker in that PR. Cherry-picks do not advance it. Verify `git show -s --format='%H%n%cs%n%s' <upstream-commit>` and ancestry to the fork branch.

## Platforms and CI

Focus on macOS and Linux (Ubuntu). Do not add other platforms to fork CI/releases. Preserve other inherited code/workflows unless removal is requested. Other inherited workflow changes need approval. Inspect triggers: [V8 canary](../.github/workflows/v8-canary.yml) runs on PRs; expensive builds depend on relevant changes. [CLA](../.github/workflows/cla.yml) restricts its job to `openai`.

Use standard GitHub-hosted runners; paid larger runners need explicit approval. V8 canary Apple Silicon variants use `macos-15` and two Bazel jobs for runner memory.

- [Blocking CI](../.github/workflows/blocking-ci.yml) and [postmerge CI](../.github/workflows/postmerge-ci.yml) are manual-only `workflow_dispatch`, never automatic or required.
- Blocking CI keeps workspace formatting, production Clippy, and result collection. Cover `codex-cli`, `codex-tui`, `codex-core`, and `codex-config` on both release targets. Preserve `--lib --bin codex -- -D warnings`. No test-target matrix, `cargo shear`, or warning suppression. Check other affected packages narrowly.
- Postmerge CI keeps optimized builds, packaging, smoke checks, uploads, diagnostics, and results for targeted cross-platform checks. Do not run it for every merge. Releases retain their own build/package checks; publication does not require blocking CI. Report existing failures separately.
- Port useful upstream action/toolchain/security/build fixes without broad matrices or routine local CI repetition. Exclude unrelated platforms, Bazel suites, SDKs, remote executors, V8 source-build canaries, and OpenAI-only infrastructure. The required two-target V8 build is not a broad suite/canary.

## Releases

### Operate a release

1. **Prepare:** require an explicit release request. Prioritize the complete installable package over unrelated features. Use the existing workflow and explicit repo `gh ... -R trungnt13/asm`. Resume unfinished candidates first; preserve their pinned source. For a new release, fetch upstream with tags/prune. Capture latest `upstream/main`; rebase onto that fixed target under sync/PR/validation/push rules. Verify the V8 prerequisite before source pushes to `origin/main`. Push the intended commit before dispatch; never release unpushed work. Publication grants no install/shared-server-restart authority.
2. **Check CI:** inspect failures; fix relevant code/package errors. For runner allocation/communication failures, inspect annotations/logs and rerun only failed jobs. Do not infer code defects or change build settings from runner failure alone. If the owner requires recovery first, verify retry success before dispatch. Read prior candidate logs in memory with supported GitHub CLI raw-output opt-in. Never render untrusted log escapes to a terminal.
3. **Check live use:** before dispatch, run one local turn with default config and existing sign-in. Do not substitute cheaper profiles/models without approval. This smoke has standing approval for required System1/helpers through configured endpoints/models. Use the release-source CLI/helper pair and [real-session rules](#real-session-checks). Limit to 120 seconds and the smallest read-only Code Mode task. Stop at proof or first failure. Require a real response and successful read-only Code Mode output. Failure, timeout, or missing authentication blocks dispatch. Broader tests, new endpoints/models, and unrelated paid calls need approval.
4. After every smoke attempt, report actual request counts, elapsed time, input/cached-input/output/reasoning tokens, and required helper usage separately where reported. Distinguish totals/subtotals and unavailable fields. Include failed-attempt usage; never replace actual usage with estimates. Planning estimate: 20–60K input and 1–5K output/reasoning plus helpers, not a cap or actual usage claim. Report source/live result separately from package checks. Keep credentials out of CI.
5. **Dispatch:** run [`fork-rust-release.yml`](../.github/workflows/fork-rust-release.yml) on `main` with `publish_release=true`. Let it select version, create candidate, build, publish, verify, and update remote `main`. Do not repeat these manually or wait for postmerge builds. Candidates build/reuse archives. Ordinary branch dispatch is build-only; a `main` push does not request publication.
6. **Recover:** after interruption, rerun failed jobs. For a new resume dispatch, use `main`, `publish_release=true`, and `resume_run_id=<original run ID>`. Preserve original version/commit despite upstream advances. Never invent versions or replace tags. Report failures needing owner decisions. Only with explicit approval, supersede a source-defective failed unpublished candidate with a new immutable candidate. Record old run/source/tag and draft assets in `.github/scripts/superseded-fork-releases.json`. Preparation must verify that record and reject publication or asset drift. Preserve old candidate/tag/assets and occupied version. Never apply this exception to published or partially ready releases.
7. **Verify:** a platform is ready after its checked archive and platform checksum manifest are published. Candidate completion requires `release`, both `verify_native` jobs, and `sync_main` success for that candidate. Legacy `v*` pushes/dispatches require publication and both native checks, not `sync_main`. Inspect recorded checks/current metadata; do not repeat successful checks on unchanged artifacts. Builds, tag pushes, and version labels do not prove publication.
8. **Finish:** fetch and safely fast-forward local `main`. Preserve local edits/concurrent commits. Report release commit, upstream baseline, checks, remote/local integration, and blocked updates.

### Release notes

Publish notes on GitHub and in the final report. Compare with the previous completed release; include only packaged changes. Keep separate `ASM` and `Codex` headings. Write `No changes` in empty sections. Use ASD-STE100: short active sentences and consistent terms.

Use one flat source-line item per feature/fix: `**Name:** What changed. User effect or required action.` Include relevant defaults, limits, experimental status, and supporting commit/file links. Keep version, platform readiness, and validation outside feature lists. Never claim unverified effects.

### Workflow requirements

These govern release code, not another manual procedure.

#### Version

- Select the highest upstream `rust-v` prerelease tag by Semantic Versioning, regardless of creation/publication time or publication. Increment only its numeric patch by one; retain suffix: `rust-v0.162.0-alpha.18` → `v0.162.1-alpha.18`.
- For repeats, append one positive numeric counter: highest used plus one, without gaps. Preserve upstream identifiers; put the counter before build metadata. `alpha.18.1` precedes `alpha.18.2`; new upstream `alpha.19` starts `v0.162.1-alpha.19`. Installer accepts repeated numeric alpha/beta parts.
- Resume unfinished candidates/releases first. Prove complete published assets. For the current family and highest fork version, also prove release, both native checks, and main sync for the exact candidate; successful resumes count. Older candidates need no expired Actions logs. If completion is unproved, no upstream prerelease exists, or the proposed version is not above every fork version, stop and ask.
- Create immutable `agent/release-<run-id>`, not a `main` version commit. Set `[workspace.package].version` in [`Cargo.toml`](../codex-rs/Cargo.toml) and matching [`Cargo.lock`](../codex-rs/Cargo.lock) workspace versions. Do not change dependencies.
- Tag, Cargo, and CLI `--version` must agree except prefixes. Tag renames do not change binary bytes. Never overwrite tags/assets or move candidates/tags on retries. Keep candidate branches for recovery and pre-policy bumps until the next successful release.

#### Build and package

Keep fork/upstream release workflows separate. Use GitHub-hosted runners:

| Platform | Target | Runner | Build timeout |
| --- | --- | --- | --- |
| macOS ARM64 | `aarch64-apple-darwin` | `macos-15` | 300 minutes |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | 90 minutes |

- Candidates allow build limit plus 15 publication minutes: 315 total on macOS. Preserve optimization, flags, runner, and candidate; allow needed time rather than weakening builds. Concurrency experiments retain 180-minute builds. Resume candidates; never replace ready platform assets.
- Legacy lookup allows 190 minutes; wait guards are macOS 150/Linux 185. Timeout fails; do not duplicate builds. Only legacy tag jobs allow 360 total minutes. Actual builds retain table limits; fresh candidates do not wait for postmerge.
- Keep Ubuntu 22.04 / glibc 2.35 as the Linux minimum and build/native-smoke baseline. A GNU triple on newer runners is insufficient. Before April 2027 retirement, use an Ubuntu 22.04 container on supported runners. Never raise the minimum.
- Use optimized `--release` and `CARGO_PROFILE_RELEASE_STRIP=debuginfo`, not `dev-small`. Retain function symbols/optimization. Fix timeouts without weakening profiles. Upstream tests do not justify more symbol/dependency removal. Keep shell commands compatible with macOS Bash 3.2, including `set -u` array expansion.
- Build `codex`, `codex-code-mode-host`, and `codex-exec-server` from one commit/target/profile. Pass all three prebuilt executables to canonical packaging. `codex-<target>.tar.gz` contains `bin/codex`, `bin/codex-code-mode-host`, `bin/codex-exec-server`, `codex-package.json`, and `codex-path/rg`. Linux also contains `codex-resources/bwrap`, built from matching source/release profile; embed its final SHA-256 in the CLI before packaging.
- Keep canonical layout/pinned ripgrep manifest. Include only required resources as regular executables with matching version/target metadata, including `execServer: "bin/codex-exec-server"`. Shared-server startup must recognize/validate the package without host helpers.
- Publish six final assets: two archives, two `SHA256SUMS-<target>`, aggregate `SHA256SUMS`, and [`install.sh`](../scripts/install/install.sh). Each platform manifest covers its archive and pinned installer. No source trees/diagnostics.
- Detect candidate contract from pinned smoke's required `codex-package.json`, not optional resource flags. Pass `--no-zsh` only if supported. Preserve older candidates' original contracts.
- Pin builds, packaging, installer bytes, checks, and reuse to candidate SHA. Pin orchestration to dispatch SHA; save controls before checkout. Never fetch newer controls mid-run. Legacy uses verified exact tagged SHA. Candidate packaging may precede tagging.
- Before upload, run [native smoke](../.github/scripts/smoke-codex-archive.py). CLI/exec-server versions match Cargo; all three help commands succeed with usage; manifest/resources and bundled ripgrep/Bubblewrap pass. Code-mode-host `--version` is not required. Record binary/archive sizes.
- GNU source-built ELF requires x86_64 GNU loader and glibc symbols no newer than 2.35. Pinned ripgrep may be static. Check architecture/minimum-OS execution; run version/help in a clean Ubuntu 22.04 container with only declared runtime libraries. Runners alone do not prove compatibility.
- No Apple signing/notarization, paid Apple membership, Azure Key Vault, release secrets, or self-hosted runners. Bundle only required ripgrep/Linux Bubblewrap; use system shell. No voice, patched zsh, or optional helpers. Preserve sandbox/security defaults.
- Fork Linux uses native GNU and [`install-gnu-build-tools.sh`](../.github/scripts/install-gnu-build-tools.sh), not Zig/MUSL wrappers. Preserve inherited MUSL tooling. Retain `AWS_LC_SYS_NO_JITTER_ENTROPY` and verified V8 through [`setup-rusty-v8`](../.github/actions/setup-rusty-v8/action.yml). Install Python 3.11+ explicitly on Ubuntu 22.04.
- Preserve upstream GNU system allocator, locale, and PTY choices; do not mimic MUSL in Rust conditions. Use system OpenSSL 3; system liblzma is permitted. Match [`smoke-ubuntu-archive.sh`](../.github/scripts/smoke-ubuntu-archive.sh) libraries to ELF dependencies. Do not add ALSA/GStreamer requirements for GNU migration.
- Agree intent before adding platforms, DMGs, optional resources, npm, R2, WinGet, or website/OpenAI-only publishing.

#### V8 dependency release

Build V8 separately only when version/target/baseline lacks a verified fork release. Use [`fork-v8-release.yml`](../.github/workflows/fork-v8-release.yml) and `asm-v8-v<exact resolved v8 crate version>-glibc2.35.2`. The suffix identifies the GNU baseline generation and matching macOS pair. Preserve the old MUSL release.

- Manual branch dispatch defaults to build-only. Explicit `publish=true` builds/smokes, then tags/publishes at that commit. Tag pushes also publish. Tag moves or published-asset replacement need approval.
- Reuse unchanged verified inputs. Same-version archive reuse for Cargo rebases/releases has standing approval only with independent proof of Cargo ABI, bindings, and full dependency-closure compatibility. Native consumer build/JavaScript/sandbox checks must pass. Record omitted source-recipe fixes; never claim source equivalence. Ask for incompatible dependencies/bindings, changed baselines, unsafe reuse, or ambiguous intent. Never replace published assets.
- Do not reuse V8 152.2.0's original `glibc2.35` pair: CppGC sizes and typed-array threshold bindings are stale. Preserve its assets. `glibc2.35.2` rebuilds matching source-generated archives/bindings with Chromium C++ runtime isolation.
- Build only optimized sandbox/pointer-compression pairs for macOS ARM64/Linux x86_64 GNU. Use existing Bazel source pair/staging helper locally on GitHub-hosted runners. No BuildBuddy, remote execution, paid infrastructure, or broad suites. Retain required static-library symbols.
- Require both native `codex-v8-poc` sandbox/JavaScript smokes, including ignored CppGC ABI checks and Ubuntu 22.04 GNU. Reuse GNU Bazel platform/older glibc sysroot.
- Publish each target's archive, Rust binding, and two-file manifest only. Use a normal release, **not Latest**, without installer; keep it separate from CLI discovery.
- Before the first consuming `main` push, publish/verify both new version/generation pairs. CI/CLI releases use only matching verified fork V8. Verify GitHub SHA-256 digests and exact manifest names/hashes. Missing/bad assets fail without upstream fallback. Preserve upstream action paths for other workflows.

#### Reuse and publish

Serialize releases. Finish without later chat actions or token-push-triggered workflows. Legacy tags must already contain correct Cargo versions.

Use [artifact lookup](../.github/scripts/find-postmerge-artifacts.sh) per target for unexpired smoke-checked archives from current dispatch/original candidate run. Legacy may reuse successful same-repo `main` push target jobs at exact tagged SHA. Wait for matching active runs, not other platforms. Never duplicate builds or rebuild ready published platforms. Timeouts/API errors fail; API errors are not cache misses. Build only without usable archives.

1. Create pinned tag, draft, and common installer once. Never change candidates, move tags, or replace assets. Resume uploads only with matching names/sizes/digests.
2. Independently build/reuse/check/upload each platform. Verify GitHub archive/installer digests; smoke the downloaded archive. Upload `SHA256SUMS-<target>` last as readiness marker.
3. At first readiness, publish non-draft normal release, including `alpha`, **not Latest** while incomplete. Append other checked assets. Keep payloads immutable by policy. GitHub's immutable-release setting must stay disabled for append-only publication; never change it automatically.
4. After both platforms pass, append aggregate manifest for old installers. Verify six assets, mark Latest, and retain both native checks. Only then merge candidate into `origin/main`. Preserve concurrent changes; stop on version conflicts. Prefer fast-forwards; necessary rewrites follow push permissions.
5. Verify ready platforms; resume only missing work. For archives uploaded before readiness markers, recover/verify exact public bytes, even after Actions expiry. Do not rebuild them. Partial releases are incomplete. Legacy completed four-asset releases remain valid. Candidates with installers lacking platform manifests retain all-platform publication.

Before first partial release, deploy platform-aware `tngo-workflow/setup.py`. Select the highest published host-ready version; macOS must not select Linux-only. Installer defaults to Latest; explicit `--release` may install an earlier ready platform. Preserve frozen policy for in-flight runs.

### Installer

- Use only `trungnt13/asm` GitHub `v*` releases for macOS ARM64/Linux x86_64 GNU. Before installing the complete package, verify GitHub digests and host `SHA256SUMS-<target>`. Use legacy `SHA256SUMS` only when host manifest is absent, never after invalid digests/manifests.
- Reject incomplete two-binary archives. Keep older complete canonical packages without declared `execServer` compatible. If declared, require its exact regular executable; reject undeclared exec-server. Historical incomplete releases keep pinned installers.
- Before metadata downloads/state changes, reject unsupported targets or Linux without glibc 2.35+. No MUSL fallback; historical MUSL requires its original installer. Use separate target-qualified GNU package directories; preserve MUSL packages and shared config/state.
- Preserve `--release`, `CODEX_HOME`, `CODEX_INSTALL_DIR`, locking, and safe `current` selection. Store in `packages/asm-standalone`. Preserve upstream packages/update markers, shared Codex config/state, and `codex` command.
- Only the owner installs externally. No OpenAI/CDN/legacy npm downloads, daemon-only installs, or `auto-update-version` writes. Do not port upstream standalone-release pruning. Preserve fork packages/layout.
- Disable built-in update checks/prompts/commands/daemon loops regardless of settings/markers. CLI/daemon must not install/download upstream releases. Preserve ordinary daemon startup/restart separately.

### Build experiment

Optional `macos_concurrency_experiment` compares cold same-commit `macos-15` builds with 2/3 Cargo jobs and otherwise equal settings. Save separate timing/resource/size/smoke results. No postmerge reuse or publication, even on tags. Measure before changing defaults; one run does not prove general speedup.

## Intentional behavior differences

### Desktop app-context filtering

`app_context_filter` defaults to an empty list. App-server removes only exact named
`###` sections inside Desktop `<app-context>` instructions before session creation.
Keep unknown sections, surrounding instructions, per-turn additional context, and
existing rollout history unchanged. Filtered session instructions remain consistent
when rebuilding initial context. Use a fresh chat after changing the filter.

### External runtime boundary

Keep ASM's inference and tool runtime separate from external CLI execution. Use
`core/src/external_runtime` for ASM session integration and `ext/external-runtime`
for shared process/framing/cleanup plumbing plus separate Claude and Cursor drivers.
Shared session starts, prompts, and outcomes use neutral types; each driver owns its
CLI protocol encoding. Preserve the legacy `claude-runtime` checkpoint directory,
canonical history encoding, and checkpoint hashes when changing internal names.
Thread deletion removes ASM-owned linkage and private Cursor configuration only after
exclusive runtime ownership is available; leave native CLI conversation history untouched.
Record bounded CLI versions from protocol initialization in opt-in failure diagnostics,
without extra CLI launches or version-number admission gates. Shared errors use neutral names.

### Catalog-bound model providers

When `load_external_models_json` is true, merge bundled external models into custom
`model_catalog_json` catalogs in memory; exact custom IDs retain all their metadata overrides.
When false, keep the custom catalog alone. Provider routing remains bundled and exact-match-only.

Bundled `external_models.json` entries may select an existing named `model_provider`.
Accept the bundled entry’s optional provider- or runtime-qualified `alias` as well as its canonical slug.
Normalize aliases before validation and requests; persist the canonical model/provider pair so
alias changes cannot reroute resumed chats. Preserve custom canonical-ID metadata overrides,
existing full IDs, and managed provider requirements. Do not infer routing from slash-separated
names or accept routing metadata from remote catalogs. Keep endpoints and
credentials in user provider configuration and enforce managed provider requirements.
DeepSeek V4.1 Flash 1017 selects `fireworks`; its alias is
`fireworks/deepseek-v4.1-flash`. No profile or custom agent role is required.
Show aliases in picker entries while retaining canonical selection identity. Native-runtime entries use
`claude/…` and `cursor/…` aliases for their existing `claude-code/…` and `cursor-agent/…` IDs;
do not add HTTP provider bindings or change native model/runtime selection.
Default its context window to 300,000 tokens; retain the 1,000,000-token maximum for explicit overrides.
Its catalog entry selects direct tools; live validation could not execute Code Mode's custom tool through Fireworks.
Provider changes in the picker start fresh sessions and preserve the old chat.
Input and resume admission keep valid catalog-bound and native-runtime routes independent
of the global HTTP-provider default. Validate saved and requested identities separately;
request overrides cannot grant an exception to an unrelated saved chat. Keep mandatory
provider policy, configuration errors, missing-provider checks, and history/runtime boundaries.
Ordinary unbound API models retain upstream saved-provider admission rules. Bound and native
input routes still require their current provider definition and valid configuration, without
requiring the global provider default to match. Route classification uses existing in-memory
model bindings and runtime detection, not discovery.
Cross-provider subagents require `fork_turns="none"`; reject cross-provider history
reuse and preserve saved provider identity on resume. Build a provider-specific
model manager for bound models rather than reuse the parent's discovery identity.
User-spawned subagents using a catalog-bound provider ID must select an explicitly
bound model or a native runtime, including descendants and cold resumes. Reject
unbound selections before startup rather than guessing reverse provider routing.
For unbound custom subagent models, configure a separate provider ID. Ordinary
main sessions and internal workers retain their existing provider behavior.
Keep upstream catalog/API model types unchanged. Disabling bundled catalog display
does not disable explicit selection, matching native external model behavior.

### Claude Code thread runtime

Explicit `claude-code/<model>` IDs select the official persistent `claude -p` CLI
for main threads and ASM-managed children. Versioned `sonnet-` and `opus-` suffixes
are expanded to native `claude-` model names at CLI launch; other native suffixes pass
through unchanged. Saved session model IDs remain unchanged for safe resume.
Other models retain upstream inference.
Native sessions keep their original executor and working directory. Allow ASM-only
workspace-root, configuration, and capability metadata updates without treating them
as a native environment switch; genuine executor or directory changes still require
a fresh thread.
Keep upstream `models.json` unchanged. Bundle Claude Code entries separately in
[`external_models.json`](../codex-rs/models-manager/external_models.json).
Top-level `load_external_models_json` defaults to `true`; set it to `false` to
hide bundled external entries. Explicit `claude-code/<model>` selection remains available.
Append external entries when reading the default catalog, including remote and cached catalogs.
External entries override duplicate IDs without changing cached payloads.
Provider-owned catalogs remain authoritative and ignore this flag.
Explicit custom catalogs honor this flag and retain metadata for duplicate IDs.
Claude Code owns native model access and effort acceptance; metadata grants no access.

Native Claude and Cursor sessions discard inherited service tiers from config files and
profiles. Explicit request or session-flag tiers remain subject to native startup
validation: only Default is supported. Reject unsupported persistent and turn-only
tier updates before committing settings. OpenAI service-tier resolution is unchanged.

Keep CC entries visible independently of OpenAI login; never bypass filtering by
marking them API-supported. Preserve ordinary model defaults.
The model picker starts a fresh session after confirmation when entering/leaving this
runtime or changing its model. Keep the original history; never replay it
into the new native conversation or change saved defaults as part of this action.
Same-model/provider effort changes stay in the current conversation and leave saved
model defaults untouched. Apply the native control at the next idle turn boundary,
require confirmation, then save the new effort before submitting input. Preserve the
native process, session ID and history; never rebuild context for an effort change.
Cold resume restores saved effort before applying a requested change. Unsupported
changes fail before prompting without fallback. Native caching remains provider-owned;
preserve cache-relevant context, but do not promise an identical cache key or fixed hit
rate. Claude Code gates cache-prefix preservation on its native
`per_turn_effort_active` feature flag, which the native server can disable.
ASM child selections use catalog defaults and supported efforts when present; native
model IDs absent from the catalog remain usable without OpenAI fallback effort metadata.
When a role changes the selected external model, replace only inherited parent effort
with that model's catalog default, or leave effort unset when native metadata is absent.
Preserve explicit spawn, role and configured subagent efforts for normal validation;
never silently discard unsupported explicit choices. Same-model role selection preserves
inherited effort. OpenAI role effort behavior is unchanged.
Keep the V2 spawn tool description explicit: `claude-code/*` requires
`fork_turns="none"` and a self-contained task message, including with custom tool
descriptions or hidden catalogs. Do not rely on a failed spawn to teach this constraint.

- ASM owns thread identity, queues, user input, output display and process lifecycle.
  Claude Code owns conversation context, instructions, skills, tools, permissions,
  hooks and its internal subagents. All external runtimes share one input policy before
  native dispatch or mailbox delivery. Forward explicitly queued plaintext ASM context and
  tool outputs in labeled JSON context blocks separate from the actual user message.
  Skip encrypted injected items with a warning, preserving the actual prompt. Do not
  forward ciphertext or replay previously ignored history. Persist a projection marker
  for newly forwarded context so warm turns and cold resume retain native input accounting.
  Persist newly requested idle `!` shell output through that same labeled context projection
  for the next native turn; do not start inference or backfill older shell history.
  Do not synchronize generated ASM instructions or approval receipts. The instruction
  exceptions are root Claude communication guidance and a spawned child's selected role.
  For children, append only the selected role's own
  `developer_instructions` through `--append-system-prompt`, preserving Claude's
  default prompt. Pass the text as one argument, without shell interpolation or a
  temporary prompt file. Process arguments expose the text and OS argument-size
  limits apply; never truncate or silently fall back to a file. Never forward merged
  or inherited ASM instructions.
  Root Claude sessions append only the original Claude Code paragraph beginning
  "Assume users can't see most tool calls or thinking" to request a short update
  before tools and at material milestones. Do not add this paragraph to ASM subagents,
  internal sessions or temporary side conversations; child role prompts remain unchanged.
  Preserve native root prompt snapshots and native internal-agent behavior; this
  startup policy does not rewrite existing context or guarantee model compliance.
  Keep loaded role instructions during warm turns. On cold reload, recover the role
  through normal child configuration and supply its current text on native startup.
  Do not pin role text to saved linkage or reject changes to the same role. Native
  prompt snapshots must not retain an older role after cold reload. For managed
  Claude children, pass `--system-prompt-snapshot off` even when the current role
  has no instructions, so removing instructions also clears the old role. This
  requires Claude Code 2.1.257 or later; fail rather than silently retain a snapshot
  on an unsupported CLI. Claude rebuilds the prompt each request, which can affect
  cache reuse and retained thinking. Keep loaded role text fixed during warm turns;
  preserve native defaults for root and side sessions. Managed Claude children also
  set `FORCE_PROMPT_CACHING_5M=1` in their process environment and flag settings,
  requesting five-minute caching for the child and its native helper requests.
  Apply this on new sessions and cold resume, including children without a role;
  keep native caching-disable controls and managed policy authoritative. Do not change
  personal configuration or cache policy for root, side, Cursor, or OpenAI sessions.
  Managed Claude children also set `disableClaudeAiConnectors: true` in the same
  inline settings object to skip automatically fetched claude.ai connectors. Preserve
  configured local MCP servers, built-in coding tools, and the ASM communication bridge;
  do not disable connectors for root or side sessions or change personal settings.
  Set `syncClaudeAiSkills: false` only in the child's inline settings to skip account-synced
  skills without deleting their shared cache. Preserve personal, project, plugin and
  bundled skills, and retain root and side skill defaults.
  Keep runtime-setting and uncertain-turn recovery checks unchanged.
  Native main sessions receive no role instruction injection. Cursor children use the
  capability-gated ACP path below, not a CLI prompt flag.
- Main sessions and sides have no injected ASM communication tools. ASM-managed
  MultiAgentV2 children use one authenticated loopback JSON dispatcher, without MCP:
  Claude registers tools through a session-local native mod; Cursor calls the existing
  ASM binary's private helper through its native shell. No external JavaScript or Python
  runtime is required. Claude's native tool names still start with `mcp__asm__`;
  that naming does not imply an MCP transport. Preserve other native user integrations.
  Discovery exposes only canonical collaboration tools other than `spawn_agent`.
  `disable_direct_message` removes `send_message` and `followup_task`;
  `wait_agent` follows `wait_agent_enabled`; `interrupt_agent` and `list_agents` stay;
  `disable_native_tools` removes them all. `agent_message_board` adds the extension's
  nine discussion tools, bound to the child at thread start with its own storage gates,
  independent of `disable_native_tools`. Reuse canonical registration and handlers;
  duplicate names fail the turn. Never forward arbitrary ASM tools or create another
  agent registry. The private child directory contains an owner-only credential
  descriptor; prompts and argv carry no token. Calls are active-turn scoped, serialize
  through the existing execution gate, and are not retried automatically. Both use
  the helper's timeout covering eight queued maximum waits plus 30 seconds. Claude
  invokes it with native `$.process.spawn`: `$.http.fetch` has a 30-second deadline,
  and `$.process.run` is capped at ten minutes in Claude Code 2.1.310. Preserve native
  tool-search defaults rather than forcing every schema into each model prompt.
  Board notifications reach only a running turn: Claude folds them in at its next
  tool boundary; Cursor can read them through its CLI wait result during an active
  prompt. Neither wakes an idle child.
- Deliver messages during a busy Claude child's native run as UUID-stamped queued
  user input, never Send now or another interrupt. Native folds it in at its next tool
  boundary, so running tools finish instead of returning a rejection that tells the
  model to stop. Enable this only when `system/init` advertises `interrupt_send_now_v1`,
  which identifies the verified CLI generation whose command lifecycle reconciles
  queued input; otherwise retain turn-boundary delivery. Persist input before
  submission; reconcile every submitted command before saving clean idle. Idle
  `send_message` remains queue-only and Codex delivery stays unchanged. Redirect
  urgently with explicit `interrupt_agent`, then `followup_task`; explicit
  interruption retains its separate confirmed-idle or kill-and-dirty rules.
- Native idle is not final while `background_tasks_changed` lists live non-ambient
  tasks: their completion notification starts another native turn. Keep that work in
  the same ASM turn until a later idle without live background work, so a parent does
  not see completion before the child can wake. Notifications confer no new authority.
  Long-lived background tasks keep the turn running until they end or are interrupted.
  A plain interrupt leaves background shells running; its confirmation rules are
  unchanged. A notification result answers a turn only after that turn's own input
  succeeded, so a notification turn that ran between ASM turns cannot answer new input.
- Project native tool, skill and task activity into existing MCP display cells for
  streaming, disclosure, grouping and replay. These are display events, not tool
  execution requests or model-context items. Attribute native child activity without
  inventing ASM threads; preserve failures and avoid duplicate bridge-tool events.
  Include distinct observed native child tasks in the completed turn's `ag` count,
  deduplicated across lifecycle events and display replay. Historical child activity
  must not inflate a currently active ancestor turn's count.
- Use native Claude Code settings and subscription login; resolve `claude` through
  PATH. Do not extract subscription credentials or enable API billing implicitly.
  Provide deterministic conversational subscription login through the shared
  app-server path, using existing chat requests and responses without Desktop changes.
  Explicit `claude login` and `cursor login` select the authentication provider in any
  main chat, including GPT parents authenticating native subagents, without changing
  the chat model. Keep the provider in the pending login session rather than infer it
  from the chat model. Subagents cannot start interactive login. Login exchanges are
  display-only and must not enter the main chat's model history or persisted rollout.
  Intercept login replies before normal turn ingestion, history, hooks, or inference;
  send one-time authorization codes only to the official Claude login subprocess.
  Never include codes in model requests, backend logs, or rollouts. Client-local
  drafts and prompt recall are accepted. Expired or interrupted login exchanges stay
  fail-closed: unrecognized replies receive fixed restart instructions, never normal
  turn ingestion. Only explicit non-secret actions may exit or restart login mode.
  Retain a code-free pending-login marker across backend restart and thread resume;
  never store authorization codes or OAuth state in that marker or alter checkpoints.
  Keep login output private and errors fixed;
  verify subscription authentication rather than falling back to Console, API keys,
  or cloud providers. Login does not authorize replaying failed input or clearing an
  uncertain execution checkpoint.
  Do not disable native tools, skills or instructions to emulate ASM inference. The one
  exception: ASM thread-spawned children (V1 and V2) launch with
  `--disallowedTools AskUserQuestion`, because nobody can answer a headless child's
  questions. Main sessions keep native question behavior.
- Claude permissions govern Claude tools. ASM sandbox and approval settings do not
  govern them; display that distinction clearly. By user decision, every Claude session,
  main and children, runs in native bypass mode (`--dangerously-skip-permissions`).
  Native deny rules, hooks and managed policy still apply. Keep
  `--permission-prompts none`, so anything that still prompts fails explicitly; never
  grant protocol approval requests. Configured native hooks remain native.
  If managed policy requires an ASM-managed sandbox, refuse external execution.
- Keep one CLI process per loaded Claude session, with private external-session
  linkage and unfinished-turn state. Resume only a known clean session; never replay
  an uncertain submitted turn or completed tool calls automatically.
- On interrupt, stop native work and retain the process only after a matching
  successful interrupt acknowledgement with an explicit empty queued-input receipt,
  explicit idle, and termination of observed
  native child tasks. Track a local agent task by ID and its latest run ID: a resumed
  task keeps its ID with a new, later-sorting run ID. An older-run or run-less start or
  terminal never replaces or clears a known newer run. Persist clean linkage before allowing another turn. Missing
  confirmation, timeout, disconnect or forced abort kills the owned group and keeps
  linkage dirty. Give only external tasks a bounded cleanup grace period.
  UUID-stamp submitted native user inputs so the receipt accounts for queued work.
  Native aborted-streaming and aborted-tools results are resumable only after an
  ASM-requested interrupt, for the submitted UUID with its cancelled-command receipt
  and the confirmed cleanup above. Continue with new input; never replay the interrupted
  input or assume its tools had no side effects. A structured Claude authentication
  failure may leave the turn failed but resumable only after its submitted commands
  are reconciled, native idle is confirmed, and no tool work or active child tasks
  make the outcome uncertain. Persist the consumed input and clean linkage before
  reporting the authentication failure. Allow only new input after login; never
  resend the failed prompt automatically. Missing confirmation and other execution
  errors keep the no-replay guard and require explicit recovery. Do not clear older
  unfinished checkpoints based on their error text.
- Bound framing, stderr and cleanup waits. Kill only owned native processes; preserve
  primary failures and retain diagnostics only when requested. Record the first
  process state before cleanup separately from its final exit status; cleanup must not
  erase whether the native CLI had already exited or was still running.
  Give each native CLI a unique inherited process-ownership marker so ordinary detached tools remain identifiable
  after their CLI exits. Combine marker ownership with verified live ancestry. Never
  signal ASM’s own process group. This is lifecycle cleanup, not a sandbox: processes
  that clear the marker and leave observable ancestry can escape cleanup.
- Forward native user images as typed content blocks, preserving text/image
  order and delivery checkpoints. Accept inline image data and local attachments through
  existing image processing; keep injected ASM context plaintext-only. Native history keeps
  prepared image bytes inline, without OpenAI uploads or replay transformations. Bound
  Claude image user frames to 31 MiB with a 32 MiB echoed-frame limit; validate before
  recording input or advancing its checkpoint. Keep ordinary text/control frames at
  1 MiB. These are ASM transport bounds, not provider limits. Reject unsupported history forks,
  rollback, runtime switches and native-only operations explicitly. Suppress native
  model prewarm, title inference and System1 for external threads. Native memory
  extraction may summarize their saved ASM transcripts.
- Claude history forks only into temporary `/side`/`/btw` (and its `/sync`), whole and at
  main's model. Admission requires a loaded, idle main with no native background work and a
  clean checkpoint whose delivered input equals the forked history. The checkpoint's
  optional `fork_point` is the final top-level assistant message UUID of the last
  successful, uninterrupted turn; older checkpoints lack it, so `/side` refuses until main
  completes a new turn. The side's first turn starts a separate process with `--resume
  <main> --resume-session-at <fork_point> --fork-session --no-session-persistence
  --session-id <new>`, so later main turns are excluded. This relies on Claude's hidden
  cutoff flag, verified on 2.1.309: an unknown UUID fails closed, without fallback to
  latest. Native agent, task and worktree controls (Agent, Task, TaskStop, SendMessage,
  ListAgents, CronCreate, CronDelete, ScheduleWakeup, EnterWorktree, ExitWorktree) are
  disallowed. A short boundary is sent once, because ASM side instructions are not native
  context; it keeps Side's reference-only, no-agent and no-worktree rules. Never replay
  history or copy session files. The side checkpoint stays in memory and the side thread
  unloads as soon as it is closed, ending its process; a lost process or closed side
  cannot cold-resume. Cursor sides, saved `/fork` and historical cuts of
  native history stay rejected.
- External threads use Default collaboration mode only and never use guardian
  approval review. Mandatory managed-execution restrictions still apply.
- Native turns have no total-duration deadline; cancellation and bounded cleanup
  remain available while tools and background agents are running.
- Do not impose a separate approximate-token prompt cap or truncate instructions.
  Claude Code manages its context; transport bounds still apply.
- Explicit child role context limits, otherwise exact V2
  `subagent_model_context_windows` entries, set native `--autocompact` windows.
  Accept only 100000–1000000 tokens; reject unsupported values, never ignore them.
  Set `CLAUDE_CODE_AUTO_COMPACT_WINDOW` in the child environment and flag settings
  so inherited environment and user/project settings cannot override the explicit
  value. Native managed policy retains precedence. Unmatched children retain native
  defaults and environment settings.
  New root sessions use explicit `model_context_window` the same way, with the same
  range check; when unset, they retain native defaults and environment settings.
  Catalog metadata never sets this window.
  Bind the configured window to native linkage. Changing it requires a new child
  chat; main chats and their forks keep the window saved when the main chat started.
  This is a soft compaction window, not a hard maximum context capacity or an
  exact trigger. Native model capacity still clamps it; Claude subtracts output
  reserve and compaction headroom. Native 2.1.296 uses a 20000-token output reserve
  and 13000-token headroom, so 272000 triggers at 239000 with that output reserve.
  Preserve native automatic-compaction enable/disable controls, memory and hooks;
  the setting cannot force disabled compaction to run. Keep reported context fill
  based on native model capacity, not this configured compaction window.
- Report checked per-process usage deltas from native cumulative model counters.
  First cold-resume usage and missing/reset counters are unknown, not zero; warn
  that ASM totals and token-budget accounting are partial in those cases.
- Thinking is a reported subset of output, never an additional output charge.
  Display context fill from the latest main request input, not cumulative billing
  or child requests. Use its native model window; hide unknown context metrics.
  Cache rate is the cached share of that latest main request.
- Require native stream-json result followed by session-idle completion; CLI 2.1.296
  is the verified protocol target. Queued delivery and background continuation were
  verified with 2.1.309. Revalidate compatibility when upgrading it.
- Keep opt-in until real Claude-main and Sol-parent/Claude-child sessions pass with
  the intended installed CLI. Never replace installed binaries or global profiles
  as part of testing. Cross-runtime context parity is not a requirement.

### Cursor Agent thread runtime

Explicit `cursor-agent/<model>` IDs select the official Cursor CLI over ACP (`agent acp`,
JSON-RPC on stdio) for main threads and ASM-managed children. It extends the Claude Code
runtime above: the same rules apply except where listed here, including plaintext context
blocks and warning/skip behavior for encrypted input. Keep this policy shared; only the
transport and supported delivery timing differ. Do not add a separate runtime
copy, backend hierarchy or event vocabulary; branch only where the protocols differ.

```bash
codex --no-daemon --model 'cursor-agent/gpt-5.6-luna'
```

- Bundle Cursor model and effort metadata in `external_models.json`, like Claude Code.
  Catalog loading and startup must not launch or authenticate either native CLI.
  Negotiate ACP `parameterizedModelPicker` only when starting a Cursor session. Validate
  the requested model and effort against that session's advertised choices, apply the
  actual parameter ID from the `thought_level` category, and require confirmation before
  prompting. Preserve saved automatic model IDs: prefer an exact advertised match, then
  translate `auto` or `auto-smart` to ACP `default` only when that value is advertised.
  This is Auto identity translation, not fallback to another model. Other unavailable
  choices fail without fallback. Saved bracket-encoded model IDs
  retain their native parameters; an explicit effort replaces that parameter. Custom
  catalogs remain authoritative; bundled metadata grants no native model access.
- Forward user images through ACP `session/prompt` image blocks only when the agent
  advertises `promptCapabilities.image`. Reject unsupported image input before recording
  history or advancing the native checkpoint; never drop images or replace them with paths.
  Preserve inline image history. Bound ACP image prompts to 31 MiB and image replay
  frames to 32 MiB; ordinary text/control frames retain the 1 MiB bound.
- The concrete runtime is part of identity. Claude and Cursor threads never resume, fork or
  switch into each other; the checkpoint keeps the full model ID. Cursor reuses the Claude
  checkpoint path and format, storing the session ID that Cursor assigns.
- Keep conversational login orchestration and process safety in provider-neutral `native_login`;
  separate Claude and Cursor adapters own their CLI, environment, URL, reply and status policies.
  Preserve the legacy `claude-runtime` pending-marker path for interrupted exchange compatibility.
- Support `cursor login` through the same deterministic backend chat interception as Claude,
  using official `agent login` with browser opening disabled and an allowlisted sign-in URL.
  Browser authentication stays with the native CLI; accept only `done` to check completion,
  never keys or tokens. Keep pending replies fail-closed across expiry and backend restart,
  and never replay the failed prompt or bypass dirty-session safeguards.
- Use Cursor's native authenticated ACP bootstrap and detect typed auth-required errors
  (`error.code = -32000`) during setup and turns. Route them into the shared login-required
  guard and fixed provider-specific instructions; never classify arbitrary assistant prose
  or generic provider errors as expired authentication. Do not invoke ACP `authenticate`,
  which can open its own browser flow outside shared interception. Preserve dirty checkpoints
  on uncertain prompt failures. Installed Cursor versions that swallow established-turn auth
  failures into ordinary assistant text cannot provide reliable automatic expiry detection;
  those sessions still require explicit `cursor login` until the native protocol exposes it.
- Remove `CURSOR_API_KEY` and `CURSOR_AUTH_TOKEN` from the child. By user
  decision, every Cursor session, main and children, launches as `agent --force acp`
  (native run-everything mode). Explicit Cursor denies and team auto-run controls still
  apply; never pass `--approve-mcps`, `--trust` or `--sandbox`. Cursor ACP has no supported
  control to remove its question tool, so children keep it; questions are refused, not
  disabled. Answer remaining approval requests with a reject option,
  plans with `rejected`, questions with `skipped`, and other agent requests with an error, so
  that a turn cannot hang or fall back to a local plan. Show each refusal.
- Cursor saves session model selections in its config directory. Give each thread a private
  `CURSOR_CONFIG_DIR` beside its checkpoint. Refresh it on every spawn with private copies of
  the user's `cli-config.json` and `permissions.json`, using Cursor's directory precedence, so
  that native permissions still apply and the user's default model never changes. Login,
  rules, skills, hooks and MCP settings stay in their native locations; never copy or extract
  credentials.
- Negotiate ACP subagents (`clientCapabilities._meta.subagents`); require the agent to
  advertise `sessionCapabilities.subagents`. Cursor then publishes native child sessions and
  makes the prompt wait for background children, so their completion drives the parent
  follow-up within the same ASM turn. Accept updates only for the session and announced
  children. Show child tools and text in a child-scoped activity cell, not the main
  transcript; children stay native, with no ASM threads. A prompt response stays clean only
  after every announced child reported `completed`, `failed` or `cancelled`. Cursor also
  answers after its cancel wait times out and marks children `disconnected`; that does not
  prove a stop, so the turn fails and the linkage stays dirty.
- Cursor reports finished tools as `completed`. Mark a tool failed from typed top-level
  `rawOutput` fields only: nonzero `exitCode`, a string `error`, `rejected: true` or
  `permissionDenied: true`. A missing field means unknown; a failed tool does not fail the
  turn.
- Forward only selected child-role instructions through ACP `systemPrompt` with
  `mode = "append"` on both `session/new` and `session/load`. Require an advertised
  `sessionCapabilities.systemPrompt` object before authentication or session creation/loading.
  Unsupported Cursor versions fail explicitly; never fall back to user-message prefixes,
  replacement prompts, or shared rule files. Supply current loaded role text on
  cold reload, without comparing it to saved role text. Omit the field entirely when
  no role text applies.
- Child role context limits and `subagent_model_context_windows` set no Cursor compaction
  window and skip the Claude window validation; Cursor manages its own context.
- Bound each login and session setup request (60 s), including stdin submission; Cursor
  can stall before reading a request. Native turns stay unbounded.
- `session/load` replays native history before it responds; discard that replay so the ASM
  transcript does not get duplicates. Set the model again after every new or loaded session.
- Cursor starts shell tools in their own process groups below the ACP process. Use the
  shared native ownership-marker cleanup, including after the ACP leader exits. Never kill
  by socket, path or name. Cleanup succeeds only after identified owned processes stop;
  marker-clearing processes outside observable ancestry remain outside its guarantee.
  On interrupt, send `session/cancel`. After `stopReason: cancelled`, stop the process tree and keep the linkage
  clean only when descendant cleanup succeeded; the next turn loads the session. A missing
  acknowledgement, failed cleanup or any other failure leaves the linkage dirty.
- Cursor reports no token usage or context fill. Show them as unavailable, never zero, in ASM
  usage accounting and `/status`. Known gap: `codex exec --json` `turn.completed.usage` has
  required numeric fields and reports zeros when usage is unknown; the session warning says
  usage is unavailable. Keep that wire shape until the owner approves an API change.
- Cursor children deliberately use the local CLI bridge, not ACP MCP servers:
  `session/new` and `session/load` carry empty `mcpServers`. Team MCP policy blocked the
  previous transport; do not reintroduce it or bypass native policy. Cursor mail
  and board notices can be delivered inside an active prompt through the canonical
  wait tool's CLI result. The owning runtime records mail and advances its dirty checkpoint
  before returning it, so the next prompt does not replay it. Each nonempty delivery gets
  a unique receipt. Save clean linkage only after Cursor reports that receipt in a
  successful root shell tool result; internal bridge receipt or HTTP completion alone
  is insufficient. Missing or truncated confirmation requires explicit recovery, without
  retrying potentially side-effecting tools. Queued user input and followups with
  next-turn options stay ordered for the next prompt. ASM still returns
  Cursor's final answer; there is no Send now. Cursor children saved before this bridge recorded no messaging; their
  cold resume fails closed and needs a new thread. Mains are unaffected.
- Verified with Cursor CLI 2026.10.22-27dcf5f. Revalidate the protocol when upgrading it.

### Minimal diagnostics

- Default `diagnostic_log_capture = false`. This startup policy controls Rust stderr/file logs, persistent diagnostics, feedback ring/continuous tags, optional TUI replay, and rollout-trace recorders. Disabled capture cannot be enabled by `RUST_LOG`, `log_dir`, or recorder environment variables.
- Opt in with `diagnostic_log_capture = true`; restart affected processes. Existing filters then apply. Explicit `log_dir` also routes SQLite logs unless managed `sqlite_home` overrides it. Propagate policy to code-mode helper; inherited helper settings cannot override it. Standalone helpers without user config need explicit opt-in.
- When disabled, do not allocate feedback capture, install its tracing layers, open/migrate diagnostic databases, or run insertion/pruning/reclamation workers. Ordinary startup leaves historical files unchanged.
- Explicit history inspection/feedback export may read old logs without capture. Thread deletion removes historical diagnostic records through existing-only on-demand access; never create missing files. Preserve feedback submission/upload errors; never invent evidence.
- Preserve direct errors/warnings, exit status, protocol events, saved conversation/tool history, subagent relationships, memory, and operational state. Detached-daemon startup/fatal stderr and Windows sandbox security-audit logs remain separate. Do not suppress them or weaken security. Keep product analytics/account usage unchanged.
- Disable the built-in Statsig metrics exporter, including optimized releases/account reloads. Keep its config spelling; default `otel.metrics_exporter` to `none`. Preserve explicit OTLP collectors. Opt-in `runtime_metrics` uses a manual reader without timer/network exporter when none is configured. No unsolicited session-storage size telemetry scan. Check fork-disabled paths with focused real-binary scenarios under the validation rules, not broad upstream retesting.

### System1

Task and steering ingestion precedes skipped checks and invalidates stale decisions; bounded evidence rendering follows cadence, trigger, and recovery gates. Rendering keeps the original deadline and must finish before client setup.

The canonical feature name is System1, configured with `[system1]` and toggled with `/system1`. Accept the former `[adaptive_reasoning]` table when reading existing configuration, including deny-only roles, and former rollout fields when resuming saved sessions. Emit only canonical names. Preserve layer precedence across old and new spellings, but reject both tables in one layer. Before writing canonical settings into a file that still uses the old table, rename that table; reject conflicting writes without changing the file.

Use opt-in `[system1]` in the shared backend, not the TUI. Require `features.step_model_switching` and `features.reasoning_effort_override`.

| Setting | Default | Validation |
| --- | --- | --- |
| `enabled` | `false` | Opt-in only |
| `profile` | Absent: legacy OpenAI configuration | User-defined name in `profiles`; cannot mix with legacy model/key settings |
| `profiles.<name>` | Empty map | Required `protocol` (`openai` or `typesafe`), nonblank `model`, ASCII identifier `api_key_env`; optional full `endpoint` URL |
| `decision_model` | `"gpt-6-luna"` | Nonblank |
| `decision_model_api_env` | `"DECISION_API_KEY"` | ASCII environment-variable identifier, not a secret |
| `min_effort` / `max_effort` | `"low"` / `"max"` | Ordinary levels; `min_effort <= max_effort` |
| `decision_timeout_ms` | `200` | 1–5000 ms |
| `max_context_bytes` | `8192` | 128–32768 UTF-8 bytes across all Decisions input items |
| `update_on` | `["turn_start", "tool_result"]` | Only these trigger types |
| `decision_every_n_turns` | `1` | Positive integer; adapt turns 1, 1 + N, 1 + 2N, ... |
| `decision_every_n_steps` | `1` | Positive integer; next due step is the last accepted decision step + N |
| `low_effort_description` / `medium_effort_description` / `high_effort_description` / `xhigh_effort_description` / `max_effort_description` | Built-in descriptions below | Nonblank; each at most 4096 UTF-8 bytes |
| `rubric_instructions` | Built-in rubric when absent | Nonblank; at most 4096 UTF-8 bytes |

Failure warnings use yellow chat text with the error category, current fallback effort, and runtime cooldown duration (or recovery blocked). Show a warning when the error category changes within a failure episode; keep identical errors quiet. Old/restored events without recovery details must not invent a cooldown. Transport failures include bounded send/body-read codes, using typed HTTP classifications where available; never expose raw error messages, URLs, or credentials. Legacy transport events remain readable.

Generate main-step selection guidance from the model default, not fixed effort labels: the default is the normal-work reference; a `max` default uses the preceding catalog-supported ordinary effort to reserve `max` for exceptional difficulty. Compute the reference before applying configured bounds. The existing low/medium/high/xhigh description fields are shared difficulty templates for one-below-reference/at-reference/one-above-reference/further-above-reference choices; efforts further below the reference use fixed mechanical-execution guidance; `max_effort_description` always describes max. None/minimal retain direct-execution guidance. Without a supported model default, retain effort-labeled guidance. Never alter the task baseline to compute the reference. Model meaning stays catalog-owned.

Choice descriptions are separate from the selection/safety rubric. Main decisions send only allowed choices; warmup retains its synthetic low/medium pair. Replacing the rubric must not remove or duplicate their descriptions. `none` and `minimal` retain fixed descriptions; `max_effort_description` configures max guidance with its previous fixed text as the default.

```toml
[system1]
low_effort_description = "Straightforward coding, focused searches, and bounded analysis."
medium_effort_description = "Ordinary substantive work, including moderately complex coding."
high_effort_description = "Difficult debugging, multiple interacting constraints, conflicting evidence, or substantial uncertainty. Use max only for exceptionally difficult problems."
xhigh_effort_description = "Extensive reasoning for exceptionally difficult problems, deep investigations, and demanding long-horizon planning."
max_effort_description = "Exceptionally difficult problems requiring the maximum supported reasoning budget."
```

Profiles name connection settings, not separate adaptation policies. Keep effort bounds,
cadence, evidence limits, timeout, and rubric shared. Profile names have no built-in
meaning. Require explicit protocol, model, and credential-variable name in each
profile; do not infer a protocol from its name, endpoint, or credentials. Keep
`backend` as a compatibility alias for `protocol`; specifying both is an error.
Validate all defined profiles and reject unknown selections/protocols rather than falling back. Without
`profile`, preserve legacy OpenAI defaults and explicit overrides. Selecting a
profile together with either legacy `decision_model` or `decision_model_api_env`
is an error, even if the legacy value matches its default.

```toml
[system1]
enabled = true
profile = "jev"

[system1.profiles.jev]
protocol = "typesafe"
model = "jev-latest"
api_key_env = "TYPESAFE_API_KEY"

[system1.profiles.decision_api]
protocol = "openai"
model = "gpt-6-luna"
api_key_env = "DECISION_API_KEY"

[system1.profiles.local_gateway]
protocol = "openai"
endpoint = "http://127.0.0.1:8080/v1/decisions"
model = "my-decision-model"
api_key_env = "LOCAL_DECISION_KEY"
```

Resolve the connection profile once on first use in each session, including failed
credential attempts. Keep its protocol/endpoint/model together across warm-up and decisions;
configuration reloads do not switch an active session's profile. Both adapters use
the same cancellation, deadline, allowed-choice validation, and recovery policy.
No automatic failover or adapter-internal retries. Without `endpoint`, use the
official HTTPS endpoint for the selected protocol. An override is the full endpoint
URL; append no path. It selects the destination, not the request/response format.
Require HTTPS, except HTTP on loopback for local gateways. Reject URL credentials
and fragments. Blank endpoints fail config validation; other invalid URLs fail
client construction without exposing the URL in errors.

The selected endpoint receives bounded conversation evidence and the credential
from `api_key_env`. A custom endpoint does not necessarily belong to OpenAI or
TypeSafe, even when it uses their protocol. Choose the destination deliberately;
keep credentials separate from main-model authentication and diagnostics sanitized.

- Adapt ordinary root conversations and spawned subagents. Keep internal workers unchanged. Use the selected decision backend to select supported effort within bounds. Keep Ultra/Persistent selections, models, service tiers, execution modes, permissions, and saved defaults unchanged.
- If fewer than two efforts remain, skip Decisions and warm-up; keep current effort.
- Read credentials only from the selected profile's `api_key_env`, or the legacy named backend environment variable (default `DECISION_API_KEY`). Do not fall back to `OPENAI_API_KEY`; explicit variable-name overrides remain supported. Keep main-model authentication separate. Do not provision credentials or log keys or evidence. Do not route Decisions requests through Guardian.
- If set, use `rubric_instructions` instead of the built-in rubric, preserving its text. Prefix both backends’ decision instructions with trusted configured reasoning baseline context, separate from evidence: report separate `model_default_effort`, `task_baseline_effort`, and `current_effort` fields; use the turn’s configured effort as the task baseline, refreshed on accepted manual effort/model updates but never System1 selections; report unspecified for absent values. Consider the model default and task baseline together whenever available: the model default is its normal operating point, while the task baseline is task-difficulty guidance. Neither is a floor or ceiling; weigh both alongside concrete next-step evidence. Current effort is state, not independent difficulty evidence. For every allowed choice, send both labeled sections: `Model meaning` from the trimmed catalog effort description, bounded to 1024 UTF-8 bytes (`unavailable` for missing, blank, or fallback metadata), and `Selection guidance` from the existing System1 effort description, preserving its full configured text. The combined choice description is bounded by 8192 UTF-8 bytes at the API layer; configuration guidance remains capped at 4096 bytes. Do not infer baseline provenance. Include the next step’s canonical model name and catalog description as JSON-quoted metadata alongside baseline context in both backends. Bound descriptions to 1024 UTF-8 bytes at a character boundary; use `unavailable` for missing, blank, or fallback metadata. Keep the decision-service model and warmup unchanged. For the same reasoning demands, the built-in rubric favors lower effort for more capable models and higher effort for less capable models. It treats baseline as guidance rather than a ceiling and reassesses the next step. Enforce choices and runtime limits in code. Keep instructions and results outside the main conversation.
- Collect genuine user, assistant, and tool evidence, newest first. Exclude injected user instructions, compaction output, images, and encrypted reasoning. Scale scan limits with the byte budget: at least 128 history/input items and 32 content parts, up to 512 items and 128 parts at 32768 bytes. These scan limits and the byte budget can still omit older history; they do not promise a complete transcript.
- **P0 manual review:** combined effort-choice descriptions can exceed 1000 tokens; manually review both sections and bounds (1024-byte meaning plus at most 4096-byte guidance and fixed labels, below the 8192-byte API cap). Bound total evidence and rubric separately using the table limits. Split larger evidence into ordered OpenAI input messages or TypeSafe state-array strings of at most 8192 UTF-8 bytes each, preferring line boundaries. TypeSafe keeps a string state when one chunk suffices. Preserve text and the default single-message request. Configured rubric instructions remain at most 4096 bytes; the bounded description and baseline prefix is additional. Each item can exceed 1000 tokens, but its byte cap keeps it below the 10K-token item limit. Keep this evidence outside the main conversation.
- At eligible session start, send one tiny synthetic warm-up in the background. Without triggers or supported effort within bounds, skip it. Reuse its pooled Decisions client. Keep separate minimal instructions. Discard its answer. Keep history and settings unchanged.
- Limit warm-up to three seconds. At shutdown, abort and join it. Do not retry, ping periodically, or add warm-up settings. Warm-up can add one billable request. It does not guarantee a warm backend or connection.
- Before each cadence-selected main step with an enabled trigger, collect current evidence and await Decisions and validation in the turn task, without a separate step job or completion channel. Then capture immutable step settings and run the model. Do not retry or wait for a busy settings-publication permit. Keep only warm-up and log writing in the background.
- Publish a current result within its original deadline through live next-step settings. Keep captured requests unchanged. Later evidence invalidates pending decisions, not already applied settings.
- After a current main-step decision failure, restore the latest trusted configured/manual baseline for the next request and during cooldown or blocked recovery. Use the model default when the baseline is unset. Never replace the baseline with an adaptive selection. Preserve current effort for unsupported models/efforts, Ultra/Persistent, cancellation, or warmup-only failures. Role bounds restrict adaptive choices, not the configured/manual fallback. A Decisions failure must not stop the main step.
- On accepted user, assistant, or tool evidence, invalidate old decisions. Reject results after cancellation, task replacement, shutdown, settings changes, or deadline expiry.
- Reuse live step-settings validation and publication. Preserve the original request-effort pin, trusted append-only updates, reconnect behavior, and explicit compaction handling.
- In the shared history-to-request projection, exclude effort updates without harness provenance and keep the last complete envelope in each adjacent update run after filtering. Preserve retained messages, reasoning, tool declarations, and tool results as separators. Do not change stored history or the request-effort pin. This repairs filtered forks and replayed checkpoints without a separate transport workaround. A changed projected tail can lose prefix/cache reuse; let existing incremental-request checks choose full input rather than force a session reset.
- `/system1 status` reads the current thread's mode, selected and last captured effort, and reported health without changing settings or sending a model turn. Show decision backend/protocol, model, named profile or legacy configuration, and client initialization from the session-fixed resolved connection. Capture the profile name with its resolved settings; never present reloaded configuration as the connection used by an existing client. Before resolution report that identity is not resolved; initialization is not proof of a successful provider request. Do not resolve a profile, inspect credentials, or start warmup for status. It also queries fixed-size in-memory decision totals for the currently loaded backend runtime. Exclude warmup, cadence/backoff skips, and incomplete tasks; accepted includes unchanged decisions, effort changes include only applied changes. Count local client setup failures separately. Show total/last completed-decision wall time, not estimated net overhead, cost, or savings. Totals reset on runtime unload/restart; children have separate totals. Do not scan rollouts, persist totals, add per-step notifications, expose credentials/endpoints, or resume unloaded threads for this read. Older servers show stats unavailable.
- Manual model and effort changes preserve adaptation’s on/off state and invalidate pending decisions. Effort changes refresh the baseline for subsequent decisions. Unsupported models skip adaptation without disabling it. Persistent thread settings refresh the active turn’s baseline without changing already captured requests or the existing live-step settings contract. Log mode transitions. Ordinary turn-start and spawn settings do not turn it off. Use `/system1` to toggle it for the current thread, without changing saved defaults. Keep runtime state separate from captured step settings.
- Report runtime mode and its revision at thread attachment and on each transition. Ignore older revisions so delayed notifications cannot restore old mode. Show `<effort>·auto` when adaptation is on without an outstanding error, including startup and the wait before a decision. Show `<effort>·err` after a Decisions error; keep adaptation on and follow the main-step failure fallback rule above. When off, show only effort. Use the baseline before the first captured step. The marker reports runtime health, not a promised effort change. Unsupported steps keep their effort without a Decisions call. Before each eligible main request, report captured effort through `turn/reasoningEffortUpdated`, including fallback steps. Keep mode and captured effort separate; toggles do not change already-running steps.
- A spawned subagent copies its parent’s runtime mode once at spawn. Later parent toggles do not change that child. Preserve the child’s own mode across live eviction and reload, not backend restarts. Do not apply this rule to internal workers. If the assigned task is encrypted and no readable task is available, log the fallback and keep current effort; never forward ciphertext or guess the task.
- Retain captured effort and turn identity in live TUI thread snapshots, outside saved history. Restore them after switching threads without accepting stale history settings. Clear them for a new turn or manual settings change.
- Deliver transient effort notifications directly to clients, without entering rollout persistence before the main request.
- Clients must use this backend. The feature does not replace Desktop's backend. Use HTTP for Decisions, not the Responses WebSocket.
- Report errors through backend runtime state, independent of diagnostics. Include health at attachment and after live child reload. New children inherit mode, not parent errors. Deduplicate visible warnings by failure episode. Clear errors only after a current main-step decision passes acceptance guards, including unchanged effort. Warm-up errors count for health, but never arm the main-step effort fallback. Warm-up timeouts do not change recovery failures, cooldown, or blocked state. Other warm-up errors retain normal recovery behavior. Warm-up success, cancellation, and supersession do not clear errors. Identify warm-up timeouts explicitly with their deadline in the visible warning; record backend, model, elapsed time, and remaining deadline at request start in diagnostic tracing, without message content or endpoints.
- With `diagnostic_log_capture = true`, append sanitized attempt metadata to a separate per-session `decision.jsonl`. Include credential failures, API outcome, and acceptance outcome. Do not log evidence, rubric text, bodies, or authentication headers. With capture off, create no log file or writer. Bound buffering and storage; never wait for disk on the decision path. Report dropped records or writer failures separately. Crash-interrupted attempts can lack an end record.
- Report decision latency and fallbacks without sensitive input. The deadline bounds result eligibility and local publication, not API latency. Preserved prefixes do not guarantee cache hits. Verify live update order and backend acceptance with real-binary scenarios under the isolation and cost limits before making claims.

#### Space decision calls by turn and step

A turn is one request and its resulting work. A step is one ordinary model request
within that turn, not an individual tool call. Defaults select every turn and every
eligible step. With eligible steps and no failures, decide before steps 1, 2, 3, ... .
For nondefault two-step spacing, set `decision_every_n_steps = 2`; decisions then
precede steps 1, 3, 5, ... .

```toml
[system1]
enabled = true
decision_every_n_turns = 1
decision_every_n_steps = 1
```

- Turn selection is unchanged: every N turns selects turns 1, 1 + N, 1 + 2N, ... .
  Reset step numbering and successful-decision spacing at each new turn. With
  every 2 turns / 3 steps, turns 1, 3, ... are selected; acceptance at step 1
  makes step 4 due, and acceptance at step 4 makes step 7 due.
- In a selected turn, the first eligible step is due. After a **current accepted**
  decision at step S,
  the next due step is S + N, including when the accepted effort is unchanged.
  Due steps must still pass `update_on` and eligibility checks. Ineligible, failed,
  cancelled, or superseded decisions never advance spacing; try at a later eligible
  step when recovery permits. Collect fresh bounded evidence then.
- Keep current effective effort between decisions, except for the failure fallback
  above. Skipped turns do not force a new effort without a main-step failure.
- Each agent owns recovery state across turns. A transient failure starts a
  one-second cooldown; consecutive failures double it, capped at 30 seconds.
  After expiry, probe on the next due eligible step. Never sleep the main turn or
  schedule a background recovery timer. Cooldown skips do not extend cooldown.
- Missing/invalid credentials, invalid requests, rejected settings, and HTTP 4xx
  errors other than 408/429 block Decisions until
  adaptation is explicitly turned off and on. This resets recovery and makes the
  next eligible step due, even before a normally skipped turn. It does not reload
  a successfully initialized client's cached credentials; changing those requires
  a backend restart. Missing credentials are not cached: set them in the backend
  environment and retry off/on. An HTTP credential failure retried after off/on
  uses the same backend client.
- Only a current accepted main-step decision clears error health; it also resets
  recovery. Explicit off/on resets recovery, not error health. Warm-up errors
  affect health; timeouts leave recovery unchanged, while other errors affect recovery.
  Warm-up success never clears errors
  or anchors spacing. Cancellation and supersession do not clear errors either.

Example with defaults, all shown steps eligible, and times measured from session start:

| Turn / step | Time | Decisions behavior |
| --- | --- | --- |
| 1 / 1 | 0.0s | Accepted; next due step is 2 |
| 1 / 2 | 0.2s | Accepted unchanged effort; next due step is 3 |
| 1 / 3 | 0.4s | Transient failure; cooldown until 1.4s; step remains due |
| 1 / 4 | 0.8s | Use the baseline during cooldown |
| 2 / 1 | 1.0s | New turn resets spacing, not cooldown; use its baseline |
| 2 / 2 | 1.5s | Probe fails; cooldown doubles to 2s, until 3.5s |
| 2 / 3 | 3.6s | Accepted unchanged effort; clear error/recovery; next due step is 4 |
| 2 / 4 | 3.8s | Accepted; next due step is 5 |
| 2 / 5 | 4.0s | Due again |

- Children inherit cadence configuration, not parent counters or recovery state.
- Count an ordinary turn when it starts, before setup. Setup failure or cancellation
  still counts; continuing that turn does not count again. Warm-up, compaction, and
  transport retries do not count as ordinary steps or separate ordinary turns.
- Counters and recovery are runtime-only. Resume, backend restart, or session
  eviction starts turn numbering at 1; do not rewrite conversation history.
- Larger intervals reduce decision frequency, not necessarily total runtime. They
  can retain unsuitable effort between steps; do not claim speedups without measurement.

### CLI resume matching

`asm resume <text>` accepts a trimmed, lowercase-normalized substring of a displayed
session name (or preview for unnamed sessions), or a UUID prefix. Search active
sessions under the existing provider/source eligibility rules, across working
directories. Resume a unique match directly; open the existing resume picker
restricted to the matching IDs when several match. A full UUID retains direct
lookup. Do not give exact names priority over other partial matches. Preserve
exact matching and duplicate rejection for fork, archive/delete, and queue commands.

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

### Content-provenance wire compatibility

Retain upstream per-content provenance in local history, prompt assembly, and compaction. Until deployed endpoint support is established, omit only `content_item_metadata` from the shared ordinary Responses HTTP/WebSocket request copy, including Responses Lite. Preserve other internal metadata and the original prompt. Responses Lite selection alone is not a capability guarantee. This temporary compatibility constraint withholds server-side per-part attribution; it does not enable full provenance on the wire.

### Standalone request audit

- Keep `debug prompt-input`. `debug prompt-request` exports versioned JSON: effective base instructions, production request, ordered input, full advertised tool definitions, Responses Lite prefixes, optional output schema.
- Reuse app-server extension installer/request builder. No audit work in ordinary inference paths.
- Default ephemeral. Session records/persistent state-backed tools require `--allow-session-state`; setup may opt in within isolated temporary homes. Never dispatch queued input.
- Label fresh standalone debug turn, not live-session/exact transport capture. Report omitted history, hooks, input-triggered skill/plugin injections, uncaptured tools, auth-dependent metadata, and server additions.
- Never send inference or claim startup lacks network/state effects. Exports can contain sensitive instructions/tool content.

### Model catalog ownership

- [`codex-rs/models-manager/models.json`](../codex-rs/models-manager/models.json) is upstream-owned. Accept upstream sync changes, not personal/fork edits; never freeze it.
- Personal edits belong only in `/home/trungnt13/codes/tngo-workflow/configs/model-catalogs/models.json`. Never copy them into the built-in catalog.

### Role-scoped skill visibility

Optional `allowed_agent_roles` on a `[[skills.config]]` entry limits its normal catalog visibility and skill selection to the listed child roles:

```toml
[[skills.config]]
name = "review"
enabled = true
allowed_agent_roles = ["reviewer"]

[[skills.config]]
name = "oracle"
enabled = true
allowed_agent_roles = ["reviewer"]
```

- Match the current child's explicit role, never an ancestor's role or the displayed TUI view. Omission keeps existing visibility; an empty list hides the matching skills from every role.
- Root, ordinary Side, saved user forks, and unnamed/nonmatching children do not match. Explicit reviewer children match with fresh or V2 full-history context; resume restores their own role. Preserve V1 full-history role restrictions.
- Keep parent enablement and permission boundaries. Roles remain deny-only for skill overrides; a matching role cannot re-enable an otherwise disabled skill. Scope restrictions also survive enables using another matching selector.
- Share local snapshots for identical effective visibility, without task/thread/raw-role cache keys. Respect scope in both config-aware and cwd cache paths. Keep prompt lists stable and use existing append-only availability updates and context budgets.
- This is visibility, not access isolation. Do not change filesystem, script execution, sandbox, network, or approval permissions. Inherited history can retain old lists or previously read skill content; never scrub history to conceal it.
- Configure the two skills only after using a supporting binary. Keep reviewer role descriptions generic so root's role list does not disclose their skill inventory.

### Role configuration restrictions

Role TOML files may disable web search with `web_search = "disabled"` and System1 with `[system1] enabled = false`. Only explicit disabling is
applied; omitted fields retain inheritance, including the parent's live System1 toggle.
Role settings cannot enable either capability against parent restrictions. System1
restrictions survive child spawn and reload. Roles may override `min_effort` and
`max_effort` independently in `[system1]`; omitted bounds retain inherited values.
Validate the merged range with the ordinary-effort rules. Intersect it with the
current model's supported efforts; fewer than two choices skips Decisions.
Bounds follow the applied role through model changes and child reload. New
children resolve their own role without inheriting ancestor role bounds; V1
full-history forks retain their role bounds. Other System1 settings remain inherited.
This is a decision-choice range, not a clamp on manual effort or fallback requests.
After a supporting release is published and installed, set the personal reviewer
to `min_effort = "high"` and `max_effort = "xhigh"`; do not change it before then.
Keep the projected role layer aligned with runtime settings. This does not add support
for arbitrary role configuration fields or change session authority.

### Model catalog instruction visibility

Top-level `include_model_catalog_in_spawn_agent_list`, default `true`:

- False removes generated model/effort/tier lists from V1/V2 spawn descriptions and `<model_catalog>`, plus guidance pointing there.
- Preserve custom descriptions, arguments, loading, validation, and history. Use normal invalidation when retained lists stop applying.
- Inclusion is independent of `model_catalog_in_context` placement and `expose_spawn_agent_model_overrides` argument exposure.

Compact external-model alias discovery follows `load_external_models_json`:

- When true (the default), include a compact exact-alias list in V1/V2 spawn descriptions, independently of the full catalog visibility setting and context placement. When false, omit this list.
- List only picker-visible, backend-compatible catalog entries with bundled aliases: native runtimes or configured API providers permitted by managed requirements. Discovery does not prove runtime, credentials, or service availability.
- Keep the list hidden when V2 model override arguments are disabled. Include cross-provider fresh-history guidance: V1 `fork_context=false`, V2 `fork_turns="none"`.
- Do not change personal configuration automatically. Unknown-model errors identify truncated model previews as partial.

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

### Saved forks and upstream sides

- Stack-budget exception to async instrumentation: leave `start_companion_conversation` and `select_agent_thread_and_discard_side` uninstrumented. Their tracing wrappers duplicate large futures in unoptimized poll frames and exhaust the unchanged 16 MiB UI stack. Keep companion event-dispatch futures boxed; do not increase stack limits. Construct startup and run-loop event futures through a separate non-inlined closure boundary at the existing allocation sites, so their construction temporaries do not remain in the run-loop poll frame. Do not add a nested selection allocation as a substitute.

- Saved/temporary root forks keep the parent's effective prompt-cache routing key independently of thread/session ID. Persist for resume/further forks. Old rollouts need no ancestor lookups. Keep upstream guardian/subagent routing. Shared routing does not guarantee cache hits.
- Optional top-level `side_model`, `side_reasoning_effort`, and `side_service_tier` (for example, `"fast"` or `"default"`) independently override parent settings for temporary `/side` and `/btw` forks, including `/sync`. Unset values inherit current parent settings. Do not apply them to saved forks. Preserve native-runtime fork admission restrictions, including Claude’s same-model requirement.
- Optional top-level `side_prompt` adds default developer instructions to temporary `/side` and `/btw` conversations, including `/sync`, and recognized Desktop side chats. For example: `side_prompt = "Act as a critical reviewer. Keep answers concise."` Unset or whitespace-only values preserve existing behavior. Preserve nonblank text and append it before the built-in side policy, without duplicating it on `/sync`. Forks also append the custom developer message once after their reference-only boundary because inherited context skips initial developer instructions; keep the configured prompt for compaction. Do not apply it to main or ordinary saved forks. It does not replace side boundaries, permissions, or capability restrictions.
- Remote Desktop side chats also use these settings as initial defaults, including attached forks and new side threads. Recognize only the complete shipped Desktop 26.1027.51404 developer block; unknown wording leaves ordinary behavior unchanged. Recognize the complete shipped side boundary at the start of its user message, either alone or followed by a paragraph separator. Treat appended reference paragraphs as opaque context, not a protocol to parse; changed boundary wording remains unrecognized. Do not change the signed Desktop bundle or API shapes. Synchronize effective settings after client context injection; correct only the first new side thread turn's known inherited settings, including its inherited null effort/tier. Explicit picker settings win per field, persist normally, and are never replaced on later turns or resume. Keep ordinary saved forks and prediction forks unchanged.
- `/side` and `/btw` retain upstream temporary lifecycle: restricted commands, reference-only instructions, no subagents/saved pairing. Ctrl+/ switches; Ctrl+C returns/discards. Ordinary picker navigation discards sides too. Preserve helper names/constants/tests where practical; avoid needless upstream rewrites.
- Side has no extra question-only, non-mutating inspection, or explicit-mutation/escalation instruction gate; normal permissions and approvals still apply. Preserve reference-only inherited history, waiting for a new request, the subagent prohibition, and the ephemeral/worktree attachment restrictions in both boundary and developer instructions.
- In temporary Side, `/sendlast [extra message]` forwards raw last completed assistant reply plus optional text as main user input. No prefix/navigation/settings change; atomically start or steer main. Reject complete items above 10,000 UTF-8 bytes, never truncate. Explicit item can exceed 1K tokens; require manual review when changed.
- `/sync` replaces idle temporary Side with main's latest context, retaining model/effort/tier. Discard old history only after replacement setup/selection succeeds. Keep Side pairing/boundaries. Context refresh does not guarantee cache hits.
- In idle temporary Side, `/fork [name]` saves an independent ordinary conversation:
  - No navigation, pairing changes, or automatic work. Copy retained inherited replay/temporary history. After paginated main compacts, do not recover older raw entries outside model context.
  - Persist current model/effort/tier through resume. Append durable ordinary-role context; never rewrite history.
  - Default `<main title> (fork)` or `Untitled (fork)`; keep explicit names.
  - Keep raw replay in memory only for temporary root forks. Normally append only new items; copy retained history only on explicit save. Temporary chats stay pathless/unlisted until saved.
- `/parallel` is removed. Existing saved parallel conversations remain ordinary threads with unchanged IDs, history, permissions, and reference-only boundaries. Do not rewrite history, restore pairs, attach counterpart threads, or delete saved conversations.
- Read legacy child transcript-display boundaries from client-local `asm-side-conversations` metadata per app-server target. Keep the existing namespace and display trimming across legacy reads, rollout reads, and paginated history. Unreadable metadata logs a warning and never blocks history. Do not write new pairing records or clean up personal metadata.
- Preserve upstream Side lifecycle, including attached workspace sides and tab controls. Keep temporary Side restrictions and the `/sendlast`, `/sync`, and non-navigating `/fork` exceptions above. Stock clients continue to see ordinary threads through unchanged APIs; files and explicit global config remain shared.

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

Local CLI help descriptions and TUI messages use ASM; displayed command hints use `asm`, including resume, agents, login, and MCP setup. Keep underlying executable names, CLI parsing names, internal identifiers, model-visible prompts and parsed markers, upstream product names, and installer commands unchanged. Command hints require a user-provided `asm` alias or wrapper.

Keep `codex` executable/version prefix, crate/config/state/archive/tag/workflow/job/check identifiers. Never redirect upstream links/installers for branding.

On Linux, normal CLI, app-server, code-mode-host, and exec-server processes set only
`/proc/self/comm` to `asm`, `asm-app-server`, `asm-code-host`, and `asm-exec-server`,
respectively. Preserve executable filenames, command-line arguments, helper dispatch,
package discovery, and protocol identity. Other platforms remain unchanged. A label
failure warns without blocking startup; process viewers showing full command lines
still show the canonical executable names. External name-based monitoring must use
the new labels. UI plugin helpers intentionally share the `asm-code-host` label
with the regular code-mode host. CLI-dispatched sandbox and filesystem helper
modes retain upstream labels.

### Background terminal waits

Top-level `background_terminal_min_timeout` / `background_terminal_max_timeout`, milliseconds, defaults 5000 / 300000. Reject zero/reversed/unrepresentable bounds. Clamp empty `write_stdin` polls; show bounds in description and finish early on exit.

Preserve initial/nonempty waits and Code Mode outer `exec`/`wait` limits. Exit alone does not resume idle model turns.

### Memory reasoning effort

Optional `[memories]` `extract_reasoning_effort` / `consolidation_reasoning_effort` for V1/V2 use existing effort type. Defaults `low` / `medium`, independent of parent. Preserve models/other memory behavior.

### Compact Markdown links

- Render named web and resolvable local file links as OSC 8 labels, without destination suffixes.
- Keep bare URLs visible. Do not require terminal detection for compact labels.
- Resolve relative paths against the session cwd lexically; never read the filesystem or frontend home.
- Preserve path encoding and location fragments. Opening a line depends on the terminal/editor.
- Keep unsupported or unresolved destinations visible, and retain destinations in text-only output.
- Reuse existing hyperlink layout and output transport; keep trusted artifact capabilities separate.

### Conversation switching

Owned fullscreen switches keep the displayed frame and terminal diff buffer until the destination's replay is complete, then draw the new conversation once. Continue collecting background progress and replay current history in order; a parked screen is not authoritative conversation state. Keep native-scrollback clearing unchanged. This behavior does not require the experimental workspace feature.

Creating a side conversation while Main runs a tool snapshots inherited history, not live execution. Complete pending calls only in the fork's persisted interrupted suffix; leave Main and referenced ancestor records unchanged. The side must remain valid after cold resume and must not execute or cancel Main's pending work.

### Transcript spacing

Keep Markdown paragraphs/code blocks/list items adjacent while streaming and completed. Preserve blanks inside code blocks, raw output, message boundaries, and user padding.

### Click-to-quote

`[tui] click_to_quote = true` is opt-in; default `false`:

- Only the owned fullscreen transcript supports this gesture; native scrollback is unchanged.
- Fresh mouse selections still copy normally. Clicking the existing composer attaches the selected text, not clipboard contents, as a frozen removable preview above the comment draft.
- Clipboard-confirmed deselection retains the pending quote. Explicit deselection, navigation, or thread changes clear pending selection; no timed expiration.
- Attached quotes belong to their chat draft and survive switching away and back. Whole-draft replacement installs its own quote state; quote-only drafts cannot be overwritten by plain history navigation.
- Markdown quoted references are literal: they do not auto-activate skill or app mentions. Unquoted comment references and explicit structured mentions retain normal resolution.
- Fresh selections append references in selection order without replacing earlier references. Each [×] removes only its reference; preview text does not remove it. Mouse wheel scrolls the combined reference/image list when height is limited, keeping the comment visible.
- Number references from 1 in both previews and submitted blockquotes. Keep global indices while scrolling; renumber remaining references after removal. Include numbering bytes in the existing size bound.
- Each attachment appends a numbered comment below existing text in the same composer, with a grey “Add an optional comment…” hint that is never submitted. Keep pretyped general text unnumbered and preserve earlier entered comments; remove only empty generated comment rows. Preserve custom and restricted-input hints.
- Plain Enter on an empty generated comment clears its number and keeps a blank line for a general message, without submitting. Preserve references and earlier comments; modified submit keys keep their normal behavior.
- Track generated comment prefixes separately from user-written numbered lists. Escape generated comment numbering in submitted Markdown so omitted comments do not change displayed indices. Submit every reference and only entered comments; removing a reference renumbers associated comments and keeps its entered comment as ordinary draft text. Preserve associations across draft restoration; changed external-editor text may detach them rather than guess.
- Removing a quote preserves the draft. Clicking never sends a model request; Enter uses the existing submission pipeline with a Markdown blockquote and optional comment.
- Reject an attachment if the cumulative formatted references alone exceed 10,000 UTF-8 bytes, preserving the existing references and comment. No reference-count limit.
- Reject the complete quote-plus-comment item above 10,000 UTF-8 bytes, including quoting and expanded pastes. Never truncate. Ordinary unquoted drafts retain their existing limits.
- Quote items can exceed 1K tokens and require manual review when changed. No new hidden instructions, API fields, or history rewriting.

### Collapsed tool calls

`[tui] collapse_tool_calls = true` is opt-in; default `false`. Rich fullscreen behavior:

- Collapse consecutive shell/MCP/dynamic calls, routine V2 subagent interactions, normal agent starts/completions, and routine waits into one expandable summary.
- Separate counts for calls/starts/completions. Lifecycle notifications are not calls/running tools. Show distinct command/tool names or agent paths in first-use order.
- Count each successful subagent message/follow-up once; matching wait start/end previews once per contiguous displayed group. Completed waits cannot stay running. Keep waits with results/failures/interruption visible.
- Replace overflowing names with `+N more`. Positive integer `[tui] collapse_tool_calls_max_lines` defaults to `1`. Budget applies to each new header's counts/names; larger values wrap. Retained reading snapshots keep captured content on resize and may wrap further. Ellipsize if even counts cannot fit.
- Interruptions, other activity, and chat messages split groups. Keep active tool in a separate short row until committed history includes it.
- Expansion restores original previews/order. Keep failures/approvals visible. Preserve execution/model history/raw output/full transcript. Disabled keeps existing rendering.

### Completed turn activity

- In the rich owned transcript, show commentary and tools live. When the final answer starts, fold that turn's commentary, reasoning, and successful tool previews under an expandable `Worked for …` row. Unphased legacy output folds at successful completion, excluding the last answer.
- Keep user messages, notices, failures, approvals, and final answers visible. Interrupted turns without a final answer remain unfolded. Raw/full transcript, copy sources, execution and model history retain their original cells. Native terminal scrollback is unchanged.
- Reuse activity disclosure controls. Index appended cells once; rebuild only after structural history changes. Bound rendered fold layouts to 64 entries; do not format hidden output.

### Compact status line

- Join with `·`, no spaces. Use `CtxN%` for used context, `CtxN%left` for remaining context, and `F:on` / `F:off` for fast mode.
- Context capacity: compact token count only, no `window` suffix, including `/statusline` previews.
- Lowercase model labels; remove `gpt-`; retain at most first three letters, including custom aliases. Append all version digits without separators: `GPT-6-Astra` → `ast6`, `GPT-6.1-Sol` → `sol61`, `Terra 5.6` → `ter56`. No digits means no suffix.
- Reasoning: first three letters, including model-with-reasoning: `medium` → `med`, `high` → `hig`, `xhigh` → `xhi`.
- Apply to footer, `/statusline` preview, `/subagents` model info. Preserve IDs, other picker labels, terminal titles. Remove role-leading space only in footer: `Main[default]`. Preserve paths/roles/colors/order/single-row truncation.
- Opt-in `cache-hit-rate` in `/statusline`/`tui.status_line`; defaults unchanged. Show current thread's latest reported request cached/input ratio as `CchN.N%`, rounded to one decimal. Use existing usage; no averages/totals/new requests/APIs. Hide unknown/zero-input usage. Missing cache detail remains indistinguishable from reported zero.
- Offer opt-in `output-tps` with unchanged defaults. Show `Ntps`, rounded to the nearest whole number, for the latest successful ordinary Responses request or native Claude message: provider-reported non-reasoning output tokens (`output_tokens - reasoning_output_tokens`), divided by the duration from its first nonempty text or tool-argument delta to completion. Exclude failed attempts, retry waits, compaction, warmup, tools, and other threads. Exclude initial request latency. This is observed streaming generation throughput, not pure decoding speed: stream buffering can affect the estimate. Exclude reasoning deltas from the timing start so hidden reasoning is not divided by the visible output interval. Require an explicit reasoning-token breakdown; never treat missing details as zero for TPS. Native Claude uses the matching root message_delta output/thinking usage and message_stop boundary, never whole-turn cumulative usage; ignore native child streams. Cursor TPS remains unavailable. Omit requests with no generated delta or a zero-millisecond generation duration. Calculate once at live response completion and carry the result on the existing token-usage event. Cache the formatted value in the TUI and use its existing refresh without another notification, redraw request, timer, or streaming estimate. Retain the latest valid result across failed requests; omit unavailable timing or usage and do not reconstruct it from replayed history.

### Continue through model capacity failures

Top-level `continue_when_model_at_capacity = true` opts ordinary model requests
and foreground compaction into unlimited retries for `ServerOverloaded` errors.
Omission or `false` preserves upstream retry behavior. Children inherit the setting.

- Retry the failed request on the same model, without restarting the task or
  deliberately replaying completed tools. Keep ordinary retry budgets separate.
- Without server advice, wait 30, 60, 120, 240, then 300 seconds between attempts;
  subsequent local waits remain capped at five minutes. A valid `Retry-After`
  takes precedence, including deadlines beyond five minutes; reporting time must
  count toward that deadline.
- With the option enabled, pass structured HTTP 503 overloads directly to this
  waiter instead of running the ordinary short HTTP retry burst first. Keep
  unrelated HTTP failures and non-Responses endpoint retry policies unchanged.
- Surface the next retry delay, attempt count, and elapsed capacity-wait time.
  Preserve cancellation and sampling input preemption. Do not add periodic redraws
  or retry background compaction candidates indefinitely.
- Leave other error classifications unchanged, including policy-restricted capacity,
  Flex, quota, authentication, and invalid requests. Overload codes do not prove
  eventual recovery: a backend that mislabels a permanently disabled model as
  overloaded will also keep waiting until cancellation. Do not classify backend
  prose with string heuristics or promise completion if capacity never recovers.

### Multi-agent V2 depth

- Apply `[agents] max_depth` to V2 child creation, counting root as depth 0. The existing default is 1, so omitted settings allow only direct children. Reject deeper spawns before preparing a child; keep existing agents and communication tools available.

### Subagent context windows, service tiers, and picker

Optional top-level `subagent_model_context_windows = { "gpt-6-astra" = 272000, "gpt-6.1-sol" = 272000 }`:

- Positive `i64` integers; exact final model-ID matches only for V2 spawned children through startup/model changes/reload. Missing entries retain scalar/model fallback.
- Keep original `model_context_window` so matches cannot spill into unmatched grandchildren. Preserve maximum clamping, usable headroom, and compaction settings. Root/review/guardian unchanged; no API/rollout fields.

Role TOML may set a positive `model_context_window`, for example `model_context_window = 128000`. An applied child role overrides the map and scalar fallback through startup, model changes, and reload. New children resolve their own role; do not inherit the parent role limit. V1 full-history forks retain existing role inheritance. Keep maximum clamping, usable headroom, compaction settings, and root/internal-worker behavior unchanged. The picker still shows top-level map/scalar configuration, not the effective role limit.

`[subagent_service_tiers]`, e.g. `gpt-6-sol = { high = "fast" }`:

- Match final child model/effective effort after overrides/roles. Matches override root tier throughout child lifetime, including later root-tier changes. Unmatched follows upstream; root requests unchanged.
- Reject unsupported matched tiers and Fast when disabled. No spawn-argument exposure.

`/subagents` labels:

- Append known compact model/effort to main/child titles: `gpt-6.1-sol-high-fast` → `sol61-hig-fast`, `gpt-6-astra-xhigh` → `ast6-xhi`.
- `-fast` only for matched Fast or inherited Fast; matched `default` suppresses inheritance. Do not guess settings/remove thread-ID descriptions/change navigation.
- Append known positive configured capacity: `sol61-hig-fast-272k` / `sol61-max-272k`. Exact map applies only to canonical-path V2 spawned children, never main/V1. Otherwise use configured scalar, including main. Omit unknown/nonpositive values, not catalog guesses.
- Labels show config, not confirmed routing/runtime capacity/usage/remaining context. Capacity precedes model clamping/headroom. Keep TUI-only with other metadata/navigation unchanged; no new APIs/request tracking.

### Plaintext inter-agent messages

Multi-Agent V2 sends parent/child messages as plaintext. Upstream marks `message` on `spawn_agent`, `send_message`, and `followup_task` as `encrypted`, so the server returns ciphertext that only the backend can read.

- Bundled schemas omit the marker. Catalog parameter overrides drop `encrypted` from top-level properties, where the server places it; nested markers are not stripped. A catalog schema that omits `message` is accepted like other V2 catalog schemas instead of falling back.
- Spawn and message tools always deliver `AgentMessage::Plaintext`, for direct, code-mode, and namespaced calls, whatever `encrypted_function_args` the server reports. Responses Lite omits that field for unmarked tools, so the upstream rule would treat plaintext as ciphertext.
- Keep reasoning, compaction, and output-replay encryption unchanged. Old rollouts keep ciphertext and replay it through the unchanged `AgentMessage::Encrypted` delivery branch. Message delivery APIs stay unchanged; raw response items, transcripts, and tool-result logs outside the `collaboration` namespace now show message text. The subagent transcript display adds a typed `interAgentMessage` item to app-server history and notifications.
- V2 child transcripts show the original supplied spawn arguments below the initial assignment, excluding the already displayed message. Render them in the TUI as a dimmed `key: value` list matching the turn completion footer, without JSON braces or string quotes. Expose them separately as typed transcript display metadata and rebuild derived history caches on upgrade; never identify them by parsing assignment text. Persist these as local display metadata only; do not infer omitted settings or add them to model context. Old transcripts without this metadata remain unchanged.
- The TUI shows full plaintext incoming assignments/messages and outgoing communication in parent-owned subagent transcripts, live and on replay. Initial task headers show the child’s actual role when available. Main transcripts keep their existing summaries. This is presentation only: do not change model context or delivery, rewrite history, or attempt to display encrypted payloads. Keep the renderer isolated from upstream generic tool rendering. Thread-history migration `0009_inter_agent_communication_projection.sql` clears only derived SQLite history so existing plaintext is projected again; preserve this cache rebuild when resolving an upstream migration-number conflict. Rebuild invalidated projections only as paginated reads visit segments, including newer turn-shadowing dependencies; do not eagerly replay unused ancestors for bounded recent-history pages. Serialize catch-up for each visited segment. A committed batch does not prove completion; retry failed or cancelled rebuilds. Reuse completed catch-up only while its source revision and SQLite checkpoint still match, without decoding unchanged compressed history again. Older outgoing messages without display records are not backfilled. Paginated incoming records require saved turn metadata; records without it are not assigned to a guessed turn.
- Store fork display projections in `asm_thread_history_1.sqlite`, not stock `thread_history_1.sqlite`. Follow the [shared compatibility gate](#shared-compatibility-gate) for cache versions, wire consumers, validation, and manual stock-cache recovery. The new namespace does not repair stock caches written by older ASM. Cold reads lazily replay visited rollout segments. Warm reads reuse checkpoints. The separate cache costs disk space and first-read replay, not another lookup layer per request.
- After initialization, `asmTranscript/enable` with `version: 1` enables fork transcript display on that connection. ASM TUI opts in before history reads and after reconnects. Without opt-in, omit typed `interAgentMessage` from snapshots, paginated display items, and live notifications. Preserve ordinary history, actual collaboration summaries, model context, and storage pagination cursors. For unsupported clients, skip display-only item/timeline pages until a visible row or EOF. Never truncate history on an empty filtered page. Do not disguise communication as user messages.
- Include live transcript display and Desktop rendering in affected consumer checks. A stock reader pass does not prove Desktop rendering.
- On every upstream sync, search for new `with_encrypted`, `encrypted` schema markers, and `AgentMessage::Encrypted` producers on agent tools. A missed marker sends ciphertext to a child as plaintext text.

### Multiline-aware Enter

Opt-in `[tui] multiline_enter = true` keeps plain Enter sending single-line drafts and
inserting a newline into multiline drafts, including collapsed pending paste content.
Require plain Enter in both submit and editor newline bindings; preserve custom keymaps
and popup actions. Modified submit bindings always send, flushing pending paste bursts.
The default is unchanged. Example explicit newline/send bindings:

```toml
[tui]
multiline_enter = true

[tui.keymap.composer]
submit = ["enter", "ctrl-enter"]

[tui.keymap.editor]
insert_newline = ["enter", "shift-enter"]
```

Use Shift+Enter to start a multiline draft, Ctrl+Enter to send it. Terminals must support
the configured modified keys. Queue bindings retain their existing behavior.
Vim Insert/Replace use the editor newline binding; Vim Normal retains its editing semantics.

### Composer destination

Keep the active prompt destination visible in the composer’s top-right padding: Main or Side. Use a bold filled badge with one space on each side: neutral for Main and magenta for companions, with contrasting text. Do not add borders or recolor prompt markers or typed text. Derive the destination from active thread state, not footer text, and preserve input geometry.

### Built-in agent role descriptions

Explicit spawn `model` overrides the role model; the role model overrides inheritance.
Resolve the final model before validating effort and selecting the runtime. Preserve
role effort and other constraints; incompatible effective settings fail instead of
switching runtimes. Keep the public `agent_type` spelling.

Keep the built-in explorer purpose and original delegation Rules block in the spawn-agent schema. Keep the worker task list and concise Rules requiring explicit task/file ownership, preservation of concurrent edits, and one owner for shared prerequisites. Coordinate ownership before continuing overlapping work; continue independent work meanwhile.

### Subagent global instructions

For spawned subagents, `$CODEX_HOME/SUBAGENTS.md`, when present, replaces the global
user-instruction contribution from `AGENTS.md` or `AGENTS.override.md`. An empty
file clears that contribution. A missing file retains the existing inheritance
contract; other read errors fail startup rather than silently use root instructions.
Resolve the file at child startup, including cold reload, for fresh and full-history
children. Keep project instructions, host thread instructions, and root/internal
worker behavior unchanged. Full-history children retain history and receive the
replacement through normal incremental context updates, not history rewriting.

### Wait and sleep intent reminders

`clock.sleep`, V2 `agents.wait_agent`, Code Mode
`wait`, ASM TUI `wait_threads` (including its MCP exposure), and empty-input
`write_stdin` polls require a `reason`. Nonempty terminal input stays unchanged.
Trim reasons and reject more than 500 characters. Always return continuation
guidance naming the wait tool; an empty reason omits only its context line.
Return nonempty reasons as JSON-quoted tool
context, separate from command/script output and its truncation budget.

When new input interrupts a wait, remind the model to understand the user's intent;
a status question alone does not cancel or pause pending work. If work remains
pending, give status as commentary and call the named wait tool again with valid
current arguments rather than ending the turn. Use neutral guidance when a return
does not identify interruption. Preserve running children, cells, processes,
and collected terminal output. Explicit stop and cell termination must not prompt
resumption. Reminders are guidance, not guaranteed continuation or automatic waits.
Code Mode, terminal polling, and TUI waits use the existing `instant_interrupt`
steering signal; disabling that feature preserves their normal wait behavior.
An interrupted TUI wait returns `interrupted: true` with no target snapshots;
it cancels only the observation, not the target chats. Limit MCP early cancellation
to the marked `codex_tui.wait_threads` tool and preserve its MCP result envelope.
Do not rewrite user messages, add persistent reminder state, or change app-server
schemas. Amend effective tool schemas after catalog overrides. Legacy V1 waits,
environment readiness, desktop-owned tools, and initial command execution are
outside this change.

### Test-only dependencies

Keep `codex-ollama`'s `wiremock` dependency under `[dev-dependencies]` while only tests use it. This removes its test-server dependency chain from production builds, not necessarily the linked binary. Drop this drift if upstream adopts it; restore the normal dependency if production code needs it.

### Advertised subagent models

Top-level `subagent_model_list` optionally selects advertised model IDs in user order for both the spawn-tool description and `<model_catalog>` context. Omission retains automatic picker-visible selection; `[]` advertises no choices. Explicit IDs may select hidden models from the loaded catalog. Duplicate IDs are deduplicated; only the first five choices are advertised, and the context catalog retains its 1,000-byte bound. All configured IDs are checked against the loaded catalog and spawning-backend compatibility before truncation; an invalid ID renders a configuration error instead of falling back. This setting does not restrict runtime spawning or change the model picker.

### Flat reasoning-effort picker

Show all model-supported reasoning efforts, including Max and Ultra, in one list.
Do not gate them behind a “More reasoning…” submenu. Preserve selection persistence,
Plan-mode scope, and the explicit picker requirement for raising into Ultra by shortcut.

### GNU malloc arena cap

At the start of `main`, the Linux GNU `codex` calls `mallopt(M_ARENA_MAX, 4)` before any thread starts. A set `MALLOC_ARENA_MAX` takes precedence. glibc otherwise allows up to 8 arenas per CPU, and many-core hosts keep idle heap in them. Keep the GNU system allocator: upstream uses jemalloc only on musl, to replace the musl allocator. Mock-model runs (64 CPUs, 32 conversations × 4 turns, 3 runs) measured 6–15% lower RSS and 10–28% lower anonymous memory after unload, with no measurable wall or CPU cost. The effect on real sessions is unmeasured. Drop this drift if upstream caps glibc arenas itself.

### Session HTTP transport reuse

Keep one model API transport per session and reuse the existing route-aware pool. Rebuild it when the resolved client factory, endpoint, redirect policy, authentication owner, runtime, or default headers change. Resolve provider authentication for every request; token refresh alone does not discard connections. Preserve existing TLS, CA, proxy, cookie, redirect, and managed destination-policy behavior. Route selection and policy checks still run for every request. This change covers model HTTP requests, not auxiliary clients or WebSocket lifetime.

### Tokio worker cap

On Linux, the runtime builder starts at most `min(CPUs, 8)` Tokio workers unless `TOKIO_WORKER_THREADS` is set. Tokio otherwise starts one worker per CPU, 64 on the primary host. Mock-model runs with the glibc arena cap (64 CPUs; 32 conversations × 30 turns and 64 × 20) measured 9–15% lower RSS and about 21% lower total CPU than the default, with no wall-time loss. The effect on real sessions and on peak async CPU is unmeasured. macOS and Windows keep Tokio's default. Drop this drift if upstream caps workers itself.

### Desktop agent-created worktrees

ASM app-server handles the recognized `codex_app.create_worktree` and `attach_worktree` dynamic tools on the thread execution host. Create or attach a durable managed worktree without changing the chat cwd, workspace roots, or skill context. Return the worktree path for explicit command execution; do not invoke the desktop handler that changes the chat workspace. Keep explicit user navigation unchanged. Match TUI trust behavior: reject explicitly untrusted chat and source projects; an unset trust level is allowed. Require a non-ephemeral chat, local execution environment, and durable attachment storage. Creation can complete immediately even when `allowAsync` is true. Preserve partial-registration recovery without creating duplicate checkouts. Desktop attachment rendering and subsequent-turn skill retention require validation in the actual desktop client; backend-only checks do not establish them.
