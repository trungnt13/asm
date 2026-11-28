# Release contract

## Version

- Select the highest upstream `rust-v` prerelease tag by Semantic Versioning, regardless of creation/publication time or publication. Increment only its numeric patch by one; retain suffix: `rust-v0.162.0-alpha.18` → `v0.162.1-alpha.18`.
- For repeats, append one positive numeric counter: highest used plus one, without gaps. Preserve upstream identifiers; put the counter before build metadata. `alpha.18.1` precedes `alpha.18.2`; new upstream `alpha.19` starts `v0.162.1-alpha.19`. Installer accepts repeated numeric alpha/beta parts.
- Resume unfinished musl candidates/releases first. Prove complete published assets. For the highest musl release, also prove publication and both native checks for the exact commit. Require main sync for registered candidates; successful resumes count. Older release tags still determine version ordering. If current release completion is unproved, no upstream prerelease exists, or the proposed version is not above every fork version, stop and ask.
- Create immutable `agent/release-<run-id>`, not a `main` version commit. Set `[workspace.package].version` in [`Cargo.toml`](../../../../codex-rs/Cargo.toml) and matching [`Cargo.lock`](../../../../codex-rs/Cargo.lock) workspace versions. Do not change dependencies.
- Tag, Cargo, and CLI `--version` must agree except prefixes. Tag renames do not change binary bytes. Never overwrite tags/assets or move candidates/tags on retries. Keep candidate branches for recovery and pre-policy bumps until the next successful release.

## Build and package

Keep fork/upstream release workflows separate. Use GitHub-hosted runners:

| Platform | Target | Runner | Build timeout |
| --- | --- | --- | --- |
| macOS ARM64 | `aarch64-apple-darwin` | `macos-15` | 300 minutes |
| Linux x86_64 | `x86_64-unknown-linux-musl` | `ubuntu-22.04` | 330 minutes |

- Except for direct tag builds, allow 15 minutes beyond the build for setup,
  packaging, smoke checks, and uploads. Normal job limits are 315 minutes on
  macOS and 345 on Linux. For macOS postmerge and concurrency experiments, allow
  180 minutes to build and 195 minutes per job. Preserve optimization, flags,
  runner, and candidate on retries. Do not replace ready assets.
- Direct tag builds allow 190 minutes for lookup and 360 total minutes. Artifact lookup waits at most 150 minutes on macOS or 185 on Linux. A timeout fails; do not duplicate builds. Actual builds retain table limits; fresh candidates do not wait for postmerge.
- Keep Ubuntu 22.04 as the Linux build and acceptance baseline and test portable core execution on Alpine. Use a container before the 22.04 runner retires. The core must not require a libc loader.
- Use optimized `--release`, not `dev-small`. Inherit `debug = "none"` and `strip = true` for standard and v3 packages; do not archive separate debug symbols. Fix timeouts without weakening optimization. Keep shell commands compatible with macOS Bash 3.2, including `set -u` array expansion.
- Build `codex`, `codex-code-mode-host`, and `codex-exec-server` from one commit/target/profile. Pass all three prebuilt executables to canonical packaging. `codex-<target>.tar.gz` contains `bin/codex`, `bin/codex-code-mode-host`, `bin/codex-exec-server`, `codex-package.json`, and `codex-path/rg`. Linux also contains `codex-resources/bwrap`, built from matching source/release profile; embed its final SHA-256 in the CLI before packaging.
- Keep canonical layout/pinned ripgrep manifest. Include only required resources as regular executables with matching version/target metadata, including `execServer: "bin/codex-exec-server"`. Shared-server startup must recognize/validate the package without host helpers.
- Publish six required assets: two archives, two `SHA256SUMS-<target>` files,
  aggregate `SHA256SUMS`, and [`install.sh`](../../../../scripts/install/install.sh).
  Each platform checksum file covers its archive and pinned installer. Do not
  publish source trees or logs.
