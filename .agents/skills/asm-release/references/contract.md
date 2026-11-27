# Release contract

## Version

- Select the highest upstream `rust-v` prerelease tag by Semantic Versioning, regardless of creation/publication time or publication. Increment only its numeric patch by one; retain suffix: `rust-v0.162.0-alpha.18` → `v0.162.1-alpha.18`.
- For repeats, append one positive numeric counter: highest used plus one, without gaps. Preserve upstream identifiers; put the counter before build metadata. `alpha.18.1` precedes `alpha.18.2`; new upstream `alpha.19` starts `v0.162.1-alpha.19`. Installer accepts repeated numeric alpha/beta parts.
- Resume unfinished candidates/releases first. Prove complete published assets. For the current family and highest fork version, also prove release, both native checks, and main sync for the exact candidate; successful resumes count. Older candidates need no expired Actions logs. If completion is unproved, no upstream prerelease exists, or the proposed version is not above every fork version, stop and ask.
- Create immutable `agent/release-<run-id>`, not a `main` version commit. Set `[workspace.package].version` in [`Cargo.toml`](../../../../codex-rs/Cargo.toml) and matching [`Cargo.lock`](../../../../codex-rs/Cargo.lock) workspace versions. Do not change dependencies.
- Tag, Cargo, and CLI `--version` must agree except prefixes. Tag renames do not change binary bytes. Never overwrite tags/assets or move candidates/tags on retries. Keep candidate branches for recovery and pre-policy bumps until the next successful release.

## Build and package

Keep fork/upstream release workflows separate. Use GitHub-hosted runners:

| Platform | Target | Runner | Build timeout |
| --- | --- | --- | --- |
| macOS ARM64 | `aarch64-apple-darwin` | `macos-15` | 300 minutes |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | 330 minutes |

- Except for legacy tag jobs, allow 15 minutes beyond the build for setup,
  packaging, smoke checks, and uploads. Normal job limits are 315 minutes on
  macOS and 345 on Linux. For macOS postmerge and concurrency experiments, allow
  180 minutes to build and 195 minutes per job. Preserve optimization, flags,
  runner, and candidate on retries. Do not replace ready assets.
- Legacy lookup allows 190 minutes; wait guards are macOS 150/Linux 185. Timeout fails; do not duplicate builds. Only legacy tag jobs allow 360 total minutes. Actual builds retain table limits; fresh candidates do not wait for postmerge.
- Keep Ubuntu 22.04 / glibc 2.35 as the Linux minimum and build/native-smoke baseline. A GNU triple on newer runners is insufficient. Before April 2027 retirement, use an Ubuntu 22.04 container on supported runners. Never raise the minimum.
- Use optimized `--release`, not `dev-small`. Inherit `strip = true` for standard and v3 packages; do not archive separate debug symbols. Keep debuginfo-only stripping when rebuilding older pinned candidates. Fix timeouts without weakening optimization. Keep shell commands compatible with macOS Bash 3.2, including `set -u` array expansion.
- Build `codex`, `codex-code-mode-host`, and `codex-exec-server` from one commit/target/profile. Pass all three prebuilt executables to canonical packaging. `codex-<target>.tar.gz` contains `bin/codex`, `bin/codex-code-mode-host`, `bin/codex-exec-server`, `codex-package.json`, and `codex-path/rg`. Linux also contains `codex-resources/bwrap`, built from matching source/release profile; embed its final SHA-256 in the CLI before packaging.
- Keep canonical layout/pinned ripgrep manifest. Include only required resources as regular executables with matching version/target metadata, including `execServer: "bin/codex-exec-server"`. Shared-server startup must recognize/validate the package without host helpers.
- Publish six required assets: two archives, two `SHA256SUMS-<target>` files,
  aggregate `SHA256SUMS`, and [`install.sh`](../../../../scripts/install/install.sh).
  Each platform checksum file covers its archive and pinned installer. Do not
  publish source trees or logs.
