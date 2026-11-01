#!/usr/bin/env python3

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from wrapper_common import (
    TOOLCHAIN_CHANNEL,
    build_final_args,
    exec_command,
    fetch_packaged_entrypoint,
    normalize_packaged_library,
    parse_wrapper_args,
    prefer_rustup_shims,
    repo_root,
    set_default_lint_env,
)


def main() -> "Never":
    root = repo_root()
    parsed = parse_wrapper_args(sys.argv[1:])
    final_args = build_final_args(parsed, root / "codex-rs" / "Cargo.toml")

    env = os.environ.copy()
    prefer_rustup_shims(env)
    set_default_lint_env(env)

    package_entrypoint = fetch_packaged_entrypoint(
        root / "tools" / "argument-comment-lint" / "argument-comment-lint",
        env,
    )
    # Dylint 6.1's driver bootstrap requires Clippy's removed master branch.
    cache = root / ".agents" / "argument-comment-lint" / "6.3.2"
    cargo_dylint = (
        cache / "bin" / ("cargo-dylint.exe" if os.name == "nt" else "cargo-dylint")
    )
    if not cargo_dylint.is_file():
        subprocess.run(
            [
                "rustup",
                "run",
                TOOLCHAIN_CHANNEL,
                "cargo",
                "install",
                "--locked",
                "--root",
                str(cache),
                "cargo-dylint@6.3.2",
            ],
            env={**env, "CARGO_TARGET_DIR": str(cache / "target")},
            check=True,
        )
    library_path = normalize_packaged_library(package_entrypoint)

    command = [str(cargo_dylint), "dylint", "--lib-path", str(library_path)]
    if not parsed.has_library_selection:
        command.append("--all")
    command.extend(final_args)
    exec_command(command, env)


if __name__ == "__main__":
    main()
