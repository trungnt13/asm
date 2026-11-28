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
RELEASE_WORKFLOW = ".github/workflows/fork-rust-release.yml"
CANDIDATE_SUBJECT = "Prepare ASM {version} release"
LINUX_MUSL = "x86_64-unknown-linux-musl"
LAST_PRE_MUSL_RELEASE = "v0.174.1-alpha.8"
NATIVE_CHECKS = {
    "Verify published aarch64-apple-darwin binaries",
    f"Verify published {LINUX_MUSL} binaries",
}
PUBLICATION_JOB = "Complete and verify immutable release"
SYNC_JOB = "Advance main only after release verification"


def git(*arguments):
    return subprocess.check_output(["git", *arguments], text=True).strip()


def git_file(revision, path):
    return subprocess.check_output(["git", "show", f"{revision}:{path}"], text=True)


def require_musl_source(revision):
    if not git(
        "ls-tree", "--name-only", revision, ".github/scripts/package-asm-linux-voice.sh"
    ):
        raise ValueError("Release source predates the musl package contract")


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


def semver_precedence(value):
    match = re.fullmatch(
        r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
        r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
        r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?",
        value,
    )
    if not match:
        return None
    identifiers = match[4].split(".") if match[4] else []
    if any(
        value.isdigit() and len(value) > 1 and value[0] == "0" for value in identifiers
    ):
        return None
    return (
        tuple(int(value) for value in match.groups()[:3]),
        match[4] is None,
        tuple(
            (0, int(value)) if value.isdigit() else (1, value) for value in identifiers
        ),
    )


def upstream_prereleases():
    for line in git(
        "ls-remote",
        "--tags",
        "--refs",
        "https://github.com/openai/codex.git",
        "rust-v*",
    ).splitlines():
        _, reference = line.split()
        tag = reference.removeprefix("refs/tags/")
        value = tag.removeprefix("rust-v")
        precedence = semver_precedence(value)
        if precedence is not None and not precedence[1]:
            yield precedence, tag


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
        return published_assets_complete(release)
    return False


def published_assets_complete(release):
    assets = release.get("assets", [])
    names = {asset["name"] for asset in assets}
    required = {
        "codex-aarch64-apple-darwin.tar.gz",
        f"codex-{LINUX_MUSL}.tar.gz",
        "SHA256SUMS",
        "install.sh",
        "SHA256SUMS-aarch64-apple-darwin",
        f"SHA256SUMS-{LINUX_MUSL}",
    }
    v3_archive = f"codex-{LINUX_MUSL}-v3.tar.gz"
    v3_manifest = f"SHA256SUMS-{LINUX_MUSL}-v3"
    if (
        release["draft"]
        or len(names) != len(assets)
        or names - {v3_archive, v3_manifest} != required
    ):
        return False
    if any(
        asset["name"] == v3_manifest and asset.get("state") == "uploaded"
        for asset in assets
    ):
        required |= {v3_archive, v3_manifest}
    return required <= names and all(
        asset.get("state") == "uploaded"
        and asset.get("size", 0) > 0
        and re.fullmatch(r"sha256:[0-9a-f]{64}", asset.get("digest") or "")
        for asset in assets
        if asset["name"] in required
    )


def publication_run_succeeded(run):
    return (
        run["path"] == RELEASE_WORKFLOW
        and run["status"] == "completed"
        and run["conclusion"] == "success"
        and (run.get("head_repository") or {}).get("full_name") == REPOSITORY
    )


def completed_publication(run, candidate, tag, candidate_ref):
    if not publication_run_succeeded(run):
        return False
    if not candidate_ref and not (
        run["event"] in {"push", "workflow_dispatch"}
        and run["head_branch"] == tag
        and run["head_sha"] == candidate
    ):
        return False
    jobs = latest_unskipped_jobs(run["id"])
    succeeded = {name for name, job in jobs.items() if job["conclusion"] == "success"}
    if not (NATIVE_CHECKS | {PUBLICATION_JOB}) <= succeeded:
        return False
    if not candidate_ref:
        return True
    if SYNC_JOB not in succeeded:
        return False
    if run["id"] == int(candidate_ref.rsplit("-", 1)[1]):
        return run["event"] == "workflow_dispatch" and run["head_branch"] == "main"
    return sync_job_matches(jobs[SYNC_JOB], candidate, tag, re.escape(candidate_ref))