- Detect candidate contract from pinned smoke's required `codex-package.json`, not optional resource flags. Pass `--no-zsh` only if supported. Preserve older candidates' original contracts.
- Pin builds, packaging, installer bytes, checks, and reuse to candidate SHA. Pin orchestration to dispatch SHA; save controls before checkout. Never fetch newer controls mid-run. Legacy uses verified exact tagged SHA. Candidate packaging may precede tagging.
- Before upload, run [native smoke](../../../../.github/scripts/smoke-codex-archive.py). CLI/exec-server versions match Cargo; all three help commands succeed with usage; manifest/resources and bundled ripgrep/Bubblewrap pass. Code-mode-host `--version` is not required. Record binary/archive sizes.
- GNU source-built ELF requires x86_64 GNU loader and glibc symbols no newer than 2.35. Pinned ripgrep may be static. Check architecture/minimum-OS execution; run version/help in a clean Ubuntu 22.04 container with only declared runtime libraries. Runners alone do not prove compatibility.
- No Apple signing/notarization, paid Apple membership, Azure Key Vault, release secrets, or self-hosted runners. Bundle only required ripgrep/Linux Bubblewrap; use system shell. No voice, patched zsh, or optional helpers. Preserve sandbox/security defaults.
- Fork Linux uses native GNU and [`install-gnu-build-tools.sh`](../../../../.github/scripts/install-gnu-build-tools.sh), not Zig/MUSL wrappers. Preserve inherited MUSL tooling. Retain `AWS_LC_SYS_NO_JITTER_ENTROPY` and verified V8 through [`setup-rusty-v8`](../../../../.github/actions/setup-rusty-v8/action.yml). Install Python 3.11+ explicitly on Ubuntu 22.04.
- Preserve upstream GNU system allocator, locale, and PTY choices; do not mimic MUSL in Rust conditions. Use system OpenSSL 3; system liblzma is permitted. Match [`smoke-ubuntu-archive.sh`](../../../../.github/scripts/smoke-ubuntu-archive.sh) libraries to ELF dependencies. Do not add ALSA/GStreamer requirements for GNU migration.
- Agree intent before adding platforms, DMGs, optional resources, npm, R2, WinGet, or website/OpenAI-only publishing.

## V8 dependency release

Build V8 separately only when version/target/baseline lacks a verified fork release. Use [`fork-v8-release.yml`](../../../../.github/workflows/fork-v8-release.yml) and `asm-v8-v<exact resolved v8 crate version>-glibc2.35.2`. The suffix identifies the GNU baseline generation and matching macOS pair. Preserve the old MUSL release.

- Manual branch dispatch defaults to build-only. Explicit `publish=true` builds/smokes, then tags/publishes at that commit. Tag pushes also publish. Tag moves or published-asset replacement need approval.

- Reuse unchanged verified inputs. Same-version archive reuse for Cargo rebases/releases has standing approval only when independent review proves Cargo ABI, bindings, and full dependency-closure compatibility. Native consumer build/JavaScript/sandbox checks must pass. Record omitted source-recipe fixes; never claim source equivalence. Ask for incompatible dependencies/bindings, changed baselines, unsafe reuse, or ambiguous intent. Never replace published assets.
- Do not reuse V8 152.2.0's original `glibc2.35` pair: CppGC sizes and typed-array threshold bindings are stale. Preserve its assets. `glibc2.35.2` rebuilds matching source-generated archives/bindings with Chromium C++ runtime isolation.
- Build only optimized sandbox/pointer-compression pairs for macOS ARM64/Linux x86_64 GNU. Use existing Bazel source pair/staging helper locally on GitHub-hosted runners. No BuildBuddy, remote execution, paid infrastructure, or broad suites. Retain required static-library symbols.
- Require both native `codex-v8-poc` sandbox/JavaScript smokes, including ignored CppGC ABI checks and Ubuntu 22.04 GNU. Reuse GNU Bazel platform/older glibc sysroot.
- Publish each target's archive, Rust binding, and two-file manifest only. Use a normal release, **not Latest**, without installer; keep it separate from CLI discovery.
- Before the first consuming `main` push, publish/verify both new version/generation pairs. CI/CLI releases use only matching verified fork V8. Verify GitHub SHA-256 digests and exact manifest names/hashes. Missing/bad assets fail without upstream fallback. Preserve upstream action paths for other workflows.

