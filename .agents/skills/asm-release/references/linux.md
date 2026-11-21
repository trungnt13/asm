# Linux release builds

## Release PGO

Profile-guided optimization (PGO) uses execution data to guide the compiler.
LTO means link-time optimization.

| Build | CPU | LTO | Codegen units | PGO |
| --- | --- | --- | --- | --- |
| Linux CI | Portable x86-64 | Thin | 4 | Yes |
| Linux local | x86-64-v3 | Fat | 1 | Yes |
| macOS CI | Apple Silicon | Upstream | Upstream | No |

Leave Cargo profiles unchanged. Use one Cargo job for Linux PGO. Build Bubblewrap
separately without PGO. CI does not need training credentials or histories. Publish the indexed
profile as a dependency, not as a CLI release.

[`setup.py`](../../../../scripts/pgo/setup.py) fetches the profile pinned in
[`agent-linux.json`](../../../../scripts/pgo/agent-linux.json). Use Python 3.11+ and
`llvm-tools-preview` from the consuming compiler. Fail if the selected profile
is missing or incompatible. Training versions record provenance; they do not
gate reuse. Keep compiler matching logs and verify the package and real-provider
behavior. Recheck after compiler or source changes. Refresh the profile if
matching or performance declines. A readable profile does not prove a speedup.

For local use, run from `codex-rs`. Omit `--env-file` to validate only:

```bash
pgo_dir=$(mktemp -d)
python3 ../scripts/pgo/setup.py --target x86_64-unknown-linux-gnu \
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

The current workflow creates new candidates in its CI preparation job. To meet
this rule for new releases, change the workflow to accept a candidate pinned
before dispatch. Until then, report the dependency. Do not build from unpinned
`main` or create a separate version for v3.

Check glibc for full v3 support on the current machine. Do not infer support
from AVX2 alone. Build in a clean checkout of the pinned candidate. Use a
matching local tag. Do not push a tag to start the v3 build. Use Ubuntu 22.04,
natively or in a local container. Install the candidate toolchain, CI build
tools, `llvm-tools-preview`, and Python 3.11+ (set `PYTHON` if needed). Use
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
    --archive "$output/codex-x86_64-unknown-linux-gnu-v3.tar.gz"
```

Upload the archive first and its verified checksum file last. Do not replace
base assets or change Latest. The installer selects v3 only if glibc reports
full support and the selected release has the uploaded v3 checksum file.
Otherwise use the portable package. If an advertised v3 package fails
verification, stop. Install the variants in separate directories.

## Train the PGO profile

Use Linux with working Bubblewrap mount and PID namespaces. Use Python 3.11+,
authorized Codex sign-in, and an instrumented CLI with its matching helper.
Set `CODEX_HOME` to the source home and `PGO_INSTRUMENTED_BIN` to the CLI.
Set `PGO_TRAINING_DIR` to an empty, private directory outside tracked sources.
Run `python3 scripts/pgo/train.py`. Do not run it in release CI.

The harness copies feature settings and disables System1. Bubblewrap makes
host files read-only and permits writes to private session and profile paths.
It does not restrict host reads or network access. The harness checks the
effective SQLite and log paths before starting a turn. If managed settings
change either path, stop. The monitor checks these limits: 890 completions, 19.9M input
tokens, 245K output tokens, 60 minutes, or $4.90 API-equivalent. Actual charges
are unknown. Never publish training homes, histories, or credentials.

Verify 10/15/20 root turns and parallel child peaks of 4/3/2. Check the cheap
model, fixture results, and streaming. Merge only that run's
`profiles/*.profraw` with matching `llvm-profdata`. Publish the indexed result
and update both hashes and provenance. Keep failed runs separate.

The selected run peaked at eight simultaneous children. It does not cover all
production paths. The local Fat LTO build reached about 30 GiB of proportional
resident memory. The CI trial timed out at 330 minutes with about 22 GiB swap
in use. Keep runner
disk and swap checks. Require a successful full build before claiming runner
feasibility.
