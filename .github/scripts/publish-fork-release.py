"""Publish checked platforms without replacing assets; promote complete releases."""

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

LINUX_GNU = "x86_64-unknown-linux-gnu"
LINUX_MUSL = "x86_64-unknown-linux-musl"
ROOT = Path(os.environ.get("GITHUB_WORKSPACE", Path(__file__).resolve().parents[2]))


def release_targets(installer):
    declarations = [
        line
        for line in installer.splitlines()
        if line.startswith(b"linux_release_target=")
    ]
    if declarations == [f'linux_release_target="{LINUX_MUSL}"'.encode()]:
        linux_target = LINUX_MUSL
    elif declarations:
        raise ValueError("Invalid pinned Linux release target")
    else:
        linux_target = LINUX_GNU
    return ("aarch64-apple-darwin", linux_target)


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


def asset_metadata(release, installer, *, complete):
    targets = release_targets(installer)
    legacy = {
        "SHA256SUMS",
        "install.sh",
        *(f"codex-{target}.tar.gz" for target in targets),
    }
    staged = legacy | {f"SHA256SUMS-{target}" for target in targets}
    v3_archive = f"codex-{targets[1]}-v3.tar.gz"
    v3_manifest = f"SHA256SUMS-{targets[1]}-v3"
    v3_assets = {v3_archive, v3_manifest}
    assets = release.get("assets", [])
    by_name = {asset["name"]: asset for asset in assets}
    names = set(by_name)
    if len(by_name) != len(assets) or not names <= staged | v3_assets:
        raise ValueError("release has duplicate or unexpected assets")
    base = names - v3_assets
    if names & v3_assets and base != staged:
        raise ValueError("optional Linux v3 assets require a complete staged release")
    if (
        by_name.get(v3_manifest, {}).get("state") == "uploaded"
        and v3_archive not in names
    ):
        raise ValueError("optional Linux v3 readiness manifest has no archive")
    if complete and base != staged and not (targets[1] == LINUX_GNU and base == legacy):
        raise ValueError(
            "release does not contain the pinned target's complete base assets"
        )
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
    payloads = {name for name in names if not name.startswith("SHA256SUMS")}
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
        if manifest == "SHA256SUMS":
            # The base manifest is unchanged when the optional v3 pair is added.
            expected = {
                name: digest
                for name, digest in digests.items()
                if not name.endswith("-v3.tar.gz")
            }
        else:
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
    assets = asset_metadata(release, installer, complete=target is None)
    v3_target = f"{release_targets(installer)[1]}-v3"
    v3_manifest = f"SHA256SUMS-{v3_target}"
    v3_ready = assets.get(v3_manifest, {}).get("state") == "uploaded"
    if v3_ready and v3_manifest.encode() not in installer:
        raise ValueError("pinned installer does not support Linux v3 assets")
    if target is None:
        names = tuple(
            name
            for name in assets
            if name not in {f"codex-{v3_target}.tar.gz", v3_manifest} or v3_ready
        )
    else:
        names = (f"codex-{target}.tar.gz", "install.sh", f"SHA256SUMS-{target}")
    with tempfile.TemporaryDirectory(prefix="asm-release-verify-") as temporary_dir:
        directory = Path(temporary_dir)
        download_verified_assets(repository, assets, directory, names)
        verify_files(directory, installer)


def download_verified_assets(repository, assets, directory, names):
    for name in names:
        asset = assets[name]
        with (directory / name).open("wb") as output:
            subprocess.run(
                [
                    "gh",
                    "api",
                    f"repos/{repository}/releases/assets/{asset['id']}",
                    "-H",
                    "Accept: application/octet-stream",
                ],
                stdout=output,
                check=True,
            )
        verify_asset(directory / name, asset)


