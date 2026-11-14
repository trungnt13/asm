# Personal fork guide

## Principles

- This fork is for Trung Ngo's personal use only.
- Keep changes minimal, safe, and minor. Prefer durable local fixes with little upstream drift or conflict. Preserve correctness, maintainability, and runtime performance.
- Assume upstream tested unchanged code. Check fork changes, conflicts, and packages narrowly; upstream tests do not prove them.
- Record current fork purposes and constraints here, not task history. Ask before contradicting intent. After approval, replace the rule; resolve mechanical conflicts without further approval.

This guide overrides conflicting instructions, including [AGENTS.md](../AGENTS.md). Its validation and platform limits replace upstream defaults. Other coding, formatting, test-authoring, and generator rules still apply. Recorded intent does not prove implementation, integration, or publication.

The linked task skills and their procedures are required parts of this guide for applicable tasks. Read them before those tasks. This guide controls conflicts. Skill use grants no new authority.

## Development workflow

### Start or resume

- Before edits or after resume, refresh Git status, branches, and worktrees. Before syncs or pushes, inspect remotes and fork history. For overlaps or unexplained changes, inspect relevant repo sessions. Recover context from Git and sessions, not owner memory or task diaries.
- Before edits or syncs, create a named `codex/` task branch. Use separate worktrees for concurrent tasks. Coordinate shared files; assign one staging/commit owner per checkout. Worktrees are not deliverables.
- Prefer reusable checkouts: `python3 scripts/dev-worktree.py acquire codex/<task>`. Keep the path through builds and validation. Each slot has an exclusive owner/branch lease. The owner defaults to `CODEX_THREAD_ID`; if unset, use `--owner <id>`. Use `--slot <name>` for parallel tasks.
- While the PR is pending, keep its checkout and lease. After approved integration includes its branch tip in clean local `main`, run `python3 scripts/dev-worktree.py release codex/<task>`. Keep the detached checkout for reuse. Never clear another owner's lease or discard work. Inspect and coordinate abandoned leases.
- Preserve existing work. If blocked, use another worktree. Get permission before committing or stashing unrelated work. Keep authorized stashes until restoration is verified. Never overwrite others' work to clean a checkout.

### Build the binary first

Before local builds or binary handoffs, use [asm-build](../.agents/skills/asm-build/SKILL.md). Keep the CLI/helper pair from matching source, target, and profile. Preserve copied-pair identity and require permission before installed-binary replacement.

For runtime/UI edits, inspect affected paths, including streaming and completed output. Make the smallest coherent change.

- Before Cargo, verify disk space. Rebase/release maintenance may remove verified inactive local build caches under existing path, lock, and process checks. Record measurements and removals. Preserve active/uncertain artifacts, source, user state, and verified binaries. If safe cleanup cannot provide space, ask.

The local Cargo wrapper must:

- Set `RUSTC_WORKSPACE_WRAPPER` to the checkout's absolute `scripts/local-rustc-workspace.sh`. Isolate workspace artifacts by checkout while sharing registry dependencies. Do not treat another checkout's old code as fresh. Keep paths stable for warm builds.
- Preserve explicit wrapper overrides, including disabling. Unless an override isolates checkouts, use a separate target directory.
- Give Clippy a checkout-private cache, defaulting to main's `clippy/<checkout-path-hash>` subdirectory; Clippy replaces the wrapper. Explicit Clippy target directories must also be private.
- Lock build/check/Clippy/nextest per effective cache. Save inputs and fingerprint reasons under main's `.agents/dev-build-cache/<date>/`. Preserve explicit `CARGO_LOG` filters. Inspect logs before blaming cache loss.
- Clean stale artifacts only on Linux with Python 3.11+, complete process visibility, and no cache users or Rust build/editor processes. Hold each profile's native Cargo lock. Use descriptor-relative traversal; reject symlink directories and artifacts outside the physical cache. Log removal counts and skip reasons. Skip other hosts or uncertain activity.
- Keep the two-day cleanup rule: `incremental` directory modification time, or latest `deps` file access/modification time. Age does not establish checkout inactivity; removal can force active checkouts to rebuild.
- Block reusable-checkout release/switching during active wrapper commands. Direct Cargo bypasses protection; finish it before releasing the checkout.
- Keep interactive `cargo run` I/O without holding the cache lock for the live app. Leave CI and Windows unchanged.

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

Before real-provider validation, use [asm-live-check](../.agents/skills/asm-live-check/SKILL.md). Follow its isolation, evidence, and cleanup procedure.

- Every PR needs a successful real-provider live check before review or merge readiness. Select the smallest changed-behavior/fork-interaction scenario; state the expected result. Builds, offline checks, and mocks are not substitutes. Missing credentials, budget, or compatible setup block readiness. Do not replay unrelated upstream features or whole sessions. Release smoke remains separately required.
- Before calls, set request/turn counts and a time limit. Minimize input/requested output; set supported token limits. Stop at proof or first failure, then inspect evidence. Do not default to broad replays, model matrices, benchmarks, or repeated cold builds.
- Use existing sign-in only as authorized. No separate spend approval is needed when the conservative estimate for the entire live check is strictly below US$1.00. Record the estimate before making calls, using current provider pricing and the planned request and token limits, including required runtime helper calls. If the cost cannot be estimated or the estimate is US$1.00 or more, get approval for the endpoint, model, request/token limits, and spend cap before making calls.
- Do not trigger unrelated billable retries, warmups, Decisions calls, or extra subagents. Budget required runtime helpers; preserve release defaults. Record calls, elapsed time, and reported tokens. Report cost only when known; otherwise mark unknown. Missing prerequisites mean blocked, not verified.

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
- An explicit upstream rebase request also authorizes merging its sync PR after required checks. Do not ask again. Other PRs, including features, require explicit merge approval.
- Within the task, allow `--force-with-lease` on fork branches, including `main`, without separate approval. Plain `--force`, tag moves, discarded work, and releases still require approval.
- An explicit release request authorizes existing-workflow candidate publication, verified remote `main` update, and safe local fast-forward. Source fixes require PRs.
- Push explicit refs with `--no-follow-tags`, never all tags. Release-triggering tags, including inherited ones, require publication approval.
- Before lease-protected pushes, fetch and preserve remote-only work, including workflow release commits. Use an explicit lease against the inspected tip. Never refresh a failed lease and retry blindly.

## Upstream sync

Before syncs or targeted ports, use [asm-upstream-sync](../.agents/skills/asm-upstream-sync/SKILL.md). Follow its conflict-reuse and evidence procedure.

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

Stock `rust-v0.168.0-alpha.7` returns an empty cold child timeline with canonical rollouts but no derived history database. Resume builds the projection and exposes messages. This missing catch-up persists through `05cf8eeb14`. Independent stock `rust-v0.169.0-alpha.6` also returned an empty first cold timeline for valid real history; resume restored its projection and saved permissions. ASM already rebuilds before querying; this sync did not introduce the difference. Isolated checks found no history loss.