def sync_job_matches(sync_job, candidate, tag, ref_pattern):
    # Logs may contain ANSI bytes. Keep them in memory, never render them to a terminal.
    api_help = subprocess.check_output(["gh", "api", "--help"], text=True)
    raw_logs = (
        ["--allow-escape-sequences"] if "--allow-escape-sequences" in api_help else []
    )
    logs = subprocess.check_output(
        [
            "gh",
            "api",
            *raw_logs,
            f"repos/{REPOSITORY}/actions/jobs/{sync_job['id']}/logs",
        ],
        text=True,
    )
    # Job environment ties both original and successful resume runs to the frozen candidate.
    return all(
        re.search(rf"\b{key}:\s+{pattern}(?:\s|$)", logs)
        for key, pattern in {
            "RELEASE_SHA": re.escape(candidate),
            "RELEASE_TAG": re.escape(tag),
            "CANDIDATE_REF": ref_pattern,
        }.items()
    )


def verify_base_release(run_id, tag, revision):
    if not re.fullmatch(r"[1-9][0-9]*", run_id):
        raise ValueError("Release run ID must be a positive decimal integer")
    require_musl_source(revision)
    path = f"repos/{REPOSITORY}/actions/runs/{run_id}"
    run = api(path)
    if not publication_run_succeeded(run) or str(run["id"]) != run_id:
        raise ValueError(f"Run {run_id} is not a successful fork release workflow")
    candidate = run["event"] == "workflow_dispatch" and run["head_branch"] == "main"
    if not candidate:
        if (
            run["event"] not in {"push", "workflow_dispatch"}
            or run["head_branch"] != tag
            or run["head_sha"] != revision
        ):
            raise ValueError(f"Run {run_id} does not identify {tag} at {revision}")
        candidate_refs = api(
            f"repos/{REPOSITORY}/git/matching-refs/heads/agent/release-"
        )
        # Older frozen refs may be removed after the next successful release.
        subject = git("show", "-s", "--format=%s", revision)
        if any(ref["object"]["sha"] == revision for ref in candidate_refs) or (
            subject == CANDIDATE_SUBJECT.format(version=tag.removeprefix("v"))
        ):
            raise ValueError(
                f"{tag} requires the successful candidate run with main sync"
            )

    latest = latest_unskipped_jobs(run_id)
    succeeded = {name for name, job in latest.items() if job["conclusion"] == "success"}
    if not (NATIVE_CHECKS | {PUBLICATION_JOB}) <= succeeded:
        raise ValueError(f"Run {run_id} lacks successful publication and native checks")
    if candidate and (
        SYNC_JOB not in succeeded
        or not sync_job_matches(
            latest[SYNC_JOB], revision, tag, r"refs/heads/agent/release-[1-9][0-9]*"
        )
    ):
        raise ValueError(
            f"Run {run_id} does not prove main sync for {tag} at {revision}"
        )


def latest_unskipped_jobs(run_id):
    # Partial reruns retain successful jobs that did not run again.
    latest = {}
    page = 1
    while True:
        jobs = api(
            f"repos/{REPOSITORY}/actions/runs/{run_id}/jobs?filter=all&per_page=100&page={page}"
        )["jobs"]
        for job in jobs:
            name = job["name"]
            if job["conclusion"] == "skipped":
                continue
            previous = latest.get(name)
            if previous is None or (job["run_attempt"], job["id"]) > (
                previous["run_attempt"],
                previous["id"],
            ):
                latest[name] = job
        if len(jobs) < 100:
            return latest
        page += 1