def upload_missing(repository, tag, release, installer, directory, names):
    assets = asset_metadata(release, installer, complete=False)
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
        linux_requirements = (
            "Linux x86_64 includes a static musl CLI. Voice requires glibc 2.35+ "
            "and host audio support; it is unavailable on plain Alpine. "
            if release_targets(installer)[1] == LINUX_MUSL
            else "Linux x86_64 requires glibc 2.35+, system OpenSSL 3 and XZ libraries; "
            "packages with bundled Bubblewrap also need libcap. "
        )
        release = api(
            repository,
            "releases",
            payload={
                "tag_name": tag,
                "target_commitish": sha,
                "name": f"ASM {tag}",
                "body": (
                    "ASM is a personal fork of OpenAI Codex. Each archive contains "
                    "the `codex` CLI and its `codex-code-mode-host` helper. Install "
                    "the entire package with its pinned install.sh. Platform checksum assets mark "
                    "packages ready for installation; Latest requires both platforms. "
                    "Use install.sh for external updates. macOS ARM64 is unsigned and "
                    f"unnotarized. {linux_requirements}"
                    "Use a system shell.\n\n"
                    f"Source commit: {sha}"
                ),
                "generate_release_notes": True,
                "draft": True,
                "prerelease": False,
                "make_latest": "false",
            },
        )
    if release.get("tag_name") != tag or release.get("prerelease") is not False:
        raise ValueError("existing release has incompatible metadata")
    with tempfile.TemporaryDirectory(prefix="asm-installer-") as temporary_dir:
        directory = Path(temporary_dir)
        (directory / "install.sh").write_bytes(installer)
        return upload_missing(
            repository, tag, release, installer, directory, ("install.sh",)
        )


def publish_target(repository, tag, sha, release, installer, target):
    if release is None:
        raise ValueError("initialize the candidate before platform publication")
    verify_tag(repository, tag, sha)
    if release.get("tag_name") != tag or release.get("prerelease") is not False:
        raise ValueError("existing release has incompatible metadata")
    manifest = f"SHA256SUMS-{target}"
    assets = asset_metadata(release, installer, complete=False)
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
        release = upload_missing(
            repository, tag, release, installer, directory, (archive,)
        )
        # Download and smoke the uploaded bytes before appending the readiness marker.
        with tempfile.TemporaryDirectory(prefix="asm-uploaded-") as temporary_dir:
            downloaded = Path(temporary_dir)
            assets = asset_metadata(release, installer, complete=False)
            download_verified_assets(repository, assets, downloaded, (archive, "install.sh"))
            for name in (archive, "install.sh"):
                verify_asset(directory / name, assets[name])
            subprocess.run([sys.executable, ROOT / ".github/scripts/smoke-codex-archive.py",
                            downloaded / archive], cwd=ROOT / "codex-rs", check=True)
            if target in (LINUX_GNU, LINUX_MUSL):
                subprocess.run(["bash", ROOT / ".github/scripts/smoke-ubuntu-archive.sh",
                                downloaded / archive, tag.removeprefix("v")], cwd=ROOT, check=True)
        release = upload_missing(
            repository, tag, release, installer, directory, (manifest,)
        )
    verify_remote(repository, tag, sha, release, installer, published=None,
                  target=target, require_latest=False)
    verified_assets = asset_metadata(release, installer, complete=False)
    if release.get("draft"):
        api(
            repository,
            f"releases/{release['id']}",
            payload={
                "tag_name": tag,
                "draft": False,
                "prerelease": False,
                "make_latest": "false",
            },
        )
    release = api(repository, f"releases/{release['id']}")
    verify_tag(repository, tag, sha)
    if release.get("tag_name") != tag:
        raise ValueError("release tag changed during platform publication")
    current_assets = asset_metadata(release, installer, complete=False)
    if release.get("draft") is not False or release.get("prerelease") is not False:
        raise ValueError("platform release was not published as a normal release")
    for name in (f"codex-{target}.tar.gz", "install.sh", manifest):
        if any(
            current_assets.get(name, {}).get(field) != verified_assets[name].get(field)
            for field in ("id", "name", "size", "digest", "state")
        ):
            raise ValueError(f"verified asset {name} changed during publication")
    print(f"Published verified {target} for {tag}; existing assets preserved")


