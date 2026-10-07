#!/usr/bin/env python3

import argparse
import fcntl
import json
import os
import subprocess
import tempfile
from pathlib import Path


def git(root: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Lease a reusable task worktree without discarding work."
    )
    parser.add_argument("action", choices=("acquire", "release"))
    parser.add_argument("branch")
    parser.add_argument("--slot", default="native")
    parser.add_argument("--owner", default=os.environ.get("CODEX_THREAD_ID"))
    args = parser.parse_args()
    if not args.owner or not args.branch.startswith("codex/"):
        parser.error("Supply an owner ID and a codex/ task branch")
    if not args.slot or any(
        char not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_"
        for char in args.slot
    ):
        parser.error(
            "Slot names must contain only letters, numbers, hyphens, or underscores"
        )
    source = Path(__file__).resolve().parents[1]
    common = Path(
        git(source, "rev-parse", "--path-format=absolute", "--git-common-dir")
    )
    root = common.parent
    state = root / ".agents/dev-worktrees" / args.slot
    worktree = state / "worktree"
    lease_path = state / "lease.json"
    lease = {"owner": args.owner, "branch": args.branch}
    git(root, "check-ref-format", "--branch", args.branch)
    if (
        state.is_symlink()
        or state.parent.is_symlink()
        or worktree.is_symlink()
        or lease_path.is_symlink()
        or (state / "checkout.lock").is_symlink()
    ):
        raise RuntimeError("Reusable checkout and lease paths must not be symlinks")
    state.mkdir(parents=True, exist_ok=True)
    with (state / "checkout.lock").open("a") as operation:
        try:
            fcntl.flock(operation, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise RuntimeError(
                "Checkout is in use; wait for its Cargo command or lease operation"
            ) from None
        if worktree.exists():
            registered = {
                Path(line.removeprefix("worktree "))
                for line in git(root, "worktree", "list", "--porcelain").splitlines()
                if line.startswith("worktree ")
            }
            if worktree not in registered:
                raise RuntimeError("Reusable path is not a registered worktree")
            git_dir = Path(
                git(worktree, "rev-parse", "--path-format=absolute", "--git-dir")
            )
            if any(
                (git_dir / name).exists()
                for name in (
                    "rebase-merge",
                    "rebase-apply",
                    "MERGE_HEAD",
                    "CHERRY_PICK_HEAD",
                    "REVERT_HEAD",
                )
            ):
                raise RuntimeError(
                    "Finish the checkout's Git operation before changing its lease"
                )
        if args.action == "acquire":
            if lease_path.exists():
                if json.loads(lease_path.read_text()) != lease:
                    raise RuntimeError(
                        f"Slot is leased: {lease_path.read_text()}; use another --slot or coordinate with its owner"
                    )
                if git(worktree, "branch", "--show-current") != args.branch:
                    raise RuntimeError(
                        "Lease and checkout branch disagree; inspect before resuming"
                    )
                print(worktree)
                return
            with tempfile.NamedTemporaryFile(
                mode="w", dir=state, delete=False
            ) as stream:
                json.dump(lease, stream)
            os.replace(stream.name, lease_path)
            # Keep the lease on failure so an incomplete checkout cannot be taken over.
            if worktree.exists():
                if (
                    Path(
                        git(
                            worktree,
                            "rev-parse",
                            "--path-format=absolute",
                            "--git-common-dir",
                        )
                    )
                    != common
                ):
                    raise RuntimeError("Reusable path belongs to another repository")
                if git(worktree, "status", "--porcelain") or git(
                    worktree, "branch", "--show-current"
                ):
                    raise RuntimeError("Reusable worktree must be clean and detached")
                git(
                    worktree,
                    "switch",
                    "--no-overwrite-ignore",
                    "-c",
                    args.branch,
                    "main",
                )
            else:
                git(root, "worktree", "add", "-b", args.branch, str(worktree), "main")
        else:
            if json.loads(lease_path.read_text()) != lease:
                raise RuntimeError(
                    "Only the recorded owner and branch can release this slot"
                )
            if git(worktree, "branch", "--show-current") != args.branch:
                raise RuntimeError("Lease and checkout branch disagree")
            if git(worktree, "status", "--porcelain"):
                raise RuntimeError(
                    "Commit and integrate work before releasing the slot"
                )
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(worktree),
                    "merge-base",
                    "--is-ancestor",
                    "HEAD",
                    "main",
                ],
                check=True,
            )
            git(worktree, "switch", "--no-overwrite-ignore", "--detach", "HEAD")
            lease_path.unlink()
    print(worktree)


if __name__ == "__main__":
    main()
