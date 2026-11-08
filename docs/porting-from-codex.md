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

`app_context_filter` defaults to `[]`. Before session creation, remove only exact named `###` sections from Desktop `<app-context>` instructions. Preserve unknown sections, surrounding instructions, per-turn context, and rollout history. Keep filtered instructions consistent when rebuilding initial context. After filter changes, use a fresh chat.

### External runtime boundary

Keep ASM inference/tools separate from external CLI execution. Use `core/src/external_runtime` for session integration. Use `ext/external-runtime` for shared process, framing, and cleanup code, with separate Claude/Cursor drivers. Use neutral shared start, prompt, outcome, and error types. Each driver owns its protocol encoding. Preserve the legacy `claude-runtime` checkpoint directory, canonical history encoding, and checkpoint hashes across internal renames.

Delete ASM linkage and private Cursor configuration only with exclusive runtime ownership. Preserve native conversation history. In opt-in failure diagnostics, record bounded CLI versions from protocol initialization. Do not launch extra CLIs or add version-number admission gates.

### Catalog-bound model providers

- With `load_external_models_json = true`, merge bundled entries into custom `model_catalog_json` in memory. Exact custom IDs retain all metadata overrides. With `false`, retain only the custom catalog. Routing remains bundled and exact-match-only.
- Bundled `external_models.json` entries can select an existing named `model_provider`. Accept canonical slugs and optional provider/runtime-qualified aliases. Normalize aliases before validation/requests. Persist canonical model/provider identity so alias changes cannot reroute resumed chats. Preserve existing full IDs and managed provider requirements. Do not infer routing from slashes or remote catalog metadata. Keep endpoints/credentials in user provider configuration.
- DeepSeek V4.1 Flash 1017 selects `fireworks`, alias `fireworks/deepseek-v4.1-flash`, without a profile or custom role. Default its context to 300,000 tokens; allow explicit overrides through 1,000,000. Select direct tools: live checks could not execute Code Mode's custom tool through Fireworks.
- Show picker aliases but retain canonical identity. Native aliases `claude/…` and `cursor/…` retain `claude-code/…` and `cursor-agent/…` IDs. Do not add HTTP bindings or change native model/runtime selection. Picker provider changes start fresh sessions and preserve old chats.
- Admit valid bound/native input and resume independently of the global HTTP-provider default. Validate saved/requested identities separately. Overrides cannot exempt unrelated saved chats. Preserve mandatory policy, configuration errors, missing-provider checks, and history/runtime boundaries. Require the current provider definition and valid configuration. Classify routes from in-memory bindings/runtime detection, not discovery. Unbound API models retain upstream saved-provider admission.
- Cross-provider subagents require `fork_turns="none"`. Reject history reuse; preserve saved provider identity on resume. Bound models use their own provider-specific model manager, not parent discovery identity.
- For user-spawned children with a catalog-bound provider ID, require an explicitly bound model or native runtime, including descendants/cold resumes. Reject unbound selections before startup; do not infer reverse routing. Use a separate provider ID for unbound custom subagent models. Preserve ordinary main/internal-worker provider behavior.
- Keep upstream catalog/API model types unchanged. Hiding bundled entries does not disable explicit selection.

### Claude Code thread runtime

The shared native rules below also apply to Cursor, except for its listed differences.

**Selection and settings**

- Explicit `claude-code/<model>` selects official persistent `claude -p` for mains and ASM children. At launch, expand versioned `sonnet-`/`opus-` suffixes to native `claude-` names; pass other suffixes unchanged. Preserve saved model IDs. Other models retain upstream inference.
- Keep the original native executor/cwd. Allow ASM-only workspace-root, config, and capability metadata updates. Actual executor/cwd changes require a fresh thread.
- Keep upstream `models.json` unchanged. Bundle native entries in [`external_models.json`](../codex-rs/models-manager/external_models.json). `load_external_models_json` defaults to `true`; `false` hides bundled entries, not explicit native selection. Append entries to default catalogs, including cached/remote catalogs. External entries override duplicate IDs without changing cached payloads. Provider-owned catalogs ignore the flag and remain authoritative. Custom catalogs honor it and retain duplicate-ID metadata. Native model/effort access remains native-owned; metadata grants no access.
- Keep CC entries visible without OpenAI login; do not mark them API-supported to bypass filtering. Preserve ordinary model defaults.
- Native sessions discard tiers inherited from config/profiles. Only Default passes explicit request/session-flag tier validation. Reject unsupported persistent/turn-only tier changes before committing settings. Preserve OpenAI tier resolution.
- On picker runtime entry/exit or model changes, require confirmation and start a fresh session. Preserve old history/defaults; never replay history into the new native conversation.
- Same-model/provider effort changes retain the conversation and saved model defaults. At the next idle turn boundary, apply the native control, require confirmation, then save effort before input. Preserve process, session ID, and history; never rebuild context. Cold resume restores saved effort before requested changes. Reject unsupported changes before prompting, without fallback.
- Preserve cache-relevant context, but do not promise cache keys/hit rates. Claude cache-prefix preservation depends on server-controlled `per_turn_effort_active`.
- For ASM children, use available catalog effort defaults/support. Native IDs absent from the catalog remain usable without OpenAI fallback metadata. When a role changes the external model, replace only inherited parent effort with the new catalog default, or leave it unset without metadata. Validate explicit spawn, role, and configured subagent efforts; do not discard unsupported choices. Same-model roles retain inherited effort. Preserve OpenAI role behavior.
- Even with custom descriptions/hidden catalogs, V2 spawn descriptions must state: `claude-code/*` requires `fork_turns="none"` and a self-contained task. Do not rely on failed spawns to teach this.

