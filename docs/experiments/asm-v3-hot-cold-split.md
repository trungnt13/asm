---
title: "ASM Linux x86-64-v3 hot/cold split experiment"
release: "v0.173.1-alpha.4.1"
source_commit: "d54a3d36ae6e58201f8fab8dc0c2eaa793e76e9b"
compiler_flag: "-Cllvm-args=-enable-split-machine-functions"
build:
  first_start_utc: "2026-11-24T19:44:13Z"
  first_oom_utc: "2026-11-24T20:29:01.503392Z"
  retry_start_utc: "2026-11-24T21:09:23Z"
  retry_cargo_end_utc: "2026-11-24T21:56:44Z"
  retry_cargo_elapsed_s: 2814
  package_verified_utc: "2026-11-24T21:59:05Z"
benchmark:
  pilot_start_utc: "2026-11-24T19:41:33.830Z"
  pilot_end_utc: "2026-11-24T19:42:09.420Z"
  first_start_utc: "2026-11-24T22:05:35.128Z"
  first_stop_utc: "2026-11-24T22:12:43.460Z"
  first_completed_runs: 23
  start_utc: "2026-11-24T22:14:05.459Z"
  end_utc: "2026-11-24T22:28:18.263Z"
  elapsed_s: 852.8
  design: "identical-artifact A/A"
  pairs_per_workload: 12
  completed_runs: 48
---

## Background

Hot/cold splitting moves code that runs less often away from code that runs often. This can reduce the number of executable pages the process must keep resident. The experiment asked if one LLVM flag could change the released ASM v3 CLI. The proposed target was a 5 MiB drop in paired median resident set size (RSS) without a latency regression.

The published package was arm O. The private rebuild with the added flag was arm H. The experiment checked the linked code first. Because the packages had the same payload bytes, the runtime test measured A/A repeatability, not the effect of the flag.

## Experiments

The first build stopped after about 45 minutes when the host OOM killer killed `rustc`. The capped retry used existing swap; it added no swap. Cargo completed in 46 min 54 s with no cgroup OOM kills. An old audit rejected unchanged CLI bytes after Cargo succeeded. The corrected audit and packaging did not need another build.

- **Build inputs:** Linux GNU `x86_64-unknown-linux-gnu`, target CPU `x86-64-v3`, fat LTO, one codegen unit, one Cargo job, and the released profile-guided optimization (PGO) data. The offline CLI rebuild used Ubuntu 22.04 (Jammy), glibc 2.35, and local Docker image ID `sha256:c7ca6c14560f7c18bcdfc6af9b2325578c6866fde6a0ea75af12f00ca53fb80c`. The compiler was `rustc 1.99.1 (58cf8f9c3)`, with LLVM `23.1.1` and Cargo `1.99.1 (5f94df478)`.
- **Retry limits:** 42,540 MiB resident memory and 24,020 MiB swap (`--memory 42540m --memory-swap 66560m`). The host had 62.71 GiB RAM and 32 GiB existing swap. The retry began after the host freed RAM. Before starting Cargo, the driver required 24 GiB free disk, 8 GiB host memory reserve, and 6 GiB host swap reserve above the respective caps.
- **Build isolation:** The guard allowed only the final `codex` compiler command to emit output. It appended `-Cllvm-args=-enable-split-machine-functions` to the 264 recorded arguments. The remaining arguments and 79 externs matched. All 5,177 `deps` and `build` file hashes matched after the retry. The PGO profile hash also matched.
- **Code and package:** Both CLI files had the same SHA-256. The new object had no `.text.split*` sections or cold-suffix symbols. The linked CLIs had no `.text.split*` sections. Both had 156 cold-suffix symbols and no Rust-mangled cold-suffix symbols. The checks did not show why the flag had no effect.
- **Smoke checks:** The rebuilt archive passed Jammy package layout, version, ELF compatibility, CLI and helper checks. The app-server check ran only `codex app-server --help`. The CLI reported `asm (x86-64-v3, PGO) 0.173.1-alpha.4.1`. Neither arm was installed. No candidate was published.

| Size (bytes) | Published O | Rebuilt H | H − O |
| --- | ---: | ---: | ---: |
| `bin/codex` | 305,437,272 | 305,437,272 | 0 |
| CLI ELF `.text` | 228,400,496 | 228,400,496 | 0 |
| Six unpacked files | 477,469,199 | 477,469,199 | 0 |
| `.tar.gz` archive | 180,197,117 | 180,197,171 | +54 |