## Reuse and publish

Serialize releases. Finish without later chat actions or token-push-triggered workflows. Require publication approval before pushing any release-triggering tag, including inherited tags. Legacy tags must already contain correct Cargo versions.

Use [artifact lookup](../../../../.github/scripts/find-postmerge-artifacts.sh) per target for unexpired smoke-checked archives from current dispatch/original candidate run. Legacy may reuse successful same-repo `main` push target jobs at exact tagged SHA. Wait for matching active runs, not other platforms. Never duplicate builds or rebuild ready published platforms. Timeouts/API errors fail; API errors are not cache misses. Build only without usable archives.

1. Create the pinned tag, draft, and common installer once. Resume uploads only when names, sizes, and digests match.
2. Independently build/reuse/check/upload each platform. Verify GitHub archive/installer digests; smoke the downloaded archive. Upload `SHA256SUMS-<target>` last as readiness marker.
3. At first readiness, publish non-draft normal release, including `alpha`, **not Latest** while incomplete. Append other checked assets. Keep payloads immutable by policy. GitHub's immutable-release setting must stay disabled for append-only publication; never change it automatically.
4. After both platforms pass, append aggregate manifest for old installers. Verify six assets, mark Latest, and retain both native checks. Only then merge candidate into `origin/main`. Preserve concurrent changes; stop on version conflicts. Prefer fast-forwards; necessary rewrites follow push permissions.
5. Verify ready platforms; resume only missing work. For archives uploaded before readiness markers, recover/verify exact public bytes, even after Actions expiry. Do not rebuild them. Partial releases are incomplete. Legacy completed four-asset releases remain valid. Candidates with installers lacking platform manifests retain all-platform publication.

Before first partial release, deploy platform-aware `tngo-workflow/setup.py`. Select the highest published host-ready version; macOS must not select Linux-only. Installer defaults to Latest; explicit `--release` may install an earlier ready platform. Preserve frozen policy for in-flight runs.

## Installer

- Use only `trungnt13/asm` GitHub `v*` releases for macOS ARM64/Linux x86_64 GNU. Before installing the complete package, verify GitHub digests and host `SHA256SUMS-<target>`. Use legacy `SHA256SUMS` only when host manifest is absent, never after invalid digests/manifests.
- Reject incomplete two-binary archives. Keep older complete canonical packages without declared `execServer` compatible. If declared, require its exact regular executable; reject undeclared exec-server. Historical incomplete releases keep pinned installers.
- Before metadata downloads/state changes, reject unsupported targets or Linux without glibc 2.35+. No MUSL fallback; historical MUSL requires its original installer. Use separate target-qualified GNU package directories; preserve MUSL packages and shared config/state.
- Preserve `--release`, `CODEX_HOME`, `CODEX_INSTALL_DIR`, locking, and safe `current` selection. Store in `packages/asm-standalone`. Preserve upstream packages/update markers, shared Codex config/state, and `codex` command.
- Only the owner installs externally. No OpenAI/CDN/legacy npm downloads, daemon-only installs, or `auto-update-version` writes. Do not port upstream standalone-release pruning. Preserve fork packages/layout.
- Disable built-in update checks/prompts/commands/daemon loops regardless of settings/markers. CLI/daemon must not install/download upstream releases. Preserve ordinary daemon startup/restart separately.

## Build experiment

Optional `macos_concurrency_experiment` compares cold same-commit `macos-15` builds with 2/3 Cargo jobs and otherwise equal settings. Save separate timing/resource/size/smoke results. No postmerge reuse or publication, even on tags. Measure before changing defaults; one run does not prove general speedup.