- Detect candidate contract from pinned smoke's required `codex-package.json`, not optional resource flags. Pass `--no-zsh` only if supported.
- Pin builds, packaging, installer bytes, checks, and reuse to candidate SHA. Pin orchestration to dispatch SHA; save controls before checkout. Never fetch newer controls mid-run. Tag builds use the verified exact tagged SHA. Candidate packaging may precede tagging.
- Before upload, run [native smoke](../../../../.github/scripts/smoke-codex-archive.py). CLI/exec-server versions match Cargo; all three help commands succeed with usage; manifest/resources and bundled ripgrep/Bubblewrap pass. Code-mode-host `--version` is not required. Record binary/archive sizes.
- Linux core executables, bundled Bubblewrap, and ripgrep must have no ELF interpreter, shared-library requirements, or GLIBC version imports. Verify the same package on clean Ubuntu 22.04 and Alpine. The GNU voice helper and its runtime are a separate dependency closure; verify them on Ubuntu and do not require them for CLI startup.
- No Apple signing/notarization, paid Apple membership, Azure Key Vault, release secrets, or self-hosted runners. Use the system shell; no patched zsh or unrelated helper bundles. Linux packages include the same-commit GNU voice helper and verified native runtime; macOS stays unchanged. Preserve sandbox/security defaults.
- Build new Linux packages with [upstream musl tools](../../../../.github/scripts/install-musl-build-tools.sh) and Zig 0.14.0; add `-Clink-arg=-Wl,--no-dynamic-linker` to get static PIE. Build the GNU voice helper in its own environment. Retain `AWS_LC_SYS_NO_JITTER_ENTROPY` and verify the target-matched V8 dependency through [`setup-rusty-v8`](../../../../.github/actions/setup-rusty-v8/action.yml). Install Python 3.11+ on Ubuntu 22.04.
- Keep the existing musl jemalloc arena ratio and the Linux Tokio worker cap. Measure allocator changes before changing either default. Use musl only for Linux CLI release assets. Preserve already-published assets without consuming obsolete Linux CLI builds.
- Agree intent before adding platforms, DMGs, optional resources, npm, R2, WinGet, or website/OpenAI-only publishing.

## V8 dependency release

Build V8 separately only when version/target/baseline lacks a verified fork release. Use [`fork-v8-release.yml`](../../../../.github/workflows/fork-v8-release.yml) and `asm-v8-v<exact resolved v8 crate version>-musl.1` for Linux musl. Keep the matching macOS pair in `asm-v8-v<version>-glibc2.35.2`.

- Manual branch dispatch defaults to build-only. Explicit `publish=true` builds/smokes, then tags/publishes at that commit. Tag pushes also publish. Tag moves or published-asset replacement need approval.

- Reuse unchanged verified inputs. Same-version archive reuse for Cargo rebases/releases has standing approval only when independent review proves Cargo ABI, bindings, and full dependency-closure compatibility. Native consumer build/JavaScript/sandbox checks must pass. Record omitted source-recipe fixes; never claim source equivalence. Ask for incompatible dependencies/bindings, changed baselines, unsafe reuse, or ambiguous intent. Never replace published assets.
- Do not reuse V8 152.2.0's original `glibc2.35` pair: CppGC sizes and typed-array threshold bindings are stale. Preserve its assets. `glibc2.35.2` rebuilds matching source-generated archives/bindings with Chromium C++ runtime isolation.
- Build only optimized sandbox/pointer-compression pairs for macOS ARM64/Linux x86_64 musl. Use existing Bazel source pair/staging helper locally on GitHub-hosted runners. No BuildBuddy, remote execution, paid infrastructure, or broad suites. Retain required static-library symbols.
- Require both native `codex-v8-poc` sandbox/JavaScript smokes, including ignored CppGC ABI checks on the selected target. Use the archive for the exact consuming target.
- Publish each target's archive, Rust binding, and two-file manifest only. Use a normal release, **not Latest**, without installer; keep it separate from CLI discovery.
- Before the first consuming `main` push, publish/verify both new version/generation pairs. CI/CLI releases use only matching verified fork V8. Verify GitHub SHA-256 digests and exact manifest names/hashes. Missing/bad assets fail without upstream fallback. Preserve upstream action paths for other workflows.

