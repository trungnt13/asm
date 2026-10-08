import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
from pathlib import Path

repo_root = Path(__file__).resolve().parents[2]
os.environ.setdefault("CODEX_REPO_ROOT", str(repo_root))
sys.path.insert(0, str(repo_root / "scripts"))

from codex_package.layout import validate_package_dir
from codex_package.targets import PACKAGE_VARIANTS
from codex_package.targets import TARGET_SPECS

archive_path = Path(sys.argv[1])
inspect_only = len(sys.argv) > 2 and sys.argv[2] == "--inspect-only"
release_targets = {"aarch64-apple-darwin", "x86_64-unknown-linux-gnu"}
target = archive_path.name.removeprefix("codex-").removesuffix(".tar.gz")
if target not in release_targets:
    raise SystemExit(f"Unsupported CLI archive: {archive_path}")
spec = TARGET_SPECS[target]
version = tomllib.loads((repo_root / "codex-rs/Cargo.toml").read_text())["workspace"][
    "package"
]["version"]
required_dirs = {"bin", "codex-resources", "codex-path"}
executables = {"bin/codex", "bin/codex-code-mode-host", "codex-path/rg"}
if spec.is_linux:
    executables.add("codex-resources/bwrap")
required_files = executables | {"codex-package.json"}
with tarfile.open(archive_path, "r:gz") as archive:
    members = archive.getmembers()
    if (
        {member.name for member in members} != required_dirs | required_files
        or len(members) != len(required_dirs | required_files)
        or any(
            not member.isdir()
            if member.name in required_dirs
            else (
                not member.isfile()
                or member.size == 0
                or (member.name in executables and not member.mode & 0o111)
            )
            for member in members
        )
        or any(
            member.name == "codex-package.json" and member.size > 4096
            for member in members
        )
    ):
        raise SystemExit(f"Invalid complete CLI package archive: {archive_path}")
    with tempfile.TemporaryDirectory() as temporary_dir:
        package = Path(temporary_dir)
        # Only exact regular files and directories above can reach extraction.
        for member in members:
            path = package / member.name
            if member.isdir():
                path.mkdir(parents=True, exist_ok=True)
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                with archive.extractfile(member) as source, path.open("wb") as output:
                    shutil.copyfileobj(source, output)
                os.chmod(path, member.mode & 0o777)
        validate_package_dir(package, PACKAGE_VARIANTS["codex"], spec)
        metadata_fields = json.loads(
            (package / "codex-package.json").read_text(), object_pairs_hook=list
        )
        expected_metadata = {
            "layoutVersion": 1,
            "version": version,
            "target": target,
            "variant": "codex",
            "entrypoint": "bin/codex",
            "resourcesDir": "codex-resources",
            "pathDir": "codex-path",
        }
        if (
            len(metadata_fields) != len(expected_metadata)
            or dict(metadata_fields) != expected_metadata
            or type(dict(metadata_fields)["layoutVersion"]) is not int
        ):
            raise SystemExit("Package metadata does not match the candidate contract")
        if inspect_only:
            print(f"archive={archive_path} target={target} version={version} package=complete")
            raise SystemExit(0)
        if spec.is_linux:
            for name in sorted(executables):
                executable = package / name
                headers = subprocess.check_output(
                    [
                        "readelf", "--file-header", "--program-headers",
                        "--wide", executable,
                    ],
                    text=True,
                )
                # The pinned x86_64 ripgrep is static MUSL; ASM and bwrap are GNU.
                if not all(
                    value in headers
                    for value in ("ELF64", "Advanced Micro Devices X86-64")
                ) or (
                    name != "codex-path/rg" and "/lib64/ld-linux-x86-64.so.2" not in headers
                ):
                    raise SystemExit(f"{name} is not an expected x86_64 executable")
                versions = subprocess.check_output(
                    ["readelf", "--version-info", "--wide", executable], text=True
                )
                glibc_versions = set(re.findall(r"Name: (GLIBC_\S+)", versions))
                if (name != "codex-path/rg" and not glibc_versions) or any(
                    not re.fullmatch(r"GLIBC_[0-9]+(?:\.[0-9]+)+", value)
                    or tuple(int(part) for part in value[6:].split(".")) > (2, 35)
                    for value in glibc_versions
                ):
                    raise SystemExit(
                        f"{name} exceeds the glibc 2.35 baseline: {sorted(glibc_versions)}"
                    )
                dependencies = subprocess.check_output(
                    ["readelf", "--dynamic", "--wide", executable], text=True
                )
                libraries = re.findall(r"\(NEEDED\).*?\[(.*?)\]", dependencies)
                print(
                    f"{name} glibc_versions={sorted(glibc_versions)} needed={libraries}"
                )
        binary_path = package / "bin/codex"
        helper_path = package / "bin/codex-code-mode-host"
        reported_version = subprocess.check_output(
            [binary_path, "--version"], text=True, timeout=30
        ).strip()
        if reported_version != f"codex-cli {version}":
            raise SystemExit(
                f"Wrong binary version: {reported_version}; expected codex-cli {version}"
            )
        for executable in (binary_path, helper_path):
            output = subprocess.check_output(
                [executable, "--help"], text=True, timeout=30
            )
            if not any(line.startswith("Usage:") for line in output.splitlines()):
                raise SystemExit(f"{executable.name} help lacks Usage")
        subprocess.run([package / "codex-path/rg", "--version"], check=True, timeout=30)
        if spec.is_linux:
            output = subprocess.check_output(
                [package / "codex-resources/bwrap", "--help"], text=True, timeout=30
            )
            if not all(option in output for option in ("--argv0", "--perms", "--as-pid-1")):
                raise SystemExit("Bundled Bubblewrap lacks required sandbox options")
        print(
            f"archive={archive_path} bytes={archive_path.stat().st_size} "
            f"version={reported_version} package=complete help=ok helper_help=ok resources=ok"
        )
        print(
            f"binary_bytes={binary_path.stat().st_size} helper_bytes={helper_path.stat().st_size}"
        )
