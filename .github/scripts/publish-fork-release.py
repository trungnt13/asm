"""Publish checked platforms without replacing assets; promote complete releases."""

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
TARGETS = ("aarch64-apple-darwin", "x86_64-unknown-linux-gnu")
PLATFORM_MANIFESTS = tuple(f"SHA256SUMS-{target}" for target in TARGETS)
ROOT = Path(os.environ.get("GITHUB_WORKSPACE", Path(__file__).resolve().parents[2]))


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
    if (
        len(by_name) != len(assets)
        or not set(by_name) <= set(ASSET_NAMES + PLATFORM_MANIFESTS)
    ):
        raise ValueError("release has duplicate or unexpected assets")
    if complete and set(by_name) not in (
        set(ASSET_NAMES), set(ASSET_NAMES + PLATFORM_MANIFESTS)
    ):
        raise ValueError("release must contain exactly four legacy or six staged assets")
    return by_name


def file_digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify_asset(path, asset):
    if asset.get("state") != "uploaded":
        raise ValueError(
            f"asset {path.name} has not finished uploading; never replacing it"
        )
    digest = asset.get("digest", "")
    if not isinstance(digest, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
        raise ValueError(f"missing GitHub SHA-256 digest for {path.name}")
    actual = file_digest(path)
    if path.stat().st_size != asset["size"] or digest != f"sha256:{actual}":
        raise ValueError(f"GitHub size or SHA-256 mismatch for {path.name}")


def verify_files(directory, installer):
    names = {entry.name for entry in directory.iterdir()}
    payloads = names - {"SHA256SUMS", *PLATFORM_MANIFESTS}
    digests = {
        name: file_digest(directory / name)
        for name in payloads
    }
    for manifest in names - payloads:
        checksums = {}
        for line in (directory / manifest).read_text(encoding="utf-8").splitlines():
            match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.-]+)", line)
            if not match or match.group(2) in checksums:
                raise ValueError(f"invalid or duplicate {manifest} entry")
            checksums[match.group(2)] = match.group(1)
        expected = digests
        if manifest != "SHA256SUMS":
            archive = f"codex-{manifest.removeprefix('SHA256SUMS-')}.tar.gz"
            expected = {name: digests[name] for name in (archive, "install.sh")}
        if checksums != expected:
            raise ValueError(f"{manifest} does not match its exact payloads")
    if (directory / "install.sh").read_bytes() != installer:
        raise ValueError("installer differs from the candidate commit")
    for name in payloads - {"install.sh"}:
        subprocess.run(
            [sys.executable, ROOT / ".github/scripts/smoke-codex-archive.py",
             directory / name, "--inspect-only"], check=True,
        )


def verify_remote(repository, tag, sha, release, installer, *, published,
                  target=None, require_latest=True):
    verify_tag(repository, tag, sha)
    if release.get("tag_name") != tag or release.get("prerelease") is not False:
        raise ValueError("release tag or normal-release flag is incorrect")
    if published:
        if release.get("draft") is not False:
            raise ValueError("release is still a draft")
        if require_latest and api(repository, "releases/latest")["id"] != release["id"]:
            raise ValueError("release is not Latest")
    elif published is False and release.get("draft") is not True:
        raise ValueError("expected a draft before publication")
    assets = asset_metadata(release, complete=target is None)
    names = tuple(assets) if target is None else (
        f"codex-{target}.tar.gz", "install.sh", f"SHA256SUMS-{target}",
    )
    with tempfile.TemporaryDirectory(prefix="asm-release-verify-") as temporary_dir:
        directory = Path(temporary_dir)
        for name in names:
            with (directory / name).open("wb") as output:
                subprocess.run(
                    ["gh", "api", f"repos/{repository}/releases/assets/{assets[name]['id']}",
                     "-H", "Accept: application/octet-stream"], stdout=output, check=True,
                )
            verify_asset(directory / name, assets[name])
        verify_files(directory, installer)


def upload_missing(repository, tag, release, directory, names):
    assets = asset_metadata(release, complete=False)
    for name in names:
        if name in assets:
            verify_asset(directory / name, assets[name])
        else:
            subprocess.run(["gh", "release", "upload", tag, str(directory / name),
                            "-R", repository], check=True)
    return api(repository, f"releases/{release['id']}")


