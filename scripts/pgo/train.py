import asyncio
import json
import os
import shutil
import subprocess
import time
import tomllib
from pathlib import Path

R = Path(os.environ["PGO_TRAINING_DIR"]).resolve()
R.mkdir(mode=0o700, parents=True, exist_ok=True)
if R.stat().st_mode & 0o077:
    raise PermissionError("Training directory must be private (0700)")
if any(R.iterdir()):
    raise ValueError("Training directory must be empty")
source_home = Path(os.environ["CODEX_HOME"]).resolve()
config = (source_home / "config.toml").read_text()
(R / "requested-features.json").write_text(
    json.dumps(tomllib.loads(config)["features"])
)
cache = json.loads((source_home / "models_cache.json").read_text())
luna = next(m for m in cache["models"] if m["slug"] == "gpt-6-luna")
(R / "cheap-model-catalog.json").write_text(json.dumps({"models": [luna]}))


async def session(index, turns, children, sandbox, raw):
    d = R / f"session-{index}"
    d.mkdir(mode=0o700)
    h = d / "home"
    h.mkdir(mode=0o700)
    tmp = d / "tmp"
    tmp.mkdir(mode=0o700)
    w = d / "work"
    shutil.copytree(Path(__file__).resolve().parent / "fixture", w)
    (w / "checks.py.in").rename(w / "test_eventlog.py")
    shutil.copy2(source_home / "config.toml", h / "config.toml")
    if (source_home / "agent-roles").is_dir():
        shutil.copytree(source_home / "agent-roles", h / "agent-roles")
    for name in ["auth.json", "models_cache.json"]:
        shutil.copy2(Path(os.environ["CODEX_HOME"]) / name, h / name)
        (h / name).chmod(0o600)
    flags = {
        "model": "gpt-6-luna",
        "system1.enabled": False,
        "model_provider": "codex-validation",
        "model_reasoning_effort": "low",
        "model_context_window": 65536,
        "service_tier": "default",
        "model_catalog_json": str(R / "cheap-model-catalog.json"),
        "subagent_model_list": ["gpt-6-luna"],
        "notify": [],
        "diagnostic_log_capture": True,
        "sqlite_home": str(h / "sqlite"),
        "log_dir": str(h / "log"),
        "model_providers.codex-validation.name": "OpenAI",
        "model_providers.codex-validation.requires_openai_auth": True,
        "model_providers.codex-validation.supports_websockets": True,
        "model_providers.codex-validation.request_max_retries": 0,
        "model_providers.codex-validation.stream_max_retries": 0,
    }
    args = [
        *sandbox,
        "--bind",
        str(d),
        str(d),
        "--bind",
        str(raw),
        str(raw),
        "--",
        str(Path(os.environ["PGO_INSTRUMENTED_BIN"]).resolve()),
        "--no-daemon",
    ]
    for k, v in flags.items():
        args += ["-c", f"{k}={json.dumps(v)}"]
    args += ["app-server", "--listen", "stdio://"]
    env = dict(
        os.environ,
        CODEX_HOME=str(h),
        TMPDIR=str(tmp),
        LLVM_PROFILE_FILE=str(raw / f"s{index}-%m-%p.profraw"),
    )
    for k in ["CODEX_SQLITE_HOME", "JEV_API_KEY", "DECISION_API_KEY"]:
        env.pop(k, None)
    log = (d / "rpc.jsonl").open("w")
    err = (d / "stderr.log").open("w")
    p = await asyncio.create_subprocess_exec(
        *args,
        cwd=w,
        env=env,
        stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE,
        stderr=err,
        limit=16 * 1024 * 1024,
    )
    pending = {}
    done = {}
    counter = 0

    async def read():
        while line := await p.stdout.readline():
            x = json.loads(line)
            if x.get("id") in pending:
                future, method = pending.pop(x["id"])
                future.set_result(x)
                if method == "config/read":
                    continue  # The effective user config may contain credentials.
            log.write(json.dumps({"time": time.time(), "event": x}) + "\n")
            log.flush()
            if x.get("method") == "turn/completed":
                key = x["params"]["threadId"]
                done.setdefault(key, asyncio.Queue()).put_nowait(x)

    async def rpc(method, params):
        nonlocal counter
        counter += 1
        f = asyncio.get_running_loop().create_future()
        pending[counter] = (f, method)
        p.stdin.write(
            (
                json.dumps({"id": counter, "method": method, "params": params}) + "\n"
            ).encode()
        )
        await p.stdin.drain()
        x = await asyncio.wait_for(f, 90)
        if "error" in x:
            if method == "config/read":
                raise RuntimeError("Unable to verify isolated SQLite and log paths")
            raise RuntimeError(x)
        return x["result"]

    reader = asyncio.create_task(read())
    try:
        await rpc(
            "initialize",
            {
                "clientInfo": {"name": "parallel-pgo", "version": "1"},
                "capabilities": {"experimentalApi": True},
            },
        )
        p.stdin.write(b'{"method":"initialized"}\n')
        effective = (await rpc("config/read", {"cwd": str(w)}))["config"]
        for key in ("sqlite_home", "log_dir"):
            if effective.get(key) != flags[key]:
                raise RuntimeError(
                    f"Training requires the session-local {key}; check managed settings"
                )
        thread = (
            await rpc(
                "thread/start",
                {
                    "cwd": str(w),
                    "model": "gpt-6-luna",
                    "modelProvider": "codex-validation",
                    "approvalPolicy": "never",
                    "sandbox": "danger-full-access",
                },
            )
        )["thread"]["id"]
        await ready(index)
        for turn in range(1, turns + 1):
            prompt = f"Isolated coding exercise, session {index}, user turn {turn}/{turns}. Work only in {w}. Preserve sorted JSON output. "
            if turn in [1, (turns + 1) // 2, turns]:
                prompt += f"Spawn exactly {children} new default-role subagents in PARALLEL, model gpt-6-luna, reasoning_effort low, fork_turns none, distinct task_names prefixed wave{turn}_. Launch every child before waiting. Give them different bounded read-only audits or real shell checks of this fixture and accumulated scenario files; no recursive spawning or edits. Do not attempt direct messaging, followup_task, or direct app-server input: those are disabled. Collect all results via list_agents/wait_agent. "
            else:
                prompt += "Do not spawn agents this turn; the next parallel audit batch is scheduled later in this session. "
            if turn == 1:
                prompt += "Inspect source and existing tests. You own edits: fix read_events to skip blank lines while preserving JSON errors, summarize to ignore missing/invalid/negative/bool durations and missing/empty names. Run unittest and cli sample. "
            elif turn == 2:
                prompt += "Recall the original output-format constraint from history, add optional --min-ms CLI filtering of totals, demonstrate default and threshold 5. Run unittest. "
            else:
                prompt += f"Create scenario-{turn}.jsonl with varied valid data, verify default and --min-ms 5 output via actual tools, run unittest. Relate one result to previous turns without changing default behavior. "
            prompt += "Batch shell checks in one code-mode execution where practical; keep tool outputs short. Final answer below 80 words; finish TURN_OK only after checks pass."
            child_ids = []
            (d / f"prompt-{turn:02}.txt").write_text(prompt)
            result = await rpc(
                "turn/start",
                {"threadId": thread, "input": [{"type": "text", "text": prompt}]},
            )
            for current in [thread, *child_ids]:
                event = await asyncio.wait_for(
                    done.setdefault(current, asyncio.Queue()).get(), 900
                )
                status = event["params"]["turn"]["status"]
                if status != "completed":
                    raise RuntimeError(
                        f"Session {index} turn {turn} ended with status {status}"
                    )
            (d / "progress.json").write_text(
                json.dumps(
                    {
                        "thread": thread,
                        "completed_turns": turn,
                        "requested_turns": turns,
                        "children": children,
                    }
                )
            )
            print(index, turn, "completed", flush=True)
    finally:
        p.stdin.close()
        await asyncio.wait_for(p.wait(), 45)
        await reader
        log.close()
        err.close()


started = time.monotonic()
waiting = set()
barrier = asyncio.Event()


async def ready(index):
    waiting.add(index)
    if len(waiting) == 3:
        barrier.set()
    await barrier.wait()


async def monitor():
    while True:
        await asyncio.sleep(1)
        count = inputs = outputs = 0
        for f in R.rglob("rollout-*.jsonl"):
            last = None
            seen = set()
            for line in f.read_text().splitlines():
                try:
                    x = json.loads(line)
                except json.JSONDecodeError:
                    continue
                v = x.get("payload", {})
                if v.get("type") == "token_count" and v.get("info"):
                    last = v["info"]["total_token_usage"]
                    key = (last["input_tokens"], last["output_tokens"])
                    if key != (0, 0):
                        seen.add(key)
            count += len(seen)
            if last:
                inputs += last["input_tokens"]
                outputs += last["output_tokens"]
        (R / "usage.json").write_text(
            json.dumps(
                {
                    "completions": count,
                    "input": inputs,
                    "output": outputs,
                    "api_equivalent_usd": inputs * 0.1 / 1e6 + outputs * 0.5 / 1e6,
                }
            )
        )
        if (
            count >= 890
            or inputs >= 19_900_000
            or outputs >= 245_000
            or inputs * 0.1 / 1e6 + outputs * 0.5 / 1e6 >= 4.9
            or time.monotonic() - started >= 3600
        ):
            raise RuntimeError("Budget/time boundary; stop all sessions")


async def main():
    bwrap = shutil.which("bwrap")
    if not bwrap:
        raise RuntimeError("PGO training requires bubblewrap (bwrap) on Linux")
    # Managed paths can override -c before app-server accepts config/read.
    sandbox = [
        bwrap,
        "--die-with-parent",
        "--unshare-pid",
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
    ]
    try:
        subprocess.run([*sandbox, "--", "/bin/true"], check=True, timeout=10)
    except (OSError, subprocess.SubprocessError) as error:
        raise RuntimeError(
            "PGO training requires working bubblewrap mount and PID namespaces"
        ) from error
    raw = R / "profiles"
    raw.mkdir(mode=0o700)
    tasks = [
        asyncio.create_task(session(i, *v, sandbox, raw))
        for i, v in enumerate([(10, 4), (15, 3), (20, 2)], 1)
    ]
    m = asyncio.create_task(monitor())
    all_sessions = asyncio.gather(*tasks)
    finished, _ = await asyncio.wait(
        [all_sessions, m], return_when=asyncio.FIRST_COMPLETED
    )
    try:
        for f in finished:
            await f
    finally:
        m.cancel()
        for t in tasks:
            t.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)


if __name__ == "__main__":
    asyncio.run(main())
