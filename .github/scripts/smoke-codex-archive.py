import json
import os
import re
import selectors
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import tomllib
from pathlib import Path

repo_root = Path(__file__).resolve().parents[2]
os.environ.setdefault("CODEX_REPO_ROOT", str(repo_root))
sys.path.insert(0, str(repo_root / "scripts"))

from codex_package.layout import validate_package_dir
from codex_package.targets import PACKAGE_VARIANTS
from codex_package.targets import TARGET_SPECS

LINUX_GNU = "x86_64-unknown-linux-gnu"
LINUX_MUSL = "x86_64-unknown-linux-musl"
VOICE_DIR = "codex-resources/voice"
VOICE_HELPER = f"{VOICE_DIR}/bin/codex-voice-host"
VOICE_NOTICES = {
    "NOTICE.md",
    "sources.json",
    "licenses/LGPL-2.1.txt",
    "licenses/Opus.txt",
    "licenses/PCRE2.md",
    "licenses/libffi.txt",
    "licenses/proxy-libintl.txt",
    "licenses/sljit.txt",
    "licenses/zlib.txt",
}


def verify_voice_package(package, version):
    sys.path.insert(0, str(repo_root / "third_party/voice"))
    from linux_runtime import SYSTEM_IMPORTS, finalize_copy, inspect
    from package_runtime import runtime_files
    from runtime import digest

    voice = package / VOICE_DIR
    commit = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=repo_root, text=True
    ).strip()
    libraries = runtime_files(voice, LINUX_GNU, public_release=True)
    receipt = json.loads((voice / "runtime.json").read_text())
    expected = {f"{VOICE_DIR}/{name}": checksum for name, checksum in libraries.items()}
    for name in VOICE_NOTICES:
        checksum = digest(voice / name)
        if checksum != digest(repo_root / "third_party/voice" / name):
            raise SystemExit(f"Voice notice differs from the candidate: {name}")
        expected[f"{VOICE_DIR}/{name}"] = checksum
    expected.update(
        {name: digest(package / name) for name in ("bin/codex", VOICE_HELPER)}
    )
    manifest_fields = json.loads(
        (voice / "manifest.json").read_text(), object_pairs_hook=list
    )
    manifest = dict(manifest_fields)
    digest_fields = manifest.pop("sha256")
    voice_files = {
        path.relative_to(package).as_posix()
        for path in voice.rglob("*")
        if path.is_file()
    }
    if (
        receipt["sourceCommit"] != commit
        or len(manifest_fields) != 6
        or type(manifest["schemaVersion"]) is not int
        or len(digest_fields) != len(expected)
        or dict(digest_fields) != expected
        or voice_files
        != expected.keys() - {"bin/codex"} | {f"{VOICE_DIR}/manifest.json"}
        or manifest
        != {
            "schemaVersion": 1,
            "buildCommit": commit,
            "appTarget": LINUX_MUSL,
            "voiceTarget": LINUX_GNU,
            "appVersion": version,
        }
    ):
        raise SystemExit("Voice package does not match the exact candidate and runtime")
    binaries = {
        voice / name: inspect(voice / name, LINUX_GNU)
        for name in libraries
        if name != "runtime.json"
    }
    imports = SYSTEM_IMPORTS | {metadata.identity for metadata in binaries.values()}
    for path, metadata in binaries.items():
        if not set(metadata.imports) <= imports:
            raise SystemExit(f"Missing packaged library dependency for {path.name}")
        finalize_copy(path, metadata, {})
    return commit, [
        VOICE_HELPER,
        *(path.relative_to(package).as_posix() for path in binaries),
    ]


def smoke_voice_runtime(executable, commit):
    environment = {
        "GST_PLUGIN_PATH": "",
        "GST_PLUGIN_PATH_1_0": "",
        "GST_PLUGIN_SYSTEM_PATH": "",
        "GST_PLUGIN_SYSTEM_PATH_1_0": "",
        "GST_REGISTRY": "/dev/null",
        "GST_REGISTRY_UPDATE": "no",
        "GST_REGISTRY_FORK": "no",
    }
    with (
        subprocess.Popen(
            [executable], stdin=subprocess.PIPE, stdout=subprocess.PIPE, env=environment
        ) as process,
        selectors.DefaultSelector() as ready,
    ):
        ready.register(process.stdout, selectors.EVENT_READ)
        os.set_blocking(process.stdout.fileno(), False)
        try:
            for message, expected in (
                ({"type": "hello", "protocol": 1, "buildCommit": commit}, "ready"),
                ({"type": "initializeRuntime"}, "runtimeReady"),
                ({"type": "close"}, "closed"),
            ):
                payload = json.dumps(message).encode()
                process.stdin.write(struct.pack(">I", len(payload)) + payload)
                process.stdin.flush()
                if not ready.select(timeout=15):
                    raise SystemExit("Voice helper did not reply")
                # Each short reply is one atomic pipe write; reject partial frames.
                header = process.stdout.read1(4)
                if len(header) != 4:
                    raise SystemExit("Voice helper sent an invalid frame header")
                (length,) = struct.unpack(">I", header)
                if not 0 < length <= 128 * 1024 or json.loads(
                    process.stdout.read1(length)
                ) != {"type": expected}:
                    raise SystemExit(f"Voice helper did not report {expected}")
            if process.wait(timeout=10) != 0:
                raise SystemExit("Voice helper did not close cleanly")
        finally:
            if process.poll() is None:
                process.kill()
    print("voice=runtime-ready (no audio devices opened)")