def initialize(repository, tag, sha, release, installer):
    verify_tag(repository, tag, sha, create=True)
    if release is None:
        release = api(repository, "releases", payload={
            "tag_name": tag, "target_commitish": sha, "name": f"ASM {tag}",
            "body": (
                "ASM is a personal fork of OpenAI Codex. Each archive contains "
                "the `codex` CLI and its `codex-code-mode-host` helper. Install "
                "the entire package with its pinned install.sh. Platform checksum assets mark "
                "packages ready for installation; Latest requires both platforms. "
                "Use install.sh for external updates. macOS ARM64 is unsigned and "
                "unnotarized. Linux x86_64 requires glibc 2.35+, system OpenSSL 3 "
                "and XZ libraries; packages with bundled Bubblewrap also need libcap. "
                "Use a system shell.\n\n"
                f"Source commit: {sha}"
            ),
            "generate_release_notes": True, "draft": True, "prerelease": False,
            "make_latest": "false",
        })
    if release.get("tag_name") != tag or release.get("prerelease") is not False:
        raise ValueError("existing release has incompatible metadata")
    with tempfile.TemporaryDirectory(prefix="asm-installer-") as temporary_dir:
        directory = Path(temporary_dir)
        (directory / "install.sh").write_bytes(installer)
        return upload_missing(repository, tag, release, directory, ("install.sh",))


def publish_target(repository, tag, sha, release, installer, target):
    if release is None:
        raise ValueError("initialize the candidate before platform publication")
    verify_tag(repository, tag, sha)
    if release.get("tag_name") != tag or release.get("prerelease") is not False:
        raise ValueError("existing release has incompatible metadata")
    manifest = f"SHA256SUMS-{target}"
    assets = asset_metadata(release, complete=False)
    if manifest not in assets:
        directory = ROOT / "dist"
        archive = f"codex-{target}.tar.gz"
        (directory / "install.sh").write_bytes(installer)
        digests = {name: file_digest(directory / name)
                   for name in (archive, "install.sh")}
        (directory / manifest).write_text(
            "".join(f"{digest}  {name}\n" for name, digest in digests.items())
        )
        verify_files(directory, installer)
        release = upload_missing(repository, tag, release, directory, (archive,))
        # Download and smoke the uploaded bytes before appending the readiness marker.
        with tempfile.TemporaryDirectory(prefix="asm-uploaded-") as temporary_dir:
            downloaded = Path(temporary_dir)
            for name in (archive, "install.sh"):
                asset = asset_metadata(release, complete=False)[name]
                with (downloaded / name).open("wb") as output:
                    subprocess.run(["gh", "api", f"repos/{repository}/releases/assets/{asset['id']}",
                                    "-H", "Accept: application/octet-stream"], stdout=output, check=True)
                verify_asset(downloaded / name, asset)
                verify_asset(directory / name, asset)
            subprocess.run([sys.executable, ROOT / ".github/scripts/smoke-codex-archive.py",
                            downloaded / archive], cwd=ROOT / "codex-rs", check=True)
            if target == "x86_64-unknown-linux-gnu":
                subprocess.run(["bash", ROOT / ".github/scripts/smoke-ubuntu-archive.sh",
                                downloaded / archive, tag.removeprefix("v")], cwd=ROOT, check=True)
        release = upload_missing(repository, tag, release, directory, (manifest,))
    verify_remote(repository, tag, sha, release, installer, published=None,
                  target=target, require_latest=False)
    verified_assets = asset_metadata(release, complete=False)
    if release.get("draft"):
        api(repository, f"releases/{release['id']}",
            payload={"draft": False, "prerelease": False, "make_latest": "false"})
    release = api(repository, f"releases/{release['id']}")
    verify_tag(repository, tag, sha)
    current_assets = asset_metadata(release, complete=False)
    if release.get("draft") is not False or release.get("prerelease") is not False:
        raise ValueError("platform release was not published as a normal release")
    for name in (f"codex-{target}.tar.gz", "install.sh", manifest):
        if any(
            current_assets.get(name, {}).get(field) != verified_assets[name].get(field)
            for field in ("id", "name", "size", "digest", "state")
        ):
            raise ValueError(f"verified asset {name} changed during publication")
    print(f"Published verified {target} for {tag}; existing assets preserved")