**Ownership and input**

- ASM owns thread IDs, queues, user input, display, and process lifecycle. The native CLI owns context, instructions, skills, tools, permissions, hooks, and internal agents.
- Apply one input policy before native dispatch/mailbox delivery. Forward explicitly queued plaintext ASM context/tool output in labeled JSON blocks, separate from user input. Warn and skip encrypted injected items; preserve the actual prompt. Never forward ciphertext or backfill ignored history.
- Persist projection markers for newly forwarded context to retain warm/cold input accounting. Project newly requested idle `!` output the same way for the next native turn. Do not start inference or backfill old shell history.
- Do not synchronize generated ASM instructions or approval receipts. Only root Claude communication guidance and the child's selected role are instruction exceptions. Never forward merged/inherited ASM instructions. Native mains receive no role injection.
- For Claude children, append only the selected role's `developer_instructions` through `--append-system-prompt`, preserving the default prompt. Pass one argument without shell interpolation or temporary files. Arguments expose text and have OS limits; never truncate or fall back to files. Cursor uses its gated ACP path.
- Keep loaded role text fixed during warm turns. On cold reload, recover current text from normal child configuration. Do not pin text to linkage, or reject same-role edits. For managed Claude children, always pass `--system-prompt-snapshot off`, including empty roles. Require Claude Code 2.1.257+; fail on unsupported CLIs. Prompt rebuilding can affect caching/retained thinking. Preserve root/side snapshot defaults and runtime-setting/uncertain-turn recovery checks.
- Root Claude appends only the original paragraph beginning "Assume users can't see most tool calls or thinking". It requests updates before tools and at milestones. Exclude ASM children, internal sessions, and temporary sides. Preserve child role prompts, root snapshots, and native internal agents. This startup policy neither rewrites context nor guarantees compliance.
- For managed Claude children, including roleless children and cold resume, set `FORCE_PROMPT_CACHING_5M=1` in environment and flag settings. Request five-minute caching for children/helpers; preserve native cache-disable controls and managed policy. Do not change personal settings or root/side/Cursor/OpenAI cache policy.
- In the same child inline settings, set `disableClaudeAiConnectors: true` and `syncClaudeAiSkills: false`. Skip fetched claude.ai connectors/account skills without deleting shared skill caches. Preserve configured local MCP servers, built-in tools, ASM bridge, and personal/project/plugin/bundled skills. Preserve root/side connector and skill defaults and personal settings.

**Communication and display**

- Do not inject ASM communication tools into mains/sides. Managed MultiAgentV2 children use one authenticated loopback JSON dispatcher, not MCP. Claude uses a session-local native mod; Cursor uses the existing ASM binary's private helper through native shell. No external JavaScript/Python runtime is required. Claude's `mcp__asm__` names do not imply MCP. Preserve other native integrations.
- Discover only canonical collaboration tools except `spawn_agent`. `disable_direct_message` removes `send_message`/`followup_task`. `wait_agent` follows `wait_agent_enabled`; retain `interrupt_agent`/`list_agents`. `disable_native_tools` removes all these tools. Independently, `agent_message_board` adds nine extension discussion tools at thread start, with its storage gates.
- Reuse canonical registration/handlers; duplicate names fail the turn. Do not forward arbitrary ASM tools or add a registry. Keep credentials in an owner-only descriptor in the private child directory, never prompts/argv. Scope calls to active turns, serialize through the execution gate, and do not retry automatically.
- Both bridges use the helper timeout: eight queued maximum waits plus 30 seconds. Claude uses native `$.process.spawn`; `$.http.fetch` has a 30-second deadline, and `$.process.run` has a ten-minute cap in 2.1.310. Preserve native tool-search defaults; do not force every schema into prompts.
- Board notices reach running turns only: Claude at the next tool boundary; Cursor through active-prompt CLI wait results. Never wake idle children.
- Busy Claude children receive UUID-stamped queued user input, never Send now/another interrupt. Native delivery occurs at a tool boundary without rejecting running tools. Require `system/init` capability `interrupt_send_now_v1`; otherwise deliver at turn boundaries. Persist input before submission and reconcile every submitted command before clean idle. Idle `send_message` stays queue-only; preserve Codex delivery. For urgent redirection, use `interrupt_agent`, then `followup_task`, with normal interrupt safeguards.
- While `background_tasks_changed` lists live non-ambient tasks, native idle is not final. Keep completion-triggered native turns in the same ASM turn until idle without live work. Long-lived tasks keep it running until completion/interruption. Notifications grant no authority. Plain interrupt leaves background shells running; retain confirmation rules. A notification result answers new input only after that input succeeded, not from a turn between ASM turns.
- Project native tool/skill/task activity into existing MCP display cells for streaming, disclosure, grouping, and replay. These are display events, not execution/context items. Attribute native child activity without ASM threads. Preserve failures; avoid duplicate bridge events. Count distinct observed native child tasks in completed-turn `ag`, deduplicated across lifecycle/replay. Historical activity must not inflate active ancestor counts.

