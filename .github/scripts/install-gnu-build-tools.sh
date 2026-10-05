#!/usr/bin/env bash
set -euo pipefail

# Build against Ubuntu 22.04's system libraries, not a newer glibc sysroot.
sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
  build-essential binutils pkg-config libssl-dev libcap-dev \
  clang lld cmake