Independent stock Linux `rust-v0.169.0-alpha.7` also reproduces the limitation: cold search finds the thread, but cold timeline, turn, and item reads are empty. Resume restores the saved projection and permissions without changing the canonical rollout. The stock tag is a release commit off `6bd17ba6ae`, not a build of `e1cc96712f`. The history and wire code in thread-store, rollout, state, app-server, and app-server-protocol matches the sync target.

Through `b6d93b4cba`, the stock cold read paths still query the saved projection without catch-up. The independently tested alpha.7 reader has the same core cold-query path. This is source evidence for the newer target, not a runtime result for alpha.8. The stock cold timeline, turn and item implementations remain unchanged through `37103d7f01`; attached-thread search and fork previews do not add cold catch-up for those reads. Through `627b579219`, experimental `thread/sample` uses a separate path and the ordinary cold readers remain unchanged.

Independent stock Linux `rust-v0.169.0-alpha.9` also finds old real history through cold search but returns no cold timeline, turns, or items. Resume restores exact saved history and permissions without changing canonical bytes. Through `5228d94a27`, the ordinary stock cold-reader paths still lack catch-up. The owner accepted this limitation for this sync through `5228d94a27b7f84cae4854acbb5ea3b3e90dfeaf` only.

The owner accepted this limitation for the sync through `05cf8eeb14`, for PR #179 through `e1cc96712f9ad4f5404fe821b9eba7ff29598816`, for PR #180 through `b6d93b4cbadd6f449395d326f72465a5823f1492`, for PR #181 through `37103d7f01e82162472c6fba7b4088d5a1303321`, and for PR #182 through `627b579219d492b5445d6d433d3110eb53e507dc`. The owner also accepted the unchanged stock cold-history limitation through `392ced7d3a9160fd35d40c606359869215f50522` for this sync only. For the sync through `5c3ff1dabf644169f4bd74b215719f381e4fdb3f`, independent stock Linux `rust-v0.169.0-alpha.11` again finds both old and fresh real history through cold search but returns no cold timeline, turns, or items. Resume-assisted and subsequently reopened warm reads restore exact history without changing canonical bytes. This observation is not acceptance for this target. The ordinary stock cold-reader source remains unchanged from `5c3ff1dabf` through `72532ecae5efe500d9e722f3ed5214e63cc8098a`; this is source evidence, not a runtime build of that target. For PR #189, the owner accepted this unchanged cold-history limitation through `72532ecae5efe500d9e722f3ed5214e63cc8098a` only. A fresh real rollout written by the current candidate also passes ASM cold search/timeline/turn/item reads. Independent stock alpha.11 finds it through cold search but returns empty cold timeline/turn/items; resume and reopened warm reads restore the exact two saved items, one turn and saved permissions without changing canonical bytes. This exception does not waive other validation failures. These decisions do not cover later upstream commits or waive any other checks. Keep cold-stock reading unresolved, not passed, and report resume-assisted handoff separately. Do not change personal history or caches.

### Live backend model catalog

Every upstream rebase/sync must fetch the live authenticated backend catalog. Source, cached, and bundled catalogs cannot establish current availability; backend models can change without source changes.