- **Archive difference:** The gzip timestamp and tar/PAX member times differed. The SHA-256 hashes of all six unpacked files matched. The archive size difference does not show a code size change.
- **Historical build:** The command was `bash .agents/hotcold-v3-build/build.sh retry-capped 42540 24020`, where `.agents/hotcold-v3-build/` is a restore path. It required a guarded preflight and the exact failed build snapshot. The private bundle does not contain that snapshot. Do not recreate the OOM failure.
- **Baseline preparation:** Obtain the pinned inputs. With network access in the pinned container, run `bash /src/scripts/build-release-v3.sh v0.173.1-alpha.4.1 /output /v8` against a fresh private `/output`. PGO setup downloads the pinned profile even if a local copy exists. Verify the flag-off payload hashes and prove the build graph is fresh with a guarded no-op. Take a private snapshot of Cargo home and the target directory. Keep the container paths, wrappers, V8 files, release tools, `agent.profdata`, and `pgo.env` fixed.
- **Fresh replay:** Invalidate only the final CLI fingerprint in the snapshot's target release directory. Move the one matched `.fingerprint/codex-cli-*/bin-codex` file out of that directory. Leave the helper and dependency fingerprints intact. The bundle has no ready-to-run capped driver for a fresh snapshot. Enforce the cgroup RAM and existing-swap limits and the disk, RAM, and swap reserves above. Stop if the host cannot meet them. For the guarded CLI step, disable the network. Require the guard to match the original final CLI argv before it appends `-Cllvm-args=-enable-split-machine-functions`. The recorded inner command ran from `/src/codex-rs` with Python 3.11, the release environment, and the compiler guard:

  ```sh
  python3.11 ../scripts/local-cargo.py build --locked --offline --release \
    --target x86_64-unknown-linux-gnu --verbose \
    --bin codex --bin codex-code-mode-host --bin codex-exec-server --timings
  ```

  Confirm Cargo succeeded with exactly one compile and one guarded `rustc` execution that emitted the final `codex` CLI. Verify the captured argv differ from the original only by the single split flag. Check that the PGO hash, reusable artifact inventory, and helper hashes still match. Reject a `Fresh` result for the final CLI. Matching CLI bytes alone do not prove a rebuild.

| Build or release input | SHA-256 |
| --- | --- |
| Published `codex-x86_64-unknown-linux-gnu-v3.tar.gz` | `32029a0b093c414a79907ba42ddbe0d846433b25d0ff8ccaa36f88de2091a808` |
| `bin/codex` in both packages | `bb48d2162865bb961aa2fa3a189f72d396d808d5f5c8d2b574c8a86616a21b29` |
| `agent.profdata` | `479324908915006b79ff07d5dd284ae331b1543943d49976024d967ecba38a35` |
| `pgo.env` | `96a3fcc2bb13412cb1101405c5f1813077725bec462a32880ea3c619c401e7f6` |
| `codex-rs/Cargo.lock` at the release commit | `3268fe342cf7285dfe4dae8f8034de834a7b2b86235836217825a092af82ae72` |
| V8 152.2.0, `glibc2.35.2`, GNU x86-64 pointer-compression sandbox archive | `8aa2eaaffc7029cade82456b3565041f84aa10dbf9352f62b54eb0b4b33b8011` |
| Matching V8 Rust bindings | `de9a71ee1cb4b5ab12ab5bfe382eab023087693bcf874faa72bff6fef0c2d4d5` |

The original-only pilot checked behavior, not comparative timing. The first A/A attempt stopped after 23 runs when its guard detected a concurrent build. Do not pool that attempt with the complete retry. The table uses only the complete retry. It includes all 48 runs without discarding large differences.

