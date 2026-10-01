#!/usr/bin/env python3

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest


INSTALL_SCRIPT = Path(__file__).with_name("install.sh")
VERSION = "0.159.1-alpha.9"
NEXT_VERSION = "0.159.1-alpha.10"


def write_release(
    root: Path,
    version: str,
    target: str,
    *,
    helper: bool = True,
    bad_checksum: bool = False,
) -> tuple[Path, Path, Path]:
    source = root / f"source-{version}"
    source.mkdir(exist_ok=True)
    if not helper:
        (source / "codex-code-mode-host").unlink(missing_ok=True)
    codex = source / "codex"
    codex.write_text(
        f'#!/bin/sh\n[ "$1" = "--version" ] && echo "codex-cli {version}"\n'
    )
    codex.chmod(0o755)
    if helper:
        host = source / "codex-code-mode-host"
        host.write_text('#!/bin/sh\necho "Usage: codex-code-mode-host"\n')
        host.chmod(0o755)
    asset = f"codex-{target}.tar.gz"
    archive = root / f"{version}-{asset}"
    with tarfile.open(archive, "w:gz") as tar:
        for name in ("codex", "codex-code-mode-host"):
            path = source / name
            if path.exists():
                tar.add(path, arcname=name)
    archive_digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    manifest = root / f"{version}-SHA256SUMS"
    manifest.write_text(f"{'0' * 64 if bad_checksum else archive_digest}  {asset}\n")
    manifest_digest = hashlib.sha256(manifest.read_bytes()).hexdigest()
    metadata = root / f"{version}-metadata.json"
    metadata.write_text(
        json.dumps(
            {
                "tag_name": f"v{version}",
                "assets": [
                    {"name": asset, "digest": f"sha256:{archive_digest}"},
                    {"name": "SHA256SUMS", "digest": f"sha256:{manifest_digest}"},
                ],
            }
        )
    )
    return archive, manifest, metadata


def run_installer(
    root: Path,
    release: str,
    files: tuple[Path, Path, Path] | None = None,
    *,
    system: str = "Linux",
    machine: str = "x86_64",
    rosetta: bool = False,
    metadata_failure: bool = False,
    daemon_only: bool = False,
    release_arg: str | None = None,
    libc_version: str = "glibc 2.35",
) -> tuple[subprocess.CompletedProcess[str], list[str]]:
    shim = root / "shim"
    shim.mkdir(exist_ok=True)
    curl = shim / "curl"
    curl.write_text("""#!/bin/sh
url=''; output=''
while [ "$#" -gt 0 ]; do
  case "$1" in -o) output="$2"; shift;; https://*) url="$1";; esac
  shift
done
printf '%s\\n' "$url" >> "$TEST_REQUEST_LOG"
case "$url" in
  https://api.github.com/repos/trungnt13/asm/releases/*)
    [ "$TEST_METADATA_FAILURE" = 0 ] || exit 22
    source="$TEST_METADATA" ;;
  https://github.com/trungnt13/asm/releases/download/*/SHA256SUMS)
    source="$TEST_MANIFEST" ;;
  https://github.com/trungnt13/asm/releases/download/*/codex-*.tar.gz)
    source="$TEST_ARCHIVE" ;;
  *) exit 99 ;;
esac
[ -n "$source" ] || exit 22
if [ -n "$output" ]; then cp "$source" "$output"; else cat "$source"; fi
""")
    curl.chmod(0o755)
    uname = shim / "uname"
    uname.write_text(
        f'#!/bin/sh\ncase "$1" in -s) echo {system};; -m) echo {machine};; esac\n'
    )
    uname.chmod(0o755)
    sysctl = shim / "sysctl"
    sysctl.write_text(f"#!/bin/sh\necho {1 if rosetta else 0}\n")
    sysctl.chmod(0o755)
    getconf = shim / "getconf"
    getconf.write_text(
        '#!/bin/sh\n[ "$1" = "GNU_LIBC_VERSION" ] || exit 1\nprintf "%s\\n" "$TEST_LIBC_VERSION"\n'
    )
    getconf.chmod(0o755)
    request_log = root / "requests.log"
    request_log.unlink(missing_ok=True)
    archive, manifest, metadata = files if files is not None else (None, None, None)
    home = root / "home"
    home.mkdir(exist_ok=True)
    env = dict(
        os.environ,
        HOME=str(home),
        CODEX_HOME=str(root / "codex-home"),
        CODEX_INSTALL_DIR=str(root / "install-bin"),
        CODEX_NON_INTERACTIVE="1",
        CODEX_RELEASE=release,
        CODEX_INSTALL_DAEMON_ONLY="1" if daemon_only else "0",
        TEST_REQUEST_LOG=str(request_log),
        TEST_ARCHIVE=str(archive or ""),
        TEST_MANIFEST=str(manifest or ""),
        TEST_METADATA=str(metadata or ""),
        TEST_METADATA_FAILURE="1" if metadata_failure else "0",
        TEST_LIBC_VERSION=libc_version,
        PATH=f"{shim}:/usr/bin:/bin",
    )
    command = ["/bin/sh", str(INSTALL_SCRIPT)]
    if release_arg is not None:
        command.extend(("--release", release_arg))
    result = subprocess.run(command, env=env, text=True, capture_output=True)
    requests = request_log.read_text().splitlines() if request_log.exists() else []
    return result, requests