def next_release_version(upstream):
    match = re.fullmatch(r"rust-v(\d+)\.(\d+)\.(\d+)(-.+)", upstream)
    major, minor, patch, suffix = match.groups()
    base = f"{major}.{minor}.{int(patch) + 1}{suffix}"
    stem, plus, metadata = base.partition("+")
    tags, candidates, releases = {}, {}, {}
    for line in git(
        "ls-remote", "origin", "refs/tags/v*", "refs/heads/agent/release-*"
    ).splitlines():
        sha, reference = line.split()
        if reference.startswith("refs/heads/"):
            candidates[reference] = sha
        else:
            tag = reference.removeprefix("refs/tags/").removesuffix("^{}")
            if reference.endswith("^{}") or tag not in tags:
                tags[tag] = sha
    page = 1
    while True:
        batch = api(f"repos/{REPOSITORY}/releases?per_page=100&page={page}")
        releases.update((release["tag_name"], release) for release in batch)
        if len(batch) < 100:
            break
        page += 1
    superseded = validated_superseded_releases(candidates, tags, releases)
    occupied = set(tags) | set(releases)
    versions = {
        tag: semver_precedence(tag.removeprefix("v"))
        for tag in occupied
        if tag.startswith("v")
    }
    versions = {tag: order for tag, order in versions.items() if order is not None}
    counter_pattern = re.compile(
        rf"v{re.escape(stem)}\.([1-9][0-9]*){re.escape(plus + metadata)}"
    )
    family = {
        tag for tag in occupied if tag == f"v{base}" or counter_pattern.fullmatch(tag)
    }
    counter = max(
        (
            int(match[1])
            for tag in occupied
            if (match := counter_pattern.fullmatch(tag))
        ),
        default=0,
    )
    selected = (
        f"{stem}.{counter + 1}{plus}{metadata}"
        if f"v{base}" in occupied or counter
        else base
    )
    cutover = semver_precedence(LAST_PRE_MUSL_RELEASE.removeprefix("v"))
    highest = max(cutover, max(versions.values(), default=cutover))
    if semver_precedence(selected) <= highest:
        raise ValueError(
            f"Proposed version {selected} does not exceed existing fork versions; ask the owner"
        )
    # Historical versions still occupy SemVer space, but only musl releases can resume.
    musl_tags = {tag for tag, order in versions.items() if order > cutover}
    proof_tags = {tag for tag in musl_tags if tag in family or versions[tag] == highest}
    # A failed initialization can leave a candidate before its tag or draft exists.
    for reference, sha in candidates.items():
        if not any(tags.get(tag) == sha for tag in versions):
            raise ValueError(
                f"Unfinished candidate {reference}; resume its original run"
            )
    completed = set()
    resume_runs = None
    for tag in sorted(musl_tags - superseded, key=versions.__getitem__):
        release = releases.get(tag)
        if not tags.get(tag) or not release or not published_assets_complete(release):
            raise ValueError(f"Incomplete release {tag}; resume its original run")
        # Older complete releases need not depend on retained Actions logs forever.
        if tag not in proof_tags:
            continue
        references = [
            reference for reference, sha in candidates.items() if sha == tags[tag]
        ]
        if not references:
            verify_completed_tag_release(tag, tags[tag])
            continue
        for reference in references:
            git("fetch", "--no-tags", "origin", reference)
            if git("rev-parse", "FETCH_HEAD^{commit}") != tags[tag]:
                raise ValueError(f"Candidate {reference} changed during lookup")
            require_musl_source(tags[tag])
            run_id = reference.rsplit("-", 1)[1]
            run = api(f"repos/{REPOSITORY}/actions/runs/{run_id}")
            if completed_publication(run, tags[tag], tag, reference):
                completed.add(tag)
                break
            if resume_runs is None:
                resume_runs = []
                page = 1
                while True:
                    batch = api(
                        f"repos/{REPOSITORY}/actions/workflows/fork-rust-release.yml/runs?status=success&per_page=100&page={page}"
                    )["workflow_runs"]
                    resume_runs.extend(batch)
                    if len(batch) < 100:
                        break
                    page += 1
            if any(
                completed_publication(resume, tags[tag], tag, reference)
                for resume in resume_runs
            ):
                completed.add(tag)
                break
        if tag not in completed:
            raise ValueError(
                f"Release verification or main sync unfinished for {tag}; resume its original run"
            )
    return selected


def verify_completed_tag_release(tag, revision):
    git("fetch", "--no-tags", "origin", f"refs/tags/{tag}")
    if git("rev-parse", "FETCH_HEAD^{commit}") != revision:
        raise ValueError(f"Tag {tag} changed during lookup")
    require_musl_source(revision)
    if git("show", "-s", "--format=%s", revision) == CANDIDATE_SUBJECT.format(
        version=tag.removeprefix("v")
    ):
        raise ValueError(f"{tag} requires its completed candidate run with main sync")
    page = 1
    while True:
        runs = api(
            f"repos/{REPOSITORY}/actions/workflows/fork-rust-release.yml/runs"
            f"?head_sha={revision}&status=success&per_page=100&page={page}"
        )["workflow_runs"]
        if any(completed_publication(run, revision, tag, "") for run in runs):
            return
        if len(runs) < 100:
            raise ValueError(f"Cannot prove completed tag release {tag}; ask the owner")
        page += 1


