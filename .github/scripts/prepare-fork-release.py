"""Freeze a restartable release candidate and sync its version after publication."""

import json
import os
import re
import subprocess
import sys
import time
import tomllib
from pathlib import Path

MANIFEST = "codex-rs/Cargo.toml"
LOCKFILE = "codex-rs/Cargo.lock"
REPOSITORY = os.environ["GITHUB_REPOSITORY"]
AUTHOR = "Trung Ngo"
EMAIL = "1390402+trungnt13@users.noreply.github.com"


def git(*arguments):
    return subprocess.check_output(["git", *arguments], text=True).strip()


def git_file(revision, path):
    return subprocess.check_output(["git", "show", f"{revision}:{path}"], text=True)


def api(path, *arguments, allow_missing=False):
    for attempt in range(3):
        result = subprocess.run(
            ["gh", "api", path, *arguments], text=True, capture_output=True, check=False
        )
        if result.returncode == 0:
            return json.loads(result.stdout)
        status = re.search(r"\bHTTP (\d{3})\b", result.stderr)
        if status and status[1] == "404" and allow_missing:
            return None
        retryable = (
            500 <= int(status[1]) < 600
            if status
            else bool(
                re.search(
                    r"stream error|http2|unexpected EOF|connection reset|connection refused|"
                    r"timeout|timed out|TLS handshake|network is unreachable|dial tcp|"
                    r"temporary failure|unexpected end of JSON input",
                    result.stderr,
                    re.IGNORECASE,
                )
            )
        )
        if not retryable or attempt == 2:
            raise RuntimeError(f"GitHub API {path} failed: {result.stderr.strip()}")
        time.sleep(2 ** (attempt + 1))


def upstream_prereleases():
    for line in git(
        "ls-remote",
        "--tags",
        "--refs",
        "https://github.com/openai/codex.git",
        "rust-v*",
    ).splitlines():
        _, reference = line.split()
        match = re.fullmatch(
            r"refs/tags/rust-v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-"
            r"([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)"
            r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?",
            reference,
        )
        if not match:
            continue
        identifiers = match[4].split(".")
        if any(
            value.isdigit() and len(value) > 1 and value[0] == "0"
            for value in identifiers
        ):
            continue
        precedence = (
            tuple(int(value) for value in match.groups()[:3]),
            tuple(
                (0, int(value)) if value.isdigit() else (1, value)
                for value in identifiers
            ),
        )
        yield precedence, reference.removeprefix("refs/tags/")


def version(revision):
    return tomllib.loads(git_file(revision, MANIFEST))["workspace"]["package"][
        "version"
    ]