**Login and permissions**

- Resolve `claude` through PATH; use native settings/subscription login. Do not extract subscription credentials or enable API billing implicitly. Preserve native tools/skills/instructions, except V1/V2 ASM-spawned Claude children use `--disallowedTools AskUserQuestion`. Mains retain native questions.
- Use deterministic subscription login through shared app-server chat requests/responses, without Desktop changes. Explicit `claude login`/`cursor login` selects the provider in any main chat, including GPT parents, without changing models. Store that provider in pending login state; never infer it from the chat model. Children cannot start interactive login.
- Keep login exchanges display-only, outside model history/rollouts. Intercept replies before normal ingestion, history, hooks, or inference. Send authorization codes only to the official Claude login subprocess. Never put codes in model requests, logs, or rollouts; client drafts/recall are accepted.
- Expired/interrupted exchanges fail closed. Unrecognized replies get fixed restart instructions, not normal ingestion. Only explicit non-secret actions exit/restart login. Preserve a code-free pending marker across backend restart/resume, without codes/OAuth state or checkpoint changes. Keep output private and errors fixed. Verify subscription auth; never fall back to Console/API keys/cloud providers.
- Login never authorizes replay or uncertain-checkpoint recovery. Display that native permissions, not ASM sandbox/approvals, govern native tools. Refuse external execution when managed policy requires an ASM sandbox.
- Every Claude main/child uses `--dangerously-skip-permissions` and `--permission-prompts none`. Preserve native denies, hooks, and managed policy. Remaining prompts fail explicitly; never grant protocol approvals. Configured native hooks stay native.

**Recovery and cleanup**

- Keep one CLI process per loaded Claude session, private native linkage, and unfinished-turn state. Resume only known-clean sessions. Never automatically replay uncertain submitted input or completed tools. Other execution errors require explicit recovery; error text cannot clear older unfinished checkpoints.
- UUID-stamp native input. After an ASM interrupt, retain the process only with matching successful acknowledgement, explicit empty queued-input receipt, explicit idle, and stopped observed native children. Persist clean linkage before another turn. Otherwise, on missing confirmation, timeout, disconnect, or forced abort, kill the owned group and keep linkage dirty. Give only external tasks bounded cleanup grace.
- Track native tasks by ID/latest run ID. Resumes retain task IDs with later-sorting run IDs. Older-run/run-less starts or terminals cannot replace/clear a known newer run.
- Resume aborted-streaming/aborted-tools results only after an ASM-requested interrupt with the submitted UUID's cancelled-command receipt and confirmed cleanup above. Continue with new input; do not assume tools had no effects.
- A structured Claude auth failure can be failed but resumable only after command reconciliation, confirmed idle, and no uncertain tools/children. Persist consumed input/clean linkage before reporting failure. After login, accept new input only; do not resend failed prompts.
- Bound frames, stderr, and cleanup waits. Kill only owned processes; preserve primary failures and retain diagnostics only when requested. Record pre-cleanup process state separately from final exit status, including whether the CLI was already stopped.
- Give each native CLI a unique inherited ownership marker. Combine marker ownership with verified live ancestry to identify detached tools after CLI exit. Never signal ASM's process group. Cleanup is not sandboxing; processes that clear markers and leave observable ancestry can escape.

**Images and lifecycle**