Fork (`origin`): [`trungnt13/asm`](https://github.com/trungnt13/asm). Parent (`upstream`): [`openai/codex`](https://github.com/openai/codex).

Last incorporated commit: `05229d1874d4e04bc746a9954c71931a1ab852c6`, 2026-11-14. Subject: Add snapshot pagination for persisted realtime history (#61480).

- Before fetching, verify the upstream URL. Sync on a task branch and submit a PR. Use the requested method. Otherwise, merge to preserve published history; rebase only unpublished branches. Follow push permissions for authorized rewrites. Use `git cherry-pick -x` for targeted ports.

## Platforms and CI

Focus on macOS and Linux (Ubuntu). Do not add other platforms to fork CI/releases. Preserve other inherited code/workflows unless removal is requested. Other inherited workflow changes need approval. Inspect triggers: [V8 canary](../.github/workflows/v8-canary.yml) runs on PRs; expensive builds depend on relevant changes. [CLA](../.github/workflows/cla.yml) restricts its job to `openai`.

Use standard GitHub-hosted runners; paid larger runners need explicit approval. V8 canary Apple Silicon variants use `macos-15` and two Bazel jobs for runner memory.

- [Blocking CI](../.github/workflows/blocking-ci.yml) and [postmerge CI](../.github/workflows/postmerge-ci.yml) are manual-only `workflow_dispatch`, never automatic or required.
- Blocking CI keeps workspace formatting, production Clippy, and result collection. Cover `codex-cli`, `codex-tui`, `codex-core`, and `codex-config` on both release targets. Preserve `--lib --bin codex -- -D warnings`. No test-target matrix, `cargo shear`, or warning suppression. Check other affected packages narrowly.
- Postmerge CI keeps optimized builds, packaging, smoke checks, uploads, diagnostics, and results for targeted cross-platform checks. Do not run it for every merge. Releases retain their own build/package checks; publication does not require blocking CI. Report existing failures separately.
- Port useful upstream action/toolchain/security/build fixes without broad matrices or routine local CI repetition. Exclude unrelated platforms, Bazel suites, SDKs, remote executors, V8 source-build canaries, and OpenAI-only infrastructure. The required two-target V8 build is not a broad suite/canary.

## Releases

### Operate a release

Before release operations, use [asm-release](../.agents/skills/asm-release/SKILL.md). Require an explicit release request. Publication grants no installation or shared-server-restart authority. The procedure covers preparation, CI recovery, live smoke, dispatch, and finishing.

6. **Recover:** after interruption, rerun failed jobs. For a new resume dispatch, use `main`, `publish_release=true`, and `resume_run_id=<original run ID>`. Preserve original version/commit despite upstream advances. Never invent versions or replace tags. Report failures needing owner decisions. Only with explicit approval, supersede a source-defective failed unpublished candidate with a new immutable candidate. Record old run/source/tag and draft assets in `.github/scripts/superseded-fork-releases.json`. Preparation must verify that record and reject publication or asset drift. Preserve old candidate/tag/assets and occupied version. Never apply this exception to published or partially ready releases. The owner separately approved superseding cancelled run `42465703649` on 2026-11-09 to release current main. Preserve draft `v0.169.1-alpha.3.1`, its tag, candidate and installer unchanged; verify its recorded cancelled state before creating the new candidate.
7. **Verify:** a platform is ready after its checked archive and platform checksum manifest are published. Candidate completion requires `release`, both `verify_native` jobs, and `sync_main` success for that candidate. Legacy `v*` pushes/dispatches require publication and both native checks, not `sync_main`. Inspect recorded checks/current metadata; do not repeat successful checks on unchanged artifacts. Builds, tag pushes, and version labels do not prove publication.

### Release notes

For release notes, use the notes section of [asm-release](../.agents/skills/asm-release/SKILL.md).

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

For V8 dependency preparation or publication, use [asm-v8-release](../.agents/skills/asm-v8-release/SKILL.md). Its procedure defines workflow dispatch and tag identity. Keep the contracts below.

- Reuse unchanged verified inputs. Same-version archive reuse for Cargo rebases/releases has standing approval only when independent review proves Cargo ABI, bindings, and full dependency-closure compatibility. Native consumer build/JavaScript/sandbox checks must pass. Record omitted source-recipe fixes; never claim source equivalence. Ask for incompatible dependencies/bindings, changed baselines, unsafe reuse, or ambiguous intent. Never replace published assets.
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
- V2 `spawn_agent` requires an explicit `fork_turns` string. Document `"none"` and `"all"` as the supported choices. Preserve upstream compatibility: legacy positive integer strings still select full history.
- Cross-provider subagents require `fork_turns="none"`. Reject history reuse; preserve saved provider identity on resume. Bound models use their own provider-specific model manager, not parent discovery identity.
- For user-spawned children with a catalog-bound provider ID, require an explicitly bound model or native runtime, including descendants/cold resumes. Reject unbound selections before startup; do not infer reverse routing. Use a separate provider ID for unbound custom subagent models. Preserve ordinary main/internal-worker provider behavior.
- Keep upstream catalog/API model types unchanged. Hiding bundled entries does not disable explicit selection.

Standalone search may select independent OpenAI authentication in any provider definition:
`[model_providers.fireworks]` with `standalone_web_search_provider = "openai"`. Resolve
this setting from the selected model's current provider, including external catalogue bindings
and model switches. A matching override enables the existing standalone path without a global
feature flag; disabled search, provider capabilities, and MCP/backend exclusions still apply.
Reuse the upstream search extension, native defaults, and existing tool-output/history path;
do not silently fall back between services. Separate search sends its usual recent context
to the selected OpenAI backend. A matched non-OpenAI provider exposes the same executor as
ordinary `web_run` because namespaced Fireworks calls did not produce dispatchable function
calls in the live check; native OpenAI retains `web.run`.

For Fireworks, defer MCP/app and desktop dynamic tools in the harness, discover them
through an ordinary function call, and expose only selected tools on subsequent requests.
Preserve explicitly direct-only namespaces. Keep discovery in the harness: do not send native
tool-search items or undiscovered schemas to Fireworks. Preserve native tool search for
providers that support it and resolve discovered tools against current permissions on resume.


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
- Every Claude session, including main, Side, and children, uses `--dangerously-skip-permissions` and `--permission-prompts none`. Preserve native denies, hooks, and managed policy. Remaining prompts fail explicitly; never grant protocol approvals. Configured native hooks stay native.

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

**ASM-only Claude launch configuration**

`[native_runtimes.claude_code]` applies only to Claude processes launched by ASM,
including roots, children, Side and cold resume. Unset controls preserve native defaults.
Never modify Claude user/project settings, skill files, installation or authentication.
Native managed policy and ASM's mandatory child/Side restrictions remain authoritative.

- `disallowed_tools` adds native tool exclusions, including `Agent(name)` selectors.
- `skill_overrides` maps native skill names to `on`, `name-only`, `user-invocable-only`
  or `off`. Pass these through one native `--settings` overlay, merged with ASM's
  existing launch settings. Native Claude owns name resolution and plugin limitations.
- `page_context` is `inherit` (default), `when_present` or `off`. Filter only queued
  Page snapshot/instruction blocks, never actual user messages or unrelated context.
  `when_present` omits Page context only when the queued snapshots explicitly show no
  Page and native history has no earlier Page context. Preserve clearing snapshots after
  a Page was forwarded; missing or malformed snapshots preserve context. Cursor and saved
  history are unchanged.
- Capture launch settings per process; config edits do not restart a live process.
  Page filtering remains fixed while loaded. Cold launch/resume uses current config;
  previously saved native prompt snapshots may still retain earlier context.

Example (opt-in, not installed defaults):

```toml
[native_runtimes.claude_code]
disallowed_tools = ["ReportFindings", "Agent(statusline-setup)"]
page_context = "when_present"

[native_runtimes.claude_code.skill_overrides]
"claude-api" = "off"
"plugin-authoring" = "off"
"anthropic-skills:built-in-browser" = "off"
"anthropic-skills:chrome-browser" = "off"
"anthropic-skills:computer-use" = "off"
"anthropic-skills:google-workspace" = "off"
"anthropic-skills:morning" = "off"
"anthropic-skills:skill-creator" = "off"
```

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

`asm resume <text>` matches trimmed, lowercase-normalized substrings of displayed names, unnamed-session previews, or UUID prefixes. Search eligible active sessions across working directories under existing provider/source rules. Resume a unique match. For multiple matches, restrict the existing picker to matching IDs. Keep direct full-UUID lookup. Do not prefer exact names over partial matches. Keep exact matching and duplicate rejection for fork, archive/delete, and queue commands.

### TUI automatic session names

Keep `[auto_rename]` TUI-only. Preserve Desktop naming, conversation model/history, and `/rename` suggestions. Defaults: enabled, first user message, user-message context, initial naming only, 36 characters, 960-byte initial prompt.

Use `gpt-6-luna`/low only for OpenAI with a ChatGPT account and an available catalog entry. Otherwise use the conversation model. Keep upstream model selection for manual `/rename`.

- `enabled`: default true. `model`/`reasoning_effort` affect titles only; omissions keep defaults. Preserve explicit models server-side. Fail unavailable models without fallback. Custom-provider models need no catalog entry.
- `first_trigger`: `first_user_message` (default) or `first_completed_turn`. Exclude failed/interrupted completions.
- `context`: `user_message` (default) or `recent_conversation`. Recent context includes the initial request and substantive user/assistant messages, not commentary/tools.
- `max_context_bytes`: 128–8192 source bytes, excluding added guidance. Omission keeps the original budget.
- `recent_message_limit`: 1–32, default 8, including the initial request; 1 retains only the latest message. Under byte pressure, drop earlier messages and keep markup complete.
- `max_title_chars`: 1–128, default 36. Optional positive integer `max_title_words` is prompt-only: request at most that count. Omission keeps “under five words where possible.” Never truncate by words. Bind prompt/character limits to their originating request.
- `additional_naming_guidance`: append at most 512 UTF-8 bytes; preserve fixed rules/prompt. No `instructions` alias. Cap the complete prompt at 9500 bytes. **P0 manual review:** larger opt-in budgets can exceed 1000 tokens. Keep the 10K-token item limit.
- `auto_update`: default false. Refresh requires both `enabled` and `auto_update`. Include the captured previous title as bounded JSON-string data. Request changes only for meaningful task changes, preserving the core subject and wording. Routine progress is insufficient. Keep initial/manual prompts unchanged and fit guidance without splitting markup.
- Save each generated title in one client-local `CODEX_HOME` record, scoped to the local/remote app server. Eligibility requires a record matching the saved name. Protect named sessions with missing/corrupt/mismatched records; `/autorename` can opt in. Save initial automatic titles even when updates are disabled, for later resume. Keep counters across view switches. Resume resets the interval and excludes completed turns.
- `auto_update_interval_turns`: positive integer, default 5. Count successful displayed user-turn completions, not duplicate/failed/interrupted/background/helper/replay events. Start at most one attempt per interval. Reset on attempt start, not success. Failures keep ownership and wait another interval. Count while requests are pending; start none until settlement.
- Track unnamed opt-in threads before initial requests; ownership requires a successful save. Retain only the latest user-turn/item IDs, one ordering anchor, and at most 8192 UTF-8 user bytes. Preserve context through event-buffer eviction during long turns or pending initial naming. For first-user-message naming saved before completion, count that completion. For completed-turn initial naming, count later completions.

`/autorename` immediately generates/applies one title with the same context/model/effort/budgets/limits/guidance. Ignore `enabled`, `first_trigger`, `auto_update`, and interval only for this explicit action. Later refresh obeys them.

- Use the latest substantive user text or shared bounded recent collector. Allow replacement of existing/resumed names. Preserve `/rename` suggestions/supplied names.
- Supersede pending background naming. Success restores persisted eligibility, even for an identical title. Failure preserves name/eligibility. Deduplicate and report explicit failures. Discard results for undisplayed threads or changed saved names. Reuse app-server APIs; do not change Desktop.

Naming safety:

- Preserve background requests, manual-name precedence, cancellation, and originating-thread checks. Before replacement, compare the current server name with the captured name. Identical generated names need no RPC.
- Before RPC, remove eligibility for `/rename`, dashboard naming, or explicit `set_thread_title`, including same-name requests. Abort on removal failure. A later RPC failure can leave refresh disabled.
- Generic cancellation clears only in-memory ownership. External name changes cancel automatic work.
- Cap records at 512 UTF-8 bytes/128 characters. Validate reads and write atomically. Read only at attachment or recurring attempt/save, not streaming/rendering.
- Before refresh, recheck persisted eligibility for another TUI's opt-out. Fail closed on corrupt/unreadable records; report persistence errors.
- File/server writes are not jointly atomic. Same-name external renames and concurrent reads/writes remain best-effort. Do not add shared APIs, rollout records, or Desktop policy to solve them.

### Account-security setup reminder

Do not fetch/display optional Daybreak setup reminders, including reconnect/account refresh. Suppress loaded events. Preserve authentication, security enforcement, approvals, account email, backend usage banners, and upstream reminder rendering/checks. Never weaken security.

### Content-provenance wire compatibility

Keep upstream per-content provenance in local history, prompt assembly, and compaction. Until deployed endpoint support is established, omit only `content_item_metadata` from shared ordinary Responses HTTP/WebSocket request copies, including Responses Lite. Keep other internal metadata and the original prompt. Responses Lite does not guarantee capability. This constraint withholds server-side per-part attribution. It does not enable full wire provenance.

### Standalone request audit

- Keep `debug prompt-input`. Export versioned JSON with `debug prompt-request`: effective base instructions, production request, ordered input, full advertised tool definitions, Responses Lite prefixes, and optional output schema.
- Reuse the app-server extension installer/request builder. Do no audit work in ordinary inference paths.
- Default to ephemeral. Require `--allow-session-state` for session records/persistent state-backed tools. Setup can opt in within isolated temporary homes. Never dispatch queued input.
- Label this a fresh standalone debug turn, not a live-session/exact transport capture. Report omitted history, hooks, input-triggered skill/plugin injections, uncaptured tools, auth-dependent metadata, and server additions.
- Never send inference or claim startup has no network/state effects. Exports can contain sensitive instructions/tool content.

### Model catalog ownership

- Keep [`codex-rs/models-manager/models.json`](../codex-rs/models-manager/models.json) upstream-owned. Accept upstream sync changes. Do not freeze it or make personal/fork edits.
- Put personal edits only in `/home/trungnt13/codes/tngo-workflow/configs/model-catalogs/models.json`. Never copy them into the built-in catalog.

### Role-scoped skill visibility

Optional `allowed_agent_roles` in `[[skills.config]]` restricts normal catalog visibility and skill selection to listed child roles. Example: `name = "review"`, `enabled = true`, `allowed_agent_roles = ["reviewer"]`.

- Match the current child's explicit role, not an ancestor or displayed TUI view. Omission keeps existing visibility; `[]` hides matching skills from all roles.
- Root, ordinary Side, saved user forks, and unnamed/nonmatching children do not match. Reviewer children match in fresh or V2 full-history context; resume restores their role. Preserve V1 full-history restrictions.
- Keep parent enablement and permissions. Role overrides are deny-only and cannot re-enable disabled skills. Scope restrictions also survive enables using another matching selector.
- Share local snapshots for identical effective visibility, without task/thread/raw-role cache keys. Apply scope in config-aware and cwd caches. Keep prompt lists stable, append-only availability updates, and existing context budgets.
- This controls visibility, not access. Preserve filesystem/script/sandbox/network/approval permissions. History can retain old lists or read skill content; never scrub it.
- Configure `review` and `oracle` only after using a supporting binary. Keep reviewer descriptions generic so root's role list does not disclose its skills.

### Role configuration restrictions

Role TOML can explicitly disable web search with `web_search = "disabled"` or System1 with `[system1] enabled = false`. Apply only explicit disabling. Omission keeps inheritance, including the parent's live System1 toggle. Roles cannot enable either against parent restrictions. Keep System1 restrictions through child spawn/reload.

Roles can independently override `[system1] min_effort` and `max_effort`; omitted bounds inherit. Validate the merged ordinary-effort range. Intersect it with model-supported efforts; fewer than two choices skips Decisions. Keep role bounds through model changes/reload. New children resolve their own role, without ancestor role bounds; V1 full-history forks retain their role bounds. Other System1 settings inherit. Bounds constrain decision choices, not manual effort or fallback.

After a supporting release is published and installed, set the personal reviewer to `min_effort = "high"`, `max_effort = "xhigh"`. Do not change it earlier. Align the projected role layer with runtime settings. Do not add arbitrary role fields or change session authority.

### Model catalog instruction visibility

Top-level `include_model_catalog_in_spawn_agent_list` defaults to `true`.

- False removes generated model/effort/tier lists and references from V1/V2 spawn descriptions and `<model_catalog>`. Preserve custom descriptions, arguments, loading, validation, and history. Use normal invalidation when retained lists cease to apply.
- Keep inclusion independent of `model_catalog_in_context` placement and `expose_spawn_agent_model_overrides` arguments.

`load_external_models_json` controls compact exact-alias discovery independently of full-catalog visibility and placement. True (default) includes it in V1/V2 spawn descriptions; false omits it.

- List only picker-visible, backend-compatible entries with bundled aliases: native runtimes or configured API providers allowed by managed requirements. Discovery proves no runtime, credentials, or service availability.
- Hide the list when V2 model override arguments are disabled. Include cross-provider fresh-history guidance: V1 `fork_context=false`, V2 `fork_turns="none"`.
- Do not change personal configuration automatically. Identify truncated model previews as partial in unknown-model errors.

### MCP resource tools

Top-level `mcp_resource_tools_enabled` defaults to `true`. False removes `list_mcp_resources`, `list_mcp_resource_templates`, and `read_mcp_resource` registration, calls, and declarations from direct tools, Code Mode, and `ALL_TOOLS`. True keeps existing registration conditions. Keep MCP connections, other server tools, apps/plugins, and the shared TypeScript preamble independent. Apply at config load; do not add live reload or rewrite history.

### Web search copyright instruction visibility

Top-level `include_web_search_copyright_compliance` defaults to `true`. False removes only terminal Copyright compliance from local `web.run` descriptions. Preserve preceding quotation/word limits, arguments, execution, search modes, history, hosted `web_search` instructions, and model safeguards. If the subsection changes or is not terminal, warn and keep the full description. Apply to standalone cached/indexed/live modes.

For all three visibility/tool settings above, wait for a supporting CLI release before updating setup-managed profiles.

### Saved forks and upstream sides

- Stack-budget exception to async instrumentation: leave `start_companion_conversation` and `select_agent_thread_and_discard_side` uninstrumented. Their tracing wrappers duplicate large futures in unoptimized poll frames and exhaust the unchanged 16 MiB UI stack. Keep companion event-dispatch futures boxed; do not increase stack limits. Construct startup and run-loop event futures through a separate non-inlined closure boundary at the existing allocation sites, so their construction temporaries do not remain in the run-loop poll frame. Do not add a nested selection allocation as a substitute.

- Saved/temporary root forks keep the parent's effective prompt-cache routing key independently of thread/session ID. Persist for resume/further forks. Old rollouts need no ancestor lookups. Keep upstream guardian/subagent routing. Shared routing does not guarantee cache hits.
- Optional top-level `side_model`, `side_reasoning_effort`, and `side_service_tier` (for example, `"fast"` or `"default"`) independently override parent settings for `/side` and `/btw` forks, including attached workspace Side chats and `/sync`. Unset values inherit current parent settings. Do not apply them to ordinary saved forks. Preserve native-runtime fork admission restrictions, including Claude’s same-model requirement.
- Optional top-level `side_prompt` adds default developer instructions to `/side` and `/btw` conversations, including attached workspace Side chats, `/sync`, and recognized Desktop side chats. For example: `side_prompt = "Act as a critical reviewer. Keep answers concise."` Unset or whitespace-only values preserve existing behavior. Preserve nonblank text and append it before the built-in side policy, without duplicating it on `/sync`. Forks also inject the custom developer message once beside their reference-only boundary because inherited context skips initial developer instructions; keep the configured prompt for compaction. Do not apply it to main or ordinary saved forks. It does not replace side boundaries, permissions, or capability restrictions.
- Remote Desktop side chats also use these settings as initial defaults, including attached forks and new side threads. Recognize only the complete shipped Desktop 26.1027.51404 developer block; unknown wording leaves ordinary behavior unchanged. Recognize the complete shipped side boundary at the start of its user message, either alone or followed by a paragraph separator. Treat appended reference paragraphs as opaque context, not a protocol to parse; changed boundary wording remains unrecognized. Do not change the signed Desktop bundle or API shapes. Synchronize effective settings after client context injection; correct only the first new side thread turn's known inherited settings, including its inherited null effort/tier. Explicit picker settings win per field, persist normally, and are never replaced on later turns or resume. Keep ordinary saved forks and prediction forks unchanged.
- `/side` and `/btw` use upstream storage-aware lifecycle: eligible workspace Side chats are saved and attached; unsupported storage and native runtime Side chats remain temporary. Preserve multiple Side chats across navigation, distinguish creating a new Side from reopening an existing one, and reopen saved chats through the chooser. Ctrl+/ switches; Ctrl+C archives attached chats through upstream confirmation and discards temporary chats. Only attached Side chats may be renamed. Preserve upstream tab controls, helper names, and tests where practical.
- Side has no extra question-only, non-mutating inspection, or explicit-mutation/escalation instruction gate; normal permissions and approvals still apply. Preserve reference-only inherited history, waiting for a new request, the subagent prohibition, and the storage-specific attachment restrictions in both boundary and developer instructions. Temporary Side cannot attach a worktree; attached Side uses upstream workspace attachment rules.
- In Side, `/sendlast [extra message]` forwards raw last completed assistant reply plus optional text as main user input. No prefix/navigation/settings change; atomically start or steer main. Reject complete items above 10,000 UTF-8 bytes, never truncate. Explicit item can exceed 1K tokens; require manual review when changed.
- `/sync` replaces idle Side with main's latest context, retaining model/effort/tier and using upstream storage eligibility. Prepare and select the replacement before closing the old tab. Discard old temporary history only after success; keep the old attached chat saved, without automatic archive or deletion. Keep storage-matched Side boundaries. Context refresh does not guarantee cache hits.
- In idle Side, `/fork [name]` saves an independent ordinary conversation:
  - No navigation, pairing changes, or automatic work. Copy the available inherited context and Side history. After paginated main compacts, do not recover older raw entries outside model context.
  - Persist current model/effort/tier through resume. Append durable ordinary-role context; never rewrite history.
  - Default `<main title> (fork)` or `Untitled (fork)`; keep explicit names.
  - Keep raw replay in memory only for temporary root forks. Normally append only new items; copy retained history only on explicit save. Temporary chats stay pathless/unlisted until saved.
- `/parallel` is removed. Existing saved parallel conversations remain ordinary threads with unchanged IDs, history, permissions, and reference-only boundaries. Do not rewrite history, restore pairs, attach counterpart threads, or delete saved conversations.
- Remove legacy `/parallel` display metadata readers and transcript trimming. Existing saved chats display their canonical ordinary history; do not migrate, rewrite, or clean up personal metadata files.
- Preserve upstream Side lifecycle, including attached workspace sides and tab controls. Keep temporary Side restrictions and the `/sendlast`, `/sync`, and non-navigating `/fork` exceptions above. Stock clients continue to see ordinary threads through unchanged APIs; files and explicit global config remain shared.

### Recap session ID

End manual/automatic TUI recaps, after any next action, with dimmed `Session: <id>`. Use the displayed conversation ID, not the generation thread. Preserve generation/timing.

### Turn completion footer

Use one dimmed logical line: `Worked for 5s • 11:05 AM • 12.4k in • 9.8k cc • 820|600 ou • 3ag • <session-id>`.

- Preserve timing, optional runtime metrics, and narrow wrapping. Use displayed conversation ID, not turn ID.
- Show provider-reported turn totals, including compaction. Cached input belongs to input; reasoning belongs to output. Omit unknown counts; do not substitute context estimates/session totals. Reuse saved usage. Paginated history can omit counts without migration.
- `Nag` counts distinct related subagents observed running during a live turn, including descendants/already-running agents. Keep counts across view switches. Omit restored counts without an observed start. Keep tokens parent-only and requests/accounting unchanged.

### Copy session ID

`/copyid` copies only the displayed session ID through the existing clipboard. Do not open `/status`, reply, or show a picker. Allow during tasks, sides, and parent-owned child views. Report missing IDs/clipboard failures. Preserve `/copy` and platforms.

### ASM branding

Use ASM for README heading, repo description, release titles/introduction, workflow step descriptions, fork TUI labels, and terminal title. Credit OpenAI Codex. Keep actual upstream product/service names.

Use ASM in local CLI help/TUI messages and `asm` in command hints, including resume, agents, login, and MCP setup. Hints require a user-provided alias/wrapper. Preserve executable/CLI parsing names, internal identifiers, model-visible prompts/parsed markers, upstream names, and installer commands. Keep the `codex` executable/version prefix and crate/config/state/archive/tag/workflow/job/check identifiers. Never redirect upstream links/installers for branding.

On Linux, set only `/proc/self/comm` for normal CLI/app-server/code-mode-host/exec-server to `asm`/`asm-app-server`/`asm-code-host`/`asm-exec-server`. Preserve filenames, argv, helper dispatch, package discovery, and protocol identity. Keep other platforms unchanged. Warn on label failure without blocking startup. Full-command viewers still show canonical filenames; external name-based monitoring must use new labels. UI plugin helpers share `asm-code-host`. CLI-dispatched sandbox/filesystem helpers retain upstream labels.

### Background terminal waits

Top-level `background_terminal_min_timeout`/`background_terminal_max_timeout` use milliseconds; defaults 5000/300000. Reject zero/reversed/unrepresentable bounds. Clamp empty `write_stdin` polls, show bounds in descriptions, and finish early on exit. Preserve initial/nonempty waits and Code Mode outer `exec`/`wait` limits. Exit alone does not resume idle model turns.

### Memory reasoning effort

Optional `[memories] extract_reasoning_effort`/`consolidation_reasoning_effort` use the existing effort type for V1/V2. Defaults: `low`/`medium`, independent of parent. Preserve models and other memory behavior.

### Compact Markdown links

- Render named web/resolvable local file links as OSC 8 labels without destination suffixes. Keep bare URLs visible. Do not require terminal detection.
- Resolve relative paths lexically against session cwd. Do not read the filesystem/frontend home. Preserve encoding/location fragments. Line opening depends on terminal/editor.
- Show unsupported/unresolved destinations and retain destinations in text-only output. Reuse hyperlink layout/output transport. Keep trusted artifact capabilities separate.

### Conversation switching
In `asm resume`, Esc clears a nonempty search or exits the picker when the search is empty, without starting a new session. Keep the startup `asm fork` fallback unchanged.

During owned fullscreen switches, retain the displayed frame/terminal diff buffer until destination replay completes. Draw the destination once. Continue background collection and ordered current-history replay. A parked screen is not authoritative state. Preserve native-scrollback clearing. No experimental workspace feature is required.

When Main runs a tool, side creation snapshots inherited history, not execution. Complete pending calls only in the fork's persisted interrupted suffix. Keep Main/ancestor records unchanged. Preserve cold resume; never execute/cancel Main's pending work.

### Transcript spacing

Keep Markdown paragraphs/code blocks/list items adjacent during streaming and completion. Preserve blanks within code blocks, raw output, message boundaries, and user padding.

### Click-to-quote

`[tui] click_to_quote = true` is opt-in, default `false`. Apply only to owned fullscreen transcripts; preserve native scrollback.

- Fresh selections copy normally. A composer click attaches selected text, not clipboard contents, as a frozen removable preview above the comment draft. It sends no model request.
- Clipboard-confirmed deselection retains pending quotes. Explicit deselection/navigation/thread changes clear pending selection. Do not expire it by time.
- Attach quotes to chat drafts and retain across switches. Whole-draft replacement installs its own quote state. Plain history navigation cannot overwrite quote-only drafts.
- Treat quoted references literally; do not auto-activate skills/apps. Keep normal resolution for unquoted comments and explicit structured mentions.
- Append selections in order; do not replace prior references. [×] removes only its reference; preview text does not remove it. With limited height, wheel-scroll the combined reference/image list and keep comments visible.
- Number previews/submitted blockquotes from 1. Keep global indices during scrolling; renumber after removal. Count numbering bytes toward size limits.
- Each attachment appends a numbered comment below existing composer text. Use grey “Add an optional comment…” as an unsubmitted hint. Keep pretyped general text unnumbered and prior comments intact. Remove only empty generated comment rows. Preserve custom/restricted-input hints.
- Plain Enter on an empty generated comment clears its number, leaves a general-message blank line, and does not submit. Preserve references/prior comments and modified-submit behavior.
- Track generated prefixes separately from user numbered lists. Escape generated comment numbers in submitted Markdown so omitted comments do not change displayed indices. Submit all references and only entered comments. On reference removal, renumber associated comments and retain entered text as ordinary draft text. Restore associations with drafts; external-editor changes can detach them rather than guess.
- Quote removal preserves the draft. Enter uses the existing pipeline with a Markdown blockquote and optional comment.
- Reject attachments when formatted references alone exceed 10,000 UTF-8 bytes; preserve existing references/comments. Do not limit reference count. Reject complete quote-plus-comment items above 10,000 UTF-8 bytes, including quoting/expanded pastes. Never truncate. Keep ordinary unquoted limits.
- Items can exceed 1K tokens; require manual review when changed. Do not add hidden instructions/API fields or rewrite history.

### Collapsed tool calls

`[tui] collapse_tool_calls = true` is opt-in, default `false`. In rich fullscreen:

- Collapse consecutive shell/MCP/dynamic calls, routine V2 interactions, normal agent starts/completions, and routine waits into one expandable summary.
- Count calls/starts/completions separately. Lifecycle notices are not calls/running tools. List distinct command/tool names or agent paths in first-use order.
- Count each successful child message/follow-up once. Count matching wait start/end previews once per contiguous displayed group. Completed waits cannot remain running. Keep result/failure/interrupted waits visible.
- Replace overflowing names with `+N more`. Positive integer `[tui] collapse_tool_calls_max_lines` defaults to `1`. Apply to new headers' counts/names; wrap larger budgets. Preserve captured reading snapshots on resize; they can wrap further. Ellipsize if counts alone cannot fit.
- Split groups on interruption, other activity, or chat messages. Keep active tools in separate short rows until committed history includes them.
- Expansion restores previews/order. Keep failures/approvals visible and preserve execution/model history/raw output/full transcript. Disabled retains existing rendering.

### Completed turn activity

- In rich owned transcripts, show commentary/tools live. At final-answer start, fold that turn's commentary/reasoning/successful tool previews under expandable `Worked for …`. Fold unphased legacy output at successful completion, excluding the last answer.
- Keep user messages/notices/failures/approvals/final answers visible. Leave interrupted turns without final answers unfolded. Preserve original cells in raw/full transcript, copy sources, execution/model history. Keep native scrollback unchanged.
- Reuse disclosure controls. Index appended cells once; rebuild only for structural history changes. Cap rendered fold layouts at 64 entries. Do not format hidden output.

### Compact status line

- Join with `·`, without spaces. Use `CtxN%`/`CtxN%left` for used/remaining context and `F:on`/`F:off` for fast mode. Show capacity as a compact token count without `window`, including `/statusline` previews.
- For model labels, lowercase, remove `gpt-`, and keep at most three letters, including aliases. Append all version digits without separators: `GPT-6-Astra` → `ast6`, `GPT-6.1-Sol` → `sol61`, `Terra 5.6` → `ter56`. Without digits, omit the suffix.
- Abbreviate effort to three letters, including model-with-reasoning: `medium` → `med`, `high` → `hig`, `xhigh` → `xhi`.
- Apply to footer, `/statusline` previews, and `/subagents` model info. Preserve IDs, other picker labels, titles, paths/roles/colors/order/single-row truncation. Remove role-leading space only in footer: `Main[default]`.
- Opt-in `cache-hit-rate` in `/statusline`/`tui.status_line` keeps defaults unchanged. Show the current thread's latest reported cached/input ratio as `CchN.N%`, rounded to one decimal. Reuse usage; no averages/totals/requests/APIs. Hide unknown/zero-input usage. Missing cache detail remains indistinguishable from reported zero.
- Opt-in `output-tps` keeps defaults unchanged. Show rounded whole-number `Ntps` for the latest successful ordinary Responses request/native Claude message. Divide reported `output_tokens - reasoning_output_tokens` by time from its first nonempty text/tool-argument delta to completion. Exclude reasoning deltas from the timing start. Require explicit reasoning-token breakdown; missing is not zero.
- Exclude failures, retry waits, compaction, warmup, tools, other threads, and initial latency. This measures observed streaming throughput, not pure decoding; buffering can affect it. For Claude, use matching root `message_delta` output/thinking usage and `message_stop`, not turn totals or native children. Cursor TPS is unavailable. Omit requests without generated deltas or with zero-millisecond duration.
- Calculate TPS once at live completion. Carry it on the existing usage event. Cache formatted text in the TUI; use existing refresh without extra notifications/redraw requests/timers/streaming estimates. Retain the latest valid result across failures. Omit unavailable timing/usage; do not reconstruct from replay.

### Continue through model capacity failures

Top-level `continue_when_model_at_capacity = true` enables unlimited `ServerOverloaded` retries for ordinary requests and foreground compaction. Children inherit it. False/omission keeps upstream behavior.

- Retry the same model without task restart or deliberate completed-tool replay. Keep ordinary retry budgets separate.
- Without server advice, wait 30, 60, 120, 240, then 300 seconds; cap later local waits at five minutes. Valid `Retry-After` overrides this, even beyond five minutes. Include reporting time in its deadline.
- Send structured HTTP 503 overloads directly to this waiter, skipping the ordinary short HTTP burst. Preserve unrelated HTTP/non-Responses retry policies.
- Report next delay, attempts, and elapsed capacity wait. Keep cancellation and sampling-input preemption. Do not add periodic redraws or unlimited background-compaction retries.
- Preserve other classifications: policy-restricted capacity, Flex, quota, authentication, invalid requests. Overload codes do not prove recovery. Mislabelled permanent disablement waits until cancellation. Do not classify prose with string heuristics or promise eventual completion.

### Multi-agent V2 depth

Apply `[agents] max_depth` to V2 creation, with root at 0. Default 1 permits direct children only. Reject deeper creation before child preparation. Keep existing agents/communication available.

### Subagent context windows, service tiers, and picker

Optional top-level `subagent_model_context_windows` maps exact final model IDs to positive `i64` token counts. Apply only to V2 spawned children at startup/model changes/reload. Missing entries keep scalar/model fallback. Keep original `model_context_window` so matches cannot spill into unmatched grandchildren. Preserve clamping/headroom/compaction. Keep root/review/guardian unchanged; add no API/rollout fields.

A positive role TOML `model_context_window` overrides map/scalar fallback at startup/model changes/reload. New children resolve their own role, not parent role limits. V1 full-history forks keep role inheritance. Preserve clamping/headroom/compaction and root/internal workers. The picker shows top-level map/scalar configuration, not effective role limits.

`[subagent_service_tiers]` maps model/effort to tiers, e.g. `gpt-6-sol = { high = "fast" }`. Match final child model/effective effort after overrides/roles. Matches override root tier throughout child lifetime, including later root-tier changes. Unmatched/root behavior stays upstream. Reject unsupported matched tiers and Fast when disabled. Do not expose spawn arguments.

`/subagents` labels:

- Append known model/effort using [compact status rules](#compact-status-line). Append `-fast` only for matched/inherited Fast; matched `default` suppresses inheritance. Do not guess settings, remove thread-ID descriptions, or change navigation.
- Append known positive configured capacity: `sol61-hig-fast-272k` or `sol61-max-272k`. Use exact map only for canonical-path V2 children, not main/V1. Otherwise use configured scalar, including main. Omit unknown/nonpositive values; do not infer catalog values.
- Labels show configuration before clamping/headroom, not confirmed routing/runtime capacity/usage/remaining context. Keep other metadata/navigation and TUI-only scope. Add no APIs/request tracking.

### Plaintext inter-agent messages

V2 sends parent/child messages as plaintext. Upstream marks `message` on `spawn_agent`, `send_message`, and `followup_task` as `encrypted`.

- Omit the marker in bundled schemas. For catalog overrides, drop `encrypted` from top-level properties only, not nested markers. Accept schemas without `message` without fallback.
- Always deliver `AgentMessage::Plaintext` for direct/code-mode/namespaced spawn/message calls, regardless of `encrypted_function_args`. Responses Lite omits that field for unmarked tools; do not misclassify plaintext as ciphertext.
- Preserve reasoning/compaction/output-replay encryption and old ciphertext replay through `AgentMessage::Encrypted`. Keep delivery APIs. Raw items/transcripts/tool-result logs outside `collaboration` now expose message text. Add typed `interAgentMessage` to app-server history/notifications.
- Below V2 child assignments, show original supplied spawn arguments except the displayed message. Use dimmed `key: value` lists matching the completion footer, without braces/string quotes. Expose typed display metadata separately; never parse assignments. Persist locally for display only; do not infer omitted settings or add model context. Preserve old transcripts without metadata. Rebuild derived history caches on upgrade.
- Show full plaintext incoming assignments/messages and outgoing communication in parent-owned child transcripts, live/replayed. Use actual role in initial headers when known. Preserve main summaries. Do not change context/delivery/history or display ciphertext. Keep the renderer separate from generic upstream tool rendering.
- Keep `0009_inter_agent_communication_projection.sql` clearing only derived SQLite history to reproject plaintext. Preserve this rebuild when resolving migration-number conflicts. Replay invalidated projections only for paginated visited segments and newer turn-shadowing dependencies. Do not eagerly replay unused ancestors for bounded recent pages. Serialize segment catch-up. Retry failed/cancelled rebuilds; committed batches do not prove completion. Reuse catch-up only with matching source revision/SQLite checkpoint, without decoding unchanged compressed history. Do not backfill old outgoing messages without display records. Incoming paginated records require saved turn metadata; never guess their turn.
- Store projections in `asm_thread_history_2.sqlite`, not `thread_history_1.sqlite`. The second namespace avoids upstream migration 9 colliding with the first namespace’s applied fork migration. Leave the old ASM cache untouched; rebuild projections from canonical rollouts. Apply the [shared compatibility gate](#shared-compatibility-gate), including cache versions, consumers, validation, and manual stock-cache recovery. The namespace does not repair previously contaminated stock caches. Cold reads lazily replay visited segments; warm reads reuse checkpoints. This costs disk/first-read replay, not an extra per-request lookup layer.
- After initialization, `asmTranscript/enable` with `version: 1` enables connection-local display. TUI opts in before history reads and after reconnects. Without opt-in, omit `interAgentMessage` from snapshots/paginated display/live notifications. Preserve ordinary history/collaboration summaries/context/storage cursors. For unsupported clients, skip display-only item/timeline pages until a visible row or EOF. Never truncate on empty filtered pages or disguise communication as user messages.
- Check live display and Desktop rendering for affected consumers. Stock-reader success does not prove Desktop rendering.
- At each upstream sync, search agent tools for new `with_encrypted`, `encrypted` schema markers, and `AgentMessage::Encrypted` producers. Missed markers deliver ciphertext as plaintext.

### Multiline-aware Enter

Opt-in `[tui] multiline_enter = true` makes plain Enter send single-line drafts and insert newlines in multiline drafts, including collapsed pending pastes. Keep defaults unchanged. Require plain Enter in both submit/editor-newline bindings. Preserve custom keymaps/popup actions. Modified submit always sends and flushes pending pastes.

Example bindings: `[tui.keymap.composer] submit = ["enter", "ctrl-enter"]`; `[tui.keymap.editor] insert_newline = ["enter", "shift-enter"]`. Use Shift+Enter to start multiline input and Ctrl+Enter to send. Terminals must support configured modifiers. Keep queue bindings unchanged. Vim Insert/Replace use editor-newline bindings; Normal retains editing semantics.

### Composer destination

Show Main or Side in the composer's top-right padding. Use a bold filled badge with one space each side: neutral for Main, magenta for companions, contrasting text. Do not add borders or recolor markers/input. Derive destination from active thread state, not footer text. Preserve input geometry.

### Built-in agent role descriptions

Resolve model before effort validation/runtime selection: explicit spawn `model` > role model > inheritance. Preserve role effort/constraints. Fail incompatible settings; do not switch runtimes. Keep `agent_type` spelling.

Keep the explorer purpose/original delegation Rules in spawn schemas. Keep worker tasks and concise Rules: explicit task/file ownership, concurrent-edit preservation, one shared-prerequisite owner. Coordinate before overlapping edits; continue independent work meanwhile.

### Subagent global instructions

At child startup/cold reload, `$CODEX_HOME/SUBAGENTS.md` replaces global user instructions from `AGENTS.md`/`AGENTS.override.md`. Empty clears them; missing retains inheritance; other read failures stop startup. Apply to fresh/full-history children. Keep project/host instructions and root/internal workers unchanged. Full-history children retain history and receive normal incremental updates, not rewritten history.

### Wait and sleep intent reminders

Require `reason` for `clock.sleep`, V2 `agents.wait_agent`, Code Mode `wait`, ASM TUI `wait_threads` including MCP, and empty `write_stdin` polls. Keep nonempty input unchanged. Trim reasons; reject over 500 characters. Always return continuation guidance naming the tool. Empty reason omits only its context line. Return nonempty reasons as JSON-quoted tool context, outside command/script output and truncation budgets.

- On input interruption, remind the model to understand intent. A status question alone does not cancel or pause work. If work remains, give commentary and call the named wait again with current valid arguments. Otherwise-unidentified returns use neutral guidance.
- Preserve children/cells/processes/collected output. Explicit stop/termination must not prompt resumption. Reminders do not guarantee continuation or automatic waits.
- Use existing `instant_interrupt` for Code Mode, terminal polls, and TUI waits. Disabling it preserves normal waits. Interrupted TUI waits return `interrupted: true` without snapshots and cancel observation only.
- Limit MCP early cancellation to marked `codex_tui.wait_threads`; preserve its envelope. Do not rewrite user messages, persist reminders, or change app-server schemas. Amend effective schemas after catalog overrides.
- Exclude V1 waits, environment readiness, desktop-owned tools, and initial command execution.

### Test-only dependencies

Keep `codex-ollama`'s `wiremock` under `[dev-dependencies]` while only tests use it. This removes the test-server chain from production builds, not necessarily the linked binary. Drop drift if upstream adopts it; restore the normal dependency if production needs it.

### Advertised subagent models

Optional top-level `subagent_model_list` selects advertised model IDs in user order for spawn descriptions and `<model_catalog>`. Omission keeps automatic picker-visible selection; `[]` advertises none. Explicit IDs can select hidden loaded models. Deduplicate and advertise only the first five; keep the catalog's 1,000-byte context bound. Before truncation, validate every ID against the loaded catalog and spawning backend. Fail invalid IDs without fallback. Do not restrict runtime spawning or change the picker.

### Flat reasoning-effort picker

Show all model-supported efforts, including Max/Ultra, in one list without “More reasoning…”. Keep persistence, Plan-mode scope, and explicit-picker requirements for shortcuts that raise effort to Ultra.

### GNU malloc arena cap

Before threads start, Linux GNU `codex` calls `mallopt(M_ARENA_MAX, 4)` at `main` start unless `MALLOC_ARENA_MAX` is set. Reduce idle heap retained by glibc's CPU-scaled arenas. Keep the GNU system allocator; upstream jemalloc replaces only musl's allocator. Mock-model runs showed memory savings without measurable wall/CPU cost; real-session effects remain unmeasured. Drop this drift if upstream caps glibc arenas.

### Session HTTP transport reuse

Reuse one model HTTP transport per session through the existing route-aware pool. Rebuild on resolved client factory/endpoint/redirect-policy/authentication-owner/runtime/default-header changes. Resolve authentication and run routing/policy checks per request. Token refresh alone retains connections. Preserve TLS/CA/proxy/cookie/redirect/managed-destination policy. Exclude auxiliary clients and WebSocket lifetime.

### Tokio worker cap

On Linux, cap Tokio workers at `min(CPUs, 8)` unless `TOKIO_WORKER_THREADS` is set. Reduce CPU-scaled worker overhead. Keep macOS/Windows defaults. Mock-model runs with the arena cap showed memory/CPU savings without wall-time loss; real-session/peak-async-CPU effects remain unmeasured. Drop this drift if upstream caps workers.

### Desktop agent-created worktrees

On the thread host, app-server handles recognized `codex_app.create_worktree`/`attach_worktree` dynamic tools. Create/attach durable managed worktrees without changing chat cwd, workspace roots, or skills. Return paths for explicit execution; do not call the Desktop handler that changes workspace. Preserve explicit navigation.

Reject explicitly untrusted chat/source projects; allow unset trust, like TUI. Require non-ephemeral chats, local execution, and durable attachments. Immediate completion is allowed with `allowAsync: true`. Preserve partial-registration recovery without duplicate checkouts. Verify attachment rendering and subsequent-turn skill retention in actual Desktop; backend checks are insufficient.
