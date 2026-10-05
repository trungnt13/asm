#!/usr/bin/env python3

# Local recipes share a small profile while explicit selections and CI stay unchanged.
import os
import subprocess
import sys


def main() -> int:
    args = sys.argv[1:]
    cargo_args = args[: args.index("--")] if "--" in args else args
    nextest = args[:2] == ["nextest", "run"]
    option = "--cargo-profile" if nextest else "--profile"
    if not os.environ.get("CI") and not any(
        arg in (option, "--release", "-r") or arg.startswith(f"{option}=")
        for arg in cargo_args
    ):
        position = 2 if nextest else 1
        args[position:position] = [option, "dev-small"]
    return subprocess.call(["cargo", *args])


if __name__ == "__main__":
    raise SystemExit(main())