- **Runtime host:** AMD Ryzen 7 5800X (8 cores), Ubuntu 24.04.5 LTS, Linux `7.0.0-43-generic` x86-64. The manifest records the kernel and CPU affinity `0–15`. The CPU model and OS release came from a later inspection on the same boot. No narrower affinity was set. CPU frequency settings and per-core placement were not recorded.
- **Host checks:** In the complete retry, one-minute load ranged from 0.03 to 1.12. `MemAvailable` stayed at or above 60.72 GiB at sampled points. The median CPU, I/O, and memory pressure stall increments per run (`PSI some`) were 14.018, 36.535, and 0 ms. The maximum increments were 32.957, 104.131, and 0.001 ms. The build guard passed at run boundaries.
- **Launch:** Start a fresh `codex --no-daemon app-server --listen ws://127.0.0.1:<port>` for each run. Use a private `HOME` and `CODEX_HOME` and a non-Git temporary working directory. Verify each CLI hash and a distinct private inode for each arm. Before each launch, read the full CLI and verify all its file pages are in the page cache with `mincore`. After launch, verify `/proc/<pid>/exe` matches the private executable. A warm file cache does not imply that all those pages are mapped by the process.
- **Config:** Use Python 3.12.3 and `websocket-client` 1.9.0 for the harness. Use client `component_benchmark` version `1` with `experimentalApi` enabled. Select `bench-model` on the loopback Responses provider. Disable retries, remote control, analytics, diagnostics, update checks, web search, apps, plugins, memories, shell snapshots, and Code Mode. Enable Multi-agent V2 in the `agents` namespace. Use no credentials or live model endpoint. The mock accepts only direct, unauthenticated `POST /v1/responses` calls.
- **Multi-agent:** Send `BENCH_SPAWN` in round 0 and `BENCH_TURN <i>` in rounds 1–59. Append a newline and 1,024 copies of `repeatable synthetic content. ` to each input. The mock spawns `alpha` and `beta` with full-history forks, then follows up with both in each later round. The mock sends one two-call response and three `READY` responses per round, with a fixed 10 ms delay per HTTP handler. In round 0, pause 200 ms after the root completes, then resume both children to subscribe to their events. Wait for all three threads and pause 100 ms between rounds. Verify 65 RPC replies, 60 completed turns per thread, and 240 HTTP 200 replies per process. The largest paired difference in round totals for reserialized request bodies was 510 bytes. This is not a wire-byte count or a per-request bound.
- **Read-only:** Send 400 empty-parameter `thread/loaded/list` calls. Verify each response has empty `data` and a null `nextCursor`. Expect 402 RPC replies including setup. Expect no turns or HTTP calls. Both workloads had zero RPC or turn errors.
- **Sampling:** Take 45 process-tree `smaps` samples per run at 200 ms intervals in three windows that start 3, 6, and 9 s after the workload. Use each run's median RSS across the samples (2,160 valid samples in total). File RSS sums `Rss` minus `Anonymous` for each mapping with an absolute pathname. CLI file RSS uses the same sum, but only for mappings of the private `bin/codex`, not only `.text`.
- **Timing:** Use nearest-rank p95 on the 56 rounds left after four multi-agent warmups (rank 54). For raw wall time, measure from root `turn/start` submission to the last completion. For model-excluded time, subtract the clipped union of mock handler intervals. This retains local client, transport, and scheduling time. It can also subtract backend work that overlaps a handler interval. For read-only p95, use 300 replies after 100 warmups (rank 285).
- **Pairing:** Use 12 serial blocks, two workloads per block and one run per arm per workload. Alternate workload order. With seed `20261124`, assign three O-first and three H-first blocks within each workload order. Calculate H − O within each block. Resample whole pairs 10,000 times with seed `20261123`. Use the 2.5th and 97.5th percentiles of the resampled signed medians for descriptive 95% intervals.

O and H columns give medians across 12 runs each. The signed and absolute differences use the same 12 pairs. RSS is in MiB; latency is in ms. A negative H − O means lower RSS or lower latency in H.

| Workload and metric | O median | H median | H − O median [min, max] | 95% paired interval | Median absolute difference |
| --- | ---: | ---: | ---: | ---: | ---: |
| Multi-agent total RSS | 172.3398 | 172.0430 | −0.5293 [−4.1953, +20.7773] | [−1.5703, +2.2227] | 2.1992 |
| Multi-agent anonymous RSS | 47.0312 | 46.5527 | −0.3770 [−3.7148, +4.4492] | [−2.2773, +0.7988] | 1.0820 |
| Multi-agent file RSS | 124.6660 | 125.5586 | +0.5156 [−0.4805, +19.9570] | [−0.1934, +1.3164] | 0.6719 |
| Multi-agent CLI file RSS | 117.9609 | 118.8457 | +0.5215 [−0.5156, +19.9531] | [−0.1914, +1.3203] | 0.6777 |
| Multi-agent model-excluded p95 | 42.1185 | 40.7211 | −1.1932 [−7.7175, +5.8065] | [−1.7094, +0.0018] | 1.2893 |
| Multi-agent raw wall p95 | 81.6759 | 80.5198 | +0.5663 [−12.3955, +9.8591] | [−3.7916, +2.9697] | 3.6272 |
| Read-only total RSS | 93.5273 | 93.9473 | +0.2910 [−1.5352, +16.1289] | [−0.2227, +1.4316] | 0.9082 |
| Read-only anonymous RSS | 13.7910 | 14.0391 | +0.1953 [−0.3359, +1.0078] | [+0.0137, +0.6465] | 0.3340 |
| Read-only file RSS | 79.7285 | 79.7520 | −0.2012 [−1.1992, +16.0703] | [−0.6777, +1.0098] | 0.7793 |
| Read-only CLI file RSS | 76.1504 | 76.2754 | −0.1641 [−1.2305, +16.0703] | [−0.6270, +0.9746] | 0.7520 |
| Read-only RPC p95 | 0.0908 | 0.0840 | −0.0052 [−0.0218, +0.0162] | [−0.0145, −0.0022] | 0.0079 |

