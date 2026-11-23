---
name: asm-live-check
description: Use for every ASM real-provider check, including PR validation, failure investigation, and interactive test handoffs.
---

# Live check

Follow the [fork guide](../../../docs/porting-from-codex.md). For Desktop SSH checks, also use `asm-desktop-ssh-test`.

## Plan

- Except for [local-only upstream rebases](../../../docs/porting-from-codex.md#local-validation-for-upstream-rebases), every PR needs a successful live check before readiness.
- Select the smallest scenario that exercises the implementation changes and relevant fork interactions. Include the necessary turns and agents. For example, use multiple turns and agents to check cache changes that affect them. State the expected result. Builds, offline checks, and mocks do not replace a required live check.
- Choose the cheapest available compatible model from any provider. Use current prices and include required helper calls. Do not select the interactive model only because it is configured.
- Use the lowest adequate reasoning effort and standard speed. Apply overrides only in isolated test state. Do not change personal defaults.
- Before each run, record model selection evidence and a cost estimate. Set request, turn, time, and supported token limits for that run. Track cumulative usage across runs. Minimize input and requested output.
- Use existing sign-in only as authorized. Necessary checks and retries need no separate spend approval when they use the cheapest compatible model. Earlier standing spend and run limits do not restrict this authorization. Respect later task-specific limits and managed policy.
- Do not run unrelated retries, warmups, Decisions calls, or extra subagents. Do not default to broad replays, model matrices, benchmarks, or repeated cold builds.

## Run

- Use a private temporary `CODEX_HOME` with `diagnostic_log_capture = true`. Do not use ephemeral mode. Keep sessions, databases, logs, and diagnostics in that home. Provision only the config, authentication, and resources needed. Never copy personal history or symlink shared mutable state.
- For live checks and interactive handoffs, copy `AGENTS.md` from the original `${CODEX_HOME:-$HOME/.codex}`, if present, before switching homes. Link the test home's `AGENTS.md` to that private copy. Omit it only when testing without normal instructions. When a check depends on a rule, verify that the runtime received it.
- Before live calls, set `[features.multi_agent_v2].tool_namespace = "agents"` in the isolated config. Preserve other multi-agent settings unless the check needs an override.
- Unless the behavior needs a specific checkout, use a temporary working directory. Use `--no-daemon`. For daemon or remote checks, use an isolated server, not the personal shared daemon.
- For isolated CLI launches, including interactive TUI handoffs, pass `--dangerously-bypass-approvals-and-sandbox`. This disables approvals and the ASM sandbox for that run. Use a controlled workspace. Omit the flag when testing approvals or sandboxing, or when managed policy requires those controls. Do not change build or installed defaults.
- Stop the current run at proof or failure. Inspect its evidence. If investigation or verification needs another live check, correct the issue and continue with the cheapest compatible model. Update limits and cost estimates for the additional calls. Respect later task-specific limits and managed policy.
- Missing credentials, budget, or a compatible setup block readiness. Do not report blocked or untested behavior as verified.

## Evidence and cleanup

- Record session IDs, commands, binary and source identity, expected and actual results, calls, elapsed time, and reported tokens. Report cost only when known. Otherwise mark it unknown.
- Before cleanup, retain private rollouts, output, and diagnostics. Never report or commit credentials.
- Keep failed homes until triage ends. Remove only task-owned state after retaining evidence. Titles do not make personal sessions disposable.
