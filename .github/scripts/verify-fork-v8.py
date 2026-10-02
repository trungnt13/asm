"""Check a downloaded fork V8 release pair against GitHub asset digests."""

import hashlib
import json
import re
import sys
from pathlib import Path


def verify(metadata: dict, directory: Path, target: str, version: str) -> None:
    tag = f"asm-v8-v{version}-glibc2.35"
    if (
        metadata.get("tag_name") != tag
        or metadata.get("draft") is not False
        or metadata.get("prerelease") is not False
    ):
        raise ValueError(f"expected published normal release {tag}")

    prefix = "ptrcomp_sandbox_release"
    archive = f"librusty_v8_{prefix}_{target}.a.gz"
    binding = f"src_binding_{prefix}_{target}.rs"
    manifest = f"rusty_v8_{prefix}_{target}.sha256"
    names = (archive, binding, manifest)
    assets = metadata.get("assets", [])
    by_name = {asset["name"]: asset for asset in assets}
    if len(by_name) != len(assets) or not all(name in by_name for name in names):
        raise ValueError("V8 release has missing or duplicate target assets")

    digests = {}
    for name in names:
        expected = by_name[name].get("digest", "")
        if not isinstance(expected, str) or not re.fullmatch(
            r"sha256:[0-9a-f]{64}", expected
        ):
            raise ValueError(f"missing GitHub SHA-256 digest for {name}")
        with (directory / name).open("rb") as asset:
            actual = hashlib.file_digest(asset, "sha256").hexdigest()
        if actual != expected.removeprefix("sha256:"):
            raise ValueError(f"GitHub SHA-256 mismatch for {name}")
        if name != manifest:
            digests[name] = actual

    lines = (directory / manifest).read_text(encoding="utf-8").splitlines()
    parsed = {}
    for line in lines:
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.-]+)", line)
        if not match or match.group(2) in parsed:
            raise ValueError("invalid V8 checksum manifest")
        parsed[match.group(2)] = match.group(1)
    if parsed != {archive: digests[archive], binding: digests[binding]}:
        raise ValueError("V8 manifest does not match the exact target pair")


if __name__ == "__main__":
    try:
        verify(
            json.loads(Path(sys.argv[1]).read_text()),
            Path(sys.argv[2]),
            sys.argv[3],
            sys.argv[4],
        )
    except (OSError, ValueError, KeyError, IndexError) as error:
        raise SystemExit(f"Fork V8 verification failed: {error}") from error
