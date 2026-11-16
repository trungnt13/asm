#!/usr/bin/env bash
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_OS:-}" == Linux ]] || { echo 'Only run on an ephemeral Linux Actions runner' >&2; exit 1; }
# These preinstalled SDKs are unused by the native GNU release build.
sudo rm -rf /usr/share/dotnet /usr/local/lib/android /opt/ghc /opt/hostedtoolcache/CodeQL
available=$(df --output=avail -B1 "$RUNNER_TEMP" | tail -1)
swap_bytes=$((32 * 1024 * 1024 * 1024))
reserve_bytes=$((12 * 1024 * 1024 * 1024))
if (( available < swap_bytes + reserve_bytes )); then swap_bytes=$((24 * 1024 * 1024 * 1024)); fi
(( available >= swap_bytes + reserve_bytes )) || { echo 'Insufficient disk for PGO swap plus build reserve' >&2; df -h; exit 1; }
swapfile="$RUNNER_TEMP/asm-pgo.swap"
sudo fallocate -l "$swap_bytes" "$swapfile"
sudo chmod 600 "$swapfile"
sudo mkswap "$swapfile"
sudo swapon "$swapfile"
free -h
df -h
