---
name: asm-desktop-ssh-test
description: Set up or reuse an isolated SSH account to test candidate ASM app-server builds in the actual Desktop client. Use for same-host test remotes such as asm-test, candidate updates, and Desktop acceptance checks.
---

# ASM Desktop SSH tests

Test a candidate through Desktop without replacing the normal backend.
Use a separate Unix account, private state, and a separate SSH alias.
An alias with the normal user does not isolate the backend.

Read [the procedure](references/setup.md) before setup, updates, or experiments.
Follow `asm-build` and `asm-live-check` for build and live-test requirements.
Keep commands and config consistent with the candidate version.

- Confirm the host, account, candidate pair, and expected result.
- Inspect an existing setup before changing it. Check for another active owner.
- Reuse valid configuration. Back it up before changes. Do not rerun overwrite commands blindly.
- Obtain authorization before account changes or credential reuse. Follow `asm-live-check` for model calls.
- Change only the test setup. Keep the normal account and backend unchanged.
- Configure the SSH alias on the Desktop machine, not the remote host.
- Verify the actual Desktop backend identity. Shell startup alone is not acceptance.
- Set call and time limits before inference. Stop the current run at proof or failure. Follow `asm-live-check` to investigate and retry.
- Retain private evidence. Report failed or untested behavior without claiming success.

Use the worktree scenario only for worktree changes. For other experiments,
select a small Desktop scenario that checks the requested behavior.
Skill use does not authorize deployment, cleanup, or model calls.
