# Release procedure

## Operate a release

1. **Prepare:** require an explicit release request. Prioritize the complete installable package over unrelated features. Use the existing workflow and explicit repo `gh ... -R trungnt13/asm`. Use the candidate and version rules in the [release contract](contract.md#version). Start new releases from the intended commit on `origin/main`. Do not sync upstream source unless the owner asks. Verify the V8 prerequisite before dispatch. Never release unpushed work. Follow the [Linux v3 procedure](linux.md#linux-v3-package) to pin the candidate and start the local build. A release request authorizes candidate publication, the verified remote `main` update, and a safe local fast-forward. It does not authorize installation or a shared-server restart. Source fixes require PRs.
2. **Check CI:** inspect failures; fix relevant code/package errors. For runner allocation/communication failures, inspect annotations/logs and rerun only failed jobs. Do not infer code defects or change build settings from runner failure alone. If the owner requires recovery first, verify retry success before dispatch. Read prior candidate logs in memory with supported GitHub CLI raw-output opt-in. Never render untrusted log escapes to a terminal.
3. **Check live use:** before dispatch, run one local turn with default config and existing sign-in. Do not substitute cheaper profiles/models without approval. This smoke has standing approval for required System1/helpers through configured endpoints/models. Do not ask again for each release. Use the release-source CLI/helper pair and [real-session rules](../../asm-live-check/SKILL.md). Limit to 120 seconds and the smallest read-only Code Mode task. Stop at proof or first failure. Require a real response and successful read-only Code Mode output. Failure, timeout, or missing authentication blocks dispatch. Broader tests, new endpoints/models, and unrelated paid calls need approval.
4. After every smoke attempt, report actual request counts, elapsed time, input/cached-input/output/reasoning tokens, and required helper usage separately where reported. Distinguish totals/subtotals and unavailable fields. Include failed-attempt usage; never replace actual usage with estimates. Planning estimate: 20–60K input and 1–5K output/reasoning plus helpers, not a cap or actual usage claim. Report source/live result separately from package checks. Keep credentials out of CI.
5. **Dispatch:** run [`fork-rust-release.yml`](../../../../.github/workflows/fork-rust-release.yml) on `main` with `publish_release=true` and `pinned_candidate_sha=<full SHA>` for a new release. The workflow checks the pin and registers it under the run ID. For a resume, use the original run ID instead of a pin. Publication does not require blocking or postmerge CI. Do not duplicate the workflow. Candidates build or reuse archives. A `main` push does not request publication.
6. **Recover:** if a release stops, follow [Recovery](#recovery).
7. **Verify:** follow [Completion](#completion) and the [publish contract](contract.md#reuse-and-publish).
8. **Finish:** fetch and safely fast-forward local `main`. Preserve local edits/concurrent commits. Report release commit, upstream baseline, checks, remote/local integration, and blocked updates.

## Recovery

After interruption, rerun failed jobs. For a new resume dispatch, use `main`, `publish_release=true`, and `resume_run_id=<original run ID>`. Preserve original version/commit despite upstream advances. Never invent versions or replace tags. Report failures needing owner decisions.

Only with explicit approval, supersede a source-defective failed unpublished candidate with a new immutable candidate. Record old run/source/tag and draft assets in `.github/scripts/superseded-fork-releases.json`. Preparation must verify that record and reject publication or asset drift. Preserve old candidate/tag/assets and occupied version. Never apply this exception to published or partially ready releases.

The owner separately approved superseding cancelled run `42465703649` on 2026-11-09 to release current main. Preserve draft `v0.169.1-alpha.3.1`, its tag, candidate and installer unchanged; verify its recorded cancelled state before creating the new candidate.

## Completion

A platform is ready after its checked archive and platform checksum manifest are published. Candidate completion requires `release`, both `verify_native` jobs, and `sync_main` success for that candidate. Legacy `v*` pushes/dispatches require publication and both native checks, not `sync_main`. Inspect recorded checks/current metadata; do not repeat successful checks on unchanged artifacts. Builds, tag pushes, and version labels do not prove publication.

A release request includes Linux v3. Follow the [v3 upload steps](linux.md#linux-v3-package). Report the release complete only after v3 passes or the owner approves a skip. Report an approved skip explicitly.

## Release notes

Publish notes on GitHub and in the final report. Compare with the previous completed release; include only packaged changes. Keep separate `ASM` and `Codex` headings. Write `No changes` in empty sections. Use ASD-STE100: short active sentences and consistent terms.

Use one flat source-line item per feature/fix: `**Name:** What changed. User effect or required action.` Include relevant defaults, limits, experimental status, and supporting commit/file links. Keep version, platform readiness, and validation outside feature lists. Never claim unverified effects.
