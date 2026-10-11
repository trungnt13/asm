#!/usr/bin/env python3
"""Format repository sources or check that they are already formatted."""

import argparse
import os
import re
import shlex
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]


@dataclass(frozen=True)
class Command:
    args: tuple[str, ...]
    cwd: Path = REPO_ROOT


@dataclass(frozen=True)
class FormatterGroup:
    name: str
    commands: tuple[Command, ...]


@dataclass(frozen=True)
class FormatterResult:
    name: str
    output: str
    returncode: int


def just_formatter_group(*, check: bool) -> FormatterGroup:
    args = ["just", "--unstable", "--fmt"]
    if check:
        args.append("--check")
    return FormatterGroup("Just", (Command(tuple(args)),))


def rust_formatter_group(*, check: bool, files: tuple[str, ...] = ()) -> FormatterGroup:
    if files:
        cargo_toml = (REPO_ROOT / "codex-rs/Cargo.toml").read_text()
        package = re.search(
            r"(?ms)^\[workspace\.package\]\s*\n(.*?)(?=^\[|\Z)", cargo_toml
        )
        edition = (
            re.search(r'^edition\s*=\s*"(2015|2018|2021|2024)"', package[1], re.M)
            if package
            else None
        )
        if edition is None:
            raise ValueError("Cannot determine the Rust workspace edition")
        args = [
            "rustfmt",
            "--edition",
            edition[1],
            "--config-path",
            str(REPO_ROOT / "codex-rs/rustfmt.toml"),
            "--config",
            "imports_granularity=Item,skip_children=true",
        ]
    else:
        args = ["cargo", "fmt", "--", "--config", "imports_granularity=Item"]
    if check:
        args.append("--check")
    if files:
        args.extend(("--", *files))
    command = Command(tuple(args), REPO_ROOT / "codex-rs")
    return FormatterGroup("Rust", (command,))


def is_buildifier_file(path: Path) -> bool:
    name = path.name
    return (
        name in {"BUILD", "WORKSPACE", "MODULE.bazel"}
        or name.startswith(("BUILD.", "WORKSPACE."))
        or name.endswith((".BUILD.bazel", ".MODULE.bazel", ".bzl", ".sky"))
        or ".bzl." in name
        or ".sky." in name
    )


def buildifier_formatter_group(
    *, check: bool, files: tuple[str, ...] = ()
) -> FormatterGroup:
    if files:
        buildifier_files = ["--", *files]
    else:
        repository_files = subprocess.check_output(
            ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
            cwd=REPO_ROOT,
        ).split(b"\0")
        buildifier_files = sorted(
            Path(os.fsdecode(path)).as_posix()
            for path in repository_files
            if path and is_buildifier_file(Path(os.fsdecode(path)))
        )

    # Invoke DotSlash explicitly because Windows does not honor shebangs.
    buildifier_args = [
        "dotslash",
        str(REPO_ROOT / "tools" / "buildifier"),
        "-mode=check" if check else "-mode=fix",
        "-lint=off",
        *buildifier_files,
    ]
    return FormatterGroup("Bazel/Starlark", (Command(tuple(buildifier_args)),))


def python_sdk_formatter_group(
    *, check: bool, files: tuple[str, ...] = ()
) -> FormatterGroup:
    # Each `--project` retains its local dependency and Ruff configuration context.
    uv_run_args = [
        "uv",
        "run",
        "--frozen",
        "--project",
        "sdk/python",
        "--only-group",
        "format",
    ]
    format_args = [
        *uv_run_args,
        "ruff",
        "format",
    ]
    if check:
        format_args.append("--check")
        # `ruff check --diff` reports lint-driven rewrites without changing files.
        # It is the check-mode counterpart of `--fix --fix-only`, not a full lint gate.
        lint_args = ["ruff", "check", "--diff"]
    else:
        # Ruff's lint fixer and formatter are separate passes: the first applies
        # fixable lint rewrites, while the second formats source layout.
        lint_args = ["ruff", "check", "--fix", "--fix-only"]

    if files:
        lint_args.append("--force-exclude")
        format_args.append("--force-exclude")
    paths = ("--", *files) if files else ("sdk/python",)
    return FormatterGroup(
        "Python SDK",
        (
            Command((*uv_run_args, *lint_args, *paths)),
            Command((*format_args, *paths)),
        ),
    )