- Forward user images as typed blocks in text/image order, preserving delivery checkpoints. Use existing processing for inline data/local attachments. Injected ASM context stays plaintext-only. Keep prepared image bytes inline in native history, without OpenAI uploads/replay transformations.
- Validate before recording input/advancing checkpoints. Image input frames have a 31 MiB limit; echoed/replay frames, 32 MiB; ordinary text/control frames, 1 MiB. These are ASM transport limits, not provider limits.
- Explicitly reject unsupported history forks, rollback, runtime switches, and native-only operations. Suppress native prewarm, title inference, and System1. Native memory extraction can summarize saved ASM transcripts.
- Claude forks only whole history into temporary `/side`/`/btw` and `/sync`, at main's model. Require loaded idle main, no native background work, and a clean checkpoint whose delivered input matches forked history. Optional `fork_point` is the last successful uninterrupted turn's final top-level assistant UUID. Without it, refuse `/side` until a new main turn completes.
- Start Side's first turn separately with `--resume <main> --resume-session-at <fork_point> --fork-session --no-session-persistence --session-id <new>`. Exclude later main turns. The hidden cutoff flag was verified on 2.1.309; unknown UUIDs fail closed, never fall back to latest.
- In Claude Side, disallow native Agent, Task, TaskStop, SendMessage, ListAgents, CronCreate, CronDelete, ScheduleWakeup, EnterWorktree, and ExitWorktree. Send one short boundary to preserve Side's reference-only/no-agent/no-worktree rules; ASM side instructions are not native context. Never replay history/copy session files. Keep side checkpoints in memory; closing unloads the thread/process. Lost/closed sides cannot cold-resume. Reject Cursor sides, saved `/fork`, and historical native cuts.
- External threads use Default collaboration only, never guardian review. Keep mandatory managed-execution restrictions. Native turns have no total deadline; retain cancellation/bounded cleanup during tools/background agents. Do not add an approximate-token prompt cap or truncate instructions; Claude manages context within transport limits.

**Claude context and usage**

- Explicit child role context limits, otherwise exact V2 `subagent_model_context_windows`, set native `--autocompact` windows. Accept only 100000–1000000 tokens; reject unsupported values. Set `CLAUDE_CODE_AUTO_COMPACT_WINDOW` in child environment and flag settings. Explicit values override inherited environment/user/project settings, not native managed policy. Unmatched children retain native defaults/environment.
- New roots use explicit `model_context_window` with the same range/precedence; unset retains native defaults/environment. Catalog metadata never sets it. Bind the window to linkage. Changes require a new child chat; mains/forks keep main's original window.
- The window is soft, not a hard capacity/exact trigger. Native capacity clamps it; output reserves/compaction headroom reduce it. Preserve native compaction enable/disable controls, memory, and hooks; do not force disabled compaction. Base context fill on native model capacity, not the configured window.
- Report checked process deltas from native cumulative model counters. First cold-resume usage and missing/reset counters are unknown, not zero. Warn that ASM totals/token budgets are partial. Thinking is an output subset, not an extra charge. Use latest main request input/native capacity for context fill, not cumulative billing/children. Hide unknown metrics. Cache rate is that request's cached input share.
- Require stream-json result followed by session-idle completion. Verified protocol target: 2.1.296; queued delivery/background continuation: 2.1.309. Revalidate upgrades.
- Keep opt-in until real Claude-main and Sol-parent/Claude-child checks pass with the intended installed CLI. Do not replace installed binaries/global profiles for tests. Cross-runtime context parity is not required.

### Cursor Agent thread runtime

Explicit `cursor-agent/<model>` selects official Cursor ACP (`agent acp`, JSON-RPC over stdio) for mains/ASM children. Apply shared native rules above, including plaintext context and encrypted-input warnings. Do not add runtime copies, backend hierarchies, or event vocabularies. Branch only for protocol/transport/delivery differences below.

