#!/usr/bin/env bash
set -euo pipefail

archive="${1:?absolute archive path is required}"
version="${2:?expected Cargo version is required}"
[[ "$archive" == /* && -f "$archive" ]] || { echo "Expected an absolute archive path: $archive" >&2; exit 1; }

expected_version="asm (PGO) $version"
case "${archive##*/}" in
  codex-x86_64-unknown-linux-musl.tar.gz | codex-x86_64-unknown-linux-musl-v3.tar.gz) target=musl ;;
  codex-x86_64-unknown-linux-gnu.tar.gz | codex-x86_64-unknown-linux-gnu-v3.tar.gz) target=gnu ;;
  *) echo "Unsupported Linux archive: $archive" >&2; exit 1 ;;
esac
if [[ "${archive##*/}" == *-v3.tar.gz ]]; then expected_version="asm (x86-64-v3, PGO) $version"; fi

# The caller validates the package structure before either container extracts it.
# shellcheck disable=SC2016
smoke='
    image=$3
    target=$4
    if [ "$target" = gnu ]; then
      apt-get update
      DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        ca-certificates libssl3 liblzma5 libcap2 libgcc-s1 libstdc++6
    fi
    directory=$(mktemp -d)
    tar -xzf /archive.tar.gz -C "$directory"
    for binary in bin/codex bin/codex-code-mode-host bin/codex-exec-server; do
      if [ "$target" = gnu ]; then ldd "$directory/$binary"; fi
      help_output=$("$directory/$binary" --help)
      printf "%s\n" "$help_output" | grep -q "^Usage:"
    done
    if [ "$target" = gnu ]; then ldd "$directory/codex-resources/bwrap"; fi
    "$directory/codex-resources/bwrap" --help > "$directory/bwrap-help"
    for option in --argv0 --perms --as-pid-1; do
      grep -q -- "$option" "$directory/bwrap-help"
    done
    "$directory/codex-path/rg" --version
    if [ "$target" = musl ]; then
      voice="$directory/codex-resources/voice/bin/codex-voice-host"
      if [ "$image" = ubuntu:22.04 ]; then
        ldd "$voice"
        [ "$("$voice" --build-commit)" = "$5" ]
        echo "voice=gnu-helper-started"
      else
        [ ! -e /lib64/ld-linux-x86-64.so.2 ]
        [ -x "$voice" ]
        if "$voice" --build-commit 2>/dev/null; then
          echo "GNU voice unexpectedly ran without the glibc loader" >&2
          exit 1
        fi
        echo "voice=unavailable-without-glibc"
      fi
    fi
    reported_version=$("$directory/bin/codex" --version)
    [ "$reported_version" = "$2" ] || { echo "Wrong binary version: $reported_version" >&2; exit 1; }
    [ "$("$directory/bin/codex-exec-server" --version)" = "codex-exec-server $1" ]
    echo "image=$image version=$reported_version help=ok helper_help=ok exec_server_version=ok exec_server_help=ok resources=ok"
  '
mount="type=bind,source=${archive},target=/archive.tar.gz,readonly"
if [[ "$target" == musl ]]; then
  commit=$(git -C "$(dirname "$0")/../.." rev-parse HEAD)
  for image in ubuntu:22.04 alpine:3.22; do
    docker run --rm --platform linux/amd64 --network none --mount "$mount" \
      "$image" sh -eu -c "$smoke" smoke-linux-archive "$version" "$expected_version" "$image" "$target" "$commit"
  done
else
  docker run --rm --platform linux/amd64 --mount "$mount" \
    ubuntu:22.04 bash -euo pipefail -c "$smoke" smoke-linux-archive "$version" "$expected_version" ubuntu:22.04 "$target"
fi
