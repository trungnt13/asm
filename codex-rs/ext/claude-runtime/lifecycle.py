"""Linux ownership and startup of the interactive, subscription-authenticated worker."""

import ctypes
import os
import secrets
import shlex
import signal
import subprocess
import sys
import time
import uuid
from pathlib import Path
import json


def become_subreaper():
    if sys.platform != "linux":
        raise ValueError(
            "Claude workers require Linux child-subreaper cancellation support"
        )
    if not hasattr(os, "pidfd_open") or not hasattr(signal, "pidfd_send_signal"):
        raise ValueError(
            "Claude workers require Linux PID-file-descriptor signal support"
        )
    descriptor = os.pidfd_open(os.getpid())
    try:
        signal.pidfd_send_signal(descriptor, 0)
    finally:
        os.close(descriptor)
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(36, 1, 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), "Cannot establish Claude child ownership")


def descendants():
    parents = {}
    for entry in Path("/proc").iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            fields = (entry / "stat").read_text().rsplit(")", 1)[1].split()
            parents[int(entry.name)] = (int(fields[1]), fields[19])
        except FileNotFoundError:
            continue
    owned = {os.getpid()}
    while True:
        children = {pid for pid, (parent, _) in parents.items() if parent in owned}
        if children <= owned:
            return {pid: parents[pid][1] for pid in owned if pid != os.getpid()}
        owned |= children


def settle_tree(guard):
    deadline = time.monotonic() + 6
    escalate = time.monotonic() + 1
    while True:
        guard()
        owned = descendants()
        for pid, birth in owned.items():
            try:
                descriptor = os.pidfd_open(pid)
                try:
                    guard()
                    fields = (
                        Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
                    )
                    if fields[19] == birth:
                        signal.pidfd_send_signal(
                            descriptor,
                            signal.SIGKILL
                            if time.monotonic() >= escalate
                            else signal.SIGTERM,
                        )
                finally:
                    os.close(descriptor)
            except ProcessLookupError:
                continue
            except FileNotFoundError:
                continue
        while True:
            try:
                child, _ = os.waitpid(-1, os.WNOHANG)
                if child == 0:
                    break
            except ChildProcessError:
                break
        if not descendants():
            return
        if time.monotonic() >= deadline:
            raise RuntimeError(
                "Owned Claude descendants could not be terminated and reaped"
            )
        time.sleep(0.05)