def python_scripts_formatter_group(
    *, check: bool, files: tuple[str, ...] = ()
) -> FormatterGroup:
    args = ["uv", "run", "--frozen", "--project", "scripts", "ruff", "format"]
    if check:
        args.append("--check")
    args.extend(("--force-exclude", "--", *files) if files else (".",))
    return FormatterGroup("Python scripts", (Command(tuple(args)),))


def formatter_groups(*, check: bool) -> tuple[FormatterGroup, ...]:
    return (
        just_formatter_group(check=check),
        rust_formatter_group(check=check),
        buildifier_formatter_group(check=check),
        python_sdk_formatter_group(check=check),
        python_scripts_formatter_group(check=check),
    )


def scoped_formatter_groups(
    paths: list[str], *, check: bool
) -> tuple[FormatterGroup, ...]:
    """Select formatter groups for explicit repository files."""
    selected: dict[str, list[str]] = {
        name: [] for name in ("Just", "Rust", "Bazel", "SDK", "Python")
    }
    for raw in paths:
        path = REPO_ROOT / Path(raw)
        relative = Path(os.path.relpath(path, REPO_ROOT))
        if relative == Path("justfile"):
            name = "Just"
        elif path.suffix == ".rs" and relative.is_relative_to("codex-rs"):
            name = "Rust"
        elif is_buildifier_file(path):
            name = "Bazel"
        elif path.suffix in {".py", ".pyi"}:
            name = "SDK" if relative.is_relative_to("sdk/python") else "Python"
        else:
            raise ValueError(f"No supported formatter for: {raw!r}")
        selected[name].append(str(path))

    groups = []
    for name, builder in (
        ("Just", just_formatter_group),
        ("Rust", rust_formatter_group),
        ("Bazel", buildifier_formatter_group),
        ("SDK", python_sdk_formatter_group),
        ("Python", python_scripts_formatter_group),
    ):
        files = tuple(dict.fromkeys(selected[name]))
        if not files:
            continue
        group = (
            builder(check=check)
            if name == "Just"
            else builder(check=check, files=files)
        )
        groups.append(group)
    return tuple(groups)


def run_formatter_group(group: FormatterGroup) -> FormatterResult:
    """Run one formatter group sequentially and return its buffered output."""
    for command in group.commands:
        try:
            process = subprocess.run(
                command.args,
                cwd=command.cwd,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                check=False,
            )
        except OSError as error:
            output = f"$ {shlex.join(command.args)}\n{error}\n"
            return FormatterResult(group.name, output, 1)

        if process.returncode != 0:
            output = f"$ {shlex.join(command.args)}\n{process.stdout}"
            if process.stdout and not process.stdout.endswith("\n"):
                output += "\n"
            return FormatterResult(group.name, output, process.returncode)

    return FormatterResult(group.name, "", 0)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--check",
        action="store_true",
        help="check formatting without modifying files",
    )
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument(
        "--files",
        action="store_true",
        help="format explicit checkout-relative or absolute files",
    )
    selection.add_argument(
        "--rust", action="store_true", help="format only the complete Rust workspace"
    )
    parser.add_argument(
        "paths", nargs="*", help="files for --files; use -- before leading hyphens"
    )
    args = parser.parse_args()
    if args.files != bool(args.paths):
        parser.error("--files requires one or more paths; paths require --files")
    try:
        if args.files:
            groups = scoped_formatter_groups(args.paths, check=args.check)
        elif args.rust:
            groups = (rust_formatter_group(check=args.check),)
        else:
            groups = formatter_groups(check=args.check)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Cannot select formatting files: {error}\n")

    failures: list[str] = []
    with ThreadPoolExecutor(max_workers=len(groups)) as executor:
        futures = [executor.submit(run_formatter_group, group) for group in groups]
        for future in as_completed(futures):
            result = future.result()
            if result.returncode != 0:
                failures.append(result.name)
                print(f"==> {result.name} formatter failed", file=sys.stderr)
                print(result.output, end="", file=sys.stderr)

    if failures:
        print(f"Formatting failed: {', '.join(failures)}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