def publish_v3(repository, tag, sha, release, installer, archive, base_run_id):
    targets = release_targets(installer)
    v3_target = f"{targets[1]}-v3"
    v3_archive = f"codex-{v3_target}.tar.gz"
    v3_manifest = f"SHA256SUMS-{v3_target}"
    v3_assets = {v3_archive, v3_manifest}
    if (
        release is None
        or release.get("draft") is not False
        or release.get("prerelease") is not False
    ):
        raise ValueError("Linux v3 supplements a completed normal release")
    assets = asset_metadata(release, installer, complete=True)
    if not {f"SHA256SUMS-{target}" for target in targets} <= assets.keys():
        raise ValueError("Linux v3 requires the completed staged base release")
    if v3_manifest.encode() not in installer:
        raise ValueError("pinned installer does not support Linux v3 assets")
    if archive.name != v3_archive:
        raise ValueError(f"expected archive named {v3_archive}")
    if (
        subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip()
        != sha
    ):
        raise ValueError(
            "check out the exact release commit before publishing Linux v3"
        )
    if subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT
    ):
        raise ValueError(
            "tracked checkout changes must not affect release verification"
        )
    subprocess.run(
        [
            sys.executable,
            ROOT / ".github/scripts/prepare-fork-release.py",
            "verify-base",
            base_run_id,
            tag,
            sha,
        ],
        cwd=ROOT,
        check=True,
    )

    if v3_manifest not in assets and release.get("immutable"):
        raise ValueError("cannot append Linux v3 assets to an immutable GitHub release")
    for name in v3_assets & assets.keys():
        if assets[name].get("state") != "uploaded":
            raise ValueError(f"asset {name} has not finished uploading; never replacing it")
    receipt = archive.with_name(f"{archive.name}.provenance").read_bytes()

    with tempfile.TemporaryDirectory(prefix="asm-publish-v3-") as temporary_dir:
        directory = Path(temporary_dir)
        staged = directory / v3_archive
        shutil.copyfile(archive, staged)
        digest = file_digest(staged)
        if receipt != f"{sha}\n{digest}\n".encode():
            raise ValueError(
                "Linux v3 provenance does not match the release commit and archive"
            )
        if v3_manifest in assets:
            verify_asset(staged, assets[v3_archive])
            verify_remote(
                repository,
                tag,
                sha,
                release,
                installer,
                published=True,
                target=v3_target,
                require_latest=False,
            )
            print(
                f"Already published and verified Linux v3 for {tag}; no release edits"
            )
            return
        verify_remote(
            repository,
            tag,
            sha,
            release,
            installer,
            published=True,
            require_latest=False,
        )
        subprocess.run(
            [sys.executable, ROOT / ".github/scripts/smoke-codex-archive.py", staged],
            cwd=ROOT / "codex-rs",
            check=True,
        )
        subprocess.run(
            [
                "bash",
                ROOT / ".github/scripts/smoke-ubuntu-archive.sh",
                staged,
                tag.removeprefix("v"),
            ],
            cwd=ROOT,
            check=True,
        )
        (directory / "install.sh").write_bytes(installer)
        (directory / v3_manifest).write_text(
            f"{digest}  {v3_archive}\n"
            f"{file_digest(directory / 'install.sh')}  install.sh\n"
        )
        release = upload_missing(
            repository, tag, release, installer, directory, (v3_archive,)
        )
        assets = asset_metadata(release, installer, complete=True)
        downloaded = directory / "uploaded"
        downloaded.mkdir()
        download_verified_assets(
            repository, assets, downloaded, (v3_archive, "install.sh")
        )
        for name in (v3_archive, "install.sh"):
            verify_asset(directory / name, assets[name])
        release = upload_missing(
            repository, tag, release, installer, directory, (v3_manifest,)
        )
        current = asset_metadata(release, installer, complete=True)
        verify_asset(directory / v3_manifest, current[v3_manifest])
        for name in (v3_archive, "install.sh"):
            verify_asset(downloaded / name, current[name])
    verify_tag(repository, tag, sha)
    print(f"Published verified Linux v3 for {tag}; base release assets preserved")