def validated_superseded_releases(candidates, tags, releases):
    records = json.loads(
        Path(__file__).with_name("superseded-fork-releases.json").read_text()
    )
    superseded = set()
    for record in records:
        run_id, tag, source = record["run_id"], record["tag"], record["source"]
        expected_conclusion = record.get("conclusion", "failure")
        if expected_conclusion not in {"failure", "cancelled"}:
            raise ValueError(f"Superseded candidate {tag} has an invalid conclusion")
        reference = f"refs/heads/agent/release-{run_id}"
        release = releases.get(tag)
        if (
            candidates.get(reference) != source
            or {ref for ref, sha in candidates.items() if sha == source} != {reference}
            or tags.get(tag) != source
            or not release
            or release["id"] != record["release_id"]
            or release["target_commitish"] != source
            or not release["draft"]
            or release["published_at"] is not None
            or [
                {key: asset.get(key) for key in ("id", "name", "digest")}
                for asset in release.get("assets", [])
            ]
            != [record["installer"]]
        ):
            raise ValueError(f"Superseded candidate {tag} no longer matches its record")
        run = api(f"repos/{REPOSITORY}/actions/runs/{run_id}")
        if (
            run["status"] != "completed"
            or run["conclusion"] != expected_conclusion
            or run["event"] != "workflow_dispatch"
            or run["head_branch"] != "main"
            or run["path"] != ".github/workflows/fork-rust-release.yml"
        ):
            raise ValueError(
                f"Superseded candidate {tag} does not match its recorded release run"
            )
        jobs = api(
            f"repos/{REPOSITORY}/actions/runs/{run_id}/jobs?filter=latest&per_page=100"
        )["jobs"]
        stopped_builds = {
            job["name"]
            for job in jobs
            if job["conclusion"] == expected_conclusion
            and not (expected_conclusion == "cancelled" and not job.get("steps"))
        }
        if stopped_builds != set(record["stopped_build_jobs"]) or any(
            job["conclusion"] != "skipped"
            and not (
                expected_conclusion == "cancelled"
                and job["conclusion"] == "cancelled"
                and not job.get("steps")
            )
            for job in jobs
            if job["name"].startswith(
                (
                    "Complete and verify",
                    "Publish and verify",
                    "Verify published",
                    "Advance main",
                )
            )
        ):
            raise ValueError(
                f"Superseded candidate {tag} has unexpected publication state"
            )
        print(f"Preserving superseded candidate {tag}: {record['reason']}")
        superseded.add(tag)
    return superseded


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
        }.items():
            print(f"{key}={value}", file=stream)


def select_release_version():
    tags = list(upstream_prereleases())
    if not tags:
        raise ValueError("No upstream Rust prerelease tag found")
    return next_release_version(max(tags, key=lambda tag: tag[0])[1])


def pin():
    base = git("rev-parse", "HEAD")
    if base != remote_ref("refs/heads/main") or git("status", "--porcelain"):
        raise ValueError("Pin from a clean checkout of current origin/main")
    require_musl_source(base)
    new_version = select_release_version()
    git("checkout", "--detach", base)
    for path, contents in rewritten_files(base, new_version).items():
        Path(path).write_text(contents)
    git("add", "--", MANIFEST, LOCKFILE)
    git("commit", "-m", CANDIDATE_SUBJECT.format(version=new_version))
    revision = git("rev-parse", "HEAD")
    validate_candidate(revision)
    print(f"Pinned {revision} for v{new_version}")


