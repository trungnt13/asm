#!/usr/bin/env bash
set -euo pipefail

archive="${1:?absolute archive path is required}"
version="${2:?expected Cargo version is required}"
[[ "$archive" == /* && -f "$archive" ]] || { echo "Expected an absolute archive path: $archive" >&2; exit 1; }

# A hosted runner has extra libraries; check the archive on the minimum OS too.
# The caller has already validated the archive's two regular executable members.
docker run --rm --platform linux/amd64 \
  --mount "type=bind,source=${archive},target=/archive.tar.gz,readonly" \
  ubuntu:22.04 bash -euo pipefail -c '
    apt-get update
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      ca-certificates libssl3 liblzma5 libgcc-s1 libstdc++6
    directory=$(mktemp -d)
    tar -xzf /archive.tar.gz -C "$directory"
    for binary in codex codex-code-mode-host; do
      ldd "$directory/$binary"
      help_output=$("$directory/$binary" --help)
      grep -q "^Usage:" <<< "$help_output"
    done
    reported_version=$("$directory/codex" --version)
    [[ "$reported_version" == "codex-cli $1" ]] || { echo "Wrong binary version: $reported_version" >&2; exit 1; }
    echo "ubuntu=22.04 version=$reported_version help=ok helper_help=ok"
  ' smoke-ubuntu-archive "$version"