archive_path = Path(sys.argv[1])
inspect_only = len(sys.argv) > 2 and sys.argv[2] == "--inspect-only"
release_targets = {"aarch64-apple-darwin", LINUX_GNU, LINUX_MUSL}
target = archive_path.name.removeprefix("codex-").removesuffix(".tar.gz")
is_v3 = target in {f"{LINUX_GNU}-v3", f"{LINUX_MUSL}-v3"}
target = target.removesuffix("-v3") if is_v3 else target
if (
    target not in release_targets
    or archive_path.name != f"codex-{target}{'-v3' if is_v3 else ''}.tar.gz"
):
    raise SystemExit(f"Unsupported CLI archive: {archive_path}")
spec = TARGET_SPECS[target]
version = tomllib.loads((repo_root / "codex-rs/Cargo.toml").read_text())["workspace"][
    "package"
]["version"]
required_dirs = {"bin", "codex-resources", "codex-path"}
executables = {
    "bin/codex",
    "bin/codex-code-mode-host",
    "bin/codex-exec-server",
    "codex-path/rg",
}
if spec.is_linux:
    executables.add("codex-resources/bwrap")
required_files = executables | {"codex-package.json"}
if target == LINUX_MUSL:
    required_dirs.update(
        f"{VOICE_DIR}{suffix}"
        for suffix in ("", "/bin", "/lib", "/lib/gstreamer-1.0", "/licenses")
    )
    executables.add(VOICE_HELPER)
    required_files.update(
        {
            VOICE_HELPER,
            *(
                f"{VOICE_DIR}/{name}"
                for name in VOICE_NOTICES | {"manifest.json", "runtime.json"}
            ),
        }
    )
