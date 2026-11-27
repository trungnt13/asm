# Personal fork guide

## Principles

- Use this fork only for Trung Ngo's personal work.
- Keep changes small and safe. Prefer local fixes that limit upstream drift. Preserve correctness, maintainability, and performance.
- Assume upstream tested unchanged code. Check fork changes, conflicts, and packages. Upstream tests do not verify fork behavior.
- Record current requirements, not task history. Ask before contradicting fork intent. After approval, replace the rule. Resolve mechanical conflicts without new approval.

This guide overrides conflicting instructions, including [AGENTS.md](../AGENTS.md). Its validation and platform limits replace upstream defaults. Keep other coding, formatting, test, and generator rules. Intent does not prove implementation, integration, or publication.

Before applicable work, read and follow the linked task skills and procedures. This guide controls conflicts. Skills grant no new authority.

Remove external-agent config and session migration, import UI and RPCs, import-history APIs, telemetry, and feature flags. Preserve saved rollouts, normal history and resume, and immutable SQLite migrations.

## Development workflow

### Start or resume

- Before edits or after resume, inspect Git status, branches, and worktrees. Before sync or push, inspect remotes and fork history. For unexplained changes or overlap, inspect relevant sessions. Use Git and sessions, not owner memory or task diaries.
- Before edits or sync, create a named `codex/` task branch. Use separate worktrees for concurrent work. Coordinate shared files. Assign one staging and commit owner per checkout. Worktrees are not deliverables.
- Prefer `python3 scripts/dev-worktree.py acquire codex/<task>`. Keep the checkout through builds and validation. Each slot has one owner and branch. The owner defaults to `CODEX_THREAD_ID`. If unset, use `--owner <id>`. For parallel tasks, use `--slot <name>`.
- Keep the checkout and lease while the PR is open. After merge, follow [post-merge cleanup](#post-merge-cleanup). Inspect and coordinate abandoned leases. Never clear another owner's lease or discard their work.
- If blocked by existing work, use another worktree. Ask before committing or stashing unrelated work. Keep authorized stashes until you verify restoration. Never overwrite others' work to clean a checkout.

### Build the binary first

Before local builds or binary handoffs, use [asm-build](../.agents/skills/asm-build/SKILL.md). Keep the CLI and helper from the same source, target, and profile. Preserve the copied pair. Ask before replacing installed binaries.

For runtime or UI changes, inspect affected paths, including streaming and completed output. Make the smallest coherent change.

Before Cargo, check disk space. During rebase or release work, remove only verified inactive caches under the path, lock, and process checks. Record space and removals. Preserve active or uncertain artifacts, source, user state, and verified binaries. If safe cleanup is insufficient, ask.

The local Cargo wrapper must:

- Set `RUSTC_WORKSPACE_WRAPPER` to the checkout's absolute `scripts/local-rustc-workspace.sh`. Isolate workspace artifacts by checkout. Share registry dependencies. Do not reuse another checkout's stale code. Keep paths stable for warm builds.
- Preserve explicit wrapper overrides, including disabling. If an override does not isolate checkouts, use a separate target directory.
- Give Clippy a cache private to each checkout because it replaces the wrapper. Default to main's `clippy/<checkout-path-hash>`. Keep explicit Clippy targets private to the checkout.
- Lock build, check, Clippy, and nextest per cache. Save inputs and fingerprint reasons under main's `.agents/dev-build-cache/<date>/`. Preserve explicit `CARGO_LOG` filters. Inspect these logs before attributing a rebuild to cache loss.
- Clean stale artifacts only on Linux with Python 3.11+, full process visibility, and no cache users or Rust build or editor processes. Hold each profile's native Cargo lock. Traverse relative to directory descriptors. Reject symlink directories and artifacts outside the physical cache. Log counts and reasons for skips. Skip other hosts and uncertain activity.
- Keep the two-day rule. Use the `incremental` directory modification time or the latest `deps` file access or modification time. Age does not prove inactivity. Removal can force active checkouts to rebuild.
- Block checkout release or switching during active wrapper commands. Direct Cargo bypasses this guard. Finish it before releasing the checkout.
- Keep I/O for interactive `cargo run` without holding the lock during the live app. Leave CI and Windows unchanged.

Local profiles:

- Keep `dev-small` settings unchanged for ordinary `just` Cargo recipes, schema generators, source runners, package assembly, and VS Code Rust checks. Use the same recipe for the npm hooks generator.
- Preserve supported `--profile`, nextest `--cargo-profile`, and `--release` overrides. Nextest `--profile` selects runner settings, not the Cargo profile.
- Keep existing profiles for CI, benchmarks, source-built Dylint, VS Code run and test buttons, inherited Windows setup, and sandbox smoke utilities.
- Plain Cargo has no local default. For direct build, run, check, or clippy, pass `--profile dev-small`. For tests, use `just test`.
- Never delete caches in use by builds or editors. Use optimized or cross-platform local builds only on request or to reproduce affected behavior.

### Validate narrowly

Choose checks for changed inputs, fork interactions, conflicts, failures, and unresolved risks. Do not retest unchanged upstream behavior by default. Record checks, reused evidence, and gaps. A narrow check does not waive required evidence. A clean merge does not prove behavior. Verify upstream CI before reporting it passed.

Before PR readiness, use [asm-live-check](../.agents/skills/asm-live-check/SKILL.md) for live checks.

- **Documentation:** Check wording and links. Run `git diff --check`. Do not rebuild for prose changes alone.
- **Behavior:** Verify changed fork behavior with the built binary in a small, real user scenario. Mocks and simulations do not verify behavior. Do not add mock tests or run them by default.
- **Rust:** When needed, use existing non-mock checks with `just test -p <crate> <test-filter>`, not `cargo test`. Scope formatting and Clippy. Update affected assertions and snapshots together. Leave unrelated stale expectations alone.
- **Workflows and scripts:** Run `actionlint` on changed workflows. Check the actual script with a small case. Build natively only for affected packaging or approved experiments.
- Do not add tests or extend coverage without an explicit user request. Update existing tests and snapshots only to fix compilation or match required production output. Keep inherited checks for retained behavior. Never remove, ignore, weaken, or disable checks to hide regressions or avoid mocks.
- If ASM intentionally removed a feature, delete its obsolete case and exclusively owned snapshots. Do not restore the feature or regenerate its expectations. Delete helpers only if no retained tests use them. Apply this rule to syncs and feature removal. It overrides upstream integration and snapshot requirements. Report gaps before asking to add tests or broaden checks.
- Limit fork-added test files to 200 lines. In inherited files, limit fork-added behavior checks to 200 lines per file. Keep inherited coverage and necessary compile fixes. Keep meaningful user and safety checks. Remove low-value cases rather than splitting files or compressing format.
- **Formatting:** Use checkout-relative or absolute paths: `just fmt --files scripts/format.py justfile`. For names that start with a hyphen, use `--files -- <paths>`. Select changed Rust child modules. Parents do not format unselected children. The tool trusts paths, rejects unsupported types, honors exclusions, and reports normal file errors. Use `just fmt --rust` only when needed. Unscoped `just fmt` and CI `just fmt-check` still cover the full repository.
- Run required generators. Keep schemas, locks, and Bazel inputs correct without Bazel CI. Reuse upstream output only when source, generator inputs, output bytes, and provenance match, and fork effects stay unchanged. Record proof. Regenerate after fork changes, conflicts, or uncertain evidence.
- Do not default to whole-crate or workspace suites. If focused checks cannot verify the result or safety, explain the gap. Ask before broadening checks.

After approved integration, reuse checks and the verified CLI and helper pair only if effective source, generated files, dependencies, toolchain, features, flags, and pair provenance match. Keep caches isolated by checkout. Do not copy fingerprints to make builds look fresh. If integration or version changes affect inputs, rebuild and rerun affected checks.

### Shared config compatibility

- Put ASM-only keys at the top level in `config.toml`, not in upstream tables. For stock strict mode or warning-free startup, use an ASM-only profile.
- Test in separate private `CODEX_HOME` directories. Verify that ASM applies the key. Verify that stock `config/read` returns a non-default setting from the file. Startup alone is not a pass.

### Desktop SSH tests

For isolated Desktop SSH tests, use [asm-desktop-ssh-test](../.agents/skills/asm-desktop-ssh-test/SKILL.md). Preserve the normal backend. Follow the build and [live-check](../.agents/skills/asm-live-check/SKILL.md) rules.

### Commit, open a PR, report

Use PRs against `main` for implementation, documentation, and syncs. The lead owns delegated work. After validation and owner decisions, commit, push the task branch, and open the PR.

**Done means validated changes are committed, pushed, and ready for review.** Finish authorized merges before reporting done. A checkout or binary alone is not enough.

- Stage only completed changes, tests, generated files, and intent. Inspect the staged diff.
- Use `Trung Ngo <1390402+trungnt13@users.noreply.github.com>`. Check `git var GIT_AUTHOR_IDENT` and `git var GIT_COMMITTER_IDENT`. Fix local config if needed. Ask before rewriting published history only to fix attribution.
- Do not include AI attribution in authored deliverables. This includes commit and PR/MR titles and descriptions, co-author trailers, generated-by lines, session links, and assistant, tool, model, runtime, or engine credits. Apply the rule to all agents, providers, native tools, and runtime processing, regardless of defaults. Disable automatic attribution. Verify final commit and published PR/MR text. Config alone does not prove compliance or enforce native behavior. Discuss attribution only on explicit request. Keep required third-party copyright and license notices.
- Before updating a branch, refresh its PR base. Check affected behavior against the base, fork, and upstream. Resolve mechanical conflicts that preserve this guide. For unsafe changes, unclear intent, or owner decisions, inspect all exposed conflicts. Ask one question with recommendations. Later commits can expose more conflicts.
- Reconcile duplicate task edits without losing others' work. If validation, push, or PR creation is blocked, commit finished work. Report the blocker and list unfinished work separately.
- Report changes, exact check results, skipped checks, gaps, commit, branch, PR URL, and push status. State ready, blocked, or merged. Keep one evidence record. Reuse unchanged results and refresh changed results or status. Readiness does not authorize merge.

### Post-merge cleanup

After an authorized merge, the PR owner must complete cleanup before reporting integration done. Merge approval covers safe removal of disposable PR-only artifacts. It does not grant unrelated cleanup or override retention and deployment rules. Cleanup is not automatic.

1. Verify the PR merged and the changes are in local `main`. Before changes, inspect status, worktrees, leases, and processes. A closed but unmerged PR is not eligible. After squash or rebase, verify changes rather than assuming ancestry.
2. Inventory task artifacts, including delegated work: build outputs and caches, test binaries, private test homes, scratch files, temporary servers, branches, and worktrees. Check paths, ownership, consumers, and dependencies. Names and age do not prove disposal is safe. Do not sweep `.agents`, `/tmp`, shared targets, or other tasks by glob.
3. Keep a private record of commands, checks, source and binary identity, and required rollouts and logs before removing duplicates. Keep failed logs through triage, requested evidence, and binaries needed for handoff, rollback, release, or a running backend. Record what remains, why, and who or what permits cleanup. Never commit credentials or personal history.
4. Stop only verified task processes. Remove disposable PR files and test homes after retaining evidence. Use the existing cache path, lock, process, and platform checks. Do not use blanket `cargo clean`. Keep shared or warm caches, user state, installed packages, reusable SSH setup, and active or uncertain resources. Account removal, deployment, and shared-server restart still need their own authorization.
5. When clean local `main` contains the branch tip, use `python3 scripts/dev-worktree.py release codex/<task>` with the original slot and owner options. Keep detached reusable checkouts. If squash or rebase blocks release, report it. Do not bypass the check or discard the branch. Archive idle app-managed worktrees with the app tool. Remove other task-owned worktrees only if no uncommitted, untracked, ignored, or retained files remain. Delete PR-only branches only after you verify integration and confirm no owner, consumer, or unique work remains. Never force deletion to bypass a check.
6. Verify removals, leases, worktrees, and branches. Report removals, retained artifacts, and blockers. Keep uncertain resources and ask for the specific decision needed. Do not report incomplete cleanup as complete.

### Push permissions

- Implementation requests allow task-branch commits, pushes, and PRs. They do not allow direct implementation on `main`.
- An explicit upstream rebase request authorizes merging its sync PR after required checks. For other PRs, including features, get explicit merge approval.
- In scope, use `--force-with-lease` on fork branches, including `main`, without separate permission. Plain `--force`, tag moves, discarding work, and releases still need approval.
- Push explicit refs with `--no-follow-tags`. Never push all tags.
- Before a lease-protected push, fetch and preserve remote-only work, including release workflow commits. Use an explicit lease against the inspected tip. Do not refresh a failed lease and retry blindly.

## Upstream sync

Before syncs or targeted ports, follow the conflict-reuse and evidence procedure in [asm-upstream-sync](../.agents/skills/asm-upstream-sync/SKILL.md).

### Local validation for upstream rebases

- Run only local checks for changed build inputs, conflict resolutions, or upstream changes that affect ASM.
- Build affected binaries as needed. Run required generators and focused tests, not whole suites.
- If the base moves, reuse matching evidence. Repeat only checks with changed inputs.
- Put `[skip ci]` in each new rebase PR head commit. Do not start, retry, or wait for hosted CI unless the owner asks.
- Keep the read-only model catalog lookup. Do not run real-provider inference by default.
- If local checks are insufficient, report the gap and ask before running more.
- Keep applicable compatibility checks local. Do not bypass branch protection or report skipped checks as passing.

### Built-in instruction changes

- Review the full upstream diff and ASM's resolution. Check inline text, bundled prompts and skills, tool descriptions, and review rubrics.
- In each rebase report and sync PR, list changes to text, selection, assembly, and delivery. State if there are none.
- For each change, link the source. State who receives the text, when it applies, and what ASM kept or changed.
- Separate built-in text from user overrides and provider additions. Source review does not prove delivery.
- Flag each new model-visible item that can exceed 1,000 tokens for P0 manual review.

### Shared compatibility gate

For syncs or changes to saved caches or wire formats, apply this gate even after conflict-free rebases. Follow narrow validation and [live-check](../.agents/skills/asm-live-check/SKILL.md) rules. This gate does not authorize paid matrices, broader checks, or new tests.

1. Map all affected writers and readers, including stock consumers. Trace caches, migrations, serialization, app-server responses, raw events, and live notifications. Review consumers as well as producers. For new mandatory handshakes, inspect existing clients, fixtures, and reconnects.
2. Keep fork-only formats and migrations in ASM-owned versioned caches, never stock caches. Preserve native canonical rollouts, thread IDs, and shared metadata. For incompatible formats or migration-number or checksum collisions, change the ASM namespace. Never rewrite applied migration checksums.
3. Gate fork wire extensions with explicit versioned feature capabilities, not `experimentalApi`, client names, or versions. Verify ordinary behavior without opt-in.
4. Before resume or warming, check cold timeline, search, turn, and item reads in separate private homes. Hand bounded saved history to an independent supported stock reader. Then check read/resume and warm-cache reuse. Cover affected paging directions, hidden-only rows, and cursor contracts. Where affected, check opted-in and opted-out ASM clients concurrently.
5. Record actual backend and client executables, versions, hashes, hosts, and `CODEX_HOME`. Launcher versions do not prove backend identity. Resume-assisted reads do not prove cold reads or UI rendering. Record unchecked platforms and consumer boundaries. Provider turns and packaging or version checks do not prove interoperability.
6. Before merge, refresh the actual base and verify the candidate against it. If the base moves, inspect interactions. If relevant inputs change, rebuild the pair and repeat focused checks before merging. Reuse evidence only for matching inputs.
7. Keep remaining findings unresolved and missing boundary evidence blocked. Do not claim readiness or silently expand scope. Ask for out-of-scope decisions.
8. If older ASM contaminated a stock cache, recover manually only. Verify the host, actual reader executable, version, hash, and home. Stop all writers. Back up the derived database and WAL/SHM. Quarantine and rebuild only that cache. Preserve canonical history and shared state. Never reuse the older writer against the recovered home.

#### Known upstream cold-history limitation

Before resume builds its cache, stock Codex can return an empty cold timeline, turns, or items for ordinary text history. The owner accepts this limitation for future syncs when isolated checks show all of the following:

- ASM reads the history cold.
- Stock resume and warm reopen recover the exact saved history.
- The original rollout bytes stay unchanged.

Report the stock cold result as a known limitation, not a pass. Do not ask again if these conditions hold. Ask if the behavior changes or checks cannot establish these conditions. The acceptance excludes voice history, data loss, ASM regressions, and other validation failures.

The owner accepted the limitation for each revision below only. These decisions do not cover voice history or waive other checks. ASM cold passes cover search, timeline, turns, and items. All recoveries preserve the original rollout. A dash indicates no ASM cold result in this record.

| Revision | ASM cold | Stock cold evidence | Resume and reopen |
| --- | --- | --- | --- |
| `cf3ff1530d821ec2d26452da4971d8e3e7d18d25` | — | Independent alpha.11: empty timeline | One resume and warm reopen recover both saved items. |
| PR #245, `140815f19c05a24a70ebc273016bd54ff71ced7c` | Pass | Independent alpha.11: empty timeline | One resume and warm reopen recover both saved items. |
| PR #240, `ddfaef91b258da5a0217951ed5f6e0039add5e02` | Pass | Independent alpha.11: empty timeline | One resume and warm reopen recover both saved items. |
| `63598243af3fa93ac670fb39da48b4b0967fb365` | — | Alpha.11 finds the saved thread; timeline empty. | One resume and warm reopen recover both items. |
| PR #236, `9f2b87495bf979f6c1953cf00c98143981664914` | Pass | Alpha.11 finds the thread; timeline, turns, and items empty. | One resume and warm reopen recover both saved items. |
| PR #226, `2f0374456fd5cd280ee76de54220ab8315a0f8d0` | Pass | Alpha.11 finds the thread. | Resume makes both saved items readable. Warm reopen preserves rollout bytes. |
| PR #224, `39c29c3bb620f91e84a3bb266bb496dfb697c101` | Pass | Alpha.11 finds the thread. | One resume makes both saved items readable. Warm reopen preserves items and rollout bytes. |
| `ca21fe65d18a9b6326759fb4263c65dfbb645aa5` | Pass | Independent reader finds the thread by search but cannot read cold history. | Resume and reopen restore exact items and permissions. |
| `b8f6787e95a5006d91bc712f59d72af32ef9b205` | Pass | Independent reader finds the thread by search; cold reads empty. | Resume and warm reopen restore exact items and permissions. |
| `8fca6ffb92a602875a0b97e3e535eb48375bdd8c` | Fresh pass | Independent alpha.11 finds the thread by search; cold reads empty. | Resume and warm reopen restore exact saved items and permissions. |
| `bc2c328483ee60e82e08ad48f3a5efbb5712f914` | Fresh pass | Independent alpha.11 finds the thread by search; cold reads empty. | Resume and warm reopen restore exact items and saved permissions. |
| `595f497f15a3e3239a62f6b99cf61067df00f0d0` | Fresh isolated pass | Independent reader finds the thread by search; timeline, turns, and items empty. | Resume and reopen restore exact saved items and permissions. |

The last three rows do not establish Desktop rendering or runtime results from stock builds of those exact targets.

Earlier evidence:

- With canonical rollouts but no derived history database, stock `rust-v0.168.0-alpha.7` returns an empty cold child timeline. Resume builds the projection and exposes messages. Missing catch-up persists through `05cf8eeb14`. Independent stock `rust-v0.169.0-alpha.6` returned an empty first cold timeline for valid real history. Resume restored the projection and saved permissions. ASM already rebuilt before querying; the sync did not introduce the difference. Isolated checks found no history loss.
- Independent stock Linux `rust-v0.169.0-alpha.7` finds the thread by cold search but returns empty timeline, turn, and item reads. Resume restores the projection and permissions without changing the canonical rollout. The tag is a release commit off `6bd17ba6ae`, not a build of `e1cc96712f`. History and wire code match the target in thread-store, rollout, state, app-server, and app-server-protocol.
- Through `b6d93b4cba`, stock cold reads query the saved projection without catch-up, as in the independently tested alpha.7 reader. This is source evidence, not an alpha.8 runtime result. Cold timeline, turn, and item implementations stay unchanged through `37103d7f01`. Attached-thread search and fork previews do not add catch-up. Through `627b579219`, experimental `thread/sample` uses a separate path; ordinary cold readers stay unchanged.
- Independent stock Linux `rust-v0.169.0-alpha.9` finds old real history by cold search but returns no cold timeline, turns, or items. Resume restores exact saved history and permissions without changing canonical bytes. Ordinary stock cold readers still lack catch-up through `5228d94a27`.

The owner accepted this limitation for the syncs through `05cf8eeb14`, `5228d94a27b7f84cae4854acbb5ea3b3e90dfeaf`, and `392ced7d3a9160fd35d40c606359869215f50522` and the following PR targets only:

| PR | Revision |
| --- | --- |
| #179 | `e1cc96712f9ad4f5404fe821b9eba7ff29598816` |
| #180 | `b6d93b4cbadd6f449395d326f72465a5823f1492` |
| #181 | `37103d7f01e82162472c6fba7b4088d5a1303321` |
| #182 | `627b579219d492b5445d6d433d3110eb53e507dc` |

For `5c3ff1dabf644169f4bd74b215719f381e4fdb3f`, independent stock Linux `rust-v0.169.0-alpha.11` found old and fresh real history by cold search. Cold timeline, turns, and items were empty. Resume and warm reopen restored exact history without changing canonical bytes. This observation is not acceptance for this target.

The ordinary stock cold-reader source stays unchanged from `5c3ff1dabf` through `72532ecae5efe500d9e722f3ed5214e63cc8098a`. This is source evidence, not a stock runtime build of the target. The owner accepted the limitation for PR #189 at that target only. A fresh real rollout from the candidate passes ASM cold search, timeline, turn, and item reads. Independent stock alpha.11 finds it by cold search; timeline, turns, and items are empty. Resume and warm reopen recover the exact two saved items, one turn, and saved permissions without changing canonical bytes.

The owner accepted the unchanged ordinary limitation for PR #190 through `05229d1874d4e04bc746a9954c71931a1ab852c6` only. Source review confirms that ordinary stock timeline, turn, and item readers lack catch-up. This is not a stock runtime build of that commit. New realtime readers materialize their cache and are outside this finding. This exception does not waive fresh-writer validation.

For PR #200, the owner accepted the unchanged ordinary limitation through `65a5410f72dac1f4b40862c34ef0a8b71b12834a` only. Fresh canonical history passes ASM cold reads. Stock alpha.11 still needs resume to read exact saved items.

These revision-specific decisions do not cover later commits or waive other checks or validation failures. Keep the stock cold result unresolved, not passed. Report resume-assisted recovery separately. Do not change personal history or caches.

#### Upstream voice-history reader boundary

Upstream adds canonical realtime records `Frame`, `TurnInputAccepted`, and `SessionEndInputSaved`. Checked published stock readers (`rust-v0.169.0` and `rust-v0.171.0-alpha.5`) do not recognize these variants and can skip them. Successful resume does not prove exact voice-history handoff. Exact interchange requires an independent stock reader containing upstream `5943d7acdf` or later. Keep the upstream format. Do not rewrite saved records for older readers.

The owner accepted this separate voice boundary for PR #190 through `05229d1874d4e04bc746a9954c71931a1ab852c6` and PR #200 through `65a5410f72dac1f4b40862c34ef0a8b71b12834a`. These decisions do not establish actual Desktop voice behavior.

The owner accepted both unchanged boundaries for the targets below. Neither limitation is fixed: ordinary cold stock reads require resume, and older stock readers can skip the new voice records.

| Target | Text-history evidence |
| --- | --- |
| PR #207, `81b995a6c9a4c978f399775cfa5ce50702368749` | Fresh ASM cold reads pass. Stock alpha.11 needs resume to read exact saved history. Actual Desktop voice behavior is unverified. |
| PR #208, `87fccdc1bc0bc9af04db844a23d9c025f3b82000` | Fresh text history passes ASM cold reads. Stock alpha.11 resume and warm reopen preserve exact items, saved permissions, and original canonical bytes. |
| `77873dd0d284aeba7ce849b995b28ee8f4498d73` (accepted 2026-11-17) | Fresh ASM cold reads and stock alpha.11 resume and warm reopen preserve exact saved text items, permissions, and original rollout bytes. |
| `c8d1ba814eb984e8fdd2f595ffd9b287e825a5b3` (accepted 2026-11-17) | Fresh candidate text history passes separate ASM cold timeline, search, turn, and item reads. Stock alpha.11 finds the thread but needs resume to read exact saved items. Resume and warm reopen preserve both saved text items and canonical bytes. |

These decisions apply only to the listed revisions. They do not establish exact voice handoff or waive ordinary text-history checks or other validation failures.

#### Opt-in per-environment context

Keep upstream `per_environment_context` disabled by default. When enabled, it groups permissions and workspace details by environment. It appends scoped changes instead of rewriting saved history. Manual source review found that leaf limits do not bound the aggregate item, which can exceed 1K tokens. Preserve upstream formatting and limits. Source review does not establish opt-in runtime behavior.

### Live backend model catalog

For every upstream rebase or sync, fetch the live authenticated backend catalog. Source, cached, and bundled catalogs do not prove current availability. Backend models can change without source changes.

Fork (`origin`): [`trungnt13/asm`](https://github.com/trungnt13/asm). Parent (`upstream`): [`openai/codex`](https://github.com/openai/codex).

Last incorporated commit: `372947d9369a6c608dde98742a9130a3198a66fe`, 2026-11-27. Subject: Report pending startup dialogs through terminal program status (#64384).

Before fetching, verify the upstream URL. Sync on a task branch and submit a PR. Use the requested method. Otherwise, merge to preserve published history. Rebase only unpublished branches. Follow push permissions for authorized rewrites. For targeted ports, use `git cherry-pick -x`.

## Platforms and CI

- Support Apple Silicon macOS (`aarch64-apple-darwin`) and Linux (Ubuntu). Exclude Intel macOS (`x86_64-apple-darwin`) from support, validation, and releases. Do not add or require its CI jobs or release artifacts. Preserve inherited Intel-compatible source without promising support.
- Do not add other platforms to fork CI or releases. Preserve other inherited code and workflows unless removal is requested. Get approval for other inherited workflow changes.
- Inspect triggers: [V8 canary](../.github/workflows/v8-canary.yml) runs on PRs; expensive builds depend on relevant changes. [CLA](../.github/workflows/cla.yml) restricts its job to `openai`.
- Use standard GitHub-hosted runners. Get explicit approval for paid larger runners. For Apple Silicon, V8 canary uses `macos-15` and two Bazel jobs to limit runner memory use.
- [Blocking CI](../.github/workflows/blocking-ci.yml) and [postmerge CI](../.github/workflows/postmerge-ci.yml) are manual-only `workflow_dispatch`, never automatic or required.
- Blocking CI keeps workspace formatting, production Clippy, and result collection. Cover `codex-cli`, `codex-tui`, `codex-core`, and `codex-config` on both release targets. Preserve `--lib --bin codex -- -D warnings`. No test-target matrix, `cargo shear`, or warning suppression. Check other affected packages narrowly.
- Postmerge CI keeps optimized builds, packaging, smoke checks, uploads, diagnostics, and results for targeted cross-platform checks. Do not run it for every merge. Report existing failures separately.
- Port useful upstream action, toolchain, security, and build fixes without broad matrices or routine local CI repetition. Exclude unrelated platforms, Bazel suites, SDKs, remote executors, V8 source-build canaries, and OpenAI-only infrastructure. The required two-target V8 build is not a broad suite or canary.

## Intentional behavior differences

### Retired multi-agent V1 execution

- Remove all V1 execution: tool registration, schema builders, handlers, and V1-only close/resume adapters. Do not advertise tools that only return unsupported.
- Keep V1 config/version decoding, canonical rollout types, collaboration events, history projections, and transcript readers. Old histories stay readable without V1 execution. Keep shared V2, native-runtime, Guardian, mailbox, residency, and tree-shutdown code.
- Do not silently upgrade version defaults or rewrite saved history. Explicit `features.multi_agent_v2 = true` selects supported tools, even on old resumes. Sessions that resolve to V1 expose no V1 tools.
- On sync, treat `core/src/tools/handlers/multi_agents.rs`, its `multi_agents/` directory, and `core/src/agent/control/{legacy,resume}.rs` as retired chunks, not patches to restore. Trace new upstream callers before porting behavior to V2. Search for `multi_agent_v1`, `create_spawn_agent_tool_v1`, `SpawnConfigVersion`, `spawn_agent_with_source`, `close_agent`, and `resume_agent`. Distinguish execution producers from history/config consumers. Keep V2/shared code outside the deletion boundary. V2 loads suspended descendants in `core/src/agent/control/suspended.rs`, not the retired V1 resume adapter.
- This reduces maintenance scope but does not establish an overall speedup.

### Desktop app-context filtering

`app_context_filter` defaults to `[]`. Before session creation, remove only exact named `###` sections from Desktop `<app-context>` instructions. Keep unknown sections, surrounding instructions, per-turn context, and rollout history. Use the same filter for initial context rebuilds. Use a fresh chat after filter changes.

### External runtime boundary

Separate ASM inference/tools from external CLI execution. Use `core/src/external_runtime` for session integration and `ext/external-runtime` for shared process, framing, and cleanup code. Share neutral start, prompt, outcome, and error types. Separate Claude/Cursor drivers own their protocol encoding. Across renames, preserve the legacy `claude-runtime` checkpoint directory, canonical history encoding, and checkpoint hashes.

Delete all thread-private runtime files (linkage, Cursor configuration, diagnostics, login markers) only with exclusive runtime ownership. Keep native conversation history. In opt-in failure diagnostics, record bounded CLI versions from protocol initialization. Do not launch extra CLIs or gate admission on versions.

### Catalog-bound model providers

- With `load_external_models_json = true`, merge bundled entries into custom `model_catalog_json` in memory. Keep all metadata overrides for exact custom IDs. With `false`, keep only the custom catalog. Routing remains bundled and exact-match-only.
- Bundled `external_models.json` entries can select an existing named `model_provider`. Accept canonical slugs and optional provider/runtime-qualified aliases. Normalize before validation and requests. Persist canonical model/provider identity so alias changes cannot reroute resumes. Keep existing full IDs and managed provider requirements. Do not infer routes from slashes or remote catalog metadata. Keep endpoints/credentials in user provider config.
- DeepSeek V4.1 Flash 1017 selects `fireworks`, alias `fireworks/deepseek-v4.1-flash`, without a profile or custom role. Default to 300,000 context tokens; allow explicit overrides through 1,000,000. Use Code Mode only. Encode `exec` as a function only on the Fireworks Responses wire; keep canonical calls and saved history as Code Mode. Default to Standard. Route `ultrafast` through the Ultra-fast router without changing the saved model.
- GLM 5.3 selects `fireworks`, alias `fireworks/glm-5.3`. Use Code Mode only on Responses, with Standard as the only catalog tier. Apply ordinary resume admission, with no exception for chats saved under another provider.
- Gemini 4 Neon selects the configured `gemini` provider. Under `[model_providers.gemini]`, use `wire_api = "gemini_interactions"`, `base_url = "https://generativelanguage.googleapis.com/v1beta"`, and `env_http_headers = { "x-goog-api-key" = "GEMINI_API_KEY" }`. Default to direct tools. Honor `code_mode` and `code_mode_only` for ASM-owned execution. Keep the wire text-only. Always send `store: false`: it disables stored Interactions, not provider abuse-monitoring logs. Replay opaque thought steps only on Gemini. Do not send OpenAI credentials or Responses Lite fields.
- Show picker aliases but keep canonical IDs: `claude/…` for `claude-code/…`, `cursor/…` for `cursor-agent/…`. Do not add HTTP bindings or change native selection. Fork context into the selected model when a picker change needs a new runtime. Blank sessions start fresh; existing same-provider in-place changes stay in place.
- Admit valid bound/native inputs and resumes independently of the global HTTP-provider default. Validate saved and requested identities separately. Overrides do not exempt unrelated saved chats. Require the current provider definition and valid config. Preserve mandatory policy, config errors, missing-provider checks, and history/runtime boundaries. Classify routes by in-memory bindings or runtime detection, not discovery. For unbound API models, keep upstream saved-provider admission.
- V2 `spawn_agent` requires `fork_turns="none"` or `"all"`. Legacy positive integer strings still select full history. Roles with `fork_turns = "none"` always start fresh.
- Set `require_spawn_agent_model = true` before any TOML table header to require an explicit model on the V2 tool. This applies only with `expose_spawn_agent_model_overrides`, not to app-server RPCs or internal spawns. Remove the old key from `[features.multi_agent_v2]`: stock Codex rejects unknown keys there. In normal mode, stock Codex warns and ignores the top-level key. For strict mode, put it in `asm.config.toml` and select it only in ASM with `--profile asm`.
- Cross-provider children use the same portable handoff for `fork_turns="all"`; `"none"` transfers no history. Keep the child task separate from inherited context. Preserve destination provider identity on resume. Bound models use their provider's model manager, not parent discovery identity. Active Claude `all` requires a matching foreground call before steering. Replay acknowledgments do not identify which assistant request used the input. Retry on a new turn.
- For user-spawned children with a catalog-bound provider ID, require an explicitly bound model or native runtime, including descendants and cold resumes. Reject unbound selections before startup. Do not infer reverse routing. Use a separate provider ID for unbound custom subagent models. Keep ordinary main/internal-worker provider behavior.
- Keep upstream catalog/API model types. Hidden bundled entries remain explicitly selectable.
- Select portable history from retained, attributable user and assistant messages after compaction and rollback. Keep supported inline images, order, and role attribution. Reject unsupported authored attachments. Exclude private runtime state, encrypted content, provider reasoning, tool calls and raw results, and source instructions. Never execute inherited tools. Use destination instructions and permissions.
- `[handoff].max_history_bytes` defaults to 64000 serialized bytes for portable context. Try the latest user message, latest assistant message, then earliest retained user message. Try the rest newest first. Skip whole messages that do not fit. Send the selection chronologically. If none fit, continue without imported history. Omitted messages stay at the source. Do not apply the budget to native forks or the destination's new request. No provider/operation lists or per-transfer approval prompts.

Any provider can select independent OpenAI authentication for standalone search. For example, set `standalone_web_search_provider = "openai"` in `[model_providers.fireworks]`. Resolve the override from the selected model's current provider, including catalog bindings and model switches. A match enables the existing standalone path without a global flag. Disabled search, provider capabilities, and MCP/backend exclusions still apply. Reuse the upstream extension, native defaults, and existing tool-output/history path. Do not silently fall back between services. Send the usual recent context to the selected OpenAI backend. A matching non-OpenAI provider exposes the ordinary `web_run` executor: namespaced Fireworks calls did not produce dispatchable function calls in the live check. Native OpenAI keeps `web.run`.

For Fireworks, defer MCP/app and desktop dynamic tools in the harness. Discover them with an ordinary function call. Expose only selected tools on later requests. Keep explicitly direct-only namespaces. Do not send native tool-search items or undiscovered schemas to Fireworks. Keep native tool search where supported. On resume, resolve discovered tools against current permissions.

### Claude Code thread runtime

Cursor shares process, framing, authentication, checkpoint, and cleanup rules. Claude uses ASM-owned execution; Cursor keeps native tools and instructions. Keep shared architecture and separate drivers. Do not duplicate execution loops.

**Selection and settings**

- Explicit `claude-code/<model>` selects official persistent `claude -p` for mains and ASM children. At launch, expand versioned `sonnet-`/`opus-` suffixes to native `claude-` names. Pass others unchanged. Keep saved IDs. Other models retain upstream inference.
- Allow ASM-only workspace-root, config, and capability metadata updates. Keep the original native executor/cwd unless starting a fresh thread.
- Keep upstream `models.json` unchanged. Bundle native entries in [`external_models.json`](../codex-rs/models-manager/external_models.json). `load_external_models_json` defaults to `true`; `false` hides entries without blocking explicit selection. Append entries to default catalogs, including cached/remote catalogs. Override duplicate IDs without changing cached payloads. Provider-owned catalogs ignore the flag and stay authoritative. Custom catalogs honor it and retain duplicate-ID metadata. Native model/effort access stays native-owned; metadata grants no access.
- Show CC entries without OpenAI login. Do not mark them API-supported to bypass filtering. Keep ordinary model defaults.
- Discard inherited config/profile tiers for native sessions. Only Default passes explicit request/session-flag tier validation. Reject unsupported persistent or turn-only tier changes before committing settings. Keep OpenAI tier resolution.
- On picker runtime entry/exit or native model change, require confirmation. Fork a new session with portable history. Keep the source session and defaults.
- Same-model/provider effort changes keep the conversation and saved model defaults. At the next idle boundary, apply native control, require confirmation, then save effort before input. Keep process, session ID, and history; never rebuild context. On cold resume, restore saved effort before requested changes. Reject unsupported changes before prompting, without fallback.
- Preserve cache-relevant context without promising keys or hit rates. Claude cache-prefix preservation depends on server-controlled `per_turn_effort_active`.
- For ASM children, use available catalog effort defaults/support. Unlisted native IDs work without OpenAI fallback metadata. If a role changes the external model, replace only inherited parent effort with the new catalog default. Leave it unset without metadata. Validate explicit spawn, role, and configured subagent effort. Do not discard unsupported choices. Same-model roles keep inherited effort. Keep OpenAI role behavior.
- Even with custom descriptions or hidden catalogs, V2 spawn descriptions explain that `all` transfers portable context across runtimes and `none` starts fresh.

**Ownership and input**

- ASM owns Claude tool execution, permissions, instructions, skills, agents, queues, display, and process lifecycle. Persistent Claude owns subscription inference, native conversation state, and compaction. Cursor keeps native execution and existing role-context integration.
- Use canonical ASM base/project/role instructions, skills, environment context, and tool guidance. Deliver stable initial context once and subsequent changes incrementally. Keep durable accounting across warm turns and new-session cold resume. Never rewrite history, replay completed tools, or redeliver bridge results as context.
- Preserve explicit queued plaintext context, user input, images, and agent communication. Warn and skip encrypted injections; do not forward ciphertext. Keep Page filtering separate from genuine user input.
- Only for ASM-launched Claude, disable native tools, MCP servers, skills, instruction discovery, and unrelated plugins. Admit only the ASM bridge extension. Verify effective capabilities before inference. Fail closed on unexpected tools or unsupported launches. Never fall back to native execution. Keep personal settings, native installation, and standalone behavior unchanged.
- Keep subscription auth and managed policy. Do not use minimal launch mode that requires API-key billing. Separate native context-window and child cache settings from tool ownership.
- Manually review new instruction items over 1K tokens. Matching schemas do not prove instruction or execution parity. Claude receives the base as its system prompt and incremental developer/user context as labeled user-frame content. Labels do not grant native developer-role precedence. Validate relevant behavior without claiming identical instruction hierarchies.

**Communication and display**

- Claude mains, children, and eligible Sides use ordinary ASM's canonical router and Code Mode worker under existing permissions and Side restrictions. Adapt only protocol schemas/results. Do not duplicate registries, inference loops, or tools.
- Keep bridge entrypoints stable. Resolve discovery and execution against current authorized capabilities. Keep deferred discovery, parallel Code Mode, yield/wait, cancellation, approvals, structured errors, and supported media results. Matching inventories do not prove provider-hosted capability parity.
- Own the captured step, dispatch worker, cancellation, and result accounting across turns and continuations. Stop/drain owned calls before marking linkage clean. Record each call/result once. Never automatically retry uncertain side effects.
- Cursor children keep the authenticated local bridge and native shell helper. Expose canonical collaboration tools except spawning, with existing direct-message/wait/message-board gates. Cursor mains keep native tools without the child bridge.
- Share local transport and cleanup, with separate protocol adapters. Keep credentials in owner-only descriptors, never prompts/argv. Require no external JavaScript/Python runtime. Claude plugin names may resemble MCP names, but dispatch uses the local JSON bridge. Measure helper overhead before adding persistent transport complexity.
- Send board notices only to running turns: Claude at the next tool boundary; Cursor through active-prompt CLI wait results. Never wake idle children.
- For busy Claude children, queue UUID-stamped user input without Send now or another interrupt. Deliver at a tool boundary without rejecting running tools. Require `system/init` capability `interrupt_send_now_v1`; otherwise deliver at turn boundaries. Persist input before submission. Reconcile every submitted command before clean idle. Submit input handed over at the idle boundary before ending the turn. Idle `send_message` stays queue-only; keep Codex delivery. For urgent redirection, use `interrupt_agent`, then `followup_task`, with normal interrupt safeguards.
- Claude's native `session_state_changed` idle signal governs completion with a successful result and reconciled queued input. Do not veto it with `background_tasks_changed`: the snapshot includes nonblocking shells, and draining them need not emit idle again. Keep background-work restrictions on settings changes, forks, and auth recovery. Notifications grant no authority. Plain interrupt leaves background shells running; keep confirmation rules. A notification result answers new input only after that input succeeds, not from a turn between ASM turns.
- One owner consumes Claude events throughout the loaded session and serializes input and control. Project native continuations promptly in a separate internal ASM activity/turn. Do not send native prompts, invent user input, or reopen completed turns. Append history and count usage once. Idle with only background shells accepts input normally. Root input during autonomous continuation waits for its next idle boundary. Keep capability-gated child steering. Own consumer shutdown and cleanup. Do not tail transcripts or add readers.
- Show native tool/skill/task events in existing MCP cells for streaming, disclosure, grouping, and replay. They are not execution/context items. Attribute native child activity without ASM threads. Preserve failures and avoid duplicate bridge events. Count distinct observed native child tasks in completed-turn `ag`, deduplicated across lifecycle/replay. Do not inflate active ancestor counts with history.

**Login and permissions**

- Resolve `claude` through PATH. Keep native subscription login. Do not extract subscription credentials or implicitly enable API billing. Strip API-key, cloud-provider, and login-profile selectors from ASM-launched processes. Route questions through available ASM tools under normal child restrictions.
- Use deterministic subscription login over shared app-server chat requests/responses, without Desktop changes. Explicit `claude login`/`cursor login` selects a provider in any main chat, including GPT parents, without changing models. Store that provider in pending state. Never infer it from the chat model. Children cannot start interactive login.
- Keep exchanges display-only, outside model history/rollouts. Intercept replies before ingestion, history, hooks, or inference. Send codes only to the official Claude login subprocess. Keep them out of model requests, logs, and rollouts. Client drafts/recall are accepted.
- Fail closed on expiry/interruption. Give unrecognized replies fixed restart instructions, without normal ingestion. Only explicit non-secret actions exit or restart login. Keep a code-free pending marker across backend restart/resume, without codes, OAuth state, or checkpoint changes. Keep output private and errors fixed. Verify subscription auth without Console/API key/cloud fallback.
- Login never authorizes replay or uncertain-checkpoint recovery. Apply ASM permissions, sandbox, and approvals to Claude bridge calls. Refuse Cursor native execution under mandatory ASM sandbox policy: it cannot meet that policy. Explain this ownership distinction in notices.
- Claude uses `--dangerously-skip-permissions` and `--permission-prompts none` for its isolated bridge, not to grant ASM tool permissions. Keep managed policy. Reject unexpected native approvals. Route ordinary ASM approvals through ASM.

**Recovery and cleanup**

- Keep one CLI process per loaded Claude session, private native linkage, and unfinished-turn state. Resume known-clean sessions created under ASM-owned execution. Reject pre-bridge linkage without migration or replay. Never automatically replay uncertain submitted input or completed tools. Other execution errors need explicit recovery. Error text cannot clear older unfinished checkpoints.
- Foreground completion and warm input eligibility do not establish durable recovery safety. Keep linkage uncertain while background work can produce continuations, protocol activity is unresolved, or native output is unpersisted. Mark it clean only after confirmed reconciliation and rollout flush. Never replay input to recover uncertain state.
- UUID-stamp native input. After ASM interrupt, retain the process only with matching successful acknowledgement, explicit empty queued-input receipt, explicit idle, and stopped observed native children. Persist clean linkage before another turn. On missing confirmation, timeout, disconnect, or forced abort, kill the owned group and keep linkage dirty. Give bounded cleanup grace only to external tasks.
- Track native task ID and latest run ID. Resumes keep task IDs with later-sorting run IDs. Starts or terminals with older or missing run IDs cannot replace or clear a newer run.
- Resume aborted-streaming/aborted-tools results only after ASM-requested interrupt, the submitted UUID's cancelled-command receipt, and confirmed cleanup above. Continue with new input. Do not assume tools had no effects.
- A structured Claude auth failure is failed but resumable only after command reconciliation, confirmed idle, and no uncertain tools/children. Persist consumed input and clean linkage before reporting failure. After login, accept only new input.
- A structured Claude usage-limit failure (`api_error: usage_limit_reached`) on submitted input is likewise failed but resumable, even after tool work. Require the error result, reconciled commands, confirmed idle, and no live native agents or background work. Report separately from auth. Keep the idle process running: a temporary side fork cannot be started again. After reset, accept only new input. Other provider errors need explicit recovery.
- Bound frames, stderr, and cleanup waits. Kill only owned processes, preserve primary failures, and retain diagnostics only by request. Record pre-cleanup state separately from final exit status, including whether the CLI had stopped.
- Give each native CLI a unique inherited ownership marker. Combine markers and verified live ancestry to identify detached tools after CLI exit. Never signal ASM's process group. With unreadable environments, require verified ancestry; unrelated processes such as SSH sessions must not fail cleanup. Cleanup is not sandboxing: processes can escape by clearing markers and leaving observable ancestry.

**Images and lifecycle**

- Forward user images as typed blocks in text/image order. Preserve delivery checkpoints. Reuse processing for inline data/local attachments. Injected ASM context remains plaintext-only. Keep prepared image bytes inline in native history without OpenAI uploads/replay transformations.
- Validate before recording input or advancing checkpoints. Limit image input frames to 31 MiB, echoed/replay frames to 32 MiB, and ordinary text/control frames to 1 MiB. These are ASM transport limits, not provider limits.
- Explicitly reject unsupported history forks, rollback, runtime switches, and native-only operations. Suppress native prewarm and title inference. Native memory extraction can summarize saved ASM transcripts.
- Claude saved forks and attached Side chats use cross-provider portable history, with their own persistent sessions. Do not clone source runtime files or private state.
- For compatible idle same-model temporary Claude Sides, require a clean session without background work. Keep the native cutoff: use `--resume-session-at` with verified assistant UUID. Use portable history for active or historical snapshots. Keep temporary Side processes in memory and unload on close. Cursor does not support temporary portable Side.
- External threads use only Default collaboration, never guardian review. Keep mandatory managed-execution restrictions. Native turns have no total deadline. Keep cancellation and bounded cleanup during tool/background-agent work. Do not cap prompts by approximate tokens or truncate instructions. Claude manages context within transport limits.

**ASM-only Claude launch configuration**

`[native_runtimes.claude_code]` applies only to ASM-launched Claude processes: roots, children, Side, and cold resume. Require the ASM bridge for every launch. Never modify Claude user/project settings, skill files, installation, or authentication. Native managed policy and mandatory ASM child/Side restrictions remain authoritative.

- `page_context` is `inherit` (default), `when_present` or `off`. Filter only queued Page snapshot/instruction blocks, not user messages or unrelated context. `when_present` omits Page context only when queued snapshots explicitly show no Page and native history has no earlier Page context. Keep clearing snapshots after forwarding a Page. Missing or malformed snapshots preserve context. Leave Cursor and saved history unchanged.
- Capture launch settings per process. Config edits do not restart live processes or change loaded Page filtering. Cold launch/resume uses current config. Saved native prompt snapshots may still retain earlier context.

**Claude context and usage**

- Set native `--autocompact` windows from explicit child role limits, otherwise exact V2 `subagent_model_context_windows`. Reject values outside 100000–1000000 tokens. Set `CLAUDE_CODE_AUTO_COMPACT_WINDOW` in child environment and flag settings. Explicit values override inherited environment/user/project settings, not native managed policy. Unmatched children keep native defaults/environment.
- New roots use explicit `model_context_window` with the same range and precedence. If unset, keep native defaults/environment. Catalog metadata never sets it. Bind the window to linkage. Changes need a new child chat; mains/forks keep the main's original window.
- The window is soft, not a hard capacity or exact trigger. Native capacity clamps it; output reserves and compaction headroom reduce it. Keep native compaction controls without restoring disabled skills, tools, or customizations. Base context fill on native model capacity, not the configured window.
- Report checked process deltas from native cumulative model counters. First cold-resume usage and missing/reset counters are unknown, not zero. Warn that ASM totals/token budgets are partial. Thinking is part of output, not an extra charge. Calculate context fill from latest main request input/native capacity, not cumulative billing or children. Hide unknown metrics. Cache rate is that request's cached input share.
- Require stream-json result then session-idle completion. Verified protocol target: 2.1.296; queued delivery/background continuation: 2.1.309. Revalidate upgrades.
- Require bounded real Claude-main and ASM-parent/Claude-child checks with the intended installed CLI. For Desktop tools, validate in Desktop. Use the cheapest suitable live model. Do not replace installed binaries or global profiles for tests. Distinguish verified ASM capabilities, provider-only behavior, and untested lifecycle behavior.

### Cursor Agent thread runtime

Explicit `cursor-agent/<model>` selects official Cursor ACP (`agent acp`, JSON-RPC over stdio) for mains and ASM children. Apply shared native rules, including plaintext context and encrypted-input warnings. Do not add runtime copies, backend hierarchies, or event vocabularies. Branch only for the protocol, transport, and delivery differences below.

- Bundle Cursor model/effort metadata in `external_models.json`. Do not launch or authenticate either CLI during catalog loading/startup. Negotiate `parameterizedModelPicker` only at Cursor session start. Validate model and effort against advertised choices. Apply the actual `thought_level` parameter ID and confirm before prompting.
- Preserve saved Auto identity. Prefer exact advertised IDs; otherwise translate only `auto`/`auto-smart` to advertised ACP `default`. This is not model fallback. Reject other unavailable choices. Keep saved bracket-encoded ID parameters; explicit effort replaces the effort parameter. Custom catalogs stay authoritative. Metadata grants no access.
- Require `promptCapabilities.image` for ACP `session/prompt` image blocks. Reject unsupported images before history/checkpoint changes. Never discard images or substitute paths. Use shared inline-history and 31/32/1 MiB limits.
- Native runtime is identity. Never resume, fork, or switch between Claude and Cursor. Store full model IDs. Reuse Claude checkpoint path/format with Cursor-assigned session IDs.
- Keep login orchestration and process safety in neutral `native_login`. Separate adapters own CLI, environment, URL, reply, and status policies. Keep legacy `claude-runtime` pending-marker paths.
- `cursor login` uses official `agent login` with browser opening disabled and an allowlisted sign-in URL through shared interception. The CLI owns browser auth. Accept only `done` to check completion, never keys/tokens. Keep shared fail-closed reply/recovery rules.
- Use native authenticated ACP bootstrap. During setup/turns, detect only typed auth-required `error.code = -32000`. Use shared login guards and fixed provider instructions, not assistant prose or generic errors. Do not invoke ACP `authenticate` and its separate browser flow. Keep uncertain checkpoints dirty. If a CLI hides established-turn auth failures in prose, require explicit `cursor login`; automatic expiry detection remains unavailable.
- Strip child `CURSOR_API_KEY`/`CURSOR_AUTH_TOKEN`. Use `agent --force acp` for mains and children. Keep Cursor denies and team auto-run policy. Never pass `--approve-mcps`, `--trust`, or `--sandbox`. Cursor cannot disable its question tool; children retain it but refuse questions. Reject remaining approvals. Return `rejected` for plans, `skipped` for questions, and errors for other requests. Show refusals without hanging or falling back to local plans.
- Give each thread private `CURSOR_CONFIG_DIR` beside its checkpoint. At each spawn, refresh private user `cli-config.json`/`permissions.json` copies with native directory precedence. Keep native permissions and user model defaults. Keep login/rules/skills/hooks/MCP settings native. Never copy or extract credentials.
- Negotiate `clientCapabilities._meta.subagents` and require `sessionCapabilities.subagents`. Keep native children/background completion in the same ASM turn. Accept only session or announced-child updates. Show child tools/text in child activity cells, not the main transcript or ASM threads. Clean completion requires every announced child to report `completed`, `failed`, or `cancelled`. After cancellation timeout, `disconnected` does not prove stopping. Fail and keep dirty linkage.
- Cursor marks finished tools `completed`. Infer failure only from top-level typed `rawOutput`: nonzero `exitCode`, string `error`, `rejected: true`, or `permissionDenied: true`. Missing fields mean unknown. Tool failure does not fail the turn.
- On `session/new`/`session/load`, append only selected child-role text through ACP `systemPrompt`, `mode = "append"`. Before auth/session creation/loading, require advertised `sessionCapabilities.systemPrompt` object. Fail unsupported CLIs. Never use user prefixes, replacement prompts, or shared rule files. On cold reload, use current role text without saved-text comparison. Omit the field without applicable text.
- Cursor owns context. Child role limits and `subagent_model_context_windows` neither set Cursor compaction windows nor trigger Claude range validation.
- Limit each login/setup request, including stdin submission, to 60 s. Keep turns unbounded. Discard `session/load`'s native-history replay. Reset the model after every new/loaded session.
- Cursor shell groups can outlive ACP. Use shared ownership markers after leader exit. Never kill by socket/path/name. Stop all identified owned processes. On interrupt, send `session/cancel`. After `stopReason: cancelled`, stop the process tree. Only successful descendant cleanup permits clean linkage. Load the session on the next turn. Missing acknowledgement, cleanup failure, or other failure keeps linkage dirty.
- Treat Cursor usage/context fill as unavailable, never zero, in ASM accounting/`/status`. Exception: required numeric `codex exec --json` `turn.completed.usage` fields stay zero when unknown, with unavailable-usage warning. Keep this wire shape until owner-approved API changes.
- Children use the local CLI bridge, not ACP MCP. Send empty `mcpServers` on `session/new`/`session/load`. Do not restore policy-blocked transport or bypass team policy. Deliver active-prompt mail/board notices in canonical wait CLI results. Record mail and advance dirty checkpoints before returning it, without next-prompt replay.
- Give every nonempty delivery a unique receipt. Clean linkage requires that receipt in a successful Cursor root shell result, not just bridge/HTTP completion. Missing or truncated confirmation needs explicit recovery. Do not retry potentially side-effecting tools. Keep queued input/next-turn followups ordered. Return Cursor's final answer; no Send now. Pre-bridge children lack messaging records: fail closed on cold resume and require a new thread. Mains are unaffected.
- Verified CLI: 2026.10.22-27dcf5f. Revalidate upgrades.

### Minimal diagnostics

Do not add or depend on optional diagnostics or telemetry. Leave existing ones alone unless they block the change. Make a very small edit if sufficient. Otherwise, remove only the blockers.

When removing a collector, preserve required Codex app config and protocol behavior. If the app needs the collector, report the conflict before removal.

If app behavior could change, verify the affected flow in the Codex app with the candidate backend. Apply the [shared compatibility gate](#shared-compatibility-gate) where applicable.

- Default `diagnostic_log_capture = false`. This startup policy controls Rust stderr/file logs, persistent diagnostics, feedback ring/continuous tags, optional TUI replay, and rollout-trace recorders. `RUST_LOG`, `log_dir`, and recorder environment variables cannot override it.
- To opt in, set `diagnostic_log_capture = true` and restart affected processes. Keep existing filters. An explicit `log_dir` also routes SQLite logs unless managed `sqlite_home` overrides it. Pass the policy to the code-mode helper. Inherited helper settings cannot override it. Standalone helpers without user config require explicit opt-in.
- With capture off, omit feedback capture and its tracing layers. Do not open or migrate diagnostic databases or run insertion, pruning, or reclamation workers. Leave historical files unchanged at ordinary startup.
- Explicit history inspection and feedback export can read old logs without capture. Thread deletion removes historical diagnostics through existing-only, on-demand access. Never create missing files. Preserve feedback submission/upload errors. Do not invent evidence.
- Preserve direct errors/warnings, exit status, protocol events, saved conversation/tool history, subagent relationships, memory, and operational state. Keep detached-daemon startup/fatal stderr and Windows sandbox security-audit logs separate and active. Do not weaken security or change product analytics or account usage.
- Disable the built-in Statsig metrics exporter, including in optimized releases and on account reload. Keep its config spelling. Default `otel.metrics_exporter` to `none`. Preserve explicit OTLP collectors. Without a configured exporter, opt-in `runtime_metrics` uses a manual reader with no timer or network exporter. Do not scan session-storage sizes for unsolicited telemetry. Verify disabled paths with focused real-binary scenarios, not broad upstream retesting.

### System1

Opt in with `[system1]`. Require `features.step_model_switching` and `features.reasoning_effort_override`. Toggle with `/system1`. System1 runs in the shared backend, not the TUI. Clients must use this backend; System1 does not replace Desktop's backend. Use HTTP for Decisions, not the Responses WebSocket.

Accept former `[adaptive_reasoning]` config, including deny-only roles, and former rollout fields on resume. Emit only canonical names. Preserve layer precedence across spellings. Reject both tables in one layer. Before canonical writes, rename any old table. Reject conflicting writes without changing the file.

| Setting | Default | Validation |
| --- | --- | --- |
| `enabled` | `false` | Opt-in only |
| `profile` | Absent | Select a named `profiles` entry before enabling |
| `profiles.<name>` | Empty map | Explicit `protocol` (`openai` or `typesafe`), nonblank `model`, ASCII identifier `api_key_env`; optional full `endpoint` URL |
| `profiles.<name>.include_images` | `false` | Allow current-turn user images at this endpoint |
| `min_effort` / `max_effort` | `"low"` / `"max"` | Ordinary levels; `min_effort <= max_effort` |
| `decision_timeout_ms` | `200` | 1–5000 ms |
| `max_context_bytes` | `8192` | 128–32768 UTF-8 bytes across all Decisions input items |
| `update_on` | `["turn_start", "tool_result"]` | Only these triggers |
| `decision_every_n_turns` / `decision_every_n_steps` | `1` / `1` | Positive integers; see [cadence rules](#space-decision-calls-by-turn-and-step) |
| `low_effort_description` / `medium_effort_description` / `high_effort_description` / `xhigh_effort_description` / `max_effort_description` | Built-in text below | Nonblank; at most 4096 UTF-8 bytes each |
| `rubric_instructions` | Built-in rubric | Nonblank; at most 4096 UTF-8 bytes |

**Connections.** Profiles select connections and share bounds, cadence, evidence limits, timeout, and rubric. Profile names have no built-in meaning. Never infer protocol from names, endpoints, or credentials. Accept `backend` as a `protocol` alias, not alongside it. Validate every profile. Reject unknown profiles or protocols without fallback. When disabled, selection is optional. Never infer a connection at startup or runtime activation.

Example OpenAI Decisions connection:

```toml
[system1]
profile = "decision_api"

[system1.profiles.decision_api]
protocol = "openai"
model = "gpt-6-luna"
api_key_env = "DECISION_API_KEY"
```

Resolve the connection once per session, on first use, including failed credential attempts. Keep protocol, endpoint, and model fixed across warmup, decisions, and config reloads. Both adapters share cancellation, deadline, allowed-choice validation, and recovery. Do not use automatic failover or adapter-internal retries.

Without `endpoint`, use the protocol's official HTTPS endpoint. Overrides specify full destinations, not formats. Append no path. Require HTTPS except for loopback HTTP. Reject URL credentials and fragments. Reject blank endpoints at config validation. Reject other invalid URLs at client construction without exposing them in errors.

Choose custom endpoints deliberately: they need not belong to OpenAI or TypeSafe, and receive bounded conversation evidence and credentials. Read only the profile's `api_key_env`, separate from main-model authentication. Never provision credentials or fall back to `OPENAI_API_KEY`. Sanitize diagnostics; never log keys or evidence. Do not route Decisions through Guardian.

**Choices.** Adapt ordinary roots and spawned subagents, not internal workers. Use the selected backend to choose supported effort within bounds. Keep Ultra/Persistent, models, service tiers, execution modes, permissions, and saved defaults unchanged. With fewer than two efforts, retain current effort and skip Decisions and warmup.

Use model-specific choices and match guidance by effort name, not model default. Keep fixed direct-execution descriptions for `none` and `minimal`.

Keep descriptions separate from the selection/safety rubric, even with a replacement rubric. Do not remove or duplicate them. The built-in rubric refers to the offered range, not possibly unavailable tiers. Defaults:

```toml
[system1]
low_effort_description = "Direct execution or reporting a verified result when no judgment remains."
medium_effort_description = "Settled work that still needs care: make a scoped edit or verify a result."
high_effort_description = "Substantive work within the model’s normal range, including implementation or interpretation that requires care."
xhigh_effort_description = "Distinguishing plausible causes, reconciling interacting constraints, or catching subtle, consequential errors."
max_effort_description = "Exceptionally difficult problems requiring the maximum supported reasoning budget."
```

Before the first model request, weigh the reasoning needed to understand the task and choose an approach. On later requests, budget for work the current evidence allows, not for completed work or analysis that must wait. For direct execution, use the lowest sufficient offered level. For settled work that still needs care, use the matching choice description.

Keep the `turn_start` and `tool_result` triggers. For a later request with new user steering, use steering guidance. Weigh what the user changed and any analysis the existing evidence still requires. Do not add a trigger, rubric field, or Decisions call.

Use configured `rubric_instructions` verbatim instead of the built-in rubric. In both backends, prefix decision instructions with trusted baseline context, separate from evidence:

- Anchor on the trusted task baseline. Refresh on accepted manual baseline changes, not adaptation. Supply `task_baseline_effort` and `last_effort` (captured effective effort of this turn’s previous ordinary model step). Report absences as `unspecified`, not the model default.
- Use the header `Model: "<name>" - "<description>"` with both values JSON-quoted. Trim descriptions to 1024 UTF-8 bytes at character boundaries. For missing, blank, or fallback metadata, use `unavailable`. Keep the decision-service model and warmup unchanged.
- Send only allowed main choices with selection guidance capped at 4096 UTF-8 bytes each, not catalog effort descriptions. Keep the synthetic low/medium warmup pair.
- Reassess the whole next response against the task baseline. Use model capabilities for the task, not a fixed adjustment by name. Enforce allowed choices and runtime limits in code. Keep classifier instructions and results outside the main conversation.

**Evidence.** Collect genuine user, assistant, and tool evidence, newest first. Exclude injected user instructions, compaction output, and encrypted reasoning. Give the first evidence slot to this turn’s latest hook-accepted user message, within the text budget. Keep a distinct original task separate and before recent tool output. Do not repeat either as a user history row. Do not label inter-agent assignments as user corrections or borrow another turn’s prompt.

By default, send no images to Decisions. Require `include_images = true` in the session-fixed profile to send current-turn user attachments to its endpoint. Model capability does not authorize a second endpoint. Accept no top-level image permission. Scale scan limits with the budget: at least 128 history/input items and 32 content parts, up to 512 items and 128 parts at 32768 bytes. Older history can be omitted. Do not promise a complete transcript.

With opt-in, send at most one accepted inline PNG/JPEG per decision, including resolved local images. Cap the encoded data URL at 4 MiB, separate from the text limit. Do not fetch URLs or send file IDs, tool images, previous-turn attachments, or hook-rejected images. Skip image-only decisions without eligible images. Preserve main-model input. Select the first image that sanitizes successfully within the existing scan limits, deadline, and 8 MiB aggregate candidate budget. Strip metadata and reduce oversized dimensions. For JEV, pad short sides without stretching to meet its 280–1568 pixel limits. Malformed media does not block later decisions. OpenAI uses a low-detail image content part. TypeSafe uses its experimental image header and multipart asset format. Never log attachments in Decisions diagnostics. Image requests can cost more and exceed the default 200 ms deadline. To allow five seconds for visual and text decisions, set `decision_timeout_ms = 5000` under `[system1]`.

**P0 manual review:** choice descriptions can exceed 1000 tokens each. Review the 4096-byte bound. Bound evidence and rubric separately. Split larger evidence into ordered OpenAI messages or TypeSafe state-array strings of at most 8192 UTF-8 bytes each. Prefer line breaks. Preserve text and the default single-message request. TypeSafe uses a string state when one chunk suffices. The 4096-byte rubric cap excludes the bounded description/baseline prefix. Use byte caps to keep items below 10K tokens even when they exceed 1000 tokens. Keep evidence outside the main conversation.

**Execution.** Ingest tasks and steering before skip checks. Invalidate stale decisions. Check cadence, triggers, and recovery before bounded evidence rendering. Keep the original deadline. Finish rendering before client setup.

- At eligible session start, send one tiny synthetic background warmup with separate minimal instructions. Skip without triggers or supported effort in bounds. Reuse its pooled client. Discard its answer without changing history or settings. Limit warmup to three seconds. Abort and join it at shutdown. No retries, periodic pings, or warmup settings. Warmup can add one billable request with no guarantee of backend or connection warmth.
- Before selected main steps with an enabled trigger, collect current evidence. Await Decisions and validation in the turn task, without a separate job or completion channel. Capture immutable settings before running the model. Do not retry or wait for a busy settings-publication permit. Only warmup and log writing run in the background.
- Publish current results through live next-step settings within the original deadline. Keep captured requests unchanged. New accepted user, assistant, or tool evidence invalidates pending decisions, not applied settings. Reject results after cancellation, task replacement, shutdown, settings changes, or deadline expiry.
- Native Claude Code and Cursor use the same opt-in mode, but only `turn_start`, without warmup or mid-turn adaptation. Native Decisions uses validated input without ASM `UserPromptSubmit` filtering of text or enabled images. Intersect configured bounds and pinned model choices with native control support. Cursor must advertise choices on the active session. Keep configured effort without a decision. An accepted decision is not a settings receipt. Confirm effort on the same native session before submitting input or reporting it active. Abort before submission if confirmation fails.
- Reuse live validation and publication. Preserve the request-effort pin, trusted append-only updates, reconnects, and explicit compaction handling.
- In shared history-to-request projection, exclude effort updates without harness provenance. Then keep only the last complete envelope in each adjacent update run. Keep retained messages, reasoning, tool declarations, and results as separators. Preserve stored history and the request-effort pin. Apply this to filtered forks and replayed checkpoints without transport workarounds. A changed tail can lose prefix/cache reuse. Let incremental checks choose full input. Do not force a session reset.

**Mode and display.** Manual model or effort changes invalidate pending decisions without changing mode. Refresh baseline on effort changes. Refresh the active-turn baseline on persistent thread settings changes. Preserve captured requests and the live-step contract. For unsupported models, skip adaptation without disabling it. Log mode transitions. Ordinary turn-start and spawn settings do not disable it. `/system1` toggles only current runtime mode, not saved defaults. Keep mode separate from captured settings.

Spawned children copy parent mode once, not later toggles or parent errors. Preserve child mode across live eviction/reload, not backend restart. Leave internal workers unchanged. Keep the triggering assignment separate from queued mailbox updates. Queued notes cannot replace it or unlock encrypted assignments. Only readable replacement tasks can. With no readable task for an encrypted assignment, log fallback and retain effort. Do not send ciphertext or guess tasks.

Report mode and revision at attachment and transitions; ignore older revisions. Report errors in backend runtime state regardless of diagnostics. Include health at attachment and live child reload. Show `<effort>·auto` when enabled without error, including startup and decision waits. Show `<effort>·err` after errors without disabling adaptation. When off, show effort alone. Use baseline before the first captured step. Markers show health, not promised changes. Unsupported steps retain effort without Decisions. Before eligible main requests, send captured effort through `turn/reasoningEffortUpdated`, including fallbacks. Toggles do not change running steps.

Keep captured effort and turn identity in live TUI snapshots, outside saved history. Restore on view switches without stale history settings. Clear on a new turn or manual change. Send transient effort notifications directly to clients, without rollout persistence before the main request.

**Status.** `/system1 status` reads current mode, selected/last captured effort, health, and session-fixed connection identity. Show backend/protocol, model, captured profile name, and client initialization. Show unresolved identity before resolution. Do not present reloaded config as active. Initialization does not prove request success. Do not change settings, send a turn, resolve profiles, inspect credentials, or start warmup.

Read only fixed-size in-memory totals for the loaded runtime. Exclude warmup, cadence/backoff skips, and incomplete tasks. Count unchanged decisions as accepted. Count only applied effort changes. Count client setup failures separately. Show total/last completed-decision wall time, not estimated net overhead, cost, or savings. Totals reset on unload/restart. Children have separate totals. Do not scan rollouts, persist totals, add per-step notifications, expose credentials/endpoints, or resume unloaded threads. Older servers show unavailable stats.

**Errors and logs.** Use yellow warnings with category, fallback effort, and cooldown duration or recovery-blocked state. Deduplicate by failure episode. Warn on category changes, not identical errors. Do not invent cooldowns for old or restored events. Use bounded send/body-read codes and typed HTTP classifications where available. Preserve legacy transport events. Do not expose raw errors, URLs, or credentials. Show warmup timeouts and their deadline. Trace backend, model, elapsed time, and remaining deadline at request start. Omit content/endpoints.

With `diagnostic_log_capture = true`, append sanitized credential failure, API outcome, and acceptance metadata to each session's `decision.jsonl`. Do not log evidence, rubric, bodies, or authentication headers. With capture off, create no file or writer. Bound buffers and storage. Never wait for disk on the decision path. Report dropped records and writer failures separately. Crashes can leave attempts without end records.

Report latency and fallbacks without sensitive input. Deadlines bound result eligibility and local publication, not API latency. Preserved prefixes do not guarantee cache hits. Verify live update order and backend acceptance with isolated, cost-limited real binaries before making claims.

#### Space decision calls by turn and step

A turn is a request and its work. A step is an ordinary model request, not a tool call. Select turns 1, 1 + N, 1 + 2N, ... . Defaults select every turn and eligible step. Reset step numbering and accepted-decision spacing each turn, not recovery.

- In selected turns, the first eligible step is due. After a current decision is accepted at step S, the next due step is S + N. This includes unchanged effort. With `decision_every_n_steps = 2`, decide before steps 1, 3, 5, ... when eligible and successful.
- Due steps must pass `update_on` and eligibility checks. Ineligible, failed, cancelled, or superseded decisions do not advance spacing. Retry at a later eligible step when recovery permits, with fresh bounded evidence.
- Retain effective effort between decisions and across skipped turns except on current main-step decision failure. Restore the latest trusted configured/manual baseline for the next request and during cooldown or blocked recovery. If unset, use the model default. Never use an adaptive selection as baseline. Role bounds restrict choices, not configured/manual fallback. Preserve effort for unsupported models or efforts, Ultra/Persistent, cancellation, and warmup-only failure. Never stop the main step on Decisions failure.
- Keep per-agent recovery across turns. Transient failures start a one-second cooldown. Consecutive failures double it up to 30 seconds. After expiry, probe at the next due eligible step. Cooldown skips do not extend it. Do not sleep the main turn or add background recovery timers.
- Missing or invalid credentials, invalid requests, rejected settings, and HTTP 4xx except 408/429 block Decisions until explicit off/on. Off/on resets recovery and makes the next eligible step due, even in a normally skipped turn. It does not clear health or reload initialized credentials. Restart to change cached credentials. Missing credentials are not cached. Set them in the backend environment and retry off/on. Retry HTTP credential failures with the same client.
- Only a current main decision that passes acceptance guards clears health and resets recovery, even if effort is unchanged. Warmup errors affect health without arming main-step fallback. Warmup timeout leaves failure count, cooldown, and blocked state unchanged. Other warmup errors use normal recovery. Warmup success, cancellation, and supersession neither clear health nor anchor spacing.
- Children inherit cadence config, not counters or recovery. Count ordinary turns at start, before setup, including setup failures and cancellations. Do not recount continuations. Exclude warmup, compaction, and transport retries from ordinary steps and separate turns.
- Counters and recovery are runtime-only. Resume, restart, and eviction reset turn numbering to 1. Do not rewrite history. Larger intervals reduce decision frequency, not necessarily runtime, and can retain unsuitable effort. Measure before claiming speedups.

### CLI resume matching

`asm resume <text>` searches eligible active sessions across working directories under existing provider/source rules. Match trimmed, lowercase-normalized substrings of displayed names or unnamed-session previews, or match UUID prefixes. Resume a unique match or filter the existing picker to matching IDs. Keep direct full-UUID lookup. Do not prefer exact names. Keep exact matching and duplicate rejection for fork, archive/delete, and queue commands.

### TUI automatic session names

Keep `[auto_rename]` TUI-only. Preserve Desktop naming, conversation model/history, and `/rename` suggestions. Defaults: enabled, first user message, user-message context, initial naming only, 36 characters, 960-byte initial prompt.

Use `gpt-6-luna`/low only for OpenAI with a ChatGPT account and an available catalog entry. Otherwise use the conversation model. Keep upstream model selection for manual `/rename`.

- `enabled`: default true. `model`/`reasoning_effort` affect titles only; omission keeps defaults. Preserve explicit models server-side. Reject unavailable models without fallback. Custom-provider models need no catalog entry.
- `first_trigger`: `first_user_message` (default) or `first_completed_turn`. Exclude failed/interrupted completions.
- `context`: `user_message` (default) or `recent_conversation`. Recent context includes the initial request and substantive user/assistant messages, not commentary/tools.
- `max_context_bytes`: 128–8192 source bytes, excluding added guidance. Omission keeps the original budget.
- `recent_message_limit`: 1–32, default 8, counting the initial request. At 1, keep only the latest message. Under byte pressure, drop earlier messages and keep markup complete.
- `max_title_chars`: 1–128, default 36. Optional positive integer `max_title_words` is prompt-only: request at most that many words. Never truncate by words. Omission keeps “under five words where possible.” Bind prompt and character limits to their request.
- `additional_naming_guidance`: append at most 512 UTF-8 bytes without changing fixed rules/prompt. No `instructions` alias. Cap the complete prompt at 9500 bytes. **P0 manual review:** larger opt-in budgets can exceed 1000 tokens. Keep the 10K-token item limit.
- `auto_update`: default false. Refresh requires `enabled` and `auto_update`. Include the captured previous title as bounded JSON-string data. Request changes only for meaningful task changes, preserving core subject and wording. Routine progress is insufficient. Keep initial/manual prompts unchanged and fit guidance without splitting markup.
- Save each generated title in one client-local `CODEX_HOME` record scoped to the local/remote app server. Eligibility requires a record matching the saved name. Protect named sessions with missing, corrupt, or mismatched records. `/autorename` can opt in. Save initial automatic titles for resume even without updates. Retain counters across view switches. On resume, reset the interval; exclude completed turns.
- `auto_update_interval_turns`: positive integer, default 5. Count successful displayed user-turn completions. Exclude duplicate/failed/interrupted/background/helper/replay events. Start at most one attempt per interval. Reset on attempt start, not success. Failure retains ownership and waits another interval. Count during pending requests but start none until settlement.
- Track unnamed opt-in threads before initial requests. Require a successful save for ownership. Keep only the latest user-turn/item IDs, one ordering anchor, and at most 8192 UTF-8 user bytes. Preserve context through event-buffer eviction during long turns or pending initial naming. Count completion if first-user-message naming saved before turn completion. For completed-turn initial naming, count only later completions.

`/autorename` immediately generates and applies one title using the same context/model/effort/budgets/limits/guidance. Only this action ignores `enabled`, `first_trigger`, `auto_update`, and interval. Later refresh obeys them.

- Use the latest substantive user text or shared bounded recent collector. Allow replacement of existing/resumed names. Preserve `/rename` suggestions and supplied names.
- Supersede pending background naming. Success restores persisted eligibility, even for identical titles. Failure preserves name and eligibility. Deduplicate and report explicit failures. Discard results for undisplayed threads or changed saved names. Reuse app-server APIs; do not change Desktop.

Naming safety:

- Preserve background requests, manual-name precedence, cancellation, and originating-thread checks. Before replacement, compare current server and captured names. Skip RPC for identical generated names.
- Before RPC for `/rename`, dashboard naming, or explicit `set_thread_title`, remove eligibility even for same-name requests. Abort if removal fails. Later RPC failure can leave refresh disabled.
- Generic cancellation clears only in-memory ownership. External name changes cancel automatic work.
- Cap records at 512 UTF-8 bytes/128 characters. Validate reads and write atomically. Read only at attachment or recurring attempt/save, not streaming/rendering.
- Recheck persisted eligibility before refresh to honor another TUI's opt-out. Fail closed on corrupt/unreadable records. Report persistence errors.
- File/server writes are not jointly atomic. Same-name external renames and concurrent reads/writes remain best-effort. Do not add shared APIs, rollout records, or Desktop policy to solve these cases.

### Account-security setup reminder

Never fetch or display optional Daybreak setup reminders, even on reconnect/account refresh. Suppress loaded events. Preserve authentication, security enforcement, approvals, account email, backend usage banners, and upstream reminder rendering/checks. Never weaken security.

### Content-provenance wire compatibility

Keep upstream per-content provenance in local history, prompt assembly, and compaction. Until deployed endpoint support is established, omit only `content_item_metadata` from shared ordinary Responses HTTP/WebSocket request copies, including Responses Lite. Keep other internal metadata and the original prompt. Responses Lite does not guarantee support. This withholds server-side per-part attribution; it does not enable full wire provenance.

### Standalone request audit

- Keep `debug prompt-input`. With `debug prompt-request`, export versioned JSON: effective base instructions, production request, ordered input, full advertised tool definitions, Responses Lite prefixes, and optional output schema.
- Reuse the app-server extension installer and request builder. Keep audit work out of ordinary inference.
- Default to ephemeral. Require `--allow-session-state` for session records or persistent state-backed tools. Setup can opt in within isolated temporary homes. Never dispatch queued input.
- Label the export a fresh standalone debug turn, not a live-session/exact transport capture. Report omissions: history, hooks, input-triggered skill/plugin injections, uncaptured tools, auth-dependent metadata, and server additions.
- Never send inference or claim startup has no network/state effects. Exports can contain sensitive instructions and tool content.

### Model catalog ownership

- Keep [`codex-rs/models-manager/models.json`](../codex-rs/models-manager/models.json) upstream-owned. Accept upstream sync changes. Do not freeze it or make personal/fork edits.
- Keep personal edits only in `/home/trungnt13/codes/tngo-workflow/configs/model-catalogs/models.json`, never in the built-in catalog.

### Role-scoped skill visibility

Optional `allowed_agent_roles` in `[[skills.config]]` limits normal catalog visibility and selection to listed child roles. Example: `name = "review"`, `enabled = true`, `allowed_agent_roles = ["reviewer"]`.

- Match the current child's explicit role, not its ancestor or the displayed TUI view. Omit to retain visibility; `[]` hides matching skills from all roles.
- Root, ordinary Side, saved user forks, and unnamed/nonmatching children do not match. Reviewer children match in fresh or V2 full-history context. Resume restores their role.
- Preserve parent enablement and permissions. Role overrides only deny: they cannot re-enable skills. Scope restrictions survive enables through another matching selector.
- Share local snapshots for identical effective visibility. Do not key by task, thread, or raw role. Apply scope in config-aware and cwd caches. Keep stable prompt lists, append-only availability updates, and existing context budgets.
- Restrict only visibility, not access or filesystem/script/sandbox/network/approval permissions. History can retain old lists or skill content already read. Never scrub history.
- Use a supporting binary before configuring `review` and `oracle`. Keep reviewer descriptions generic to avoid exposing skills in root's role list.

### Role configuration restrictions

To prevent a transcript fork, set `fork_turns = "none"` in a role TOML file or its `[agents.<role>]` table. Supply the table's description directly or through its role file. Start a new parent chat after changing this policy.

Apply the policy before preparing a multi-agent V2 child, even if the parent requests `all`. Omission keeps the spawn request. Preserve explicit retained items for fresh children and do not restrict the task message.

Role TOML can disable web search with `web_search = "disabled"` or System1 with `[system1] enabled = false`. Apply only explicit disabling. Otherwise inherit, including the parent's live System1 toggle. Roles cannot enable either against parent restrictions. Keep System1 restrictions through child spawn/reload.

Top-level `collaboration = false` in role TOML applies `[subagent_tools] collaboration = false` only to that role's spawned agents. Disable messaging, message-board, and spawn tools, but deliver the final result to the parent. `true` or omission inherits the parent setting.

Roles can override `[system1] min_effort` and `max_effort` independently. Inherit omitted bounds. Validate the merged ordinary-effort range and intersect it with model-supported efforts. Skip Decisions with fewer than two choices. Retain role bounds through model changes/reload. New children resolve their own role without ancestor bounds. Constrain decision choices, not manual effort or fallback.

Role TOML can replace the rubric for that role's subagents with `[system1] rubric_instructions`. Use the base setting's nonblank requirement and 4096 UTF-8 byte limit. Higher-precedence managed rubrics win in effective config and Decisions. Omission inherits the non-role rubric or uses the built-in rubric. Restore the non-role setting before applying a nested child's own role. Keep selection choices and other System1 settings unchanged.

After publishing and installing a supporting release, set the personal reviewer to `min_effort = "high"`, `max_effort = "xhigh"`. Do not change it earlier. Align projected role settings with runtime. Do not add arbitrary role fields or change session authority.

### Model catalog instruction visibility

Top-level `include_model_catalog_in_spawn_agent_list` defaults to `true`.

- False omits generated model/effort/tier lists and references from V2 spawn descriptions and `<model_catalog>`. Preserve custom descriptions, arguments, loading, validation, and history. Invalidate retained lists normally when they no longer apply.
- Keep inclusion independent of `model_catalog_in_context` placement and `expose_spawn_agent_model_overrides` arguments.

`load_external_models_json` controls compact exact-alias discovery independently of full-catalog visibility or placement. True (default) includes it in V2 spawn descriptions; false omits it.

- List only picker-visible, backend-compatible entries with bundled aliases: native runtimes or configured API providers allowed by managed requirements. Discovery proves no runtime, credentials, or service availability.
- Hide the list if V2 model override arguments are disabled.
- Do not automatically change personal configuration. Label truncated model previews as partial in unknown-model errors.

### MCP resource tools

Top-level `mcp_resource_tools_enabled` defaults to `true`. False removes `list_mcp_resources`, `list_mcp_resource_templates`, and `read_mcp_resource` registration, calls, and declarations from direct tools, Code Mode, and `ALL_TOOLS`. True keeps existing registration conditions. Keep MCP connections, other server tools, apps/plugins, and the shared TypeScript preamble independent. Apply at config load. Do not add live reload or rewrite history.

### Web search copyright instruction visibility

Top-level `include_web_search_copyright_compliance` defaults to `true`. False removes only terminal Copyright compliance from local `web.run` descriptions. Keep preceding quotation/word limits, arguments, execution, search modes, history, hosted `web_search` instructions, and model safeguards. If the subsection changes or is not terminal, warn and keep the full description. Apply to standalone cached/indexed/live modes.

Update setup-managed profiles for these three settings only after a supporting CLI release.

### Saved forks and upstream sides

- Stack-budget exception to async instrumentation: leave `start_companion_conversation` and `select_agent_thread_and_discard_side` uninstrumented. Tracing wrappers exhaust the unchanged 16 MiB UI stack by duplicating large futures in unoptimized poll frames. Keep companion event-dispatch futures boxed. Do not increase stack limits. Construct startup and run-loop event futures through a separate non-inlined closure at existing allocation sites. This keeps construction temporaries out of the run-loop poll frame. Do not substitute a nested selection allocation.
- Ordinary same-provider root forks retain the effective parent prompt-cache routing key regardless of thread/session ID. Portable forks use new keys. Shared routing does not guarantee cache hits.
- New `/side` and `/btw` forks, including attached workspace Side chats, inherit the parent model. Optional top-level `side_reasoning_effort` and `side_service_tier` (for example, `"fast"` or `"default"`) override inherited effort and tier independently. For native runtimes and third-party providers, silently use Default if the initial Side tier is unsupported. Validate explicit tier changes normally. Exclude ordinary saved forks from Side defaults. Keep native-runtime admission restrictions for temporary native Claude forks.
- Optional top-level `side_prompt` adds default developer instructions to `/side` and `/btw` conversations, including attached workspace Side chats, `/sync`, and recognized Desktop side chats. Example: `side_prompt = "Act as a critical reviewer. Keep answers concise."` Keep existing behavior if unset or whitespace-only. Append nonblank text unchanged before built-in side policy; do not duplicate on `/sync`. Inherited context skips initial developer instructions, so forks also inject the custom developer message once beside their reference-only boundary. Retain the prompt for compaction. Exclude main and ordinary saved forks. Preserve side boundaries, permissions, and capability restrictions.
- Use these initial defaults for remote Desktop side chats, including attached forks and new side threads. Recognize only the complete shipped Desktop 26.1027.51404 developer block. Unknown wording keeps ordinary behavior. Recognize the complete shipped side boundary at the start of its user message, alone or followed by a paragraph separator. Treat appended reference paragraphs as opaque context, not a protocol. Do not recognize changed boundary wording. Keep the signed Desktop bundle and API shapes. Synchronize effective settings after client context injection. On the first new Side turn, correct only known inherited effort and tier, including inherited null values. Persist explicit effort and tier normally. Select models through ordinary thread settings and turn request handling. Keep ordinary saved forks and prediction forks unchanged.
- `/side` and `/btw` use the upstream storage-aware lifecycle for GPT and Claude. Save and attach eligible workspace Side chats; otherwise keep them temporary. Cursor does not support Side. Preserve upstream navigation, saved-chat reopening, archive, and rename.
- Side uses normal permissions and approvals, without extra question-only, non-mutating inspection, or explicit-mutation/escalation gates. In both boundary and developer instructions, keep inherited history reference-only, wait for a new request, prohibit subagents, and retain storage-specific attachment restrictions. Temporary Side cannot attach a worktree. Attached Side follows upstream workspace attachment rules.
- In Side, `/sendlast [extra message]` sends the raw last completed assistant reply plus optional text as main user input. Add no prefix and change no navigation or settings. Atomically start or steer main. Reject, never truncate, complete items above 10,000 UTF-8 bytes. Require manual review on changes: items can exceed 1K tokens.
- `/sync` replaces idle Side with main's latest context. Retain model/effort/tier and use upstream storage eligibility. Prepare and select the replacement before closing the old tab. Discard old temporary history only after success. Keep the old attached chat saved; never auto-archive or auto-delete it. Keep storage-matched Side boundaries. Context refresh does not guarantee cache hits.
- In idle Side, `/fork [name]` saves an independent ordinary conversation:
  - No navigation, pairing changes, or automatic work. Copy available inherited context and Side history. After paginated main compacts, do not recover older raw entries outside model context.
  - Persist current model/effort/tier through resume. Append durable ordinary-role context; never rewrite history.
  - Default to `<main title> (fork)` or `Untitled (fork)`. Keep explicit names.
  - Keep raw replay in memory only for temporary root forks. Normally append only new items. Copy retained history only on explicit save. Temporary chats stay pathless and unlisted until saved.
- Remove `/parallel`. Existing saved parallel chats remain ordinary threads with unchanged IDs, history, permissions, and reference-only boundaries. Do not rewrite history, restore pairs, attach counterparts, or delete saved conversations.
- Remove legacy `/parallel` display metadata readers and transcript trimming. Display canonical ordinary history for existing saved chats. Do not migrate, rewrite, or clean up personal metadata files.
- Keep upstream Side lifecycle, attached workspace sides, and tab controls. Keep temporary Side restrictions and the `/sendlast`, `/sync`, and non-navigating `/fork` exceptions above. Stock clients see ordinary threads through unchanged APIs. Files and explicit global config stay shared.

### Recap session ID

End manual and automatic TUI recaps with dimmed `Session: <id>` after any next action. Use the displayed conversation ID, not the generation thread. Preserve generation and timing.

### Turn completion footer

Use one dimmed logical line: `Worked for 5s • 11:05 AM • 12.4k in • 9.8k cc • 820|600 ou • 3ag • <session-id>`.

- Preserve timing, optional runtime metrics, and narrow wrapping. Use displayed conversation ID, not turn ID.
- Show provider-reported turn totals, including compaction. Include cached input in input and reasoning in output. Omit unknown counts; never substitute context estimates or session totals. Reuse saved usage. Paginated history may omit counts without migration.
- `Nag` counts distinct related subagents observed running during a live turn, including descendants and already-running agents. Keep counts across view switches. Omit restored counts without an observed start. Keep tokens parent-only and requests/accounting unchanged.

### Copy by turn

In fullscreen `/copy`, start on the latest completed reply. Use Up/Down to move between turns and Left/Right to select a prompt, code block, or quote within a turn. Enter copies the selection. Preserve `Ctrl+O` and inline copy behavior.

### Copy session ID

`/copyid` copies only the displayed session ID using the existing clipboard. Do not open `/status`, reply, or show a picker. Allow during tasks, sides, and parent-owned child views. Report missing IDs and clipboard failures. Preserve `/copy` and platform support.

### ASM branding

Use ASM for README heading, repo description, release titles/introduction, workflow step descriptions, fork TUI labels, and terminal title. Credit OpenAI Codex. Keep upstream product/service names.

Use ASM in local CLI help/TUI messages and `asm` in command hints, including resume, agents, login, and MCP setup. Hints require a user-provided alias/wrapper. Preserve executable and CLI parsing names, internal identifiers, model-visible prompts and parsed markers, upstream names, and installer commands. Keep the `codex` executable/version prefix and crate/config/state/archive/tag/workflow/job/check identifiers. Never redirect upstream links/installers for branding.

On Linux, set `/proc/self/comm` for normal CLI/app-server/code-mode-host/exec-server to `asm`/`asm-app-server`/`asm-code-host`/`asm-exec-server`, respectively. UI plugin helpers share `asm-code-host`. CLI-dispatched sandbox/filesystem helpers keep upstream labels. Preserve filenames, command arguments, helper dispatch, package discovery, and protocol identity. Leave other platforms unchanged. Warn on label failure without blocking startup.

On Linux, display `asm` as argv[0] for normal CLI and standalone app-server commands. Use `asm-code-mode-host` for Code Mode host (including UI plugin mode) and `asm-exec-server` for standalone executor. Preserve every trailing argument and keep the list visible. Re-exec once at startup before consuming startup environment or creating helper aliases. Retain helper dispatch and canonical help/error names. External name-based monitoring must use the new labels. Executable-path discovery stays unchanged.

### Background terminal waits

Top-level `background_terminal_min_timeout`/`background_terminal_max_timeout` use milliseconds; defaults 5000/300000. Reject zero/reversed/unrepresentable bounds. Clamp empty `write_stdin` polls. Show bounds in descriptions and finish early on exit. Keep initial/nonempty waits and Code Mode outer `exec`/`wait` limits. Exit alone does not resume idle model turns.

### Memory reasoning effort

Optional `[memories] extract_reasoning_effort`/`consolidation_reasoning_effort` use the existing effort type for V1/V2. Defaults: `low`/`medium`, independent of parent. Keep models and other memory behavior.

### Compact Markdown links

- Render named web links and resolvable local file links as OSC 8 labels without destination suffixes. Keep bare URLs visible. Do not require terminal detection.
- Resolve relative paths lexically against session cwd. Do not read the filesystem or frontend home. Preserve encoding/location fragments. Line opening depends on terminal/editor.
- Show unsupported/unresolved destinations. Keep destinations in text-only output. Reuse hyperlink layout/output transport. Keep trusted artifact capabilities separate.

### Conversation switching

In `asm resume`, Esc clears a nonempty search or exits the picker if the search is empty. Do not start a new session. Keep the startup `asm fork` fallback unchanged.

During owned fullscreen switches, hold the displayed frame and terminal diff buffer until destination replay completes. Draw the destination once. Continue background collection and ordered current-history replay. The parked screen is not authoritative. Preserve native-scrollback clearing. No experimental workspace feature is required.

When Main runs a tool, side creation snapshots inherited history, not execution. Complete pending calls only in the fork's persisted interrupted suffix. Keep Main and ancestor records unchanged. Preserve cold resume. Never execute or cancel Main's pending work.

### Transcript spacing

Keep Markdown paragraphs, code blocks, and list items adjacent during streaming and completion. Keep code-block blanks, raw output, message boundaries, and user padding.

### Click-to-quote

Opt in with `[tui] click_to_quote = true` (default `false`). Use only in owned fullscreen transcripts. Preserve native scrollback.

- Copy fresh selections normally. On composer click, attach the selection, not the clipboard. Show a fixed removable preview above the draft without a model request.
- Clear pending selection on explicit deselection, navigation, or thread change, but not after clipboard-confirmed deselection. Never expire it.
- Keep quotes with their drafts across switches. Whole-draft replacement supplies its own quotes. Plain history navigation must not replace a quote-only draft.
- Treat quotes literally. Do not activate skills or apps from quotes. Resolve unquoted comments and explicit structured mentions normally.
- Append references in order. [×] removes only its reference; clicking the preview does not remove it. With limited height, wheel-scroll the reference and image list. Keep comments visible.
- Number previews and submitted blockquotes from 1. Keep global indices while scrolling. Renumber after removal. Count numbering bytes toward limits.
- Add a numbered comment per attachment below composer text. Show grey “Add an optional comment…” without submitting it. Keep general text unnumbered, prior comments intact, and existing hints. Remove only empty generated comment rows.
- On plain Enter in an empty generated comment, clear its number and leave a general-message blank line. Do not submit. Keep references, prior comments, and modified submit unchanged.
- Track generated prefixes separately from user lists. Escape generated comment numbers in submitted Markdown. Keep displayed indices when comments are omitted. Submit all references and only entered comments. On removal, renumber associated comments. Keep the removed reference's entered comment as ordinary text. Restore associations with drafts. After external edits, detach uncertain associations rather than guessing.
- Preserve the draft when removing a quote. Enter sends the Markdown blockquote and optional comment through the existing path.
- Reject references alone or a complete quote and comment above 10,000 UTF-8 bytes, including quoting and expanded pastes. Keep existing references and comments. Never truncate or limit reference count. Keep unquoted limits.
- Items can exceed 1K tokens. Review changes manually. Do not add hidden instructions or API fields. Do not rewrite history.

### Collapsed tool calls

`[tui] collapse_tool_calls = true` opts in. The default is `false`. In rich fullscreen:

- Group consecutive shell, MCP, and dynamic calls, routine V2 interactions and waits, and normal agent starts and completions into one expandable summary.
- Count calls, starts, and completions separately. Do not count lifecycle notices as calls or running tools. List distinct command or tool names and agent paths in first-use order.
- Count each successful child message or follow-up once. Count matching wait start and end previews once per contiguous group. Clear the running state of completed waits. Keep result, failed, and interrupted waits visible.
- Use `+N more` for overflow. `[tui] collapse_tool_calls_max_lines` takes a positive integer and defaults to `1`. It limits counts and names in new headers. Wrap larger budgets. Keep reading snapshots on resize and allow further wrapping. Ellipsize counts that cannot fit.
- Split groups on interruption, other activity, or chat messages. Keep active tools in separate short rows until committed history contains them.
- On expansion, restore previews and order. Keep failures and approvals visible. Preserve execution and model history, raw output, and the full transcript. When off, keep existing rendering.

### Completed turn activity

- In rich owned transcripts, show commentary and tools live. When the final answer starts, fold that turn's commentary, reasoning, and successful tool previews under expandable `Worked for …`. On successful completion, fold unphased legacy output except the last answer.
- Keep user messages, notices, failures, approvals, and final answers visible. Leave interrupted turns without answers unfolded. Keep original cells for raw and full transcripts, copy sources, execution history, and model history. Preserve native scrollback.
- Reuse disclosure controls. Index new cells once. Rebuild only for structural history changes. Cache at most 64 rendered fold layouts. Do not format hidden output.

### Compact status line

- Join parts with `·` and no spaces. Use `CtxN%` or `CtxN%left` for used or remaining context. Keep `fast-mode`: `⚡S` Standard, `⚡F` Fast, `⚡U` Ultrafast, `⚡X` Flex. For other tiers, prefix the catalog name or ID with `⚡`. Show compact capacity without `window`, also in `/statusline` previews.
- Lowercase model labels. Remove `gpt-`. Keep at most three letters, also for aliases. Append all version digits without separators: `GPT-6-Astra` → `ast6`, `GPT-6.1-Sol` → `sol61`, `Terra 5.6` → `ter56`. Omit the suffix if no digits exist.
- Use three letters for effort, also in model-with-reasoning: `medium` → `med`, `high` → `hig`, `xhigh` → `xhi`.
- Apply to the footer, `/statusline` previews, and `/subagents` model info. Keep IDs, other picker labels, titles, paths, roles, colors, order, and single-row truncation. Remove the space before the role only in the footer: `Main[default]`.
- Opt-in `cache-hit-rate` in `/statusline` or `tui.status_line` does not change defaults. Show the latest reported cached-to-input ratio for this thread as `CchN.N%`, to one decimal. Reuse usage. Do not add averages, totals, requests, or APIs. Hide unknown or zero-input usage. Missing cache detail looks like reported zero.
- Opt-in `output-tps` does not change defaults. Show rounded `Ntps` for the latest successful ordinary Responses request or native Claude message. Divide reported `output_tokens - reasoning_output_tokens` by time from the first nonempty text or tool-argument delta to completion. Exclude reasoning deltas from the timing start. Require explicit reasoning-token usage.
- Exclude failures, retry waits, compaction, warmup, tools, other threads, and initial latency. This measures streaming, not pure decoding. Buffering can change it. For Claude, use matching root `message_delta` output and thinking usage and `message_stop`, not turn totals or native children. Cursor TPS is unavailable. Omit requests with no generated deltas or zero-millisecond duration.
- At live completion, calculate TPS once. Use the existing usage event and refresh. Cache TUI text. Add no notifications, redraw requests, timers, or estimates. Retain the last valid value across failures. If timing or usage is absent, omit it. Do not infer it from replay.

### Continue through model capacity failures

Top-level `continue_when_model_at_capacity = true` enables unlimited `ServerOverloaded` retries for ordinary requests and foreground compaction. Children inherit it. False or omission keeps upstream behavior.

- Retry the same model without restarting the task or deliberately replaying completed tools. Keep ordinary retry budgets separate.
- Without server advice, wait 30, 60, 120, 240, then 300 seconds. Cap later local waits at five minutes. Valid `Retry-After` takes precedence, even above five minutes. Include reporting time in its deadline.
- Send structured HTTP 503 overloads directly to this wait. Skip the ordinary short retry burst. Keep other HTTP and non-Responses retry policies.
- Report next delay, attempts, and elapsed capacity wait. Keep cancellation and sampling-input preemption. Do not add periodic redraws or unlimited background-compaction retries.
- Keep other classifications: restricted capacity, Flex, quota, authentication, and invalid requests. Overload codes do not prove recovery. Mislabelled permanent disablement waits until cancellation. Do not classify prose by string matching or promise completion.

### Multi-agent V2 depth

Use `[agents] max_depth` for V2 creation. Root is 0. Default 1 allows direct children. Reject deeper creation before child preparation. Keep existing agents and communication available.

### Context budget and compaction

`token_budget` adds a read-only counter and reminders. Keep summary compaction. When official Context Management and native notes tools are active, use catalog notes instructions and allow `new_context` without a summary. Otherwise, request summary compaction. Reuse or await eligible background work. Below the async threshold, compact in the foreground.

### Subagent context windows, reasoning defaults, service tiers, and picker

Optional `[subagent_default_reasoning]` maps exact final canonical model IDs to efforts, for example `"gpt-6.1-sol" = "high"`. Apply only when spawning V2 children, after model and role resolution and before effort validation and tier selection. Explicit spawn and role efforts take precedence. Matches override `[agents] default_subagent_reasoning_effort`, inherited root effort, and model defaults. Keep unmatched behavior, root, and internal workers unchanged. Persist child effort normally. Do not reapply this default on reload or model change. Use existing validation for invalid values or unsupported matched efforts. Add no API or rollout fields.

Optional top-level `subagent_model_context_windows` maps exact final model IDs to positive `i64` token counts. Apply to spawned V2 children only, at startup, model change, and reload. Keep scalar or model fallback for misses. Preserve original `model_context_window` to prevent matches from leaking to unmatched grandchildren. Keep clamping, headroom, compaction, root, review, and guardian behavior. Add no API or rollout fields.

A positive role TOML `model_context_window` overrides map and scalar fallback at startup, model change, and reload. Resolve each new child's role, not its parent's. Keep clamping, headroom, compaction, root, and internal workers unchanged. The picker shows top-level map or scalar settings, not role limits.

`[subagent_service_tiers]` maps model and effort to tiers, for example `gpt-6-sol = { high = "fast" }`. Match the final child model and effort after role and explicit overrides. Matches override root tier even when it changes later. Keep unmatched and root behavior. Reject unsupported tiers and Fast when disabled.

Under `[features.multi_agent_v2]`, `expose_spawn_agent_service_tier = true` exposes optional `spawn_agent.service_tier` when model selection is exposed. Default is off. An explicit tier takes precedence for that child and survives reloads. Use `default` for the default tier. Omit it to use mapping and inheritance. Reject tiers unavailable for the child model, provider, or features.

`/subagents` labels:

- Show known model and effort using [compact status rules](#compact-status-line). Add `-fast` for matched or inherited Fast only. Matched `default` stops inheritance. Do not guess settings or change thread-ID descriptions or navigation.
- Show known positive configured capacity: `sol61-hig-fast-272k` or `sol61-max-272k`. Use exact map matches only for canonical-path V2 children. Otherwise use the scalar, including for main and V1. Omit unknown or nonpositive values. Never infer catalog values.
- Show settings before clamping and headroom, not confirmed routing, capacity, usage, or remaining context. Keep other metadata and navigation. Apply in TUI only. Add no APIs or request tracking.

### Plaintext inter-agent messages

V2 uses plaintext between parents and children. Upstream marks `message` on `spawn_agent`, `send_message`, and `followup_task` as `encrypted`.

- Remove the marker in bundled schemas. For overrides, remove only top-level `encrypted` property markers. Keep nested markers. Accept schemas without `message` without fallback.
- Deliver `AgentMessage::Plaintext` for direct, Code Mode, and namespaced spawns and messages, regardless of `encrypted_function_args`. Responses Lite omits this field on unmarked tools. Do not treat plaintext as ciphertext.
- Keep reasoning, compaction, and output-replay encryption. Replay old ciphertext with `AgentMessage::Encrypted`. Keep delivery APIs. Raw items, transcripts, and tool-result logs outside `collaboration` expose text. Add typed `interAgentMessage` to app-server history and notifications.
- Below V2 assignments, show supplied spawn arguments except the displayed message. Use dim `key: value` lists like the completion footer, without braces or string quotes. Expose separate typed display metadata. Persist it locally for display only. Never parse assignments, infer omitted settings, or add model context. Keep old transcripts without metadata. Rebuild derived history on upgrade.
- Show full plaintext incoming assignments and messages and outgoing messages in parent-owned child transcripts, live and replayed. Use the known role in initial headers. Keep main summaries. Do not change context, delivery, or history or show ciphertext. Keep this renderer separate from generic upstream tool rendering.
- Keep `0010_inter_agent_communication_projection.sql` clearing only derived SQLite history. Keep this rebuild if migration numbers conflict. Replay invalid projections only for segments visited by pagination and newer turn-shadowing dependencies. Do not eagerly replay unused ancestors for bounded recent pages. Serialize segment catch-up. Retry failures or cancellation. Committed batches do not prove completion. Reuse catch-up only for matching source revision and SQLite checkpoint. Do not decode unchanged compressed history. Do not backfill outgoing messages without display records. For incoming paginated records, require saved turn metadata. Never guess the turn.
- Store projections in `asm_thread_history_2.sqlite`, not `thread_history_1.sqlite`. This avoids the collision between upstream migration 9 and the first ASM namespace's applied migration. Keep the old ASM cache. Rebuild from canonical rollouts. Follow the [shared compatibility gate](#shared-compatibility-gate) for versions, consumers, validation, and manual stock-cache recovery. The namespace does not repair old stock contamination. Cold reads replay visited segments. Warm reads reuse checkpoints. This costs disk and first-read replay, not a per-request lookup layer.
- After initialization, `asmTranscript/enable` with `version: 1` enables display for this connection. TUI opts in before reads and after reconnect. Without opt-in, omit `interAgentMessage` from snapshots, paginated display, and live notifications. Keep normal history, collaboration summaries, context, and storage cursors. For unsupported clients, skip display-only item or timeline pages until a visible row or EOF. Do not stop on empty filtered pages or show communication as user messages.
- Check affected live display and Desktop rendering. A stock-reader pass does not prove Desktop rendering.
- At upstream sync, search agent tools for `with_encrypted`, `encrypted` schema markers, and `AgentMessage::Encrypted` producers. Missed markers deliver ciphertext as plaintext.

### Multiline-aware Enter

Opt-in `[tui] multiline_enter = true` makes plain Enter send single-line drafts or add newlines to multiline drafts, including collapsed pending pastes. Keep defaults. Require plain Enter in both submit and editor-newline bindings. Keep custom keymaps and popup actions. Modified submit sends and flushes pending pastes.

Example: `[tui.keymap.composer] submit = ["enter", "ctrl-enter"]`; `[tui.keymap.editor] insert_newline = ["enter", "shift-enter"]`. Use Shift+Enter to start multiline input. Use Ctrl+Enter to send. The terminal must support the modifiers. Keep queue bindings. Vim Insert and Replace use editor-newline bindings. Normal keeps editing behavior.

### Composer destination

Show Main or Side in the composer’s top-right padding. Use a bold filled badge with a space on each side. Use neutral for Main and magenta for companions. Use contrasting text. Add no borders. Do not recolor markers or input. Use active thread state, not footer text. Keep input geometry.

### Built-in agent role descriptions

Before effort validation and runtime selection, resolve the model: explicit spawn `model`, then role model, then inheritance. Keep role effort and constraints. Reject incompatible settings. Do not switch runtimes. Keep `agent_type`.

Keep the explorer purpose and original delegation Rules in spawn schemas. Keep concise worker Rules: assign tasks and files, preserve concurrent edits, and assign one owner per shared prerequisite. Coordinate overlap before edits. Continue independent work.

### Subagent global instructions

At startup and cold reload, use `$CODEX_HOME/SUBAGENTS.md` instead of global `AGENTS.md` and `AGENTS.override.md` for fresh and full-history children. Empty clears global instructions. Missing keeps inheritance. Other read errors stop startup. Keep project and host instructions, root, and internal workers unchanged. Append normal updates to full history without rewriting it.

### Wait and sleep intent reminders

Require `reason` for `clock.sleep`, V2 `agents.wait_agent`, Code Mode `wait`, ASM TUI `wait_threads` including MCP, and empty `write_stdin` polls. Do not change nonempty input. Trim reasons. Reject more than 500 characters. Always name the tool in continuation guidance. Omit only the context line for empty reasons. JSON-quote other reasons outside tool output and truncation limits.

- On user interruption, prompt the model to understand intent. A status question alone does not stop work. If work remains, give commentary and call the named wait with current valid arguments. Use neutral guidance for other unidentified returns.
- Keep children, cells, processes, and collected output. Do not prompt resumption after explicit stop. Reminders do not guarantee resumption or automatic waits.
- Use `instant_interrupt` for Code Mode, terminal polls, and TUI waits. When off, wait normally. Interrupted TUI waits cancel only observation and return `interrupted: true` without snapshots.
- Cancel early only marked MCP `codex_tui.wait_threads`. Keep its envelope. Do not rewrite input, persist reminders, or change app-server schemas. Apply schema changes after catalog overrides.
- Exclude environment readiness, desktop-owned tools, and initial command execution.

### Test-only dependencies

Keep `codex-ollama`'s `wiremock` in `[dev-dependencies]` while only tests need it. This removes the test-server chain from production builds, not necessarily the linked binary. Drop the change if upstream adopts it. Restore the dependency if production needs it.

### Advertised subagent models

Optional top-level `subagent_model_list` selects model IDs in user order for spawn descriptions and `<model_catalog>`. Omit it for automatic picker-visible selection. `[]` advertises none. Explicit IDs can select hidden loaded models. Deduplicate and advertise the first five. Keep the 1,000-byte context limit. Before truncation, validate every ID against the loaded catalog and spawning backend. Reject invalid IDs without fallback. Do not restrict spawning or change the picker.

Both catalog placements show usable child context after exact model settings, provider clamping, and headroom, before role overrides. Do not start discovery for missing sizes. Show a configured Claude auto-compaction window as a setting, not capacity. Do not guess native-managed or unknown capacity.

### Flat reasoning-effort picker

Show all supported efforts, including Max/Ultra, in one list without “More reasoning…”. Keep persistence, Plan-mode scope, and explicit-picker requirements for shortcuts that raise effort to Ultra.

### GNU malloc arena cap

At Linux GNU `codex` `main`, before threads start, call `mallopt(M_ARENA_MAX, 4)` unless `MALLOC_ARENA_MAX` is set. Reduce idle glibc heap from CPU-scaled arenas. Keep the GNU system allocator. Upstream jemalloc replaces only musl's. Mock-model runs saved memory without measurable wall or CPU cost. Real sessions remain unmeasured. Drop the change if upstream caps glibc arenas.

### Session HTTP transport reuse

Reuse one model HTTP transport per session through the route-aware pool. Rebuild on changes to resolved client factory, endpoint, redirect policy, auth owner, runtime, or default headers. Resolve auth and check routing and policy per request. Token refresh alone keeps connections. Keep TLS, CA, proxy, cookie, redirect, and managed-destination policy. Exclude auxiliary clients and WebSocket lifetime.

### Tokio worker cap

On Linux, cap Tokio workers at `min(CPUs, 8)` unless `TOKIO_WORKER_THREADS` is set. Reduce CPU-scaled overhead. Keep macOS and Windows defaults. With the arena cap, mock-model runs saved memory and CPU without losing wall time. Real-session and peak async CPU effects are unmeasured. Remove drift if upstream caps workers.

### Desktop agent-created worktrees

On the thread host, app-server handles recognized `codex_app.create_worktree` and `attach_worktree` dynamic tools. Create or attach durable managed worktrees. Do not change chat cwd, workspace roots, or skills. Return paths for explicit execution. Do not call the Desktop handler that changes workspace. Keep explicit navigation.

Reject explicitly untrusted chat or source projects. Allow unset trust, like TUI. Require non-ephemeral chats, local execution, and durable attachments. Allow immediate completion with `allowAsync: true`. Recover partial registration without duplicate checkouts. Verify attachment rendering and next-turn skill retention in Desktop. Backend checks are not enough.
