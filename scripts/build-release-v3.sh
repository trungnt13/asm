#!/usr/bin/env bash
set -euo pipefail

tag=${1:?expected a release tag}
output=${2:?expected a local output directory}
v8_dir=${3:?expected a directory with the fork V8 assets and release.json}
python=${PYTHON:-python3}
target=x86_64-unknown-linux-gnu
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

if [[ "$(uname -s):$(uname -m)" != Linux:x86_64 || "$(getconf GNU_LIBC_VERSION)" != "glibc 2.35" ]]; then
  echo 'Build on x86-64 Ubuntu 22.04 (glibc 2.35), natively or in a local container.' >&2
  exit 1
fi
if ! LC_ALL=C /lib64/ld-linux-x86-64.so.2 --help | grep -F 'x86-64-v3 (supported, searched)' >/dev/null; then
  echo 'The build machine must expose the full x86-64-v3 feature set.' >&2
  exit 1
fi
sha=$(git rev-parse --verify "refs/tags/${tag}^{commit}")
if [[ "$(git rev-parse HEAD)" != "$sha" || -n "$(git status --porcelain --untracked-files=no)" ]]; then
  echo "Build from the clean checkout of $tag ($sha)." >&2
  exit 1
fi
version=$("$python" -I -c 'import tomllib; print(tomllib.load(open("codex-rs/Cargo.toml", "rb"))["workspace"]["package"]["version"])')
if [[ "$tag" != "v$version" ]]; then
  echo "Release tag $tag does not match Cargo version $version." >&2
  exit 1
fi

[[ "$output" = /* && "$v8_dir" = /* ]] || { echo 'Use absolute output and V8 paths.' >&2; exit 1; }
mkdir -p -- "$output"
output=$(cd -- "$output" && pwd -P)
exec 9>"$output/build.lock"
flock -n 9
name="codex-${target}-v3.tar.gz"
if [[ -e "$output/$name" ]]; then
  echo "Preserving existing archive: $output/$name" >&2
  exit 1
fi

# Cargo also loads config from the checkout's ancestors and CARGO_HOME.
cargo_home="$output/cargo-home"
for config in "$cargo_home"/config{,.toml}; do
  if [[ -e "$config" || -L "$config" ]]; then
    echo "Remove local Cargo configuration from the release build: $config" >&2
    exit 1
  fi
done
directory="$root/codex-rs"
while :; do
  for config in "$directory"/.cargo/config{,.toml}; do
    if [[ -e "$config" || -L "$config" ]]; then
      if [[ "$config" != "$root/"* ]] || ! git ls-files --error-unmatch -- "$config" >/dev/null 2>&1; then
        echo "Release build cannot use untracked Cargo configuration: $config" >&2
        exit 1
      fi
    fi
  done
  [[ "$directory" == / ]] && break
  directory=$(dirname -- "$directory")
done

python=$(command -v "$python")
rustup=$(command -v rustup)
toolchain=$("$python" -I -c 'import tomllib; print(tomllib.load(open("codex-rs/rust-toolchain.toml", "rb"))["toolchain"]["channel"])')
build_env=(
  env -i "HOME=$HOME" "PATH=$(dirname -- "$rustup"):/usr/bin:/bin"
  "RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}" "RUSTUP_TOOLCHAIN=$toolchain"
  "CARGO_HOME=$cargo_home" "CARGO_TARGET_DIR=$output/target" CARGO_NET_GIT_FETCH_WITH_CLI=true
  CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_BUILD_JOBS=1
  CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO=off CARGO_PROFILE_RELEASE_STRIP=debuginfo
  "RUSTFLAGS=-C target-cpu=x86-64-v3" CC=/usr/bin/cc CXX=/usr/bin/c++
  CFLAGS=-march=x86-64-v3 CXXFLAGS=-march=x86-64-v3
  "CODEX_BWRAP_SOURCE_DIR=$root/codex-rs/vendor/bubblewrap"
  "RUSTY_V8_ARCHIVE=$v8_dir/librusty_v8_ptrcomp_sandbox_release_${target}.a.gz"
  "RUSTY_V8_SRC_BINDING_PATH=$v8_dir/src_binding_ptrcomp_sandbox_release_${target}.rs"
  AWS_LC_SYS_NO_JITTER_ENTROPY=1 AWS_LC_SYS_NO_JITTER_ENTROPY_x86_64_unknown_linux_gnu=1
)

v8_version=$("${build_env[@]}" "$python" .github/scripts/rusty_v8_bazel.py resolved-v8-crate-version)
"${build_env[@]}" "$python" .github/scripts/verify-fork-v8.py "$v8_dir/release.json" "$v8_dir" "$target" "$v8_version"

cd codex-rs
echo "Building $tag at $sha: $target, x86-64-v3, Fat LTO, 1 codegen unit, PGO"
: > "$output/pgo.env"
"${build_env[@]}" "$python" ../scripts/pgo/setup.py --target "$target" --lto fat --output "$output/pgo" --env-file "$output/pgo.env"
"${build_env[@]}" "$python" ../scripts/local-cargo.py build --locked --release --target "$target" --bin bwrap 2>&1 | tee "$output/bwrap.log"
release_dir="$output/target/$target/release"
digest=$(sha256sum "$release_dir/bwrap")
mapfile -t pgo_env < "$output/pgo.env"
"${build_env[@]}" "${pgo_env[@]}" "CODEX_BWRAP_SHA256=${digest%% *}" \
  "$python" ../scripts/local-cargo.py build --locked --release --target "$target" \
  --bin codex --bin codex-code-mode-host --bin codex-exec-server --timings 2>&1 | tee "$output/build.log"

mkdir -p "$output/staging"
archive="$output/staging/$name"
"${build_env[@]}" "CODEX_REPO_ROOT=$root" "$python" ../scripts/build_codex_package.py --force --target "$target" --cargo-profile release \
  --entrypoint-bin "$release_dir/codex" --code-mode-host-bin "$release_dir/codex-code-mode-host" \
  --exec-server-bin "$release_dir/codex-exec-server" --bwrap-bin "$release_dir/bwrap" \
  --package-dir "$output/staging/package" --archive-output "$archive"
"${build_env[@]}" "$python" ../.github/scripts/smoke-codex-archive.py "$archive" | tee "$output/smoke.log"
digest=$(sha256sum "$archive")
printf '%s\n%s\n' "$sha" "${digest%% *}" > "$archive.provenance"
mv -- "$archive.provenance" "$output/$name.provenance"
mv -- "$archive" "$output/$name"
printf 'Source: %s (%s)\n%s  %s\n' "$tag" "$sha" "${digest%% *}" "$output/$name"
