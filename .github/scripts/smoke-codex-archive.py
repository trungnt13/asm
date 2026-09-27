import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
from pathlib import Path

archive_path = Path(sys.argv[1])
version = tomllib.loads(Path('Cargo.toml').read_text())['workspace']['package']['version']
with tarfile.open(archive_path, 'r:gz') as archive:
    members = archive.getmembers()
    if (
        len(members) != 1
        or members[0].name != 'codex'
        or not members[0].isfile()
        or members[0].size == 0
        or not members[0].mode & 0o111
    ):
        raise SystemExit(f'Invalid CLI archive: {archive_path}')
    with tempfile.TemporaryDirectory() as temporary_dir:
        binary_path = Path(temporary_dir) / 'codex'
        with archive.extractfile(members[0]) as source, binary_path.open('wb') as target:
            shutil.copyfileobj(source, target)
        os.chmod(binary_path, 0o700)
        reported_version = subprocess.check_output([binary_path, '--version'], text=True, timeout=30).strip()
        if reported_version != f'codex-cli {version}':
            raise SystemExit(f'Wrong binary version: {reported_version}; expected codex-cli {version}')
        help_output = subprocess.check_output([binary_path, '--help'], text=True, timeout=30)
        if not any(line.startswith('Usage:') for line in help_output.splitlines()):
            raise SystemExit('CLI help lacks Usage')
        print(f'archive={archive_path} bytes={archive_path.stat().st_size} version={reported_version} help=ok')
        print(f'binary_bytes={binary_path.stat().st_size}')