## Reuse and publish

Serialize releases. Finish without later chat actions or token-push-triggered workflows. Require publication approval before pushing any release-triggering tag, including inherited tags. Release tags must already contain correct Cargo versions.

Use [artifact lookup](../../../../.github/scripts/find-postmerge-artifacts.sh) per target for unexpired smoke-checked archives from current dispatch/original candidate run. Tag builds may reuse successful same-repo `main` push target jobs at the exact tagged SHA. Wait for matching active runs, not other platforms. Never duplicate builds or rebuild ready published platforms. Timeouts/API errors fail; API errors are not cache misses. Build only without usable archives.

1. Create the pinned tag, draft, and common installer once. Resume uploads only when names, sizes, and digests match.
2. Independently build/reuse/check/upload each platform. Verify GitHub archive/installer digests; smoke the downloaded archive. Upload `SHA256SUMS-<target>` last as readiness marker.
3. At first readiness, publish non-draft normal release, including `alpha`, **not Latest** while incomplete. Append other checked assets. Keep payloads immutable by policy. GitHub's immutable-release setting must stay disabled for append-only publication; never change it automatically.
4. After both platforms pass, append aggregate manifest for old installers. Verify six assets, mark Latest, and retain both native checks. Only then merge candidate into `origin/main`. Preserve concurrent changes; stop on version conflicts. Prefer fast-forwards; necessary rewrites follow push permissions.
5. Verify ready platforms; resume only missing work. For archives uploaded before readiness markers, recover/verify exact public bytes, even after Actions expiry. Do not rebuild them. Partial releases are incomplete. Leave previously published assets unchanged.

Before first partial release, deploy platform-aware `tngo-workflow/setup.py`. Select the highest published host-ready version; macOS must not select Linux-only. Installer defaults to Latest; explicit `--release` may install an earlier ready platform. Preserve frozen policy for in-flight runs.

## Installer

- Use only `trungnt13/asm` GitHub `v*` releases for macOS ARM64/Linux x86_64 musl. Before installing the complete package, verify GitHub digests and host `SHA256SUMS-<target>`. Only Apple Silicon macOS may use aggregate `SHA256SUMS` when the host manifest is absent, never after invalid digests/manifests.
- Reject incomplete two-binary archives. Require a declared `execServer` and its exact regular executable on Linux. On Apple Silicon macOS, accept older complete packages without the declaration only when no exec-server is present.
- Reject unsupported architectures. The Linux CLI installs and runs without glibc. A Linux release without musl assets is not installable. Preserve immutable assets, target-qualified package directories, and shared config/state. Do not fall back on invalid advertised checksums.
- Preserve `--release`, `CODEX_HOME`, `CODEX_INSTALL_DIR`, locking, and safe `current` selection. Store in `packages/asm-standalone`. Preserve upstream packages/update markers, shared Codex config/state, and `codex` command.
- Only the owner installs externally. No OpenAI/CDN/legacy npm downloads, daemon-only installs, or `auto-update-version` writes. Do not port upstream standalone-release pruning. Preserve fork packages/layout.
- Disable built-in update checks/prompts/commands/daemon loops regardless of settings/markers. CLI/daemon must not install/download upstream releases. Preserve ordinary daemon startup/restart separately.

## Build experiment

Optional `macos_concurrency_experiment` compares cold same-commit `macos-15` builds with 2/3 Cargo jobs and otherwise equal settings. Save separate timing/resource/size/smoke results. No postmerge reuse or publication, even on tags. Measure before changing defaults; one run does not prove general speedup.
