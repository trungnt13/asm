# Release procedure

Read the [fork guide](../../../../docs/porting-from-codex.md#releases) for governing requirements.

## Operate a release

1. **Prepare:** require an explicit release request. Prioritize the complete installable package over unrelated features. Use the existing workflow and explicit repo `gh ... -R trungnt13/asm`. Resume unfinished candidates first; preserve their pinned source. For a new release, fetch upstream with tags/prune. Capture latest `upstream/main`; rebase onto that fixed target under sync/PR/validation/push rules. Verify the V8 prerequisite before source pushes to `origin/main`. Push the intended commit before dispatch; never release unpushed work. Publication grants no install/shared-server-restart authority.
2. **Check CI:** inspect failures; fix relevant code/package errors. For runner allocation/communication failures, inspect annotations/logs and rerun only failed jobs. Do not infer code defects or change build settings from runner failure alone. If the owner requires recovery first, verify retry success before dispatch. Read prior candidate logs in memory with supported GitHub CLI raw-output opt-in. Never render untrusted log escapes to a terminal.
3. **Check live use:** before dispatch, run one local turn with default config and existing sign-in. Do not substitute cheaper profiles/models without approval. This smoke has standing approval for required System1/helpers through configured endpoints/models. Use the release-source CLI/helper pair and [real-session rules](../../../../docs/porting-from-codex.md#real-session-checks). Limit to 120 seconds and the smallest read-only Code Mode task. Stop at proof or first failure. Require a real response and successful read-only Code Mode output. Failure, timeout, or missing authentication blocks dispatch. Broader tests, new endpoints/models, and unrelated paid calls need approval.
4. After every smoke attempt, report actual request counts, elapsed time, input/cached-input/output/reasoning tokens, and required helper usage separately where reported. Distinguish totals/subtotals and unavailable fields. Include failed-attempt usage; never replace actual usage with estimates. Planning estimate: 20–60K input and 1–5K output/reasoning plus helpers, not a cap or actual usage claim. Report source/live result separately from package checks. Keep credentials out of CI.
5. **Dispatch:** run [`fork-rust-release.yml`](../../../../.github/workflows/fork-rust-release.yml) on `main` with `publish_release=true`. Let it select version, create candidate, build, publish, verify, and update remote `main`. Do not repeat these manually or wait for postmerge builds. Candidates build/reuse archives. Ordinary branch dispatch is build-only; a `main` push does not request publication.
6. **Recover:** follow the guide’s [recovery rules](../../../../docs/porting-from-codex.md#operate-a-release).
7. **Verify:** follow the guide’s [readiness and completion gates](../../../../docs/porting-from-codex.md#operate-a-release).
8. **Finish:** fetch and safely fast-forward local `main`. Preserve local edits/concurrent commits. Report release commit, upstream baseline, checks, remote/local integration, and blocked updates.

## Release notes

Publish notes on GitHub and in the final report. Compare with the previous completed release; include only packaged changes. Keep separate `ASM` and `Codex` headings. Write `No changes` in empty sections. Use ASD-STE100: short active sentences and consistent terms.

Use one flat source-line item per feature/fix: `**Name:** What changed. User effect or required action.` Include relevant defaults, limits, experimental status, and supporting commit/file links. Keep version, platform readiness, and validation outside feature lists. Never claim unverified effects.