def remote_ref(reference):
    result = subprocess.run(
        ["git", "ls-remote", "--exit-code", "origin", reference],
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode == 2:
        return None
    if result.returncode:
        raise RuntimeError(result.stderr)
    return result.stdout.split()[0]


def rewritten_files(base, new_version):
    old_version = version(base)
    manifest = git_file(base, MANIFEST)
    manifest, count = re.subn(
        r'(\[workspace\.package\]\s*\nversion = ")[^"]+("\n)',
        rf"\g<1>{new_version}\2",
        manifest,
        count=1,
    )
    if count != 1:
        raise ValueError("Cannot locate workspace package version")
    inherited_names = set()
    for path in git("ls-tree", "-r", "--name-only", base, "codex-rs").splitlines():
        if path.endswith("/Cargo.toml"):
            package = tomllib.loads(git_file(base, path)).get("package", {})
            if package.get("version") == {"workspace": True}:
                inherited_names.add(package["name"])
    lockfile = git_file(base, LOCKFILE)
    blocks = lockfile.split("[[package]]")
    for index, block in enumerate(blocks[1:], start=1):
        package = tomllib.loads("[[package]]" + block)["package"][0]
        if package["name"] in inherited_names and "source" not in package:
            if package["version"] != old_version:
                raise ValueError(
                    f"Workspace lock version differs for {package['name']}"
                )
            blocks[index] = block.replace(
                f'version = "{old_version}"',
                f'version = "{new_version}"',
                1,
            )
    return {MANIFEST: manifest, LOCKFILE: "[[package]]".join(blocks)}


def validate_candidate(revision):
    parents = git("rev-list", "--parents", "-n", "1", revision).split()
    if len(parents) != 2:
        raise ValueError("Candidate must have exactly one parent")
    base = parents[1]
    changed = set(git("diff", "--name-only", base, revision).splitlines())
    if not changed <= {MANIFEST, LOCKFILE} or git("diff", "--summary", base, revision):
        raise ValueError("Candidate contains changes other than its version")
    for path, expected in rewritten_files(base, version(revision)).items():
        if git_file(revision, path) != expected:
            raise ValueError(f"Candidate has unexpected changes in {path}")
    return base


def complete_candidate(tag, revision):
    tag_sha = remote_ref(f"refs/tags/{tag}")
    release = api(f"repos/{REPOSITORY}/releases/tags/{tag}", allow_missing=True)
    if tag_sha:
        git("fetch", "--no-tags", "origin", f"refs/tags/{tag}")
        if git("rev-parse", "FETCH_HEAD^{commit}") != revision:
            raise ValueError("Existing release tag points at another commit")
    if release and not release["draft"]:
        if not tag_sha:
            raise ValueError("Published release has no matching remote tag")
        names = {asset["name"] for asset in release.get("assets", [])}
        legacy = {
            "codex-aarch64-apple-darwin.tar.gz",
            "codex-x86_64-unknown-linux-gnu.tar.gz",
            "SHA256SUMS", "install.sh",
        }
        staged = legacy | {
            "SHA256SUMS-aarch64-apple-darwin", "SHA256SUMS-x86_64-unknown-linux-gnu",
        }
        return names in (legacy, staged)
    return False


def output(revision, tag="", candidate_ref="", complete=False, publish=False):
    print(
        f"Release source: {revision}; tag: {tag or '(build only)'}; candidate: {candidate_ref or '(none)'}"
    )
    with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
        for key, value in {
            "sha": revision,
            "tag": tag,
            "candidate_ref": candidate_ref,
            "complete": str(complete).lower(),
            "publish": str(publish).lower(),
            "staged": str(
                publish and "SHA256SUMS-$vendor_target" in git_file(
                    revision, "scripts/install/install.sh"
                )
            ).lower(),
        }.items():
            print(f"{key}={value}", file=stream)


def prepare():
    revision = os.environ["GITHUB_SHA"]
    reference = os.environ["GITHUB_REF"]
    publish = os.environ.get("PUBLISH_RELEASE", "false")
    if publish not in {"true", "false"}:
        raise ValueError("PUBLISH_RELEASE must be true or false")
    if publish == "false":
        if os.environ.get("RESUME_RUN_ID", ""):
            raise ValueError("Resuming a release requires PUBLISH_RELEASE=true")
        output(revision)
        return
    if reference.startswith("refs/tags/v"):
        tag = reference.removeprefix("refs/tags/")
        revision = git("rev-parse", f"{revision}^{{commit}}")
        if tag != f"v{version(revision)}":
            raise ValueError("Tag does not agree with Cargo version")
        output(
            revision, tag, complete=complete_candidate(tag, revision), publish=True
        )
        return
    if reference != "refs/heads/main":
        raise ValueError("New releases must be explicitly dispatched on main")
    resume_run_id = os.environ.get("RESUME_RUN_ID", "")
    run_id = resume_run_id or os.environ["GITHUB_RUN_ID"]
    if not re.fullmatch(r"[1-9][0-9]*", run_id):
        raise ValueError("Release run ID must be a positive decimal integer")
    candidate_ref = f"refs/heads/agent/release-{run_id}"
    candidate_sha = remote_ref(candidate_ref)
    if candidate_sha:
        git("fetch", "--no-tags", "origin", candidate_ref)
        revision = git("rev-parse", "FETCH_HEAD^{commit}")
        if revision != candidate_sha:
            raise ValueError("Candidate changed during lookup")
        base = validate_candidate(revision)
        if not resume_run_id and base != os.environ["GITHUB_SHA"]:
            raise ValueError("Candidate belongs to a different source commit")
    else:
        if resume_run_id:
            raise ValueError("Requested candidate run does not exist")
        tags = list(upstream_prereleases())
        if not tags:
            raise ValueError("No upstream Rust prerelease tag found")
        upstream = max(tags, key=lambda tag: tag[0])[1]
        match = re.fullmatch(r"rust-v(\d+)\.(\d+)\.(\d+)(-.+)", upstream)
        if not match:
            raise ValueError(f"Unsupported upstream prerelease tag: {upstream}")
        major, minor, patch, suffix = match.groups()
        new_version = f"{major}.{minor}.{int(patch) + 1}{suffix}"
        tag = f"v{new_version}"
        if remote_ref(f"refs/tags/{tag}") or api(
            f"repos/{REPOSITORY}/releases/tags/{tag}",
            allow_missing=True,
        ):
            raise ValueError(
                f"Derived release already exists: {tag}; resume its original run"
            )
        git("checkout", "--detach", revision)
        for path, contents in rewritten_files(revision, new_version).items():
            Path(path).write_text(contents)
        git("add", "--", MANIFEST, LOCKFILE)
        git(
            "-c",
            f"user.name={AUTHOR}",
            "-c",
            f"user.email={EMAIL}",
            "commit",
            "--allow-empty",
            "-m",
            f"Prepare ASM {new_version} release",
        )
        revision = git("rev-parse", "HEAD")
        validate_candidate(revision)
        git("push", "origin", f"HEAD:{candidate_ref}")
    tag = f"v{version(revision)}"
    output(
        revision, tag, candidate_ref, complete_candidate(tag, revision), publish=True
    )


def sync_main():
    revision = os.environ["RELEASE_SHA"]
    tag = os.environ["RELEASE_TAG"]
    candidate_ref = os.environ["CANDIDATE_REF"]
    if not candidate_ref:
        return  # Legacy tag publications do not own a version update to main.
    if not re.fullmatch(r"refs/heads/agent/release-[1-9][0-9]*", candidate_ref):
        raise ValueError("Invalid candidate branch")
    git("fetch", "--no-tags", "origin", candidate_ref)
    if git("rev-parse", "FETCH_HEAD^{commit}") != revision:
        raise ValueError("Candidate branch no longer matches published commit")
    base = validate_candidate(revision)
    if tag != f"v{version(revision)}" or not complete_candidate(tag, revision):
        raise ValueError("Candidate release is not complete")
    for _ in range(3):
        git("fetch", "--no-tags", "origin", "refs/heads/main")
        main_sha = git("rev-parse", "FETCH_HEAD^{commit}")
        ancestor = subprocess.run(
            ["git", "merge-base", "--is-ancestor", revision, main_sha], check=False
        ).returncode
        if ancestor == 0:
            return
        if ancestor != 1:
            raise RuntimeError("Cannot check whether main already includes the release")
        if version(main_sha) != version(base):
            raise ValueError(
                "Main version changed; refusing to overwrite another release"
            )
        git("checkout", "--detach", main_sha)
        git(
            "-c",
            f"user.name={AUTHOR}",
            "-c",
            f"user.email={EMAIL}",
            "merge",
            "--no-edit",
            revision,
        )
        result = subprocess.run(
            ["git", "push", "origin", "HEAD:refs/heads/main"], check=False
        )
        if result.returncode == 0:
            return
        if remote_ref("refs/heads/main") == main_sha:
            raise RuntimeError("Main push failed without a concurrent main update")
    raise RuntimeError("Main kept advancing; rerun sync-main without republishing")


if __name__ == "__main__":
    for role in ("AUTHOR", "COMMITTER"):
        os.environ[f"GIT_{role}_NAME"] = AUTHOR
        os.environ[f"GIT_{role}_EMAIL"] = EMAIL
    if sys.argv[1:] == ["prepare"]:
        prepare()
    elif sys.argv[1:] == ["sync-main"]:
        sync_main()
    else:
        raise SystemExit("Usage: prepare-fork-release.py prepare|sync-main")
