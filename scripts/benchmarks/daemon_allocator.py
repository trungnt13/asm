#!/usr/bin/env python3
"""Compare isolated persistent ASM processes; requires websocket-client."""

import argparse
import http.server
import json
import os
from pathlib import Path
import re
import signal
import socket
import statistics
import subprocess
import threading
import time
import uuid

import websocket


def stamp():
    return time.monotonic()


class Model(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        begin = stamp()
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        items = body.get("input", [])
        users = [x for x in items if x.get("role") == "user"]
        latest = json.dumps(users[-1]) if users else ""
        identifier = str(uuid.uuid4())
        outputs = []
        if "BENCH_SPAWN" in latest and not any(
            x.get("call_id", "").startswith("benchspawn") for x in items
        ):
            for name in ["alpha", "beta"]:
                outputs.append(
                    {
                        "type": "function_call",
                        "namespace": "agents",
                        "name": "spawn_agent",
                        "call_id": "benchspawn" + name,
                        "arguments": json.dumps(
                            {
                                "task_name": name,
                                "fork_turns": "all",
                                "message": "BENCH_CHILD: return READY",
                            }
                        ),
                    }
                )
        elif (
            "BENCH_TURN" in latest
            and (match := re.search(r"BENCH_TURN (\d+)", latest))
            and not any(
                x.get("call_id", "").startswith("follow" + match[1] + "_")
                for x in items
            )
        ):
            for name in ["alpha", "beta"]:
                outputs.append(
                    {
                        "type": "function_call",
                        "namespace": "agents",
                        "name": "followup_task",
                        "call_id": "follow" + match[1] + "_" + name,
                        "arguments": json.dumps(
                            {"target": name, "message": "BENCH_CHILD return READY"}
                        ),
                    }
                )
        else:
            outputs.append(
                {
                    "type": "message",
                    "role": "assistant",
                    "id": "msg_" + identifier,
                    "content": [{"type": "output_text", "text": "READY"}],
                }
            )
        events = [{"type": "response.created", "response": {"id": identifier}}]
        events += [{"type": "response.output_item.done", "item": x} for x in outputs]
        events += [
            {
                "type": "response.completed",
                "response": {
                    "id": identifier,
                    "status": "completed",
                    "usage": {
                        "input_tokens": 100,
                        "output_tokens": 5,
                        "total_tokens": 105,
                    },
                },
            }
        ]
        payload = "".join("data: " + json.dumps(x) + "\n\n" for x in events).encode()
        time.sleep(0.01)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
        self.wfile.flush()
        self.server.records.append(
            {
                "begin": begin,
                "end": stamp(),
                "input_bytes": len(json.dumps(body)),
                "outputs": outputs,
            }
        )


class RPC:
    def __init__(self, ws, output):
        self.ws, self.output = ws, output
        self.events = []
        self.condition = threading.Condition()
        self.counter = 0
        threading.Thread(target=self.receive, daemon=True).start()

    def receive(self):
        try:
            while True:
                msg = json.loads(self.ws.recv())
                with self.condition:
                    self.events.append((stamp(), msg))
                    self.output.write(
                        json.dumps({"time": stamp(), "message": msg}) + "\n"
                    )
                    self.output.flush()
                    self.condition.notify_all()
        except (websocket.WebSocketException, json.JSONDecodeError):
            pass

    def wait(self, predicate, start=0):
        deadline = stamp() + 30
        with self.condition:
            while stamp() < deadline:
                for t, msg in self.events[start:]:
                    if predicate(msg):
                        return t, msg
                self.condition.wait(0.1)
        raise TimeoutError("RPC notification timeout")

    def call(self, method, params):
        self.counter += 1
        identifier = self.counter
        start = len(self.events)
        self.ws.send(json.dumps({"id": identifier, "method": method, "params": params}))
        _, msg = self.wait(lambda x: x.get("id") == identifier, start)
        if "error" in msg:
            raise RuntimeError(msg["error"])
        return msg["result"]


def memory(pid, binary):
    totals = dict(rss=0, pss=0, anonymous=0, executable_file=0, other_file=0, private=0)
    paths = [Path("/proc") / str(pid)]
    for folder in paths:
        try:
            children = set()
            for task in (folder / "task").iterdir():
                try:
                    children.update((task / "children").read_text().split())
                except FileNotFoundError:
                    continue
            paths.extend(
                Path("/proc") / x for x in children if Path("/proc") / x not in paths
            )
            mapping = ""
            for line in (folder / "smaps").read_text().splitlines():
                fields = line.split()
                if "-" in fields[0] and ":" not in fields[0]:
                    mapping = fields[5] if len(fields) > 5 else ""
                elif fields[0] in [
                    "Rss:",
                    "Pss:",
                    "Anonymous:",
                    "Private_Clean:",
                    "Private_Dirty:",
                ]:
                    value = int(fields[1])
                    key = {
                        "Rss:": "rss",
                        "Pss:": "pss",
                        "Anonymous:": "anonymous",
                        "Private_Clean:": "private",
                        "Private_Dirty:": "private",
                    }[fields[0]]
                    totals[key] += value
                    if key == "anonymous" and mapping.startswith("/"):
                        totals[
                            "executable_file"
                            if mapping == str(binary)
                            else "other_file"
                        ] -= value
                    if key == "rss" and mapping.startswith("/"):
                        totals[
                            "executable_file"
                            if mapping == str(binary)
                            else "other_file"
                        ] += value
        except (OSError, ProcessLookupError):
            if folder == paths[0]:
                totals["valid"] = False
    totals.setdefault("valid", True)
    return totals


def run(args):
    output = args.output.resolve()
    output.mkdir(parents=True, mode=0o700)
    home = output / "home"
    home.mkdir(mode=0o700)
    work = output / "work"
    work.mkdir()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Model)
    server.records = []
    threading.Thread(target=server.serve_forever, daemon=True).start()
    config = f"""model = "bench-model"
model_provider = "bench"
diagnostic_log_capture = true
[model_providers.bench]
name = "Benchmark"
base_url = "http://127.0.0.1:{server.server_port}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
[features]
apps = false
plugins = false
memories = false
shell_snapshot = false
code_mode = false
[features.multi_agent_v2]
enabled = true
tool_namespace = "agents"
"""
    (home / "config.toml").write_text(config)
    with socket.socket() as temporary:
        temporary.bind(("127.0.0.1", 0))
        port = temporary.getsockname()[1]
    env = {
        k: v
        for k, v in os.environ.items()
        if k not in ["CODEX_HOME", "LD_PRELOAD", "MALLOC_CONF", "MALLOC_ARENA_MAX"]
        and not k.startswith(("OPENAI_", "CODEX_API"))
    }
    env["CODEX_HOME"] = str(home)
    if args.jemalloc:
        env["LD_PRELOAD"] = str(args.jemalloc.resolve())
        if args.jemalloc_arenas is not None:
            env["MALLOC_CONF"] = f"narenas:{args.jemalloc_arenas}"
    cmd = [
        str(args.binary.resolve()),
        "--no-daemon",
        "app-server",
        "--listen",
        f"ws://127.0.0.1:{port}",
    ]
    (output / "command.json").write_text(
        json.dumps(
            {
                "args": cmd,
                "allocator": env.get("LD_PRELOAD", "glibc"),
                "malloc_conf": env.get("MALLOC_CONF"),
            },
            indent=2,
        )
    )
    proc = subprocess.Popen(
        cmd,
        env=env,
        cwd=work,
        stdout=(output / "stdout").open("w"),
        stderr=(output / "stderr").open("w"),
        start_new_session=True,
    )
    samples = []
    stop = threading.Event()
    phase = ["startup"]

    def monitor():
        while not stop.wait(0.2):
            samples.append(
                {
                    "time": stamp(),
                    "phase": phase[0],
                    **memory(proc.pid, args.binary.resolve()),
                }
            )

    threading.Thread(target=monitor, daemon=True).start()
    turns = []
    children = []
    try:
        for _ in range(100):
            try:
                ws = websocket.create_connection(
                    f"ws://127.0.0.1:{port}", timeout=120, suppress_origin=True
                )
                break
            except (OSError, websocket.WebSocketException):
                if proc.poll() is not None:
                    raise RuntimeError("app-server exited")
                time.sleep(0.1)
        else:
            raise TimeoutError("app-server startup")
        rpc = RPC(ws, (output / "rpc.jsonl").open("w"))
        rpc.call(
            "initialize",
            {
                "clientInfo": {"name": "allocator_benchmark", "version": "1"},
                "capabilities": {"experimentalApi": True},
            },
        )
        ws.send(json.dumps({"method": "initialized", "params": {}}))
        root = rpc.call(
            "thread/start",
            {"cwd": str(work), "model": "bench-model", "approvalPolicy": "never"},
        )["thread"]["id"]
        for i in range(args.turns):
            phase[0] = "warmup" if i < 4 else "steady"
            target = root
            start_index = len(rpc.events)
            begin = stamp()
            prompt = (
                ("BENCH_SPAWN" if i == 0 else f"BENCH_TURN {i}")
                + "\n"
                + ("repeatable synthetic content. " * 1024)
            )
            result = rpc.call(
                "turn/start",
                {"threadId": target, "input": [{"type": "text", "text": prompt}]},
            )
            turn_id = result["turn"]["id"]
            end, event = rpc.wait(
                lambda m: (
                    m.get("method") == "turn/completed"
                    and m["params"]["threadId"] == target
                    and m["params"]["turn"]["id"] == turn_id
                ),
                start_index,
            )
            assert event["params"]["turn"]["status"] == "completed", event
            if i > 0:
                for child in children:
                    child_end, child_event = rpc.wait(
                        lambda m: (
                            m.get("method") == "turn/completed"
                            and m["params"]["threadId"] == child
                        ),
                        start_index,
                    )
                    assert child_event["params"]["turn"]["status"] == "completed"
                    end = max(end, child_end)
            intervals = sorted(
                (max(begin, r["begin"]), min(end, r["end"]))
                for r in server.records
                if r["end"] > begin and r["begin"] < end
            )
            union = 0
            edge = begin
            for low, high in intervals:
                union += max(0, high - max(low, edge))
                edge = max(edge, high)
            turns.append(
                {
                    "index": i,
                    "wall_ms": (end - begin) * 1000,
                    "provider_union_ms": union * 1000,
                    "non_model_ms": (end - begin - union) * 1000,
                    "thread_id": target,
                }
            )
            if i == 0:
                children = [
                    tid
                    for _, m in rpc.events
                    if m.get("method") == "item/completed"
                    and m["params"]["item"].get("tool") == "spawnAgent"
                    for tid in m["params"]["item"]["receiverThreadIds"]
                ]
                assert len(set(children)) == 2, children
                time.sleep(0.2)
                for tid in children:
                    rpc.call("thread/resume", {"threadId": tid})
            time.sleep(0.1)
        phase[0] = "steady_hold"
        hold_start = stamp()
        stability = {}
        for seconds in range(30, 91, 10):
            time.sleep(max(0, hold_start + seconds - stamp()))
            end_hold = stamp()
            windows = [
                [
                    x["rss"]
                    for x in samples
                    if x.get("valid", True)
                    and end_hold - 30 + i * 10 <= x["time"] < end_hold - 20 + i * 10
                ]
                for i in range(3)
            ]
            if any(len(window) < 30 for window in windows):
                raise RuntimeError("insufficient valid resident samples")
            medians = [statistics.median(window) for window in windows]
            relative_spread = (max(medians) - min(medians)) / statistics.median(medians)
            stability = {
                "hold_seconds": end_hold - hold_start,
                "window_start": end_hold - 30,
                "window_end": end_hold,
                "window_medians_kib": medians,
                "relative_spread": relative_spread,
                "slope_kib_s": (medians[-1] - medians[0]) / 20,
                "stable": relative_spread <= 0.02,
            }
            if stability["stable"]:
                break
        (output / "stability.json").write_text(json.dumps(stability, indent=2))
        assert stability["stable"], "RSS did not stabilize within90 seconds"
        phase[0] = "fork"
        fork_begin = stamp()
        fork = rpc.call("thread/fork", {"threadId": root, "cwd": str(work)})["thread"][
            "id"
        ]
        fork_ms = (stamp() - fork_begin) * 1000
        for tid in [fork, *children]:
            rpc.call("thread/unsubscribe", {"threadId": tid})
        idle_index = len(rpc.events)
        phase[0] = "idle"
        time.sleep(args.idle)
        closed = [
            m["params"]["threadId"]
            for _, m in rpc.events[idle_index:]
            if m.get("method") == "thread/closed"
        ]
        if args.idle >= 65:
            assert set([fork, *children]) <= set(closed), closed
        resume_begin = stamp()
        for tid in [fork, *children]:
            rpc.call("thread/resume", {"threadId": tid})
        resume_ms = (stamp() - resume_begin) * 1000
        wake_index = len(rpc.events)
        rpc.call(
            "turn/start",
            {"threadId": root, "input": [{"type": "text", "text": "BENCH_TURN 999"}]},
        )
        for tid in [root, *children]:
            _, done = rpc.wait(
                lambda m: (
                    m.get("method") == "turn/completed"
                    and m["params"]["threadId"] == tid
                ),
                wake_index,
            )
            assert done["params"]["turn"]["status"] == "completed"
        phase[0] = "reactivated"
        time.sleep(2)
        (output / "result.json").write_text(
            json.dumps(
                {
                    "turns": turns,
                    "allocator_config": {
                        "jemalloc_arenas": args.jemalloc_arenas,
                        "malloc_conf": env.get("MALLOC_CONF"),
                    },
                    "stability": stability,
                    "fork_ms": fork_ms,
                    "resume_ms": resume_ms,
                    "pid": proc.pid,
                    "root": root,
                    "fork": fork,
                    "children": children,
                    "closed_after_idle": closed,
                },
                indent=2,
            )
        )
        ws.close()
    finally:
        stop.set()
        os.killpg(proc.pid, signal.SIGTERM)
        proc.wait(timeout=15)
        server.shutdown()
        (output / "samples.json").write_text(json.dumps(samples))
        (output / "requests.json").write_text(json.dumps(server.records))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--jemalloc", type=Path)
    parser.add_argument("--jemalloc-arenas", type=int, choices=[4, 8])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--turns", type=int, default=24)
    parser.add_argument("--idle", type=float, default=65)
    args = parser.parse_args()
    if args.jemalloc_arenas is not None and args.jemalloc is None:
        parser.error("--jemalloc-arenas requires --jemalloc")
    run(args)
