#!/usr/bin/env python3

# Local recipes share a small profile while explicit selections and CI stay unchanged.
import hashlib
import json
import os
import shutil
import stat
import subprocess
import sys
import time
from contextlib import ExitStack
from datetime import date
from pathlib import Path

STALE_ARTIFACT_DAYS = 3


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


def cache_use_blocker(cache: Path) -> str | None:
    # Process visibility is required, not inferred from artifact timestamps.
    if sys.platform != "linux":
        return "complete process inspection is unavailable on this host"
    try:
        for process in Path("/proc").iterdir():
            if not process.name.isdigit() or int(process.name) == os.getpid():
                continue
            try:
                name = (process / "comm").read_text().strip()
                if name in {"cargo", "rustc", "rust-analyzer"}:
                    return f"Rust build or editor process {process.name} is active"
                paths = [os.readlink(process / "cwd")]
                for descriptor in (process / "fd").iterdir():
                    try:
                        paths.append(os.readlink(descriptor))
                    except FileNotFoundError:
                        continue  # Descriptors can close during inspection.
                for mapping in (process / "maps").read_text().splitlines():
                    fields = mapping.split(maxsplit=5)
                    if len(fields) == 6:
                        paths.append(fields[5])
            except FileNotFoundError:
                if not process.exists():
                    continue  # The process exited during inspection.
                return f"cannot inspect process {process.name} completely"
            if any(
                path.startswith("/")
                and Path(path.removesuffix(" (deleted)")).is_relative_to(cache)
                for path in paths
            ):
                return f"process {process.name} uses the cache"
    except OSError:
        return "process inspection failed or access was denied"
    return None


def remove_stale_artifacts(cache: Path) -> tuple[int, str]:
    import fcntl

    blocker = cache_use_blocker(cache)
    if blocker:
        return 0, blocker
    if sys.version_info < (3, 11) or not shutil.rmtree.avoids_symlink_attacks:
        return 0, "descriptor-relative, symlink-safe removal is unavailable"
    stale_before = time.time() - STALE_ARTIFACT_DAYS * 86400
    removed = 0
    skipped = []
    # Directory descriptors keep traversal anchored when paths change.
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    try:
        with ExitStack() as root:
            cache_fd = os.open(cache, flags)
            root.callback(os.close, cache_fd)
            for profile in os.listdir(cache_fd):
                try:
                    with ExitStack() as handles:
                        profile_fd = os.open(profile, flags, dir_fd=cache_fd)
                        handles.callback(os.close, profile_fd)
                        lock_fd = os.open(
                            ".cargo-lock", os.O_RDWR | os.O_NOFOLLOW, dir_fd=profile_fd
                        )
                        handles.callback(os.close, lock_fd)
                        if not stat.S_ISREG(os.fstat(lock_fd).st_mode):
                            skipped.append("non-regular Cargo lock")
                            continue
                        # Cargo uses this lock too, including direct builds and runs.
                        fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                        blocker = cache_use_blocker(cache)
                        if blocker:
                            skipped.append(blocker)
                            break
                        for directory in ("incremental", "deps"):
                            try:
                                directory_fd = os.open(
                                    directory, flags, dir_fd=profile_fd
                                )
                            except FileNotFoundError:
                                continue
                            handles.callback(os.close, directory_fd)
                            physical = Path(f"/proc/self/fd/{directory_fd}").resolve(
                                strict=True
                            )
                            if not physical.is_relative_to(cache):
                                skipped.append(
                                    "artifact directory is outside the cache"
                                )
                                continue
                            for artifact in os.listdir(directory_fd):
                                status = os.stat(
                                    artifact, dir_fd=directory_fd, follow_symlinks=False
                                )
                                is_dir = stat.S_ISDIR(status.st_mode)
                                if not is_dir and not stat.S_ISREG(status.st_mode):
                                    continue
                                last_use = (
                                    status.st_mtime
                                    if is_dir
                                    else max(status.st_atime, status.st_mtime)
                                )
                                if last_use >= stale_before:
                                    continue
                                if is_dir:
                                    shutil.rmtree(artifact, dir_fd=directory_fd)
                                else:
                                    os.unlink(artifact, dir_fd=directory_fd)
                                removed += 1
                except OSError:
                    # Busy Cargo locks, symlinks, missing locks and I/O errors fail closed.
                    skipped.append("unsafe, busy, or unreadable cache profile")
    except OSError:
        skipped.append("cache inspection or deletion failed")
    return removed, "; ".join(dict.fromkeys(skipped)) or "none"


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
        removed, skipped = remove_stale_artifacts(cache)
        log.write(
            f"stale_artifacts_removed={removed}\nstale_artifacts_skip_reason={skipped}\n".encode()
        )
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
