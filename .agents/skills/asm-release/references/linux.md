# Linux release builds

## Release PGO

Profile-guided optimization (PGO) uses execution data to guide the compiler.
LTO means link-time optimization.

| Build | CPU | LTO | Codegen units | PGO |
| --- | --- | --- | --- | --- |
| Linux CI | Portable x86-64 musl | Thin | 4 | Yes |
| Linux local | x86-64-v3 musl | Fat | 1 | Yes |
| macOS CI | Apple Silicon | Upstream | Upstream | No |

Leave Cargo profiles unchanged. Use one Cargo job for Linux PGO. Build Bubblewrap
separately without PGO. CI does not need training credentials or histories. Publish the indexed
profile as a dependency, not as a CLI release.

[`setup.py`](../../../../scripts/pgo/setup.py) fetches the musl profile pinned
in `agent-linux-musl.json`. Use Python 3.11+ and
`llvm-tools-preview` from the consuming compiler. Fail if the selected profile
is missing or incompatible. Training versions record provenance; they do not
gate reuse. Keep compiler matching logs and verify the package and real-provider
behavior. Recheck after compiler or source changes. Refresh the profile if
matching or performance declines. A readable profile does not prove a speedup.

For local use, run from `codex-rs`. Omit `--env-file` to validate only:

```bash
pgo_dir=$(mktemp -d)
python3 ../scripts/pgo/setup.py --target x86_64-unknown-linux-musl \
  --output "$pgo_dir" --env-file "$pgo_dir/cargo.env" &&
  while IFS= read -r setting; do export "$setting"; done < "$pgo_dir/cargo.env"
```

Load assignments without evaluating shell code. Pass the same target to Cargo.
Keep the profile until the build ends.

## Linux v3 package

Pin one release version and immutable candidate commit before either build
starts. Use the same candidate for local v3 and GitHub Actions. Start v3 as soon
as its local prerequisites pass. Do not wait for CI dispatch, a runner, a tag
from CI, or a CI result. Start CI after its own checks. Do not wait for the v3
build or a skip decision. If v3 is blocked, report the blocker and ask the owner
whether to skip v3 for this release. Keep CI running.

In a clean task checkout of current `origin/main`, run the pin command. It
selects the version and commits only the workspace version files. Record the
full commit SHA and tag that it prints. Use that checkout for v3; do not build
from unpinned `main` or create a separate version for v3.

```bash
GITHUB_REPOSITORY=trungnt13/asm python3 .github/scripts/prepare-fork-release.py pin
git tag "$tag" "$sha"  # Local only; use a task-owned checkout.
```

Start the local build using the recipe below. To hand the same commit to the
standard workflow, push it without tags:

```bash
git push --no-follow-tags origin "$sha:refs/heads/agent/pin-release-$sha"
```

After the workflow records `agent/release-<run-id>` at the pinned SHA, the
transport branch is no longer needed. Never replace an existing pin or tag.

On the Ubuntu build host, check full v3 support with the glibc loader. Do not infer support
from AVX2 alone. Build in a clean checkout of the pinned candidate. Use a
matching local tag. Do not push a tag to start the v3 build. Use Ubuntu 22.04,
natively or in a local container. Install the candidate toolchain with the
`x86_64-unknown-linux-musl` target and `llvm-tools-preview`, Zig 0.14.0, Bazel
for the GNU voice runtime, and Python 3.11+ (set `PYTHON` if needed). Use
absolute paths. For `v8_dir`, supply the archive, bindings, checksum file, and
`release.json` from the release used by
[`setup-rusty-v8`](../../../../.github/actions/setup-rusty-v8/action.yml). The build uses
only tracked Cargo configuration and a private Cargo cache in `output`.
It ignores host compiler flags and uses the tag's Bubblewrap source.

```bash
just build-release-v3 "$tag" "$output" "$v8_dir"
```

Keep the archive and its `.provenance` file together. Run the release-source
live check before uploading. Before the first v3 upload, update and validate
`tngo-workflow`: its verification and rollback assume the portable directory.

After the base release is complete, upload v3 as part of the release request.
Use Docker and a `GH_TOKEN` with Actions read and Contents write access. Set `base_run_id` to
the successful release or resume run for this exact tag. Run this from the
same tagged checkout:

```bash
GITHUB_REPOSITORY=trungnt13/asm RELEASE_TAG="$tag" RELEASE_SHA="$(git rev-parse HEAD)" \
  python3 .github/scripts/publish-fork-release.py publish-v3 --base-run-id "$base_run_id" \
    --archive "$output/codex-x86_64-unknown-linux-musl-v3.tar.gz"
```

Upload the archive first and its verified checksum file last. Do not replace
base assets or change Latest. The installer selects v3 only if glibc reports
full support and the selected release has the uploaded v3 checksum file.
Otherwise use the portable musl package. The CPU probe failing on a host without the glibc loader
is also a portable selection. If an advertised v3 package fails
verification, stop. Install the variants in separate directories.

## Train the PGO profile

Use Linux with working Bubblewrap mount and PID namespaces. Use Python 3.11+,
authorized Codex sign-in, and a musl-instrumented CLI with its matching helper.
Profiles generated from a small local smoke can verify the instrumentation and
consumption pipeline but are not a substitute for a release workload.
Add `-Cprofile-generate=<private profile directory>` and
`-Cllvm-args=-pgo-instr-select=false` to the musl Rust flags. Rust 1.99 can
otherwise emit corrupt counters for select instructions. Set
`CFLAGS_x86_64_unknown_linux_musl=-fno-profile-generate` and
`CXXFLAGS_x86_64_unknown_linux_musl=-fno-profile-generate` for training so
`cc` does not mix Zig's instrumentation format with Rust's profiling runtime.
`setup.py` keeps the Rust counter setting and disables C/C++ profile use for
musl consumers. Merge with the training toolchain’s `llvm-profdata` and reject
any unreadable raw profile.
Set `CODEX_HOME` to the source home and `PGO_INSTRUMENTED_BIN` to the CLI.
Set `PGO_TRAINING_DIR` to an empty, private directory outside tracked sources.
Run `python3 scripts/pgo/train.py`. Do not run it in release CI.

The harness copies feature settings and disables System1. Bubblewrap makes
host files read-only and permits writes to private session and profile paths.
It does not restrict host reads or network access. The harness checks the
effective SQLite and log paths before starting a turn. If managed settings
change either path, stop. The monitor checks these limits: 890 completions, 100M input
tokens, 245K output tokens, 60 minutes, or $4.90 API-equivalent. Actual charges
are unknown. Never publish training homes, histories, or credentials.

Verify 10/15/20 root turns and parallel child peaks of 4/3/2. Check the cheap
model, fixture results, and streaming. Merge only that run's
`profiles/*.profraw` with matching `llvm-profdata`. Publish the indexed result
and update both hashes and provenance. Keep failed runs separate.

Use the profile manifest for measured concurrency and provenance. Training does not
cover every production path. Keep runner disk and swap checks, and require a
successful full build before claiming runner feasibility.
