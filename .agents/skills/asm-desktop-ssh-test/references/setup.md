# Desktop SSH procedure

Use a separate SSH account to test a candidate in the actual Desktop client. The account can use the same machine. No VM or relay connection is required. An SSH alias alone does not isolate the backend.

The examples use `dev-host` for the normal SSH alias. Replace it with the approved alias for your host. Use `asm-test` as the reusable test account and Desktop SSH alias on the test host. On another host, use a dedicated test account. Keep the normal account, installed binaries, SSH service, and daemon unchanged. This procedure separates accounts and state. It does not provide VM isolation.

**Prepare the account.** Get approval before account creation or credential reuse. Run the following commands on the test host. If the account exists, verify its ownership and purpose. Do not reset an existing account or replace its files without inspection. Confirm that no other test owns the account. If ownership is unclear, stop.

For an existing setup, skip account creation. Preserve its SSH access and authentication unless changes are necessary and authorized. Back up its configuration before changes. The `install` commands below overwrite destination files. Do not rerun them blindly.

```bash
sudo useradd --create-home --user-group --shell /bin/bash asm-test
sudo chmod 700 /home/asm-test
sudo install -d -o asm-test -g asm-test -m 700 \
  /home/asm-test/.ssh \
  /home/asm-test/.codex \
  /home/asm-test/.local/bin \
  /home/asm-test/projects/worktree-check
```

Keep the password locked. Do not add sudo or other privileged groups. Use the existing SSH service. Do not change its configuration.

**Authorize SSH access.** Review the normal account's public-key authorizations. Copy approved entries into the test account. Preserve key restrictions. Do not copy private SSH keys. When all existing entries are approved, run:

```bash
sudo install -o asm-test -g asm-test -m 600 \
  "$HOME/.ssh/authorized_keys" /home/asm-test/.ssh/authorized_keys
```

**Install the candidate.** Build and retain the verified binary pair under the repository fork guide. Do not use files from a shared build cache directly. Before an update, disconnect the test host in Desktop. Stop only the test account's daemon. Retain its previous pair and evidence for rollback. Do not overwrite binaries used by a running test.

Run this block in Bash on the test host. Enter the absolute directory of the retained pair. Both files must come from the same build.

```bash
read -r -p 'Candidate binary directory: ' candidate
case "$candidate" in /*) ;; *) echo 'Use an absolute path.'; exit 1 ;; esac
test -x "$candidate/codex" && test -x "$candidate/codex-code-mode-host" || exit 1
sudo install -m 755 "$candidate/codex" /home/asm-test/.local/bin/codex
sudo install -m 755 "$candidate/codex-code-mode-host" \
  /home/asm-test/.local/bin/codex-code-mode-host
sha256sum "$candidate/codex" "$candidate/codex-code-mode-host"
sudo sha256sum /home/asm-test/.local/bin/codex \
  /home/asm-test/.local/bin/codex-code-mode-host
```

Compare both destination hashes with the retained pair. A version string alone does not identify a candidate.

**Set the shell paths.** Put these lines before any early return in the test account's `.bashrc`. Set them in `.bash_profile` too. Keep the normal account's shell files unchanged.

```bash
export PATH="$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin"
export CODEX_HOME="$HOME/.codex"
umask 077
```

Verify both a login shell and an SSH command. Desktop must find the candidate when it starts the remote backend.

