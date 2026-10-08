"""Owned interactive Claude worker; JSONL stdout is reserved for the host protocol."""

import fcntl
import json
import os
import queue
import sqlite3
import subprocess
import sys
import threading
import time
import urllib.request
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

LIMIT = 256 * 1024
from activity import Activity
from instructions import Instructions
from lifecycle import Worker, become_subreaper
from observations import Observations, TEXT_LIMIT, clipped


def exchange(config, path, payload):
    request = urllib.request.Request(
        config["url"] + path,
        json.dumps(payload).encode(),
        {
            "Authorization": "Bearer " + config["token"],
            "Content-Type": "application/json",
        },
    )
    with urllib.request.urlopen(request, timeout=8) as response:
        return json.load(response)


class Bridge(Worker, Observations, Activity, Instructions):
    def __init__(self, envelope):
        self.launch = envelope["payload"]["launch"]
        self.claude = envelope["payload"]["claude"]
        self.tmux = envelope["payload"]["tmux"]
        self.session = self.launch["runtime_session_id"]
        self.initialize_instructions(self.launch["instructions"])
        self.inbound = queue.Queue(maxsize=256)
        self.outbound = queue.Queue(maxsize=64)
        self.connected = False
        self.active = None
        self.task_deadlines = {}
        self.startup_notices = []
        self.prompt = None
        self.stop = None
        self.idle_since = None
        self.idle_without_stop_since = None
        self.activity_unavailable_since = None
        self.background_wait = False
        self.unresolved_tools = {}
        self.running = True
        self.last_poll = 0
        self.fragments = {}
        self.prompt_turns = {}
        self.pending_prompts = {}
        self.abort_reason = None
        self.worker_alive = False
        self.owns_record = False
        self.status_failure_noticed = False
        self.directory = Path(self.launch["state_dir"]) / ("claude-" + self.session)
        self.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        if self.directory.is_symlink():
            raise ValueError("Worker state directory cannot be a symlink")
        os.chmod(self.directory, 0o700)
        self.lease = open(self.directory / "lease", "a+")
        fcntl.flock(self.lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.database = sqlite3.connect(self.directory / "journal.sqlite")
        self.database.executescript("""
            CREATE TABLE IF NOT EXISTS mail (id TEXT PRIMARY KEY, payload TEXT, state TEXT);
            CREATE TABLE IF NOT EXISTS events (id TEXT PRIMARY KEY, envelope TEXT);
        """)
        uncertain = self.database.execute(
            "SELECT COUNT(*) FROM mail WHERE state IN ('written','acknowledged')"
        ).fetchone()[0]
        if uncertain:
            self.database.execute(
                "UPDATE mail SET state='unknown' WHERE state IN ('written','acknowledged')"
            )
            self.database.commit()
            self.startup_notices.append(
                "Prior delivered Claude work has unknown completion; it will not be replayed automatically"
            )
        self.http = ThreadingHTTPServer(("127.0.0.1", 0), self.http_class())
        self.url = "http://127.0.0.1:" + str(self.http.server_port)
        threading.Thread(target=self.http.serve_forever, daemon=True).start()
        for identifier, encoded in self.database.execute(
            "SELECT id,payload FROM mail WHERE state='queued'"
        ).fetchall():
            if json.loads(encoded)["kind"] == "task":
                self.database.execute(
                    "UPDATE mail SET state='unknown' WHERE id=?", (identifier,)
                )
                self.startup_notices.append(
                    "A prior queued task was not replayed; submit a new native followup"
                )
        self.database.commit()
        self.start_worker(self.launch["mode"])

    def http_class(self):
        bridge = self

        class Endpoint(BaseHTTPRequestHandler):
            def do_POST(self):
                if self.headers.get("Authorization") != "Bearer " + bridge.token:
                    self.send_error(403)
                    return
                length = int(self.headers.get("Content-Length", "0"))
                if (
                    not 0 < length <= LIMIT
                    or self.headers.get_content_type() != "application/json"
                ):
                    self.send_error(400)
                    return
                self.connection.settimeout(8)
                try:
                    payload = json.loads(self.rfile.read(length))
                    result = queue.Queue(maxsize=1)
                    bridge.inbound.put(
                        (
                            (self.path, self.headers.get("Authorization")),
                            payload,
                            result,
                        ),
                        timeout=2,
                    )
                    response = result.get(timeout=6)
                except (ValueError, OSError, queue.Full, queue.Empty):
                    self.send_error(503)
                    return
                encoded = json.dumps(response).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)

            def log_message(self, format, *args):
                pass

        return Endpoint

    def emit(self, kind, content=None, turn=None, event_id=None):
        if kind == "notice" and not turn and not self.active:
            self.startup_notices.append(content)
            return
        identifier = event_id or str(uuid.uuid4())
        observation = {"type": kind}
        if content is not None:
            observation["content"] = content
        event = {
            "id": identifier,
            "turn_id": turn
            if turn is not None
            else (self.active or {}).get("turn_id", ""),
            "kind": observation,
        }
        encoded = json.dumps(event)
        inserted = self.database.execute(
            "INSERT OR IGNORE INTO events VALUES (?,?)", (identifier, encoded)
        ).rowcount
        self.database.commit()
        if inserted:
            print(encoded, flush=True)

    def command(self, envelope):
        op, payload = envelope["op"], envelope["payload"]
        if op == "submit":
            if any(
                len(payload[key].encode()) > 256 for key in ("sender", "id", "turn_id")
            ):
                raise ValueError("Claude message identifiers exceed 256 bytes")
            if payload["kind"] == "message":
                fragment = (
                    "Mailbox "
                    + payload["id"]
                    + " from "
                    + payload["sender"]
                    + ": "
                    + payload["text"]
                )
                fragment += (
                    "\nCall asm_bridge ack with message_id "
                    + payload["id"]
                    + " before acting."
                )
                if len(fragment.encode()) > 8192:
                    raise ValueError(
                        "Claude mailbox text plus correlation metadata exceeds 8192 bytes"
                    )
            encoded = json.dumps(payload, sort_keys=True)
            if len(payload["text"].encode()) > 8192:
                raise ValueError("Claude task text exceeds 8192 bytes")
            old = self.database.execute(
                "SELECT payload,state FROM mail WHERE id=?", (payload["id"],)
            ).fetchone()
            if old and old[0] != encoded:
                raise ValueError("Conflicting reuse of Claude message ID")
            if old and old[1] in ("unknown", "interrupted"):
                raise ValueError(
                    "Prior message outcome is unknown or interrupted; submit a new message ID"
                )
            if not old:
                self.database.execute(
                    "INSERT INTO mail VALUES (?,?,?)",
                    (payload["id"], encoded, "queued"),
                )
                self.database.commit()
            if payload["kind"] == "task":
                if not old and not self.active and not self.task_deadlines:
                    self.task_deadlines[payload["id"]] = time.monotonic() + 300
                if not self.worker_alive:
                    self.start_worker("resume")
                for notice in self.startup_notices:
                    self.emit("notice", notice, turn=payload["turn_id"])
                self.startup_notices.clear()
            self.dispatch()
        elif op == "interrupt":
            if self.active and self.active["turn_id"] != payload["turn_id"]:
                raise ValueError("Claude cancellation turn mismatch")
            self.settle_worker()
            self.worker_alive, self.connected = False, False
            self.token = "revoked"
            for identifier, encoded in self.database.execute(
                "SELECT id,payload FROM mail WHERE state IN ('queued','written','acknowledged')"
            ).fetchall():
                message = json.loads(encoded)
                if (
                    message["turn_id"] == payload["turn_id"]
                    and message["kind"] == "task"
                ):
                    self.database.execute(
                        "UPDATE mail SET state='interrupted' WHERE id=?", (identifier,)
                    )
            self.database.commit()
            self.active, self.prompt, self.stop, self.idle_since = (
                None,
                None,
                None,
                None,
            )
            self.fragments.clear()
            self.unresolved_tools.clear()
            self.idle_without_stop_since = None
            self.activity_unavailable_since = None
            self.background_wait = False
            self.task_deadlines.clear()
            self.pending_prompts.clear()
            while not self.outbound.empty():
                self.outbound.get_nowait()
        elif op == "shutdown":
            self.settle_worker()
            self.worker_alive = False
            self.running = False
            self.emit("closed", turn="")
        else:
            raise ValueError("Unknown Claude bridge command")
        print(json.dumps({"reply_to": envelope["id"], "ok": True}), flush=True)

    def run(self):
        def read_commands():
            while True:
                line = sys.stdin.readline(LIMIT + 1)
                if not line:
                    break
                if len(line) > LIMIT:
                    self.inbound.put(("fatal", "Oversized host command", None))
                    return
                try:
                    self.inbound.put(("command", json.loads(line), None))
                except ValueError:
                    self.inbound.put(("fatal", "Malformed host command", None))
                    return
            self.inbound.put(("eof", None, None))

        threading.Thread(target=read_commands, daemon=True).start()
        while self.running:
            try:
                path, payload, response = self.inbound.get(timeout=0.2)
            except queue.Empty:
                self.poll()
                continue
            try:
                if isinstance(path, tuple):
                    path, authorization = path
                    if authorization != "Bearer " + self.token:
                        response.put({"error": "Stale Claude worker incarnation"})
                        continue
                if path == "command":
                    self.command(payload)
                elif path == "/hook":
                    response.put(self.hook(payload))
                elif path == "/connect":
                    self.connected = True
                    self.emit(
                        "notice",
                        "MCP connected; channel delivery remains unconfirmed until Claude acknowledges a task",
                        turn="",
                    )
                    self.dispatch()
                    response.put({})
                elif path == "/channel":
                    self.dispatch()
                    try:
                        response.put(
                            {}
                            if self.writable_clients()
                            else self.outbound.get_nowait()
                        )
                    except queue.Empty:
                        response.put({})
                elif path == "/instructions":
                    response.put(self.read_instruction_page(payload.get("page")))
                elif path == "/ack":
                    delivered = self.database.execute(
                        "SELECT state FROM mail WHERE id=?",
                        (payload.get("message_id"),),
                    ).fetchone()
                    if not self.active or not delivered or delivered[0] != "written":
                        raise ValueError("Unknown or inactive Claude message ID")
                    accepted_prompt = self.pending_prompts.pop(
                        payload["message_id"], None
                    )
                    if not accepted_prompt:
                        raise ValueError(
                            "Claude acknowledgement lacks a correlated PreToolUse prompt"
                        )
                    if not self.initial_acknowledged and (
                        len(self.instructions_fetched) != len(self.instruction_pages)
                        or (
                            self.instruction_pages
                            and self.instruction_prompt != accepted_prompt
                        )
                    ):
                        raise ValueError(
                            "Read every ASM instruction page in this task prompt before initial acknowledgement"
                        )
                    self.initial_acknowledged = True
                    self.task_deadlines.pop(payload["message_id"], None)
                    if payload["message_id"] == self.active["id"]:
                        self.prompt = accepted_prompt
                        self.prompt_turns[accepted_prompt] = self.active["turn_id"]
                    self.database.execute(
                        "UPDATE mail SET state='acknowledged' WHERE id=?",
                        (payload["message_id"],),
                    )
                    self.database.commit()
                    response.put({})
                elif path == "/v1/logs":
                    self.telemetry(payload)
                    response.put({})
                elif path in ("eof", "fatal"):
                    if path == "fatal":
                        self.emit("failed", payload)
                    self.running = False
                else:
                    raise ValueError("Unknown local bridge endpoint")
            except (
                ValueError,
                KeyError,
                OSError,
                subprocess.SubprocessError,
                sqlite3.Error,
                queue.Full,
                RuntimeError,
            ) as error:
                if path == "command":
                    print(
                        json.dumps(
                            {
                                "reply_to": payload["id"],
                                "ok": False,
                                "error": str(error),
                            }
                        ),
                        flush=True,
                    )
                elif response:
                    response.put({"error": str(error)})
                self.emit("notice", clipped(error))
            self.poll()


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--hook":
        config = json.loads(Path(sys.argv[2]).read_text())
        print(json.dumps(exchange(config, "/hook", json.load(sys.stdin))))
        return
    envelope = json.loads(sys.stdin.readline(LIMIT + 1))
    forbidden = (
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_PROFILE",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    )
    if any(key in os.environ for key in forbidden):
        print(
            json.dumps(
                {
                    "reply_to": envelope["id"],
                    "ok": False,
                    "error": "Claude subscription worker refuses provider/API/token environment overrides",
                }
            ),
            flush=True,
        )
        return
    bridge = Bridge.__new__(Bridge)
    startup_error = None
    try:
        become_subreaper()
        bridge.__init__(envelope)
        print(json.dumps({"reply_to": envelope["id"], "ok": True}), flush=True)
        bridge.run()
    except (
        ValueError,
        KeyError,
        OSError,
        subprocess.SubprocessError,
        sqlite3.Error,
        RuntimeError,
    ) as error:
        startup_error = str(error)
    finally:
        if hasattr(bridge, "owns_record"):
            bridge.settle_worker()
        else:
            from lifecycle import descendants

            if descendants():
                raise RuntimeError(
                    "Startup child ownership unavailable; refusing cleanup signals"
                )
        if hasattr(bridge, "http"):
            bridge.http.shutdown()
        if hasattr(bridge, "database"):
            bridge.database.close()
        if hasattr(bridge, "lease"):
            bridge.lease.close()
    if startup_error is not None:
        print(
            json.dumps(
                {"reply_to": envelope["id"], "ok": False, "error": startup_error}
            ),
            flush=True,
        )


if __name__ == "__main__":
    main()
