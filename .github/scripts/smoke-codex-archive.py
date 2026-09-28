import os
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
