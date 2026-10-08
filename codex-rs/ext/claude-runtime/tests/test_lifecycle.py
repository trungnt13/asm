import contextlib
import io
import importlib
import json
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch

RUNTIME = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(RUNTIME))
activity = importlib.import_module("activity")
bridge = importlib.import_module("bridge")
lifecycle = importlib.import_module("lifecycle")
observations = importlib.import_module("observations")


class LifecycleTests(unittest.TestCase):
    def test_guard_reaps_zombies_but_refuses_live_unknown_children(self):
        births = {10: "10", 11: "11", 12: "12"}
        states = {10: ("S", 99), 11: ("S", 10), 12: ("Z", 99)}

        class ProcPath:
            def __init__(self, path):
                self.pid = int(path.split("/")[2])

            def read_text(self):
                state, parent = states[self.pid]
                fields = [state, str(parent)] + ["0"] * 18
                fields[19] = births[self.pid]
                return f"{self.pid} (fixture) " + " ".join(fields)

            def stat(self):
                return types.SimpleNamespace(st_dev=1, st_ino=2)

        worker = lifecycle.Worker()
        worker.pid, worker.pane_birth = 11, "11"
        worker.owned_identities = {10: "10", 11: "11"}
        with (
            patch.object(lifecycle, "Path", ProcPath),
            patch.object(lifecycle, "descendants", return_value=births),
            patch("builtins.open", side_effect=lambda *args: io.BytesIO(b"tmux\0")),
            patch.object(
                lifecycle.os, "waitpid", side_effect=[(12, 0), (0, 0), (0, 0)]
            ) as reap,
        ):
            worker.guard_owned_tree()
            self.assertEqual(reap.call_count, 2)
            states[12] = ("S", 99)
            with self.assertRaisesRegex(RuntimeError, "Unregistered adopted child"):
                worker.guard_owned_tree()

    def test_settlement_reconciles_absent_disappearing_and_live_servers(self):
        for mode in ("absent", "disappearing", "live"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as directory:
                worker = lifecycle.Worker()
                worker.directory = Path(directory)
                worker.owns_record, worker.worker_alive = True, True
                worker.session, worker.server_pid, worker.server_birth = (
                    "fixture",
                    10,
                    "10",
                )
                worker.guard_owned_tree = lambda: None
                worker.live_sessions = lambda: []
                record = worker.directory / "ownership.json"
                record.write_text('{"settled":false}')
                snapshots = (
                    [{}, {}]
                    if mode == "absent"
                    else [{10: "10"}, {} if mode == "disappearing" else {10: "10"}]
                )
                with (
                    patch.object(lifecycle, "descendants", side_effect=snapshots),
                    patch.object(lifecycle, "settle_tree") as settle,
                    patch.object(
                        worker,
                        "writable_clients",
                        side_effect=subprocess.CalledProcessError(1, "tmux"),
                    ) as clients,
                ):
                    if mode == "live":
                        with self.assertRaisesRegex(
                            RuntimeError, "discovery unavailable"
                        ):
                            worker.settle_worker()
                        settle.assert_not_called()
                    else:
                        worker.settle_worker()
                        self.assertTrue(json.loads(record.read_text())["settled"])
                        settle.assert_called_once()
                    self.assertEqual(clients.call_count, mode != "absent")

    def test_startup_failure_preserves_both_errors_and_closes_resources(self):
        for cleanup_fails in (False, True):
            with self.subTest(cleanup_fails=cleanup_fails):
                closed = []

                class Resource:
                    def __init__(self, name):
                        self.name = name

                    def close(self):
                        closed.append(self.name)

                    shutdown = close

                class FailedBridge:
                    def __init__(self, envelope):
                        self.owns_record = True
                        for name in ("http", "database", "lease"):
                            setattr(self, name, Resource(name))
                        raise RuntimeError("original startup error")

                    def settle_worker(self):
                        if cleanup_fails:
                            raise RuntimeError("original cleanup error")

                output = io.StringIO()
                with (
                    patch.object(bridge, "Bridge", FailedBridge),
                    patch.object(bridge, "become_subreaper"),
                    patch.object(bridge.sys, "argv", ["bridge.py"]),
                    patch.object(bridge.sys, "stdin", io.StringIO('{"id":"launch"}\n')),
                    patch.object(bridge.os, "environ", {}),
                    contextlib.redirect_stdout(output),
                ):
                    if cleanup_fails:
                        with self.assertRaises(SystemExit) as exit_error:
                            bridge.main()
                        self.assertEqual(exit_error.exception.code, 1)
                    else:
                        bridge.main()
                result = json.loads(output.getvalue())
                self.assertEqual(result["settled"], not cleanup_fails)
                self.assertIn("original startup error", result["error"])
                if cleanup_fails:
                    self.assertIn("original cleanup error", result["error"])
                self.assertEqual(closed, ["http", "database", "lease"])

    def test_hooks_allow_foreground_work_and_reject_background_continuations(self):
        observer = observations.Observations()
        observer.session, observer.prompt = "fixture", "prompt"
        observer.active = {"id": "task", "turn_id": "turn"}
        observer.initial_acknowledged = True
        observer.writable_clients = lambda: False
        observer.unresolved_tools = {}
        observer.emit = lambda *args, **kwargs: None
        with sqlite3.connect(":memory:") as database:
            observer.database = database
            database.execute("CREATE TABLE mail(id TEXT,payload TEXT,state TEXT)")
            tool = {
                "session_id": "fixture",
                "hook_event_name": "PreToolUse",
                "prompt_id": "prompt",
                "tool_name": "Bash",
                "tool_use_id": "tool",
            }
            self.assertEqual(
                observer.hook(dict(tool, tool_input={"command": "true"})), {}
            )
            for name in ("Bash", "Agent", "CronCreate"):
                result = observer.hook(
                    dict(tool, tool_name=name, tool_input={"run_in_background": True})
                )
                self.assertEqual(
                    result["hookSpecificOutput"]["permissionDecision"], "deny"
                )
            stop = {
                "session_id": "fixture",
                "hook_event_name": "Stop",
                "prompt_id": "prompt",
                "background_tasks": [],
                "session_crons": [],
            }
            observer.abort_reason = None
            self.assertEqual(observer.hook(stop), {})
            self.assertEqual(observer.stop, stop)
            self.assertIsNone(observer.abort_reason)
            observer.hook(dict(stop, background_tasks=[{"id": "pending"}]))
            self.assertIn("unsupported", observer.abort_reason)
            observer.hook(
                {
                    "session_id": "fixture",
                    "hook_event_name": "UserPromptSubmit",
                    "prompt_id": "next",
                }
            )
            self.assertIn("Unsupported Claude continuation", observer.abort_reason)

    def test_waiting_notices_are_deduplicated_and_update(self):
        worker = activity.Activity()
        worker.pid, worker.session = 11, "fixture"
        worker.claude = "claude"
        worker.tmux, worker.server_name = "tmux", "isolated"
        worker.active, worker.prompt, worker.stop = {"turn_id": "turn"}, "prompt", None
        worker.worker_alive, worker.last_poll = True, 0
        worker.abort_reason, worker.waiting_for = None, None
        worker.task_deadlines = {}
        worker.writable_clients = lambda: False
        worker.dispatch = lambda: None
        notices = []
        worker.emit = lambda kind, text: notices.append((kind, text))
        for index, reason in enumerate(
            ("input needed", "input needed", "dialog open", None, "input needed")
        ):
            entry = {
                "pid": 11,
                "sessionId": "fixture",
                "status": "waiting" if reason else "busy",
                "waitingFor": reason,
            }
            with (
                patch.object(activity, "descendants", return_value={11: "11"}),
                patch.object(activity.time, "monotonic", return_value=100 + index * 2),
                patch.object(
                    activity.subprocess,
                    "check_output",
                    return_value=json.dumps([entry]),
                ),
            ):
                worker.poll()
        self.assertEqual(len(notices), 3)
        self.assertTrue(
            all("attach" in text and kind == "notice" for kind, text in notices)
        )

    @unittest.skipUnless(
        sys.platform == "linux" and shutil.which("tmux"), "requires Linux and tmux"
    )
    def test_owned_tmux_settlement_with_harmless_process(self):
        script = r"""
import json, os, tempfile, time, uuid
from pathlib import Path
from lifecycle import Worker, become_subreaper, descendants
become_subreaper()
with tempfile.TemporaryDirectory(prefix="asm-tmux-fixture-") as directory:
    for already_exited in (False, True):
        worker = Worker()
        worker.tmux = TMUX
        worker.server_name = "asm-fixture-" + str(uuid.uuid4())
        worker.directory = Path(directory)
        worker.session = worker.server_name
        worker.owns_record = True
        worker.live_sessions = lambda: []
        worker.startup_human_attached = False
        worker.directory.joinpath("ownership.json").write_text('{"settled":false}')
        environment = {"HOME": directory, "PATH": "/usr/local/bin:/usr/bin:/bin", "TMUX_TMPDIR": directory}
        worker.tmux_call(["new-session", "-d", "-s", "worker", "/bin/sleep 30"], environment)
        worker.pid = int(worker.tmux_call(["display-message", "-p", "-t", "worker", "#{pane_pid}"], environment))
        fields = Path(f"/proc/{worker.pid}/stat").read_text().rsplit(")", 1)[1].split()
        worker.server_pid = int(fields[1])
        snapshot = descendants()
        worker.pane_birth, worker.server_birth = snapshot[worker.pid], snapshot[worker.server_pid]
        worker.owned_identities = {worker.pid: worker.pane_birth, worker.server_pid: worker.server_birth}
        # Every tmux query uses only this test's private socket directory.
        tmux_call = worker.tmux_call
        worker.tmux_call = lambda arguments: tmux_call(arguments, environment)
        worker.guard_owned_tree()
        if already_exited:
            worker.tmux_call(["kill-session", "-t", "worker"])
            deadline = time.monotonic() + 5
            while descendants():
                worker.guard_owned_tree()
                assert time.monotonic() < deadline, "isolated tmux did not exit"
                time.sleep(0.02)
        worker.settle_worker()
        worker.settle_worker()
        assert not descendants(), "owned test processes survived"
        assert json.loads(worker.directory.joinpath("ownership.json").read_text())["settled"]
"""
        subprocess.run(
            [
                sys.executable,
                "-c",
                "TMUX = " + repr(shutil.which("tmux")) + "\n" + script,
            ],
            cwd=RUNTIME,
            check=True,
            timeout=25,
            capture_output=True,
        )


if __name__ == "__main__":
    unittest.main()