with tarfile.open(archive_path, "r:gz") as archive:
    members = archive.getmembers()
    if target == LINUX_MUSL:
        required_files.update(
            member.name
            for member in members
            if re.fullmatch(
                rf"{VOICE_DIR}/lib/(?:gstreamer-1\.0/)?[A-Za-z0-9_+.-]+\.so(?:\.[0-9]+)*",
                member.name,
            )
        )
    if (
        {member.name for member in members} != required_dirs | required_files
        or len(members) != len(required_dirs | required_files)
        or any(
            not member.isdir()
            if member.name in required_dirs
            else (
                not member.isfile()
                or member.size == 0
                or (member.name in executables and not member.mode & 0o111)
            )
            for member in members
        )
        or any(
            (member.name == "codex-package.json" and member.size > 4096)
            or (
                member.name
                in {f"{VOICE_DIR}/manifest.json", f"{VOICE_DIR}/runtime.json"}
                and member.size > 1024 * 1024
            )
            for member in members
        )
    ):
        raise SystemExit(f"Invalid complete CLI package archive: {archive_path}")
    with tempfile.TemporaryDirectory() as temporary_dir:
        package = Path(temporary_dir)
        # Only exact regular files and directories above can reach extraction.
        for member in members:
            path = package / member.name
            if member.isdir():
                path.mkdir(parents=True, exist_ok=True)
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                with archive.extractfile(member) as source, path.open("wb") as output:
                    shutil.copyfileobj(source, output)
                os.chmod(path, member.mode & 0o777)
        validate_package_dir(package, PACKAGE_VARIANTS["codex"], spec)
        metadata_fields = json.loads(
            (package / "codex-package.json").read_text(), object_pairs_hook=list
        )
        expected_metadata = {
            "layoutVersion": 1,
            "version": version,
            "target": target,
            "variant": "codex",
            "entrypoint": "bin/codex",
            "execServer": "bin/codex-exec-server",
            "resourcesDir": "codex-resources",
            "pathDir": "codex-path",
        }
        if (
            len(metadata_fields) != len(expected_metadata)
            or dict(metadata_fields) != expected_metadata
            or type(dict(metadata_fields)["layoutVersion"]) is not int
        ):
            raise SystemExit("Package metadata does not match the candidate contract")
        voice_binaries = []
        if target == LINUX_MUSL:
            voice_commit, voice_binaries = verify_voice_package(package, version)
        if inspect_only:
            print(
                f"archive={archive_path} target={target} version={version} package=complete"
            )
            raise SystemExit(0)
        if spec.is_linux:
            for name in sorted(executables | set(voice_binaries)):
                executable = package / name
                headers = subprocess.check_output(
                    [
                        "readelf",
                        "--file-header",
                        "--program-headers",
                        "--wide",
                        executable,
                    ],
                    text=True,
                )
                if not all(
                    value in headers
                    for value in ("ELF64", "Advanced Micro Devices X86-64")
                ):
                    raise SystemExit(f"{name} is not an expected x86_64 executable")
                versions = subprocess.check_output(
                    ["readelf", "--version-info", "--wide", executable], text=True
                )
                glibc_versions = set(re.findall(r"Name: (GLIBC_\S+)", versions))
                if any(
                    not re.fullmatch(r"GLIBC_[0-9]+(?:\.[0-9]+)+", value)
                    or tuple(int(part) for part in value[6:].split(".")) > (2, 35)
                    for value in glibc_versions
                ):
                    raise SystemExit(
                        f"{name} exceeds the glibc 2.35 baseline: {sorted(glibc_versions)}"
                    )
                dependencies = subprocess.check_output(
                    ["readelf", "--dynamic", "--wide", executable], text=True
                )
                libraries = re.findall(r"\(NEEDED\).*?\[(.*?)\]", dependencies)
                if target == LINUX_MUSL and name not in voice_binaries:
                    if re.search(r"\bINTERP\b", headers) or libraries or glibc_versions:
                        raise SystemExit(f"{name} is not static")
                elif name != "codex-path/rg" and (
                    not glibc_versions
                    or (
                        name in executables
                        and "/lib64/ld-linux-x86-64.so.2" not in headers
                    )
                ):
                    raise SystemExit(f"{name} is not a GNU x86_64 binary")
                print(
                    f"{name} glibc_versions={sorted(glibc_versions)} needed={libraries}"
                )
            if voice_binaries:
                smoke_voice_runtime(package / VOICE_HELPER, voice_commit)
        else:
            for name in sorted(executables):
                architectures = subprocess.check_output(
                    ["lipo", "-archs", package / name], text=True, timeout=30
                ).split()
                if "arm64" not in architectures:
                    raise SystemExit(f"{name} lacks native arm64 code")
        binary_path = package / "bin/codex"
        helper_path = package / "bin/codex-code-mode-host"
        reported_version = subprocess.check_output(
            [binary_path, "--version"], text=True, timeout=30
        ).strip()
        version_name = "asm"
        if is_v3:
            version_name = "asm (x86-64-v3, PGO)"
        elif spec.is_linux:
            version_name = "asm (PGO)"
        expected_version = f"{version_name} {version}"
        if reported_version != expected_version:
            raise SystemExit(
                f"Wrong binary version: {reported_version}; expected {expected_version}"
            )
        exec_server_path = package / "bin/codex-exec-server"
        exec_server_version = subprocess.check_output(
            [exec_server_path, "--version"], text=True, timeout=30
        ).strip()
        if exec_server_version != f"codex-exec-server {version}":
            raise SystemExit(f"Wrong exec-server version: {exec_server_version}")
        for executable in (binary_path, helper_path, exec_server_path):
            output = subprocess.check_output(
                [executable, "--help"], text=True, timeout=30
            )
            if not any(line.startswith("Usage:") for line in output.splitlines()):
                raise SystemExit(f"{executable.name} help lacks Usage")
        subprocess.run([package / "codex-path/rg", "--version"], check=True, timeout=30)
        if spec.is_linux:
            output = subprocess.check_output(
                [package / "codex-resources/bwrap", "--help"], text=True, timeout=30
            )
            if not all(
                option in output for option in ("--argv0", "--perms", "--as-pid-1")
            ):
                raise SystemExit("Bundled Bubblewrap lacks required sandbox options")
        print(
            f"archive={archive_path} bytes={archive_path.stat().st_size} "
            f"version={reported_version} package=complete help=ok helper_help=ok exec_server_version=ok exec_server_help=ok resources=ok"
        )
        print(
            f"binary_bytes={binary_path.stat().st_size} helper_bytes={helper_path.stat().st_size} "
            f"exec_server_bytes={exec_server_path.stat().st_size}"
        )
