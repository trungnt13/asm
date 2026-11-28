#!/usr/bin/env bash
set -euo pipefail

archive="${1:?absolute archive path is required}"
version="${2:?expected Cargo version is required}"
[[ "$archive" == /* && -f "$archive" ]] || { echo "Expected an absolute archive path: $archive" >&2; exit 1; }

case "${archive##*/}" in
  codex-x86_64-unknown-linux-musl.tar.gz) expected_version="asm (PGO) $version" ;;
  codex-x86_64-unknown-linux-musl-v3.tar.gz) expected_version="asm (x86-64-v3, PGO) $version" ;;
  *) echo "Unsupported Linux archive: $archive" >&2; exit 1 ;;
esac

# The caller validates the package structure before either container extracts it.
# shellcheck disable=SC2016
smoke='
    image=$3
    directory=$(mktemp -d)
    tar -xzf /archive.tar.gz -C "$directory"
    for binary in bin/codex bin/codex-code-mode-host bin/codex-exec-server; do
      help_output=$("$directory/$binary" --help)
      printf "%s\n" "$help_output" | grep -q "^Usage:"
    done
    "$directory/codex-resources/bwrap" --help > "$directory/bwrap-help"
    for option in --argv0 --perms --as-pid-1; do
      grep -q -- "$option" "$directory/bwrap-help"
    done
    "$directory/codex-path/rg" --version
    voice="$directory/codex-resources/voice/bin/codex-voice-host"
    if [ "$image" = ubuntu:22.04 ]; then
      ldd "$voice"
      [ "$("$voice" --build-commit)" = "$4" ]
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
    reported_version=$("$directory/bin/codex" --version)
    [ "$reported_version" = "$2" ] || { echo "Wrong binary version: $reported_version" >&2; exit 1; }
    [ "$("$directory/bin/codex-exec-server" --version)" = "codex-exec-server $1" ]
    echo "image=$image version=$reported_version help=ok helper_help=ok exec_server_version=ok exec_server_help=ok resources=ok"
  '
mount="type=bind,source=${archive},target=/archive.tar.gz,readonly"
commit=$(git -C "$(dirname "$0")/../.." rev-parse HEAD)
for image in ubuntu:22.04 alpine:3.22; do
  docker run --rm --platform linux/amd64 --network none --mount "$mount" \
    "$image" sh -eu -c "$smoke" smoke-linux-archive "$version" "$expected_version" "$image" "$commit"
done