- Bundle Cursor model/effort metadata in `external_models.json`. Catalog loading/startup must not launch/authenticate either CLI. At Cursor session start only, negotiate `parameterizedModelPicker`. Validate model/effort against advertised choices; apply the actual `thought_level` parameter ID and confirm before prompting.
- Preserve saved Auto identity: prefer exact advertised IDs, otherwise translate `auto`/`auto-smart` to advertised ACP `default` only. This is not model fallback. Other unavailable choices fail. Saved bracket-encoded IDs retain parameters; explicit effort replaces that parameter. Custom catalogs stay authoritative; metadata grants no access.
- Require `promptCapabilities.image` for ACP `session/prompt` image blocks. Reject unsupported images before history/checkpoint changes; never discard images or replace them with paths. Apply shared inline-history and 31/32/1 MiB limits.
- Native runtime is identity. Never resume/fork/switch between Claude and Cursor. Store full model IDs. Reuse Claude checkpoint path/format with Cursor-assigned session IDs.
- Keep login orchestration/process safety in neutral `native_login`. Separate adapters own CLI, environment, URL, reply, and status policies. Preserve legacy `claude-runtime` pending-marker paths.
- `cursor login` uses official `agent login`, disabled browser opening, and an allowlisted sign-in URL through shared interception. Native CLI owns browser auth. Accept only `done` to check completion, never keys/tokens. Preserve shared fail-closed pending replies/recovery rules.
- Use native authenticated ACP bootstrap. Detect only typed auth-required `error.code = -32000` during setup/turns. Use shared login guards/fixed provider instructions, never arbitrary assistant prose/generic errors. Do not invoke ACP `authenticate` and its separate browser flow. Keep uncertain checkpoints dirty. CLIs that hide established-turn auth failures in assistant prose still need explicit `cursor login`; automatic expiry detection remains unavailable.
- Remove child `CURSOR_API_KEY`/`CURSOR_AUTH_TOKEN`. Every main/child uses `agent --force acp`. Keep Cursor denies/team auto-run policy; never pass `--approve-mcps`, `--trust`, or `--sandbox`. Cursor cannot disable its question tool: children retain it, but refuse questions. Reject remaining approvals, return `rejected` for plans, `skipped` for questions, and errors for other requests. Show refusals; do not hang/fall back to local plans.
- Give each thread private `CURSOR_CONFIG_DIR` beside its checkpoint. On every spawn, refresh private user `cli-config.json`/`permissions.json` copies using native directory precedence. Preserve native permissions/user model defaults. Keep login/rules/skills/hooks/MCP settings native; never copy/extract credentials.
- Negotiate `clientCapabilities._meta.subagents`; require `sessionCapabilities.subagents`. Keep native children/background completion in the same ASM turn. Accept updates only for the session/announced children. Show child tools/text in child activity cells, not main transcript or ASM threads. A response is clean only after all announced children report `completed`, `failed`, or `cancelled`. `disconnected` after cancellation timeout does not prove stopping: fail and keep dirty linkage.
- Cursor marks finished tools `completed`. Infer failure only from top-level typed `rawOutput`: nonzero `exitCode`, string `error`, `rejected: true`, or `permissionDenied: true`. Missing fields mean unknown; tool failure does not fail the turn.
- Append only selected child-role text through ACP `systemPrompt`, `mode = "append"`, on `session/new`/`session/load`. Before auth/session creation/loading, require advertised `sessionCapabilities.systemPrompt` object. Unsupported CLIs fail; never use user prefixes/replacement prompts/shared rule files. Use current role text on cold reload, without saved-text comparison; omit the field without applicable text.
- Child role limits and `subagent_model_context_windows` do not set Cursor compaction windows or trigger Claude range validation. Cursor owns context.
- Limit each login/setup request, including stdin submission, to 60 s. Turns remain unbounded. Discard `session/load`'s native-history replay; reset the model after every new/loaded session.
- Cursor shell groups can outlive ACP. Use shared ownership-marker cleanup after leader exit; never kill by socket/path/name. Require all identified owned processes to stop. On interrupt, send `session/cancel`. After `stopReason: cancelled`, stop the process tree; clean linkage requires successful descendant cleanup. The next turn loads the session. Missing acknowledgement, cleanup failure, or other failure keeps linkage dirty.
- Cursor usage/context fill is unavailable, never zero, in ASM accounting/`/status`. Exception: required numeric `codex exec --json` `turn.completed.usage` fields remain zero when unknown, with the unavailable-usage warning. Keep this wire shape until owner-approved API changes.
- Children use the local CLI bridge, not ACP MCP: `session/new`/`session/load` send empty `mcpServers`. Do not restore the policy-blocked transport/bypass team policy. Deliver active-prompt mail/board notices through canonical wait CLI results. Record mail and advance dirty checkpoints before returning it, without next-prompt replay.
- Give each nonempty delivery a unique receipt. Clean linkage requires Cursor to report it in a successful root shell result, not just bridge/HTTP completion. Missing or truncated confirmation requires explicit recovery, without retrying potentially side-effecting tools. Keep queued input/next-turn followups ordered. Return Cursor's final answer; no Send now. Pre-bridge children have no messaging record: cold resume fails closed and requires a new thread. Mains are unaffected.
- Verified CLI: 2026.10.22-27dcf5f. Revalidate upgrades.

### Minimal diagnostics

- Default `diagnostic_log_capture = false`. This startup policy controls Rust stderr/file logs, persistent diagnostics, feedback ring/continuous tags, optional TUI replay, and rollout-trace recorders. `RUST_LOG`, `log_dir`, and recorder environment variables cannot enable disabled capture.
- To opt in, set `diagnostic_log_capture = true` and restart affected processes. Keep existing filters. Explicit `log_dir` also routes SQLite logs unless managed `sqlite_home` overrides it. Propagate policy to the code-mode helper; inherited helper settings cannot override it. Standalone helpers without user config require explicit opt-in.
- With capture disabled, do not allocate feedback capture, install its tracing layers, open/migrate diagnostic databases, or run insertion/pruning/reclamation workers. Leave historical files unchanged during ordinary startup.
- Explicit history inspection/feedback export may read old logs without capture. For thread deletion, remove historical diagnostics through existing-only, on-demand access. Never create missing files. Preserve feedback submission/upload errors; do not invent evidence.
- Preserve direct errors/warnings, exit status, protocol events, saved conversation/tool history, subagent relationships, memory, and operational state. Keep detached-daemon startup/fatal stderr and Windows sandbox security-audit logs separate and active. Do not weaken security. Keep product analytics/account usage unchanged.
- Disable the built-in Statsig metrics exporter, including optimized releases/account reloads. Keep its config spelling; default `otel.metrics_exporter` to `none`. Preserve explicit OTLP collectors. Without a configured exporter, opt-in `runtime_metrics` uses a manual reader without a timer/network exporter. Do not scan session-storage sizes for unsolicited telemetry. Validate disabled paths with focused real-binary scenarios, not broad upstream retesting.

