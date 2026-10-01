import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
from pathlib import Path

archive_path = Path(sys.argv[1])
inspect_only = len(sys.argv) > 2 and sys.argv[2] == '--inspect-only'
with tarfile.open(archive_path, 'r:gz') as archive:
    members = archive.getmembers()
    required = {'codex', 'codex-code-mode-host'}
    if {member.name for member in members} != required or len(members) != 2 or any(
        not member.isfile() or member.size == 0 or not member.mode & 0o111 for member in members
    ):
        raise SystemExit(f'Invalid CLI archive: {archive_path}')
    if inspect_only:
        print(f'archive={archive_path} members=codex,codex-code-mode-host')
        raise SystemExit(0)
    version = tomllib.loads(Path('Cargo.toml').read_text())['workspace']['package']['version']
    with tempfile.TemporaryDirectory() as temporary_dir:
        for member in members:
            binary_path = Path(temporary_dir) / member.name
            with archive.extractfile(member) as source, binary_path.open('wb') as target:
                shutil.copyfileobj(source, target)
            os.chmod(binary_path, 0o700)
        if archive_path.name == 'codex-x86_64-unknown-linux-gnu.tar.gz':
            for name in sorted(required):
                executable = Path(temporary_dir) / name
                headers = subprocess.check_output(
                    ['readelf', '--file-header', '--program-headers', '--wide', executable], text=True
                )
                if not all(value in headers for value in (
                    'ELF64', 'Advanced Micro Devices X86-64', '/lib64/ld-linux-x86-64.so.2'
                )):
                    raise SystemExit(f'{name} is not a dynamically linked x86_64 GNU executable')
                versions = subprocess.check_output(
                    ['readelf', '--version-info', '--wide', executable], text=True
                )
                glibc_versions = set(re.findall(r'Name: (GLIBC_\S+)', versions))
                if not glibc_versions or any(
                    not re.fullmatch(r'GLIBC_[0-9]+(?:\.[0-9]+)+', value)
                    or tuple(int(part) for part in value[6:].split('.')) > (2, 35)
                    for value in glibc_versions
                ):
                    raise SystemExit(f'{name} exceeds the glibc 2.35 baseline: {sorted(glibc_versions)}')
                dependencies = subprocess.check_output(
                    ['readelf', '--dynamic', '--wide', executable], text=True
                )
                libraries = re.findall(r'\(NEEDED\).*?\[(.*?)\]', dependencies)
                print(f'{name} glibc_versions={sorted(glibc_versions)} needed={libraries}')
        binary_path = Path(temporary_dir) / 'codex'
        helper_path = Path(temporary_dir) / 'codex-code-mode-host'
        reported_version = subprocess.check_output([binary_path, '--version'], text=True, timeout=30).strip()
        if reported_version != f'codex-cli {version}':
            raise SystemExit(f'Wrong binary version: {reported_version}; expected codex-cli {version}')
        help_output = subprocess.check_output([binary_path, '--help'], text=True, timeout=30)
        if not any(line.startswith('Usage:') for line in help_output.splitlines()):
            raise SystemExit('CLI help lacks Usage')
        helper_help = subprocess.check_output([helper_path, '--help'], text=True, timeout=30)
        if not any(line.startswith('Usage:') for line in helper_help.splitlines()):
            raise SystemExit('Code Mode host help lacks Usage')
        print(f'archive={archive_path} bytes={archive_path.stat().st_size} version={reported_version} help=ok helper_help=ok')
        print(f'binary_bytes={binary_path.stat().st_size} helper_bytes={helper_path.stat().st_size}')