class InstallShTest(unittest.TestCase):
    def test_metadata_failure_stops_without_asset_fallback(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result, requests = run_installer(root, VERSION, metadata_failure=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(
                requests,
                [
                    f"https://api.github.com/repos/trungnt13/asm/releases/tags/v{VERSION}"
                ],
            )
            self.assertIn("Could not fetch GitHub release metadata", result.stderr)
            self.assertFalse((root / "codex-home").exists())

    def test_latest_installs_pair_and_reinstall_reuses_it(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            files = write_release(root, VERSION, "x86_64-unknown-linux-gnu")
            upstream_marker = (
                root / "codex-home/packages/standalone/auto-update-version"
            )
            upstream_marker.parent.mkdir(parents=True)
            upstream_marker.write_text("upstream-release")
            first, requests = run_installer(root, "latest", files)
            self.assertEqual(first.returncode, 0, first.stderr)
            self.assertEqual(
                requests,
                [
                    "https://api.github.com/repos/trungnt13/asm/releases/latest",
                    f"https://github.com/trungnt13/asm/releases/download/v{VERSION}/SHA256SUMS",
                    f"https://github.com/trungnt13/asm/releases/download/v{VERSION}/codex-x86_64-unknown-linux-gnu.tar.gz",
                ],
            )
            current = root / "codex-home/packages/asm-standalone/current"
            self.assertEqual(
                current.resolve().name, f"{VERSION}-x86_64-unknown-linux-gnu"
            )
            for name in ("codex", "codex-code-mode-host"):
                self.assertEqual(
                    (root / "install-bin" / name).resolve(),
                    current.resolve() / "bin" / name,
                )
            self.assertFalse(
                (
                    root / "codex-home/packages/asm-standalone/auto-update-version"
                ).exists()
            )
            self.assertEqual(upstream_marker.read_text(), "upstream-release")
            second, requests = run_installer(root, "latest", files, release_arg=VERSION)
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertEqual(
                requests,
                [
                    f"https://api.github.com/repos/trungnt13/asm/releases/tags/v{VERSION}"
                ],
            )
            self.assertEqual(
                current.resolve().name, f"{VERSION}-x86_64-unknown-linux-gnu"
            )
            self.assertEqual(upstream_marker.read_text(), "upstream-release")

    def test_bad_checksum_and_missing_helper_keep_current_selection(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first_files = write_release(root, VERSION, "x86_64-unknown-linux-gnu")
            first, _ = run_installer(root, VERSION, first_files)
            self.assertEqual(first.returncode, 0, first.stderr)
            current = root / "codex-home/packages/asm-standalone/current"
            selected = current.resolve()
            for options, message in (
                ({"bad_checksum": True}, "metadata and SHA256SUMS disagree"),
                ({"helper": False}, "must contain codex and codex-code-mode-host only"),
            ):
                files = write_release(
                    root, NEXT_VERSION, "x86_64-unknown-linux-gnu", **options
                )
                result, requests = run_installer(root, NEXT_VERSION, files)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(message, result.stderr)
                self.assertEqual(current.resolve(), selected)
                self.assertEqual(
                    (root / "install-bin/codex").resolve(), selected / "bin/codex"
                )
                self.assertTrue(all("trungnt13/asm" in url for url in requests))
            self.assertFalse(
                (
                    root / "codex-home/packages/asm-standalone/auto-update-version"
                ).exists()
            )

    def test_linux_libc_floor_rejects_before_download_or_state_changes(self) -> None:
        for libc_version in (
            "glibc 2.34",
            "glibc 2.9",
            "musl 1.2.5",
            "",
            "glibc unknown",
        ):
            with (
                self.subTest(libc_version=libc_version),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory)
                result, requests = run_installer(
                    root, VERSION, libc_version=libc_version
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(
                    "require glibc 2.35 or newer (Ubuntu 22.04+)", result.stderr
                )
                self.assertEqual(requests, [])
                self.assertFalse((root / "codex-home").exists())
                self.assertFalse((root / "install-bin").exists())

    def test_musl_only_release_is_rejected_without_state_changes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            files = write_release(root, VERSION, "x86_64-unknown-linux-musl")
            result, requests = run_installer(root, "latest", files, release_arg=VERSION)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("is MUSL-only", result.stderr)
            self.assertEqual(
                requests,
                [
                    f"https://api.github.com/repos/trungnt13/asm/releases/tags/v{VERSION}"
                ],
            )
            self.assertFalse((root / "codex-home").exists())
            self.assertFalse((root / "install-bin").exists())

    def test_gnu_install_replaces_musl_selection_without_deleting_state(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_release(root, VERSION, "x86_64-unknown-linux-musl")
            package_root = root / "codex-home/packages/asm-standalone"
            old_release = (
                package_root / "releases" / f"{VERSION}-x86_64-unknown-linux-musl"
            )
            (old_release / "bin").mkdir(parents=True)
            for name in ("codex", "codex-code-mode-host"):
                shutil.copy2(
                    root / f"source-{VERSION}" / name, old_release / "bin" / name
                )
            (old_release / "codex").symlink_to("bin/codex")
            current = package_root / "current"
            current.symlink_to(old_release)
            install_bin = root / "install-bin"
            install_bin.mkdir()
            for name in ("codex", "codex-code-mode-host"):
                (install_bin / name).symlink_to(current / "bin" / name)
            config = root / "codex-home/config.toml"
            config.write_text('model = "custom-model"\n')
            files = write_release(root, VERSION, "x86_64-unknown-linux-gnu")
            result, requests = run_installer(
                root, VERSION, files, libc_version="glibc 2.36"
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(
                requests[-1].endswith("codex-x86_64-unknown-linux-gnu.tar.gz")
            )
            self.assertEqual(
                current.resolve().name, f"{VERSION}-x86_64-unknown-linux-gnu"
            )
            for name in ("codex", "codex-code-mode-host"):
                self.assertEqual(
                    (install_bin / name).resolve(), current.resolve() / "bin" / name
                )
                self.assertTrue((old_release / "bin" / name).is_file())
            self.assertEqual(config.read_text(), 'model = "custom-model"\n')

    def test_macos_native_and_rosetta_use_arm64_archive(self) -> None:
        for machine, rosetta in (("arm64", False), ("x86_64", True)):
            with (
                self.subTest(machine=machine, rosetta=rosetta),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory)
                files = write_release(root, VERSION, "aarch64-apple-darwin")
                result, requests = run_installer(
                    root,
                    VERSION,
                    files,
                    system="Darwin",
                    machine=machine,
                    rosetta=rosetta,
                    libc_version="",
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(
                    requests[-1].endswith("codex-aarch64-apple-darwin.tar.gz")
                )
                self.assertTrue(
                    (root / "install-bin/codex-code-mode-host").is_symlink()
                )

    def test_unsupported_targets_and_daemon_mode_leave_state_untouched(self) -> None:
        for system, machine, rosetta, daemon in (
            ("Darwin", "x86_64", False, False),
            ("Linux", "aarch64", False, False),
            ("Linux", "x86_64", False, True),
        ):
            with (
                self.subTest(system=system, machine=machine, daemon=daemon),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory)
                result, requests = run_installer(
                    root,
                    VERSION,
                    system=system,
                    machine=machine,
                    rosetta=rosetta,
                    daemon_only=daemon,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(
                    "daemon-only" if daemon else "Unsupported ASM release target",
                    result.stderr,
                )
                self.assertEqual(requests, [])
                self.assertFalse((root / "codex-home").exists())


if __name__ == "__main__":
    unittest.main()
