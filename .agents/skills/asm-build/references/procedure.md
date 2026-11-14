# Build procedure

Read the [fork guide](../../../../docs/porting-from-codex.md#build-the-binary-first) for governing requirements.

- Local `just` recipes default to main's `codex-rs/target`. Preserve explicit `CARGO_TARGET_DIR` and Cargo target-directory options.
- Keep native host mode, toolchain, profile, features, flags, checkout, and cache stable. Do not add `--target` merely to name the host. Timestamp, source, version, and other input changes can force rebuilds.
- Reuse matching verified [fork V8](../../../../docs/porting-from-codex.md#v8-dependency-release). Export the same verified absolute `RUSTY_V8_ARCHIVE` and `RUSTY_V8_SRC_BINDING_PATH`. Old Cargo fingerprints do not prove validity.
- Export `AWS_LC_SYS_NO_JITTER_ENTROPY=1` consistently; changes rebuild AWS-LC and dependents.
- From `codex-rs`, run `just build-dev`. Prioritize `codex` and `codex-code-mode-host` with matching source, native target, and `dev-small` profile. Serialize Cargo jobs sharing a target directory. Verify the producing checkout; test executables are not this pair. During builds, inspect independent code and affected tests/snapshots.
- Before releasing shared-cache ownership, verify real copies of both executable siblings in one task-owned directory under the checkout, e.g. `.agents/test-bin/<build-id>/`. Do not use symlinks or hard links. Record source commit, build inputs, absolute paths, and SHA-256 hashes. Test and hand off this unchanged pair with its evidence, not overwriteable shared-target executables.

For ordinary runtime/UI checks, use the verified absolute copied CLI with `--no-daemon` and [real-session isolation](../../../../docs/porting-from-codex.md#real-session-checks). This uses the embedded backend and sibling helper without changing/stopping the shared daemon. Check daemon/remote behavior separately when needed; `codex agents` and `--remote` cannot use `--no-daemon`.

Before handoff, verify both files, CLI `--version`, both `--help` commands, and pair source identity. Help does not prove model/tool operation. When safe, hand off before automated checks finish. Report verified absolute paths, build time, a manual check, and pending checks. After runtime edits, rebuild and copy both; never present old binaries as current. Get permission before replacing installed `codex`.
