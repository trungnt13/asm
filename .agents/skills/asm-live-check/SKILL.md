---
name: asm-live-check
description: Run bounded real-provider checks for ASM changes in isolated state. Use before PR acceptance or for focused live validation.
---

# Live Check

Read the [fork guide](../../../docs/porting-from-codex.md#real-session-checks) first. It remains authoritative.

Read the guide’s live-check gates before model calls. They control acceptance, budgets, authorization, and stopping. For Desktop SSH checks, also use the repository’s `asm-desktop-ssh-test` skill.

Follow this procedure and the guide’s gates. Skill use does not expand task scope or authorization.

## Procedure

- Use private temporary `CODEX_HOME` with `diagnostic_log_capture = true`, not ephemeral mode. Retain diagnostic history outside personal history. Securely provision only needed config, authentication, and resources. Preserve intended profile/model. Put sessions, databases, and logs inside that home. Never copy personal history or symlink shared mutable state.
- In the isolated config, explicitly set `tool_namespace = "agents"` under `[features.multi_agent_v2]`; do not fall back to the default `collaboration` namespace. Preserve the intended multi-agent settings from the user's config, including `enabled` and `disable_direct_message`, unless the check specifically targets a different configuration. Confirm these settings before provider calls; an isolated home must not silently change the agent tool surface being validated.
- Unless behavior needs a specific checkout, use a temporary working directory. Use `--no-daemon`; for daemon/remote checks, use an isolated server, never the personal shared daemon.
- Record session IDs, commands, binary/source identity, and expected/actual results. Before cleanup, retain private rollouts, output, and diagnostics. Never report or commit credentials.
- Keep failed homes until triage ends. Remove only task-owned state after retaining evidence. Titles do not make personal sessions disposable.