def finalize(repository, tag, sha, release, installer):
    if release is None or release.get("draft") is not False:
        raise ValueError("platform publication must complete before finalization")
    assets = asset_metadata(release, complete=False)
    if set(assets) == set(ASSET_NAMES):
        verify_remote(repository, tag, sha, release, installer, published=True)
        return  # Completed legacy releases remain unchanged.
    if not set(PLATFORM_MANIFESTS) <= set(assets):
        raise ValueError("both platform readiness manifests are required")
    digests = {}
    for target in TARGETS:
        verify_remote(repository, tag, sha, release, installer, published=True,
                      target=target, require_latest=False)
    for name in (*ASSET_NAMES[:2], "install.sh"):
        digest = assets[name].get("digest", "")
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
            raise ValueError(f"missing GitHub SHA-256 digest for {name}")
        digests[name] = digest.removeprefix("sha256:")
    with tempfile.TemporaryDirectory(prefix="asm-finalize-") as temporary_dir:
        directory = Path(temporary_dir)
        (directory / "SHA256SUMS").write_text(
            "".join(f"{digest}  {name}\n" for name, digest in digests.items())
        )
        release = upload_missing(repository, tag, release, directory, ("SHA256SUMS",))
    verify_remote(repository, tag, sha, release, installer, published=True, require_latest=False)
    release = api(repository, f"releases/{release['id']}", payload={"make_latest": "true"})
    verify_remote(repository, tag, sha, release, installer, published=True)
    print(f"Promoted complete verified release {tag} to Latest at {sha}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command", choices=(
            "initialize", "restore-target", "publish-target", "finalize", "publish", "verify",
        )
    )
    arguments = parser.parse_args()
    if not os.environ.get("GH_TOKEN"):
        raise ValueError("GH_TOKEN is required")
    repository = os.environ["GITHUB_REPOSITORY"]
    tag = os.environ["RELEASE_TAG"]
    sha = os.environ["RELEASE_SHA"]
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("invalid GITHUB_REPOSITORY")
    if not re.fullmatch(
        r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?"
        r"(?:\+[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*)?",
        tag,
    ):
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
    if arguments.command == "initialize":
        initialize(repository, tag, sha, release, installer)
        return
    if arguments.command in {"restore-target", "publish-target"}:
        target = os.environ["TARGET"]
        if target not in TARGETS:
            raise ValueError("unsupported publication target")
        if arguments.command == "restore-target":
            uploaded = recovered = False
            if release is not None:
                verify_tag(repository, tag, sha)
                if release.get("tag_name") != tag or release.get("prerelease") is not False:
                    raise ValueError("existing release has incompatible metadata")
                assets = asset_metadata(release, complete=False)
                uploaded = f"SHA256SUMS-{target}" in assets
                archive = f"codex-{target}.tar.gz"
                if not uploaded and archive in assets:
                    directory = ROOT / "dist"
                    directory.mkdir(exist_ok=True)
                    with (directory / archive).open("wb") as output:
                        subprocess.run([
                            "gh", "api", f"repos/{repository}/releases/assets/{assets[archive]['id']}",
                            "-H", "Accept: application/octet-stream",
                        ], stdout=output, check=True)
                    verify_asset(directory / archive, assets[archive])
                    recovered = True
            with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
                print(f"uploaded={str(uploaded).lower()}", file=stream)
                print(f"recovered={str(recovered).lower()}", file=stream)
        else:
            publish_target(repository, tag, sha, release, installer, target)
        return
    if arguments.command == "finalize":
        finalize(repository, tag, sha, release, installer)
        return
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
                    "`install.sh` to install the package and for manual updates. Updates "
                    "are installed externally; in-app updates are disabled. A system "
                    "shell is required for relevant features.\n\n"
                    "The macOS ARM64 binary is unsigned and unnotarized. The Linux "
                    "x86_64 binary requires Ubuntu 22.04 or newer (glibc 2.35), with "
                    "host OpenSSL 3 and XZ libraries; packages with bundled Bubblewrap "
                    "also need libcap.\n\n"
                    f"Source commit: {sha}"
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