class Worker:
    def tmux_call(self, arguments, environment=None):
        return subprocess.check_output(
            [self.tmux, "-L", self.server_name, "-f", "/dev/null"] + arguments,
            env=environment,
            stderr=subprocess.PIPE,
            timeout=5,
            text=True,
        )

    def writable_clients(self):
        output = self.tmux_call(["list-clients", "-F", "#{client_readonly}"])
        writable = any(line != "1" for line in output.splitlines())
        self.startup_human_attached |= writable
        if self.startup_human_attached and not writable:
            self.delivery_allowed = True
        return writable

    def guard_owned_tree(self):
        snapshot = descendants()
        parents = {}
        protected = []
        for pid, birth in snapshot.items():
            try:
                fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
                if fields[19] != birth:
                    continue
                parents[pid] = int(fields[1])
                if fields[0] == "Z" or (
                    pid == getattr(self, "pid", None)
                    and birth == getattr(self, "pane_birth", None)
                ):
                    continue
                executable = Path(f"/proc/{pid}/exe").stat()
                with open(f"/proc/{pid}/cmdline", "rb") as command:
                    arguments = command.read(8193)
                if len(arguments) > 8192:
                    raise RuntimeError(
                        "Child command identity exceeds cancellation proof bounds"
                    )
                arguments = arguments.split(b"\0")
                daemon = any(
                    left == b"daemon" and right == b"run"
                    for left, right in zip(arguments, arguments[1:])
                )
                identity = (executable.st_dev, executable.st_ino)
                if identity == getattr(self, "claude_identity", None) or daemon:
                    protected.append(pid)
            except FileNotFoundError:
                continue
            except PermissionError as error:
                raise RuntimeError(
                    "Cannot establish owned child identity; refusing all signals"
                ) from error
        fenced = getattr(self, "protected_identities", {})
        protected = set(protected) | {
            pid for pid, birth in fenced.items() if snapshot.get(pid) == birth
        }
        while True:
            children = {pid for pid, parent in parents.items() if parent in protected}
            if children <= protected:
                break
            protected |= children
        if protected:
            self.protected_identities = {
                pid: snapshot[pid] for pid in protected if pid in snapshot
            }
            raise RuntimeError(
                "Auxiliary Claude/shared daemon subtree ownership unknown; refusing all signals"
            )
        known = getattr(self, "owned_identities", {})
        owned = {pid for pid, birth in known.items() if snapshot.get(pid) == birth}
        while True:
            children = {pid for pid, parent in parents.items() if parent in owned}
            if children <= owned:
                break
            owned |= children
        if any(pid not in owned for pid in parents):
            raise RuntimeError(
                "Unregistered adopted child ownership unknown; refusing all signals"
            )
        self.owned_identities = {pid: snapshot[pid] for pid in owned}

    def verify_no_existing_worker(self):
        with open(self.claude, "rb") as executable:
            if executable.read(4) != b"\x7fELF":
                raise ValueError(
                    "Safe Claude ownership requires the native Linux ELF binary; wrappers/npm are unsupported"
                )
            metadata = os.fstat(executable.fileno())
            self.claude_identity = (metadata.st_dev, metadata.st_ino)
        ownership = self.directory / "ownership.json"
        if ownership.exists() and not json.loads(ownership.read_text()).get("settled"):
            raise ValueError(
                "Prior Claude incarnation lacks confirmed cleanup; orphan recovery requires a human"
            )
        self.verify_no_live_session()

    def live_sessions(self):
        output = subprocess.check_output(
            [self.claude, "agents", "--json"],
            stderr=subprocess.PIPE,
            timeout=5,
            text=True,
        )
        if len(output.encode()) > 256 * 1024:
            raise ValueError("Claude session discovery exceeds 256 KiB")
        entries = json.loads(output)
        if not isinstance(entries, list) or any(
            not isinstance(entry, dict) for entry in entries
        ):
            raise ValueError("Claude session discovery returned an unsupported shape")
        return entries

    def verify_no_live_session(self):
        if any(
            entry.get("sessionId") == self.session for entry in self.live_sessions()
        ):
            raise ValueError(
                "Claude session UUID is already live; refusing a second writer"
            )

    def settle_worker(self):
        self.guard_owned_tree()
        if self.owns_record:
            try:
                for entry in self.live_sessions():
                    if entry.get("sessionId") == self.session and (
                        entry.get("pid") != getattr(self, "pid", None)
                        or entry.get("kind") != "interactive"
                    ):
                        raise RuntimeError(
                            "Claude session moved outside its owned terminal; handoff is unsupported"
                        )
                if getattr(self, "worker_alive", False) and self.writable_clients():
                    raise RuntimeError(
                        "Writable human attachment prevents safe cancellation ownership proof"
                    )
            except (OSError, subprocess.SubprocessError, ValueError) as error:
                raise RuntimeError(
                    "Claude pre-cancellation discovery unavailable; refusing all signals"
                ) from error
        settle_tree(self.guard_owned_tree)
        if self.owns_record:
            try:
                self.verify_no_live_session()
                self.guard_owned_tree()
            except (OSError, subprocess.SubprocessError, ValueError) as error:
                raise RuntimeError(
                    "Owned tree stopped, but Claude handoff/cleanup status is unknown: "
                    + str(error)
                ) from error
            path = self.directory / "ownership.json"
            ownership = json.loads(path.read_text())
            ownership["settled"] = True
            path.write_text(json.dumps(ownership))

    def start_worker(self, mode):
        if mode == "new" and (self.directory / "ownership.json").exists():
            raise ValueError(
                "Claude session UUID has prior ownership history; explicit Resume mode is required"
            )
        self.verify_no_existing_worker()
        self.owns_record = True
        self.incarnation = str(uuid.uuid4())
        self.server_name = "asm-" + self.incarnation
        ownership = {
            "incarnation": self.incarnation,
            "tmux_server": self.server_name,
            "settled": False,
        }
        (self.directory / "ownership.json").write_text(json.dumps(ownership))
        self.token = secrets.token_urlsafe(32)
        self.connected = False
        self.startup_human_attached = False
        self.delivery_allowed = False
        self.incarnation_dir = self.directory / self.incarnation
        self.incarnation_dir.mkdir(mode=0o700)
        self.instructions_fetched.clear()
        self.pending_instruction_prompts.clear()
        self.served_instructions.clear()
        self.instruction_prompt = None
        self.initial_acknowledged = False
        manifest = {
            "revision": self.instruction_revision,
            "pages": self.instruction_pages,
        }
        (self.incarnation_dir / "instructions.json").write_text(
            json.dumps(manifest, ensure_ascii=False)
        )
        config_path = self.incarnation_dir / "connection.json"
        config_path.write_text(
            json.dumps(
                {
                    "url": self.url,
                    "token": self.token,
                    "instruction_pages": len(self.instruction_pages),
                    "instruction_revision": self.instruction_revision,
                }
            )
        )
        os.chmod(config_path, 0o600)
        helper = [
            sys.executable,
            str(Path(__file__).with_name("bridge.py").resolve()),
            "--hook",
            str(config_path),
        ]
        hooks = {
            name: [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": shlex.join(helper),
                            "timeout": 10,
                        }
                    ]
                }
            ]
            for name in (
                "SessionStart",
                "UserPromptSubmit",
                "MessageDisplay",
                "PreToolUse",
                "PostToolUse",
                "PostToolUseFailure",
                "PermissionRequest",
                "Stop",
                "StopFailure",
                "SessionEnd",
            )
        }
        settings = self.incarnation_dir / "settings.json"
        settings.write_text(json.dumps({"hooks": hooks}))
        mcp = self.incarnation_dir / "mcp.json"
        mcp.write_text(
            json.dumps(
                {
                    "mcpServers": {
                        "asm_bridge": {
                            "command": sys.executable,
                            "args": [
                                str(Path(__file__).with_name("channel.py").resolve()),
                                str(config_path),
                            ],
                        }
                    }
                }
            )
        )
        command = [
            self.claude,
            "--model",
            self.launch["model"],
            "--permission-mode",
            "manual",
            "--settings",
            str(settings),
            "--mcp-config",
            str(mcp),
            "--dangerously-load-development-channels",
            "server:asm_bridge",
            "--append-system-prompt",
            "Before acting on an ASM channel task, use asm_bridge read_instructions to fetch every "
            + str(len(self.instruction_pages))
            + " instruction page, separately, in index order from 0. "
            + "Revision: "
            + self.instruction_revision
            + ". Apply the complete instructions. "
            + "Then call asm_bridge ack with the channel message_id. Ack confirms receipt, not understanding or task success. "
            + "The bridge provides no permission approvals.",
            "--resume" if mode == "resume" else "--session-id",
            self.session,
        ]
        environment = os.environ.copy()
        environment.update(
            {
                "CLAUDE_CODE_ENABLE_TELEMETRY": "1",
                "OTEL_LOGS_EXPORTER": "otlp",
                "OTEL_EXPORTER_OTLP_LOGS_PROTOCOL": "http/json",
                "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT": self.url + "/v1/logs",
                "OTEL_EXPORTER_OTLP_LOGS_HEADERS": "Authorization=Bearer " + self.token,
            }
        )
        self.tmux_call(
            [
                "new-session",
                "-d",
                "-s",
                "worker",
                "-c",
                self.launch["cwd"],
                "exec " + shlex.join(command),
            ],
            environment,
        )
        self.pid = int(
            self.tmux_call(
                ["display-message", "-p", "-t", "worker", "#{pane_pid}"]
            ).strip()
        )
        if self.pid not in descendants():
            raise RuntimeError(
                "Claude tmux pane is not in the bridge's owned descendant tree"
            )
        pane_fields = (
            Path(f"/proc/{self.pid}/stat").read_text().rsplit(")", 1)[1].split()
        )
        server_pid = int(pane_fields[1])
        snapshot = descendants()
        if server_pid not in snapshot:
            raise RuntimeError(
                "tmux server identity is outside the bridge descendant tree"
            )
        server_executable = Path(f"/proc/{server_pid}/exe").stat()
        selected_tmux = Path(self.tmux).stat()
        if (server_executable.st_dev, server_executable.st_ino) != (
            selected_tmux.st_dev,
            selected_tmux.st_ino,
        ):
            raise RuntimeError("tmux server executable identity cannot be established")
        self.pane_birth = snapshot[self.pid]
        self.owned_identities = {
            self.pid: self.pane_birth,
            server_pid: snapshot[server_pid],
        }
        self.protected_identities = {}
        self.guard_owned_tree()
        ownership.update(
            {
                "pane_pid": self.pid,
                "pane_birth": snapshot[self.pid],
                "server_pid": server_pid,
                "server_birth": snapshot[server_pid],
            }
        )
        (self.directory / "ownership.json").write_text(json.dumps(ownership))
        self.worker_alive = True
        self.emit(
            "notice",
            "Personally approve development-channel consent in Claude, then detach before ASM delivery: "
            + shlex.join([self.tmux, "-L", self.server_name, "attach", "-t", "worker"])
            + "; subsequent inspection is read-only: "
            + shlex.join(
                [self.tmux, "-L", self.server_name, "attach", "-r", "-t", "worker"]
            ),
            turn="",
        )
        self.emit(
            "notice",
            "Claude usage is best-effort telemetry, not complete billing or reasoning-token accounting",
            turn="",
        )
