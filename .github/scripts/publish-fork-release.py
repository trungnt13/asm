"""Resume immutable fork publication, verifying a draft before making it Latest."""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ASSET_NAMES = (
    "codex-aarch64-apple-darwin.tar.gz",
    "codex-x86_64-unknown-linux-gnu.tar.gz",
    "SHA256SUMS",
    "install.sh",
)
ROOT = Path(__file__).resolve().parents[2]


def api(repository, endpoint, *, payload=None, optional=False):
    command = ["gh", "api", f"repos/{repository}/{endpoint}"]
    if payload is not None:
        method = "PATCH" if endpoint.startswith("releases/") else "POST"
        command += ["--method", method, "--input", "-"]
    result = subprocess.run(
        command,
        input=json.dumps(payload) if payload is not None else None,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode:
        if optional and "(HTTP 404)" in result.stderr:
            return None
        raise ValueError(f"GitHub API {endpoint}: {result.stderr.strip()}")
    return json.loads(result.stdout)


def release_for_tag(repository, tag):
    release = api(repository, f"releases/tags/{tag}", optional=True)
    if release is not None:
        return release
    # The by-tag endpoint omits drafts; authenticated release listings include them.
    matches = []
    page = 1
    while True:
        releases = api(repository, f"releases?per_page=100&page={page}")
        matches.extend(release for release in releases if release["tag_name"] == tag)
        if len(matches) > 1:
            raise ValueError(f"multiple releases use tag {tag}")
        if len(releases) < 100:
            return matches[0] if matches else None
        page += 1


def verify_tag(repository, tag, sha, *, create=False):
    reference = api(repository, f"git/ref/tags/{tag}", optional=True)
    if reference is None:
        if not create:
            raise ValueError(f"missing tag {tag}")
        reference = api(
            repository, "git/refs", payload={"ref": f"refs/tags/{tag}", "sha": sha}
        )
    target = reference["object"]
    seen = set()
    while target["type"] == "tag":
        if target["sha"] in seen:
            raise ValueError("cyclic annotated tag")
        seen.add(target["sha"])
        target = api(repository, f"git/tags/{target['sha']}")["object"]
    if target["type"] != "commit" or target["sha"] != sha:
        raise ValueError(f"{tag} does not resolve to candidate {sha}; never moving it")


def asset_metadata(release, *, complete):
    assets = release.get("assets", [])
    by_name = {asset["name"]: asset for asset in assets}
    if len(by_name) != len(assets) or not set(by_name) <= set(ASSET_NAMES):
        raise ValueError("release has duplicate or unexpected assets")
    if complete and set(by_name) != set(ASSET_NAMES):
        raise ValueError("release does not contain the exact four required assets")
    return by_name


def verify_asset(path, asset):
    if asset.get("state") != "uploaded":
        raise ValueError(
            f"asset {path.name} has not finished uploading; never replacing it"
        )
    digest = asset.get("digest", "")
    if not isinstance(digest, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
        raise ValueError(f"missing GitHub SHA-256 digest for {path.name}")
    with path.open("rb") as stream:
        actual = hashlib.file_digest(stream, "sha256").hexdigest()
    if path.stat().st_size != asset["size"] or digest != f"sha256:{actual}":
        raise ValueError(f"GitHub size or SHA-256 mismatch for {path.name}")


def verify_files(directory, installer):
    digests = {}
    for name in ASSET_NAMES:
        with (directory / name).open("rb") as stream:
            digests[name] = hashlib.file_digest(stream, "sha256").hexdigest()
    checksums = {}
    for line in (directory / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.-]+)", line)
        if not match or match.group(2) in checksums:
            raise ValueError("invalid or duplicate SHA256SUMS entry")
        checksums[match.group(2)] = match.group(1)
    if checksums != {
        name: digests[name] for name in ASSET_NAMES if name != "SHA256SUMS"
    }:
        raise ValueError("SHA256SUMS does not match the exact three payloads")
    if (directory / "install.sh").read_bytes() != installer:
        raise ValueError("installer differs from the candidate commit")
    for name in ASSET_NAMES[:2]:
        subprocess.run(
            [
                sys.executable,
                ROOT / ".github/scripts/smoke-codex-archive.py",
                directory / name,
                "--inspect-only",
            ],
            check=True,
        )


def verify_remote(repository, tag, sha, release, installer, *, published):
    verify_tag(repository, tag, sha)
    if release.get("tag_name") != tag or release.get("prerelease") is not False:
        raise ValueError("release tag or normal-release flag is incorrect")
    if published:
        if release.get("draft") is not False:
            raise ValueError("release is still a draft")
        latest = api(repository, "releases/latest")
        if latest["id"] != release["id"]:
            raise ValueError("release is not Latest; not editing a published release")
    elif release.get("draft") is not True:
        raise ValueError("expected a draft before publication")
    assets = asset_metadata(release, complete=True)
    with tempfile.TemporaryDirectory(prefix="asm-release-verify-") as temporary_dir:
        directory = Path(temporary_dir)
        for name in ASSET_NAMES:
            with (directory / name).open("wb") as output:
                subprocess.run(
                    [
                        "gh",
                        "api",
                        f"repos/{repository}/releases/assets/{assets[name]['id']}",
                        "-H",
                        "Accept: application/octet-stream",
                    ],
                    stdout=output,
                    check=True,
                )
            verify_asset(directory / name, assets[name])
        verify_files(directory, installer)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("publish", "verify"))
    arguments = parser.parse_args()
    if not os.environ.get("GH_TOKEN"):
        raise ValueError("GH_TOKEN is required")
    repository = os.environ["GITHUB_REPOSITORY"]
    tag = os.environ["RELEASE_TAG"]
    sha = os.environ["RELEASE_SHA"]
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("invalid GITHUB_REPOSITORY")
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?", tag):
        raise ValueError("invalid RELEASE_TAG")
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("RELEASE_SHA must be a full commit SHA")
    candidate_cargo = subprocess.check_output(
        ["git", "show", f"{sha}:codex-rs/Cargo.toml"], cwd=ROOT, text=True
    )
    version = tomllib.loads(candidate_cargo)["workspace"]["package"]["version"]
    if tag != f"v{version}":
        raise ValueError(f"candidate Cargo version {version} disagrees with {tag}")
    installer = subprocess.check_output(
        ["git", "show", f"{sha}:scripts/install/install.sh"], cwd=ROOT
    )
    release = release_for_tag(repository, tag)
    if arguments.command == "verify":
        if release is None:
            raise ValueError("release has not been published")
        verify_remote(repository, tag, sha, release, installer, published=True)
        print(f"Verified published Latest release {tag} at {sha}")
        return

    directory = Path("dist").resolve()
    if {entry.name for entry in directory.iterdir()} != set(ASSET_NAMES):
        raise ValueError("dist must contain exactly the four release assets")
    verify_files(directory, installer)
    verify_tag(repository, tag, sha, create=True)
    if release is None:
        release = api(
            repository,
            "releases",
            payload={
                "tag_name": tag,
                "target_commitish": sha,
                "name": f"ASM {tag}",
                "body": (
                    "ASM is a personal fork of OpenAI Codex. Each archive contains "
                    "the `codex` CLI and its `codex-code-mode-host` helper. Use "
                    "`install.sh` to install both and for manual updates. Updates "
                    "are installed externally; in-app updates are disabled. Host "
                    "`rg` and a system shell are required for relevant features.\n\n"
                    "The macOS ARM64 binary is unsigned and unnotarized. The Linux "
                    "x86_64 binary requires Ubuntu 22.04 or newer (glibc 2.35), with "
                    "host OpenSSL 3 and XZ libraries. The archive does not include "
                    "bundled Bubblewrap; install `bwrap` on the host when Linux "
                    f"sandboxing requires it.\n\nSource commit: {sha}"
                ),
                "generate_release_notes": True,
                "draft": True,
                "prerelease": False,
            },
        )
    if release.get("tag_name") != tag or release.get("prerelease") is not False:
        raise ValueError("existing release has incompatible metadata")
    assets = asset_metadata(release, complete=False)
    for name, asset in assets.items():
        verify_asset(directory / name, asset)
    if release.get("draft") is False:
        verify_remote(repository, tag, sha, release, installer, published=True)
        print(f"Already published and verified {tag} at {sha}; no release edits")
        return
    if release.get("draft") is not True:
        raise ValueError("invalid draft flag")
    missing = [str(directory / name) for name in ASSET_NAMES if name not in assets]
    if missing:
        subprocess.run(
            ["gh", "release", "upload", tag, *missing, "-R", repository], check=True
        )
    release = api(repository, f"releases/{release['id']}")
    verify_remote(repository, tag, sha, release, installer, published=False)
    release = api(
        repository,
        f"releases/{release['id']}",
        payload={"draft": False, "prerelease": False, "make_latest": "true"},
    )
    verify_remote(repository, tag, sha, release, installer, published=True)
    print(f"Published and verified Latest release {tag} at {sha}")


if __name__ == "__main__":
    try:
        main()
    except (KeyError, OSError, ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"Fork release publication failed: {error}") from error
