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
VERSION = "0.162.1-alpha.18.1.1"
NEXT_VERSION = "0.162.1-alpha.18.1.2"
TARGET = "x86_64-unknown-linux-gnu"


def write_release(
    root,
    version,
    *,
    platform_manifest=True,
    missing_helper=False,
    bad_checksum=False,
    wrong_version=False,
    invalid_digest=False,
):
    source = root / f"source-{version}"
    if source.exists():
        shutil.rmtree(source)
    for name in (
        "bin/codex",
        "bin/codex-code-mode-host",
        "codex-path/rg",
        "codex-resources/bwrap",
    ):
        if missing_helper and name == "bin/codex-code-mode-host":
            continue
        path = source / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f'#!/bin/sh\necho "codex-cli {version}"\n')
        path.chmod(0o755)
    package_metadata = {
        "layoutVersion": 1,
        "version": VERSION if wrong_version else version,
        "target": TARGET,
        "variant": "codex",
        "entrypoint": "bin/codex",
        "resourcesDir": "codex-resources",
        "pathDir": "codex-path",
    }
    (source / "codex-package.json").write_text(json.dumps(package_metadata))
    asset = f"codex-{TARGET}.tar.gz"
    archive = root / f"{version}-{asset}"
    with tarfile.open(archive, "w:gz") as tar:
        for path in sorted(source.rglob("*")):
            tar.add(path, arcname=str(path.relative_to(source)), recursive=False)
    archive_digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    installer_digest = hashlib.sha256(INSTALL_SCRIPT.read_bytes()).hexdigest()
    manifest_name = f"SHA256SUMS-{TARGET}" if platform_manifest else "SHA256SUMS"
    manifest = root / f"{version}-{manifest_name}"
    contents = f"{'0' * 64 if bad_checksum else archive_digest}  {asset}\n"
    if platform_manifest:
        contents += f"{installer_digest}  install.sh\n"
    manifest.write_text(contents)
    manifest_digest = hashlib.sha256(manifest.read_bytes()).hexdigest()
    assets = [
        {"name": asset, "digest": f"sha256:{archive_digest}"},
        {
            "name": manifest_name,
            "digest": "invalid" if invalid_digest else f"sha256:{manifest_digest}",
        },
        {"name": "install.sh", "digest": f"sha256:{installer_digest}"},
    ]
    if platform_manifest:
        assets.append({"name": "SHA256SUMS", "digest": f"sha256:{manifest_digest}"})
    metadata = root / f"{version}-metadata.json"
    metadata.write_text(json.dumps({"tag_name": f"v{version}", "assets": assets}))
    return archive, manifest, metadata


def run_installer(root, release, files):
    shim = root / "shim"
    shim.mkdir(exist_ok=True)
    scripts = {
        "uname": '#!/bin/sh\ncase "$1" in -s) echo Linux;; -m) echo x86_64;; esac\n',
        "getconf": '#!/bin/sh\necho "glibc 2.35"\n',
        "curl": """#!/bin/sh
url=''; output=''
while [ "$#" -gt 0 ]; do
  case "$1" in -o) output="$2"; shift;; https://*) url="$1";; esac
  shift
done
printf '%s\\n' "$url" >> "$TEST_REQUEST_LOG"
case "$url" in
  https://api.github.com/repos/trungnt13/asm/releases/*) source="$TEST_METADATA" ;;
  https://github.com/trungnt13/asm/releases/download/*/SHA256SUMS*) source="$TEST_MANIFEST" ;;
  https://github.com/trungnt13/asm/releases/download/*/codex-*.tar.gz) source="$TEST_ARCHIVE" ;;
  *) exit 99 ;;
esac
[ -n "$source" ] || exit 22
if [ -n "$output" ]; then cp "$source" "$output"; else cat "$source"; fi
""",
    }
    for name, contents in scripts.items():
        path = shim / name
        path.write_text(contents)
        path.chmod(0o755)
    request_log = root / "requests.log"
    request_log.unlink(missing_ok=True)
    archive, manifest, metadata = files
    home = root / "home"
    home.mkdir(exist_ok=True)
    env = dict(
        os.environ,
        HOME=str(home),
        CODEX_HOME=str(root / "codex-home"),
        CODEX_INSTALL_DIR=str(root / "install-bin"),
        CODEX_NON_INTERACTIVE="1",
        CODEX_RELEASE=release,
        CODEX_INSTALL_DAEMON_ONLY="0",
        TEST_REQUEST_LOG=str(request_log),
        TEST_ARCHIVE=str(archive),
        TEST_MANIFEST=str(manifest),
        TEST_METADATA=str(metadata),
        PATH=f"{shim}:/usr/bin:/bin",
    )
    command = ["/bin/sh", str(INSTALL_SCRIPT)]
    result = subprocess.run(command, env=env, text=True, capture_output=True)
    requests = request_log.read_text().splitlines() if request_log.exists() else []
    return result, requests


class InstallShTest(unittest.TestCase):
    def test_latest_installs_pair_and_reinstall_reuses_it(self):
        for platform_manifest in (False, True):
            with (
                self.subTest(platform_manifest=platform_manifest),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory)
                files = write_release(
                    root, VERSION, platform_manifest=platform_manifest
                )
                marker = root / "codex-home/packages/standalone/auto-update-version"
                marker.parent.mkdir(parents=True)
                marker.write_text("upstream-release")
                current = root / "codex-home/packages/asm-standalone/current"
                for release, version, expected_downloads in (
                    ("latest", VERSION, 3),
                    (VERSION, VERSION, 1),
                    (NEXT_VERSION, NEXT_VERSION, 3),
                ):
                    if version == NEXT_VERSION:
                        files = write_release(
                            root, version, platform_manifest=platform_manifest
                        )
                    result, requests = run_installer(root, release, files)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(len(requests), expected_downloads)
                    self.assertEqual(current.resolve().name, f"{version}-{TARGET}")
                    for name in ("codex", "codex-code-mode-host"):
                        self.assertEqual(
                            (root / "install-bin" / name).resolve(),
                            current.resolve() / "bin" / name,
                        )
                    self.assertEqual(marker.read_text(), "upstream-release")
                    self.assertFalse((current.parent / "auto-update-version").exists())

    def test_bad_checksum_and_missing_helper_keep_current_selection(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first, _ = run_installer(root, VERSION, write_release(root, VERSION))
            self.assertEqual(first.returncode, 0, first.stderr)
            current = root / "codex-home/packages/asm-standalone/current"
            selected = current.resolve()
            for options, message in (
                ({"bad_checksum": True}, "metadata and SHA256SUMS disagree"),
                ({"invalid_digest": True}, "Could not find SHA-256 digest"),
                ({"missing_helper": True}, "must contain a complete canonical package"),
                ({"wrong_version": True}, "Invalid ASM package metadata"),
            ):
                with self.subTest(options=options):
                    files = write_release(root, NEXT_VERSION, **options)
                    result, requests = run_installer(root, NEXT_VERSION, files)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)
                    self.assertEqual(current.resolve(), selected)
                    for name in ("codex", "codex-code-mode-host"):
                        self.assertEqual(
                            (root / "install-bin" / name).resolve(),
                            selected / "bin" / name,
                        )
                    self.assertTrue(all("trungnt13/asm" in url for url in requests))


if __name__ == "__main__":
    unittest.main()