def finalize(repository, tag, sha, release, installer):
    if release is None or release.get("draft") is not False:
        raise ValueError("platform publication must complete before finalization")
    targets = release_targets(installer)
    archives = tuple(f"codex-{target}.tar.gz" for target in targets)
    assets = asset_metadata(release, installer, complete=False)
    if targets[1] == LINUX_GNU and set(assets) == {
        *archives,
        "install.sh",
        "SHA256SUMS",
    }:
        verify_remote(repository, tag, sha, release, installer, published=True)
        return  # Completed legacy releases remain unchanged.
    if not {f"SHA256SUMS-{target}" for target in targets} <= assets.keys():
        raise ValueError("both platform readiness manifests are required")
    digests = {}
    for target in targets:
        verify_remote(repository, tag, sha, release, installer, published=True,
                      target=target, require_latest=False)
    for name in (*archives, "install.sh"):
        digest = assets[name].get("digest", "")
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
            raise ValueError(f"missing GitHub SHA-256 digest for {name}")
        digests[name] = digest.removeprefix("sha256:")
    with tempfile.TemporaryDirectory(prefix="asm-finalize-") as temporary_dir:
        directory = Path(temporary_dir)
        (directory / "SHA256SUMS").write_text(
            "".join(f"{digest}  {name}\n" for name, digest in digests.items())
        )
        release = upload_missing(
            repository, tag, release, installer, directory, ("SHA256SUMS",)
        )
    verify_remote(repository, tag, sha, release, installer, published=True, require_latest=False)
    release = api(
        repository,
        f"releases/{release['id']}",
        payload={"tag_name": tag, "make_latest": "true"},
    )
    verify_remote(repository, tag, sha, release, installer, published=True)
    print(f"Promoted complete verified release {tag} to Latest at {sha}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command", choices=(
            "initialize", "restore-target", "publish-target", "publish-v3",
            "finalize", "publish", "verify",
        )
    )
    parser.add_argument(
        "--archive", type=Path, help="Archive for publish-v3 (with neighboring .provenance)"
    )
    parser.add_argument(
        "--base-run-id", help="Successful release workflow run for the exact tag and commit"
    )
    arguments = parser.parse_args()
    if arguments.command == "publish-v3":
        if arguments.archive is None or not re.fullmatch(
            r"[1-9][0-9]*", arguments.base_run_id or ""
        ):
            parser.error("publish-v3 requires --archive and a positive --base-run-id")
    elif arguments.archive is not None or arguments.base_run_id is not None:
        parser.error("--archive and --base-run-id are only for publish-v3")
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
    if arguments.command == "publish-v3":
        publish_v3(
            repository,
            tag,
            sha,
            release,
            installer,
            arguments.archive,
            arguments.base_run_id,
        )
        return
    if arguments.command == "initialize":
        initialize(repository, tag, sha, release, installer)
        return
    if arguments.command in {"restore-target", "publish-target"}:
        target = os.environ["TARGET"]
        if target not in release_targets(installer):
            raise ValueError("unsupported publication target")
        if arguments.command == "restore-target":
            uploaded = recovered = False
            if release is not None:
                verify_tag(repository, tag, sha)
                if release.get("tag_name") != tag or release.get("prerelease") is not False:
                    raise ValueError("existing release has incompatible metadata")
                assets = asset_metadata(release, installer, complete=False)
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

    targets = release_targets(installer)
    if targets[1] != LINUX_GNU:
        raise ValueError("unstaged publication is only for pinned legacy GNU releases")
    asset_names = (
        *(f"codex-{target}.tar.gz" for target in targets),
        "SHA256SUMS",
        "install.sh",
    )
    directory = Path("dist").resolve()
    if {entry.name for entry in directory.iterdir()} != set(asset_names):
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
    assets = asset_metadata(release, installer, complete=False)
    for name, asset in assets.items():
        verify_asset(directory / name, asset)
    if release.get("draft") is False:
        verify_remote(repository, tag, sha, release, installer, published=True)
        print(f"Already published and verified {tag} at {sha}; no release edits")
        return
    if release.get("draft") is not True:
        raise ValueError("invalid draft flag")
    missing = [str(directory / name) for name in asset_names if name not in assets]
    if missing:
        subprocess.run(
            ["gh", "release", "upload", tag, *missing, "-R", repository], check=True
        )
    release = api(repository, f"releases/{release['id']}")
    verify_remote(repository, tag, sha, release, installer, published=False)
    release = api(
        repository,
        f"releases/{release['id']}",
        payload={
            "tag_name": tag,
            "draft": False,
            "prerelease": False,
            "make_latest": "true",
        },
    )
    verify_remote(repository, tag, sha, release, installer, published=True)
    print(f"Published and verified Latest release {tag} at {sha}")


if __name__ == "__main__":
    try:
        main()
    except (KeyError, OSError, ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"Fork release publication failed: {error}") from error
