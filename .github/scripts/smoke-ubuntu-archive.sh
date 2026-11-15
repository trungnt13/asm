#!/usr/bin/env bash
set -euo pipefail

archive="${1:?absolute archive path is required}"
version="${2:?expected Cargo version is required}"
[[ "$archive" == /* && -f "$archive" ]] || { echo "Expected an absolute archive path: $archive" >&2; exit 1; }

# Use the same static package on glibc and musl hosts without installing runtime libraries.
# The caller has already validated the complete package and its ELF linkage.
for image in ubuntu:22.04 alpine:3.22; do
  docker run --rm --platform linux/amd64 \
    --mount "type=bind,source=${archive},target=/archive.tar.gz,readonly" \
    "$image" sh -eu -c '
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
      reported_version=$("$directory/bin/codex" --version)
      [ "$reported_version" = "codex-cli $1" ] || { echo "Wrong binary version: $reported_version" >&2; exit 1; }
      [ "$("$directory/bin/codex-exec-server" --version)" = "codex-exec-server $1" ]
      echo "host=$2 version=$reported_version help=ok helper_help=ok exec_server_version=ok exec_server_help=ok resources=ok"
    ' smoke-ubuntu-archive "$version" "$image"
done