- **A/A replay:** Use the private evidence README to restore the harness, prior manifest, and pinned release files. The launcher checks the tag, archive provenance, and PGO profile. Verify the inherited scripts at the manifest paths or record a path adaptation and verify the same hashes. Use a quiet, build-free host window and a fresh run name. The launcher requires 4 GiB free at the start and 2 GiB at each run boundary. Verify both stages against all six payload hashes. The original H package was removed. The private evidence does not include the H CLI. If the verified historical H CLI is available, stage it with the matched release helpers and pass its real build record. Otherwise, use the copy fallback below. This repeats the control, not the rebuild. For the fallback, use `summary.json` with the new provenance record. Do not use `aa_summary.py` for the fallback; it labels H as rebuilt.

  ```sh
  # Copy fallback: run in Bash from the ASM root after restoring the pinned inputs.
  set -euo pipefail
  umask 077
  bench=.agents/hotcold-v3-bench
  python=.agents/perf-footprint/venv/bin/python  # Or a verified equivalent virtual environment.
  "$python" -IB -c 'import sys, websocket; assert sys.version_info[:3] == (3, 12, 3) and websocket.__version__ == "1.9.0"'
  run=identical-aa-rerun
  published=.agents/releases/2026-11-22-sync/v3-output/staging/package
  h_input=$(mktemp -d "$bench/.aa-input.XXXXXXXX")
  cp -a --reflink=auto -- "$published" "$h_input/H"
  printf '%s\n' 'H is a copy of the published v0.173.1-alpha.4.1 v3 payload, not the original rebuild.' > "$h_input/RESULT.txt"
  "$python" -IB "$bench/identical_control.py" "$run" --identical-control \
    --candidate "$h_input/H" \
    --candidate-sha bb48d2162865bb961aa2fa3a189f72d396d808d5f5c8d2b574c8a86616a21b29 \
    --build-record "$h_input/RESULT.txt"
  "$python" -IB "$bench/report.py" "$bench/$run"  # Generates summary.json; ignore legacy summary.md.
  ```

| Harness input | SHA-256 |
| --- | --- |
| `identical_control.py` | `f4d067ae4e31a68556a153b91d65b3b24639aa507c137f5e46ff3928eeac990a` |
| `report.py` | `0eb92fe83f9f9c6493b985976c3b86478f4372a309763a7a10bc0c4fea5188de` |
| `aa_summary.py` | `9fbd702cb5dab6220d62a2baf71ae91a3a40ee5a1ebff67ce8ffefe9338afd1b` |
| Prior dispatcher `bench.py` | `ed778a1543ac7b4098f13a6adb6283aa129c1864ef2f4773ea9ce27b4d9dee6c` |
| Prior dispatcher `summarize.py` | `210e537038b2ea2ba581c9944a175b689842cc88ba9fc94b9d873de92e9a1875` |
| Inherited `scenarios.py` | `d8a339faa5ecf24cb926d4834e1140994002f4bd4837a3aa90f707623478f9f4` |
| Inherited `matrix.py` | `051d3ba7924af19e8394f266b74b8a3e000f69e348675ddc7f7fabbc1db004d9` |
| Inherited `daemon_allocator.py` | `a221297a906b7f6839706113765b0088650392f8816d7c5f4fa69d132f6e88e7` |

- **Local evidence (not tracked):** From the original ASM root, see `.agents/hotcold-v3-evidence/README.md` for the retained harness, pins, and restore procedure. See `records/` in that bundle for the compact build audit and completed A/A evidence. The scratch paths in the commands are restore paths, not live evidence locations. The bundle does not contain the build cache, image, PGO data, or binaries. A fresh clone does not contain the bundle.

## Conclusion

Do not add the flag for this build. It produced the same CLI bytes and size, so there is no code change to test for a runtime benefit. For multi-agent runs, the A/A median absolute jitter was 2.20 MiB in total RSS, 1.29 ms in model-excluded p95, and 3.63 ms in raw wall p95. One pair differed by 20.78 MiB in total RSS, including 19.95 MiB in CLI mappings. Two read-only intervals exclude zero even with identical bytes. These intervals do not establish a flag effect or latency non-regression. This test does not generalize to live providers, other workloads, or other platforms.

For follow-up comparisons, rebuild only if a build input or target changes. Compare the linked bytes before timing new binaries. If the runtime host, kernel, compatible CPU, config, workload, host conditions, or measurement method changes, verify the existing payload hashes. Establish a new A/A reference before comparing binaries. If a guard fails, stop and fix the inputs. If mapped-page differences recur, check the executable path and inode, page cache, process mappings, and host pressure separately. If a later difference exceeds the matched A/A variation and repeats with reversed arm order, investigate it before attributing it to the change.
