#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "Usage: $0 <base package directory> <release version> <archive output>" >&2
  exit 2
fi
base=$(realpath -e -- "$1")
version=$2
archive=$(realpath -m -- "$3")
if [[ -e "$3" || -L "$3" || "$archive" == "$base" || "$archive" == "$base/"* ]]; then
  echo 'Voice archive output must be new and outside the base package' >&2
  exit 1
fi

python=${PYTHON:-python3}
voice_target=x86_64-unknown-linux-gnu
commit=$(git rev-parse --verify HEAD)
stage=$(mktemp -d "${RUNNER_TEMP:-/tmp}/asm-linux-voice.XXXXXX")
trap 'rm -rf -- "$stage"' EXIT

# Voice stays on the upstream GNU runtime even though the app targets musl.
STABLE_GIT_COMMIT="$commit" "$python" .github/scripts/run_bazel.py build --config=release \
  //codex-rs/voice-host:codex-voice-host //third_party/voice:native_runtime
"$python" third_party/voice/release_runtime.py stage --target "$voice_target" \
  --source bazel-bin/third_party/voice/native_runtime_linux_x86_64 --output "$stage/runtime"
"$python" third_party/voice/release_runtime.py seal --target "$voice_target" --output "$stage/runtime"
"$python" third_party/voice/assemble_package.py --package "$base" \
  --helper bazel-bin/codex-rs/voice-host/codex-voice-host --runtime "$stage/runtime" \
  --voice-target "$voice_target" --build-commit "$commit" --release-version "$version" \
  --output "$stage/package"

if [[ "$("$stage/package/codex-resources/voice/bin/codex-voice-host" --build-commit)" != "$commit" ]]; then
  echo 'Packaged voice helper was not built from this commit' >&2
  exit 1
fi
CODEX_REPO_ROOT="$PWD" PYTHONPATH="$PWD/scripts" "$python" - "$stage/package" "$archive" <<'PY'
import sys
from pathlib import Path

from codex_package.archive import write_archive

write_archive(Path(sys.argv[1]), Path(sys.argv[2]), force=False)
PY
