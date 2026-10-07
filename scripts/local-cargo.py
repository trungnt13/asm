#!/usr/bin/env python3

# Local recipes share a small profile while explicit selections and CI stay unchanged.
import hashlib
import json
import os
import subprocess
import sys
import time
from contextlib import ExitStack
from datetime import date
from pathlib import Path


def common_root() -> Path:
    common = subprocess.check_output(
        [
            "git",
            "-C",
            str(Path(__file__).resolve().parents[1]),
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ],
        text=True,
    ).strip()
    return Path(common).parent


def target_dir() -> str:
    return os.environ.get("CARGO_TARGET_DIR") or str(common_root() / "codex-rs/target")


def main() -> int:
    args = sys.argv[1:]
    if args == ["--print-target-dir"]:
        print(
            os.environ.get("CARGO_TARGET_DIR")
            or ("target" if os.environ.get("CI") or os.name == "nt" else target_dir())
        )
        return 0
    cargo_args = args[: args.index("--")] if "--" in args else args
    offset = int(bool(args and args[0].startswith("+")))
    nextest = args[offset : offset + 2] == ["nextest", "run"]
    option = "--cargo-profile" if nextest else "--profile"
    if not os.environ.get("CI") and not any(
        arg in (option, "--release", "-r") or arg.startswith(f"{option}=")
        for arg in cargo_args
    ):
        position = offset + (2 if nextest else 1)
        args[position:position] = [option, "dev-small"]
    if os.environ.get("CI") or os.name == "nt":
        return subprocess.call(["cargo", *args])
    import fcntl

    root = common_root()
    ownership = ExitStack()
    source = Path.cwd()
    for index, arg in enumerate(cargo_args):
        if arg == "--manifest-path" and index + 1 < len(cargo_args):
            source = Path(cargo_args[index + 1]).resolve().parent
        elif arg.startswith("--manifest-path="):
            source = Path(arg.split("=", 1)[1]).resolve().parent
    for checkout in (source, *source.parents):
        if checkout.parent.parent == root / ".agents/dev-worktrees":
            checkout_lock = ownership.enter_context(
                (checkout.parent / "checkout.lock").open("a")
            )
            fcntl.flock(checkout_lock, fcntl.LOCK_SH)
            break
    env = {**os.environ, "CARGO_TARGET_DIR": target_dir()}
    source_checkout = subprocess.run(
        ["git", "-C", str(source), "rev-parse", "--show-toplevel"],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    if (
        source_checkout.returncode == 0
        and "RUSTC_WORKSPACE_WRAPPER" not in env
        and "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER" not in env
    ):
        env["RUSTC_WORKSPACE_WRAPPER"] = str(
            Path(source_checkout.stdout.strip()) / "scripts/local-rustc-workspace.sh"
        )
    command = args[offset] if len(args) > offset else ""
    if (
        command == "clippy"
        and "CARGO_TARGET_DIR" not in os.environ
        and not any(
            arg == "--target-dir" or arg.startswith("--target-dir=")
            for arg in cargo_args
        )
    ):
        # Clippy replaces the workspace wrapper, so it needs a private cache.
        checkout_path = source_checkout.stdout.strip() or str(source)
        namespace = hashlib.sha256(checkout_path.encode()).hexdigest()
        env["CARGO_TARGET_DIR"] = str(root / "codex-rs/target/clippy" / namespace)
    if command not in {"build", "check", "clippy", "nextest"}:
        with ownership:
            return subprocess.call(["cargo", *args], env=env)

    effective_target = env["CARGO_TARGET_DIR"]
    for index, arg in enumerate(cargo_args):
        if arg == "--target-dir" and index + 1 < len(cargo_args):
            effective_target = cargo_args[index + 1]
        elif arg.startswith("--target-dir="):
            effective_target = arg.split("=", 1)[1]
    cache = Path(effective_target).resolve()
    cache.mkdir(parents=True, exist_ok=True)
    logs = root / ".agents/dev-build-cache" / date.today().isoformat()
    logs.mkdir(parents=True, exist_ok=True)
    log_path = logs / f"cargo-{time.time_ns()}-{os.getpid()}.log"
    env.setdefault("CARGO_LOG", "cargo::core::compiler::fingerprint=info")
    toolchain = args[:offset]
    source_head = subprocess.run(
        ["git", "-C", str(source), "rev-parse", "HEAD"],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    inputs = {
        "source": {
            "path": str(source),
            "head": source_head.stdout.strip() if source_head.returncode == 0 else None,
            "dirty": bool(
                subprocess.check_output(
                    ["git", "-C", str(source), "status", "--porcelain"]
                )
            )
            if source_head.returncode == 0
            else None,
        },
        "cwd": str(Path.cwd()),
        "argv": ["cargo", *(args[: args.index("--")] if "--" in args else args)],
        "target_dir": str(cache),
        "rustc": subprocess.check_output(
            ["rustc", *toolchain, "-vV"], text=True, env=env
        ),
        "env": {
            key: env.get(key)
            for key in (
                "RUSTUP_TOOLCHAIN",
                "RUSTFLAGS",
                "CARGO_ENCODED_RUSTFLAGS",
                "RUSTC_WRAPPER",
                "RUSTC_WORKSPACE_WRAPPER",
                "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
                "RUSTY_V8_ARCHIVE",
                "RUSTY_V8_SRC_BINDING_PATH",
                "AWS_LC_SYS_NO_JITTER_ENTROPY",
                "CARGO_LOG",
            )
        },
    }
    print(f"Cargo diagnostics: {log_path}", file=sys.stderr)
    with (
        ownership,
        (cache / ".asm-local-cargo.lock").open("a") as lock,
        log_path.open("wb") as log,
    ):
        log.write((json.dumps(inputs, indent=2) + "\n").encode())
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            print(f"Waiting for local Cargo cache: {cache}", file=sys.stderr)
            fcntl.flock(lock, fcntl.LOCK_EX)
        with subprocess.Popen(
            ["cargo", *args], env=env, stderr=subprocess.PIPE
        ) as child:
            for chunk in iter(lambda: child.stderr.read1(65536), b""):
                log.write(chunk)
                sys.stderr.buffer.write(chunk)
                sys.stderr.buffer.flush()
            result = child.wait()
        log.write(f"\nexit_code={result}\n".encode())
        return result


if __name__ == "__main__":
    raise SystemExit(main())
