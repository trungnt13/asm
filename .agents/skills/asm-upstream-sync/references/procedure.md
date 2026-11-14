# Upstream Sync procedure

Read the [fork guide](../../../../docs/porting-from-codex.md#upstream-sync) for governing requirements.

- Use the configured backend and authorized sign-in. Follow client path `models?client_version=<client version>`, authentication, account routing, and discovery settings. Never send credentials to an unverified endpoint. This read-only request is not inference.
- Save successful raw JSON as timestamped private task-owned `models.json` outside tracked source. Validate model-list shape. Record fetch time, backend URL, client version, and returned IDs. Keep credentials/authentication headers out of evidence, logs, and commits.
- Compare with the bundled catalog. Report new models, visibility, and relevant capabilities; link saved JSON in the rebase report. Catalog presence does not prove execution. Do not automatically change model/config defaults.
- If authentication, connectivity, or discovery policy blocks a fresh fetch, report the check as blocked with its reason. Do not substitute cached data or claim live availability was checked.

- For each sync command, enable reviewed conflict reuse without shared Git config changes. Preserve chosen strategy/history options, e.g. `--rebase-merges`. Use `git -c rerere.enabled=true -c rerere.autoupdate=false merge <upstream-target>` or `git -c rerere.enabled=true -c rerere.autoupdate=false rebase <upstream-target>`. Review reused resolutions before staging; validate affected behavior. Rerere does not prove correctness.
- After manual resolution, run `git -c rerere.enabled=true -c rerere.autoupdate=false rerere`. Continue with `git -c rerere.enabled=true -c rerere.autoupdate=false rebase --continue` or `git -c rerere.enabled=true -c rerere.autoupdate=false merge --continue`.
- For full syncs, capture baseline and target first; include the chosen upstream `main` commit and all ancestors. Report that fixed range. Focus on fork differences and affected upstream changes. Follow development validation/integration rules.
- After each rebase, group upstream changes by idea, not commit. Give one short plain sentence per idea, user effect, and source link. Keep status/validation separate.
- After full sync, update the guide's [Last incorporated commit marker](../../../../docs/porting-from-codex.md#live-backend-model-catalog) in that PR. Cherry-picks do not advance it. Verify `git show -s --format='%H%n%cs%n%s' <upstream-commit>` and ancestry to the fork branch.

After an authorized PR merge, complete the guide’s [post-merge cleanup](../../../../docs/porting-from-codex.md#post-merge-cleanup) before closing out integration. Inventory artifacts produced by this procedure; retain required evidence and active handoffs, and report cleanup exceptions.