def prepare():
    revision = os.environ["GITHUB_SHA"]
    reference = os.environ["GITHUB_REF"]
    publish = os.environ.get("PUBLISH_RELEASE", "false")
    pinned_sha = os.environ.get("PINNED_CANDIDATE_SHA", "")
    resume_run_id = os.environ.get("RESUME_RUN_ID", "")
    if pinned_sha and (
        publish != "true"
        or reference != "refs/heads/main"
        or resume_run_id
        or not re.fullmatch(r"[0-9a-f]{40}", pinned_sha)
    ):
        raise ValueError(
            "A pinned candidate SHA requires publication on main, without resume"
        )
    if publish not in {"true", "false"}:
        raise ValueError("PUBLISH_RELEASE must be true or false")
    if publish == "false":
        if resume_run_id:
            raise ValueError("Resuming a release requires PUBLISH_RELEASE=true")
        output(revision)
        return
    if reference.startswith("refs/tags/v"):
        tag = reference.removeprefix("refs/tags/")
        revision = git("rev-parse", f"{revision}^{{commit}}")
        if tag != f"v{version(revision)}":
            raise ValueError("Tag does not agree with Cargo version")
        require_musl_source(revision)
        order = semver_precedence(tag.removeprefix("v"))
        if order is None or order <= semver_precedence(
            LAST_PRE_MUSL_RELEASE.removeprefix("v")
        ):
            raise ValueError("Tag version must follow the musl cutover")
        output(revision, tag, complete=complete_candidate(tag, revision), publish=True)
        return
    if reference != "refs/heads/main":
        raise ValueError("New releases must be explicitly dispatched on main")
    run_id = resume_run_id or os.environ["GITHUB_RUN_ID"]
    if not re.fullmatch(r"[1-9][0-9]*", run_id):
        raise ValueError("Release run ID must be a positive decimal integer")
    candidate_ref = f"refs/heads/agent/release-{run_id}"
    candidate_sha = remote_ref(candidate_ref)
    if candidate_sha:
        git("fetch", "--no-tags", "origin", candidate_ref)
        revision = git("rev-parse", "FETCH_HEAD^{commit}")
        if revision != candidate_sha or (pinned_sha and revision != pinned_sha):
            raise ValueError(
                "Candidate changed during lookup or does not match the pin"
            )
        base = validate_candidate(revision)
        require_musl_source(revision)
        if not resume_run_id and base != os.environ["GITHUB_SHA"]:
            raise ValueError("Candidate belongs to a different source commit")
    else:
        if resume_run_id:
            raise ValueError("Requested candidate run does not exist")
        if not pinned_sha:
            raise ValueError("Pin the release candidate before dispatch")
        git(
            "fetch", "--no-tags", "origin", f"refs/heads/agent/pin-release-{pinned_sha}"
        )
        revision = git("rev-parse", "FETCH_HEAD^{commit}")
        if (
            revision != pinned_sha
            or validate_candidate(revision) != os.environ["GITHUB_SHA"]
        ):
            raise ValueError("Pinned candidate does not match this source commit")
        require_musl_source(revision)
        if version(revision) != select_release_version():
            raise ValueError(
                "Pinned version no longer matches the next available release"
            )
        git("push", "--no-follow-tags", "origin", f"{revision}:{candidate_ref}")
    tag = f"v{version(revision)}"
    output(
        revision, tag, candidate_ref, complete_candidate(tag, revision), publish=True
    )


def sync_main():
    revision = os.environ["RELEASE_SHA"]
    tag = os.environ["RELEASE_TAG"]
    candidate_ref = os.environ["CANDIDATE_REF"]
    if not candidate_ref:
        return  # Tag publications do not own a version update to main.
    if not re.fullmatch(r"refs/heads/agent/release-[1-9][0-9]*", candidate_ref):
        raise ValueError("Invalid candidate branch")
    git("fetch", "--no-tags", "origin", candidate_ref)
    if git("rev-parse", "FETCH_HEAD^{commit}") != revision:
        raise ValueError("Candidate branch no longer matches published commit")
    base = validate_candidate(revision)
    require_musl_source(revision)
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
    if sys.argv[1:] == ["pin"]:
        pin()
    elif sys.argv[1:] == ["prepare"]:
        prepare()
    elif sys.argv[1:] == ["sync-main"]:
        sync_main()
    elif len(sys.argv) == 5 and sys.argv[1] == "verify-base":
        verify_base_release(*sys.argv[2:])
    else:
        raise SystemExit(
            "Usage: prepare-fork-release.py pin|prepare|sync-main|verify-base RUN_ID TAG SHA"
        )