### System1

Use opt-in `[system1]` in the shared backend, not the TUI. Toggle it with `/system1`. Require `features.step_model_switching` and `features.reasoning_effort_override`. Clients must use this backend; System1 does not replace Desktop's backend. Use HTTP for Decisions, not the Responses WebSocket.

Accept former `[adaptive_reasoning]` config, including deny-only roles, and former rollout fields on resume. Emit only canonical names. Preserve layer precedence across both spellings; reject both tables in one layer. Before canonical writes, rename an existing old table. Reject conflicting writes without changing the file.

| Setting | Default | Validation |
| --- | --- | --- |
| `enabled` | `false` | Opt-in only |
| `profile` | Absent: legacy OpenAI configuration | Name in `profiles`; cannot mix with legacy model/key settings |
| `profiles.<name>` | Empty map | Explicit `protocol` (`openai` or `typesafe`), nonblank `model`, ASCII identifier `api_key_env`; optional full `endpoint` URL |
| `decision_model` | `"gpt-6-luna"` | Nonblank |
| `decision_model_api_env` | `"DECISION_API_KEY"` | ASCII environment-variable identifier, not a secret |
| `min_effort` / `max_effort` | `"low"` / `"max"` | Ordinary levels; `min_effort <= max_effort` |
| `decision_timeout_ms` | `200` | 1–5000 ms |
| `max_context_bytes` | `8192` | 128–32768 UTF-8 bytes across all Decisions input items |
| `update_on` | `["turn_start", "tool_result"]` | Only these triggers |
| `decision_every_n_turns` / `decision_every_n_steps` | `1` / `1` | Positive integers; [cadence rules](#space-decision-calls-by-turn-and-step) below |
| `low_effort_description` / `medium_effort_description` / `high_effort_description` / `xhigh_effort_description` / `max_effort_description` | Built-in text below | Nonblank; each at most 4096 UTF-8 bytes |
| `rubric_instructions` | Built-in rubric | Nonblank; at most 4096 UTF-8 bytes |

**Connections.** Profiles select connection settings, not adaptation policies. Share bounds, cadence, evidence limits, timeout, and rubric. Profile names have no built-in meaning. Do not infer protocol from names, endpoints, or credentials. Accept `backend` as an alias for `protocol`; reject both together. Validate every profile. Reject unknown profiles/protocols without fallback. Without `profile`, retain legacy OpenAI defaults and explicit overrides. Reject a selected profile with either legacy model/key setting, even when it equals its default.

Resolve the connection once per session on first use, including failed credential attempts. Keep protocol/endpoint/model fixed across warmup, decisions, and config reloads. Both adapters share cancellation, deadline, allowed-choice validation, and recovery. Do not use automatic failover or adapter-internal retries.

Without `endpoint`, use the protocol's official HTTPS endpoint. Overrides are full destination URLs, not format selectors; append no path. Require HTTPS except loopback HTTP. Reject URL credentials and fragments. Reject blank endpoints during config validation. Reject other invalid URLs during client construction without exposing them in errors.

The endpoint receives bounded conversation evidence and the `api_key_env` credential. A custom endpoint need not belong to OpenAI or TypeSafe. Choose it deliberately. Keep authentication separate from the main model and sanitize diagnostics. Read only the profile's `api_key_env` or legacy named variable, default `DECISION_API_KEY`. Retain explicit variable-name overrides. Never fall back to `OPENAI_API_KEY`, provision credentials, or log keys/evidence. Do not route Decisions through Guardian.

**Choices.** Adapt ordinary roots and spawned subagents, not internal workers. Use the selected backend to choose supported effort within bounds. Keep Ultra/Persistent, models, service tiers, execution modes, permissions, and saved defaults unchanged. With fewer than two efforts, skip Decisions and warmup; retain current effort.

Generate selection guidance from the supported model default before applying bounds. Use that default as the normal-work reference. For a `max` default, use the preceding catalog-supported ordinary effort instead. Do not change the task baseline. Model meaning remains catalog-owned.

Use low/medium/high/xhigh descriptions for one-below-reference/at-reference/one-above-reference/further-above-reference choices. Use fixed mechanical-execution guidance further below reference. Always use `max_effort_description` for max and fixed direct-execution descriptions for `none`/`minimal`. Without a supported default, keep effort-labeled guidance.

Keep choice descriptions separate from the selection/safety rubric. Send only allowed main choices; warmup retains its synthetic low/medium pair. A replacement rubric must not remove or duplicate descriptions. Default description text:

```toml
[system1]
low_effort_description = "Straightforward coding, focused searches, and bounded analysis."
medium_effort_description = "Ordinary substantive work, including moderately complex coding."
high_effort_description = "Difficult debugging, multiple interacting constraints, conflicting evidence, or substantial uncertainty. Use max only for exceptionally difficult problems."
xhigh_effort_description = "Extensive reasoning for exceptionally difficult problems, deep investigations, and demanding long-horizon planning."
max_effort_description = "Exceptionally difficult problems requiring the maximum supported reasoning budget."
```

If configured, replace the built-in rubric with unchanged `rubric_instructions`. In both backends, prefix decision instructions with trusted baseline context, separate from evidence:

- Report separate `model_default_effort`, `task_baseline_effort`, and `current_effort`; use unspecified for absent values. Do not infer baseline provenance.
- Use the turn's configured effort as baseline. Refresh it on accepted manual effort/model changes, never adaptive selections. The model default is its normal operating point; the baseline indicates task difficulty. Weigh both with next-step evidence. Neither is a floor or ceiling. Current effort is state, not independent difficulty evidence.
- Add the next step's canonical model name and catalog description as JSON-quoted metadata. Trim descriptions to 1024 UTF-8 bytes at character boundaries. Use `unavailable` for missing, blank, or fallback metadata. Keep the decision-service model and warmup unchanged.
- For each allowed choice, send labeled `Model meaning` and `Selection guidance` sections. Trim catalog meaning to 1024 UTF-8 bytes; use `unavailable` for missing, blank, or fallback metadata. Preserve full configured guidance, capped at 4096 bytes. The API caps each combined description at 8192 UTF-8 bytes.
- The built-in rubric reassesses each next step. For equal reasoning demands, favor lower effort for more capable models and higher effort for less capable models. Treat baseline as guidance, not a ceiling. Enforce choices and runtime limits in code. Keep instructions/results outside the main conversation.

**Evidence.** Collect genuine user, assistant, and tool evidence, newest first. Exclude injected user instructions, compaction output, images, and encrypted reasoning. Scale scan limits with the budget: at least 128 history/input items and 32 content parts, up to 512 items and 128 parts at 32768 bytes. Older history can remain omitted; do not promise a complete transcript.

**P0 manual review:** combined choice descriptions can exceed 1000 tokens. Review both sections and their bounds. Meaning, guidance, and labels must fit the API cap above. Bound evidence and rubric separately. Split larger evidence into ordered OpenAI messages or TypeSafe state-array strings, each at most 8192 UTF-8 bytes. Prefer line boundaries. Preserve text and the default single-message request. TypeSafe uses a string state when one chunk suffices. The 4096-byte rubric cap excludes the bounded description/baseline prefix. Items can exceed 1000 tokens but must remain below the 10K-token item limit through their byte caps. Keep evidence outside the main conversation.

**Execution.** Ingest tasks and steering before skipped checks; invalidate stale decisions. Apply cadence, trigger, and recovery gates before bounded evidence rendering. Keep the original deadline; finish rendering before client setup.

- At eligible session start, send one tiny synthetic background warmup with separate minimal instructions. Skip it without triggers or supported effort within bounds. Reuse its pooled client. Discard its answer; keep history/settings unchanged. Limit it to three seconds. At shutdown, abort and join it. Do not retry, ping periodically, or add warmup settings. It can add one billable request; backend/connection warmth is not guaranteed.
- Before each selected main step with an enabled trigger, collect current evidence. Await Decisions and validation in the turn task, without a separate job or completion channel. Capture immutable step settings, then run the model. Do not retry or wait for a busy settings-publication permit. Only warmup and log writing run in the background.
- Publish current results through live next-step settings within the original deadline. Do not change captured requests. New accepted user/assistant/tool evidence invalidates pending decisions, not applied settings. Reject results after cancellation, task replacement, shutdown, settings changes, or deadline expiry.
- Reuse live validation/publication. Preserve the request-effort pin, trusted append-only updates, reconnects, and explicit compaction handling.
- In shared history-to-request projection, exclude effort updates without harness provenance. After filtering, retain the last complete envelope in each adjacent update run. Keep retained messages, reasoning, tool declarations, and results as separators. Do not change stored history or the request-effort pin. Apply this to filtered forks/replayed checkpoints without transport workarounds. A changed tail can lose prefix/cache reuse. Let incremental checks choose full input; do not force a session reset.

**Mode and display.** Manual model/effort changes invalidate pending decisions but retain adaptation mode. Refresh baseline on effort changes and active-turn baseline on persistent thread settings. Keep captured requests and the live-step contract unchanged. Unsupported models skip adaptation without disabling it. Log mode transitions. Ordinary turn-start/spawn settings do not disable it. `/system1` toggles only the current runtime, not saved defaults; keep mode separate from captured settings.

Spawned children copy parent mode once, not later toggles or parent errors. Preserve child mode across live eviction/reload, not backend restart. Internal workers remain unchanged. For encrypted assignments without readable tasks, log fallback and retain effort; do not forward ciphertext or guess tasks.

Report mode/revision at attachment and transitions; ignore older revisions. Report errors through backend runtime state, independently of diagnostics. Include health at attachment and live child reload. Show `<effort>·auto` when enabled without an error, including startup/decision waits; show `<effort>·err` after errors, without disabling adaptation. When off, show effort alone. Before the first captured step, use baseline. Markers show health, not promised changes. Unsupported steps retain effort without Decisions. Before eligible main requests, send captured effort through `turn/reasoningEffortUpdated`, including fallbacks. Toggles do not change running steps.

Keep captured effort/turn identity in live TUI snapshots, outside saved history. Restore on view switches without stale history settings. Clear on a new turn or manual change. Send transient effort notifications directly to clients, without rollout persistence before the main request.

**Status.** `/system1 status` reads current mode, selected/last captured effort, health, and session-fixed connection identity. Show backend/protocol, model, captured profile name or legacy config, and client initialization. Do not present reloaded config as the active connection. Before resolution, show unresolved identity. Initialization does not prove a successful request. Do not change settings, send a turn, resolve profiles, inspect credentials, or start warmup.

Read fixed-size in-memory totals only for the loaded runtime. Exclude warmup, cadence/backoff skips, and incomplete tasks. Accepted includes unchanged decisions; effort changes count only applied changes. Count client setup failures separately. Show total/last completed-decision wall time, not estimated net overhead, cost, or savings. Totals reset on unload/restart; children have separate totals. Do not scan rollouts, persist totals, add per-step notifications, expose credentials/endpoints, or resume unloaded threads. Older servers show unavailable stats.

**Errors and logs.** Use yellow warnings with error category, fallback effort, and cooldown duration or recovery-blocked state. Deduplicate by failure episode; warn on category changes, not identical errors. Do not invent cooldowns for old/restored events. Use bounded send/body-read codes and typed HTTP classifications where available. Preserve legacy transport events. Do not expose raw errors, URLs, or credentials. Identify warmup timeouts and their deadline visibly. At request start, trace backend, model, elapsed time, and remaining deadline, without content/endpoints.

With `diagnostic_log_capture = true`, append sanitized attempt metadata to per-session `decision.jsonl`: credential failures, API outcomes, and acceptance outcomes. Do not log evidence, rubric, bodies, or authentication headers. With capture off, create no file/writer. Bound buffers/storage; never wait for disk on the decision path. Report dropped records/writer failures separately. Crashes can leave attempts without end records.

Report latency/fallbacks without sensitive input. Deadlines bound result eligibility/local publication, not API latency. Preserved prefixes do not guarantee cache hits. Verify live update order and backend acceptance with real binaries under isolation/cost limits before making claims.

#### Space decision calls by turn and step

A turn is a request and its resulting work. A step is one ordinary model request, not one tool call. Defaults select every turn/eligible step. Select turns 1, 1 + N, 1 + 2N, ... . Reset step numbering and accepted-decision spacing each turn, not recovery.

- In selected turns, the first eligible step is due. After a current accepted decision at step S, the next due step is S + N. Include unchanged-effort acceptance. With `decision_every_n_steps = 2`, decide before steps 1, 3, 5, ... when eligible and successful.
- Due steps must pass `update_on` and eligibility checks. Ineligible, failed, cancelled, or superseded decisions do not advance spacing. Retry at a later eligible step when recovery permits, with fresh bounded evidence.
- Retain effective effort between decisions and across skipped turns, except after current main-step decision failure. Then restore the latest trusted configured/manual baseline for the next request and during cooldown/blocked recovery. Use the model default if baseline is unset. Never substitute an adaptive selection for baseline. Role bounds restrict choices, not manual/configured fallback. Preserve effort for unsupported models/efforts, Ultra/Persistent, cancellation, and warmup-only failures. Never stop the main step because Decisions failed.
- Keep per-agent recovery across turns. Transient failures start a one-second cooldown; consecutive failures double it, capped at 30 seconds. After expiry, probe on the next due eligible step. Cooldown skips do not extend it. Do not sleep the main turn or add background recovery timers.
- Missing/invalid credentials, invalid requests, rejected settings, and HTTP 4xx except 408/429 block Decisions until explicit off/on. Off/on resets recovery and makes the next eligible step due, even in a normally skipped turn. It does not clear health or reload initialized credentials; changing cached credentials requires restart. Missing credentials are not cached: set them in the backend environment and retry off/on. HTTP credential failures retry with the same client.
- Only a current main decision that passes acceptance guards clears health and resets recovery, including unchanged effort. Warmup errors affect health but never arm main-step fallback. Warmup timeout leaves failure count/cooldown/blocked state unchanged; other warmup errors use normal recovery. Warmup success, cancellation, and supersession neither clear health nor anchor spacing.
- Children inherit cadence config, not counters/recovery. Count ordinary turns at start, before setup, including setup failure/cancellation. Continuing a turn does not count again. Exclude warmup, compaction, and transport retries from ordinary steps/separate turns.
- Counters/recovery are runtime-only. Resume, restart, and eviction reset turn numbering to 1; do not rewrite history. Larger intervals reduce decision frequency, not necessarily runtime. They can retain unsuitable effort; do not claim speedups without measurement.

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
