#!/usr/bin/env python3
"""Prepare the pinned PGO profile for a Cargo build."""

import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import urllib.request


PROFILE_MANIFESTS = {
    "x86_64-unknown-linux-gnu": "agent-linux.json",
    "x86_64-unknown-linux-musl": "agent-linux-musl.json",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, choices=PROFILE_MANIFESTS)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--env-file", type=Path, help="Append Cargo settings as NAME=VALUE lines"
    )
    parser.add_argument("--lto", choices=("thin", "fat"), default="thin")
    args = parser.parse_args()
    codegen_units = 1 if args.lto == "fat" else 4
    manifest_path = Path(__file__).resolve().with_name(PROFILE_MANIFESTS[args.target])
    if not manifest_path.is_file():
        parser.error(
            f"No published PGO profile is pinned for {args.target}: {manifest_path.name}"
        )
    manifest = json.loads(manifest_path.read_text())
    if args.target != manifest["target"]:
        parser.error(f"PGO manifest {manifest_path.name} does not match {args.target}")
    args.output.mkdir(parents=True, exist_ok=True)
    profile = args.output.resolve() / "agent.profdata"
    with tempfile.TemporaryDirectory(dir=args.output) as temporary:
        archive = Path(temporary) / "profile.gz"
        with (
            urllib.request.urlopen(manifest["url"], timeout=120) as source,
            archive.open("wb") as dest,
        ):
            shutil.copyfileobj(source, dest)
        if (
            hashlib.sha256(archive.read_bytes()).hexdigest()
            != manifest["archive_sha256"]
        ):
            raise RuntimeError("PGO archive checksum mismatch")
        staged = Path(temporary) / "profile.profdata"
        with gzip.open(archive, "rb") as source, staged.open("wb") as dest:
            shutil.copyfileobj(source, dest)
        if (
            staged.stat().st_size != manifest["profile_bytes"]
            or hashlib.sha256(staged.read_bytes()).hexdigest()
            != manifest["profile_sha256"]
        ):
            raise RuntimeError("PGO profile checksum/size mismatch")
        shutil.move(staged, profile)
    version = subprocess.check_output(["rustc", "-Vv"], text=True)
    host = next(
        line.split(": ", 1)[1]
        for line in version.splitlines()
        if line.startswith("host: ")
    )
    sysroot = subprocess.check_output(
        ["rustc", "--print", "sysroot"], text=True
    ).strip()
    tool = Path(sysroot) / "lib/rustlib" / host / "bin/llvm-profdata"
    if not tool.is_file():
        raise RuntimeError(
            "Install llvm-tools-preview for the consuming Rust toolchain"
        )
    validation = subprocess.run(
        [str(tool), "show", str(profile)],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    report = {
        "manifest": manifest,
        "consumer_rustc": version,
        "consumer_settings": {
            "lto": args.lto,
            "codegen_units": codegen_units,
            "cargo_jobs": 1,
        },
        "indexed_format_validation": validation.stdout,
        "indexed_format_exit_code": validation.returncode,
        "coverage_validation": "Inspect compiler missing/mismatched-profile diagnostics and candidate acceptance; readable format alone does not prove useful coverage.",
    }
    (args.output / "validation.json").write_text(json.dumps(report, indent=2) + "\n")
    print(version + validation.stdout)
    if validation.returncode != 0:
        return validation.returncode
    if args.env_file:
        separator = "\x1f"
        encoded = os.environ.get("CARGO_ENCODED_RUSTFLAGS")
        if encoded is None:
            flags = os.environ.get("RUSTFLAGS", "").split()
        else:
            flags = encoded.split(separator) if encoded else []
        if any("profile-" in flag for flag in flags):
            raise RuntimeError("Refusing to combine conflicting profiling flags")
        flags.extend(
            [f"-Cprofile-use={profile}", "-Cllvm-args=-pgo-warn-missing-function"]
        )
        with args.env_file.open("a") as output:
            output.write(f"CARGO_ENCODED_RUSTFLAGS={separator.join(flags)}\n")
            output.write(
                f"CARGO_PROFILE_RELEASE_LTO={args.lto}\n"
                f"CARGO_PROFILE_RELEASE_CODEGEN_UNITS={codegen_units}\n"
                "CARGO_BUILD_JOBS=1\n"
                "ASM_BUILD_VARIANT=pgo\n"
            )
    return 0


if __name__ == "__main__":
    sys.exit(main())