**Prepare authentication and config.** Use the [real-session rules](../../../../docs/porting-from-codex.md#real-session-checks). For approved file-based sign-in reuse, copy only `auth.json` into the private test home. Set its owner to `asm-test` and its mode to `600`. If file-based reuse is unavailable, sign in as the test account. Do not copy personal history, databases, keyrings, or the complete normal configuration.

After approval for file-based reuse, run on the test host:

```bash
sudo install -o asm-test -g asm-test -m 600 \
  "${CODEX_HOME:-$HOME/.codex}/auth.json" /home/asm-test/.codex/auth.json
```

For a new setup, create `/home/asm-test/.codex/config.toml` with these settings. For an existing setup, merge only required changes after backup:

```toml
diagnostic_log_capture = true
cli_auth_credentials_store = "file"

[features]
code_mode_host = true

[projects."/home/asm-test/projects/worktree-check"]
trust_level = "trusted"

[desktop]
git-worktree-root = "/home/asm-test/.codex/worktrees"
```

Before inference, set the authorized model, provider, and effort for the scenario. Copy required catalogs or instruction files into the test home. Update their paths. Match relevant tool settings with the target setup. Record each intentional difference.

A minimal config can expose different tools. For example, direct-message settings control whether `followup_task` is sent. If the provider rejects a tool schema, stop. Compare the effective tools and target settings. Do not disable unrelated features only to obtain a passing result. A config change does not prove that a schema defect is fixed.

When the target uses namespaced agent tools without direct messages, add its matching settings:

```toml
[features.multi_agent_v2]
enabled = true
disable_direct_message = true
tool_namespace = "agents"
```

**Prepare the fixture.** Use a small Git repository under the test account. Commit its baseline files. If skills are relevant, add `.agents/skills/cwd-sentinel/SKILL.md`. Give it a unique marker. Ignore `.agents/` in the fixture. This checks whether ignored local skills remain available after worktree creation.

**Add the Desktop alias.** Perform this step on the machine that runs Desktop, not on the test host. Resolve the existing connection with `ssh -G dev-host`. If the alias exists, inspect and reuse it. Otherwise, add a separate `Host asm-test` entry to that machine's SSH config. Use these rules:

- Keep the resolved hostname, port, identity, jump host, and required host-key settings.
- Set `User asm-test`. Do not use `HostName dev-host` unless `dev-host` is a real DNS name.
- Set `ControlMaster no` and `ControlPath none`. Do not reuse the normal connection's control socket.
- Clear inherited port forwards unless the test needs them.
- Back up the config. Preserve the existing `dev-host` entry. Inspect symlinks before editing.
- Validate the result with `ssh -G asm-test`.

Check the alias from the Desktop machine:

```bash
ssh asm-test 'printf "user=%s\nhome=%s\nconfig=%s\n" "$USER" "$HOME" "$CODEX_HOME"; command -v codex; codex --version'
```

Expect the test user, test home, and candidate command path. In Desktop, open **Settings → Connections → SSH**. Enable `asm-test`. Select `/home/asm-test/projects/worktree-check`. Desktop starts the backend through this SSH connection. Do not enable relay remote control for this procedure.

**Verify the scenario.** Confirm the actual backend executable, both hashes, and `CODEX_HOME`. If Desktop installs or selects another binary, stop. Restore candidate selection before testing. An SSH shell check does not prove which binary Desktop uses.

Set the request and time limits before inference. First check a small provider turn. Then check the changed behavior. For worktree tests:

1. Record the chat cwd, workspace roots, and available fixture skill.
2. Ask the Desktop agent to create one worktree.
3. Verify the returned checkout and its attachment in Desktop.
4. On the next turn, verify that the cwd and workspace roots are unchanged.
5. Verify that the fixture skill is still listed and its marker can be read.
6. Retain the chat ID, transcript, diagnostics, binary identity, and expected and actual results.

Do not substitute TUI or API-only checks for Desktop acceptance. Keep failures and untested boundaries marked as blocked.

**Update or stop the test.** Disconnect only `asm-test` in Desktop. To stop its managed daemon, run on the test host:

```bash
sudo -u asm-test -H env CODEX_HOME=/home/asm-test/.codex \
  /home/asm-test/.local/bin/codex app-server daemon stop
```

Verify that the test backend stopped before replacing its pair. Keep its failed homes and evidence until triage ends. Reuse the account for later candidates. Use a fresh private home when the scenario requires cold state. Do not delete the account, revoke shared sign-in, or clear the normal backend as routine cleanup.
