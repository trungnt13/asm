# Isolated daemon allocator benchmark

Linux-only end-to-end experiment. It does not change installed binaries, personal
configuration, system allocator defaults, or the personal app-server. Use a verified
optimized CLI/package matching the source revision under investigation.

Create a private Python environment with `websocket-client==1.9.2`, then run:

```sh
python scripts/benchmarks/allocator_matrix.py \
  --binary /absolute/private/package/bin/codex \
  --jemalloc /absolute/private/libjemalloc.so.2 \
  --jemalloc-arenas 4 \
  --output /absolute/new/evidence-directory
```

The matrix is glibc/jemalloc/jemalloc/glibc/glibc/jemalloc. Each run uses a new
credential-free home, one persistent loopback WebSocket app-server, and a local
HTTP Responses fixture. The fixture invokes actual agent spawn/followup tools:
one root plus two children complete 24 rounds. Inputs add 28 KiB of deterministic
text per round. Four warmup rounds are excluded from latency statistics. The
app-server remains resident for a 30–90-second stabilization window. Then a full-history
fork is created, the fork and both children are unsubscribed, and the default
60-second unload is checked after 65 seconds. The original children are resumed
and complete another parent-driven followup. This is not a real-provider acceptance
check and does not reproduce Code Mode helpers, plugins, or personal configuration.

The non-model wall residual is the duration of a root/children round minus the
**union** of overlapping local provider-service intervals. The round starts before
prompt preparation and ends at the last observed root/child completion. Provider
timing starts before reading and parsing the request body, and ends after flushing
the response. Subtraction excludes that work and any ASM work overlapping those
intervals. The residual includes ASM work and transport, scheduling and notification
overhead outside those intervals; it is not CPU time or all non-model processing.
The mock deliberately waits 10 ms per response. Real streaming inference can overlap
ASM work differently. Report run-level medians/ranges, not individual samples as
independent repetitions.

Steady-state memory uses the last three consecutive 10-second windows of the subscribed resident hold,
requiring window medians within 2% and reporting the endpoint slope. Wait at least
30 seconds, extend by 10 seconds until stable, and fail at 90 seconds if unresolved.
The subsequent fork is excluded. The sampler sums app-server and descendants, excluding Python
mock/sampler processes. `/proc/PID/smaps` separates anonymous RSS, file-backed
executable RSS, other file-backed RSS, PSS and private pages. Anonymous RSS is **not**
live heap. A small special-mapping residual may remain. Idle-after-unload memory is
reported separately and is not substituted for steady-state memory.

Jemalloc uses process-local `LD_PRELOAD`, affecting Rust, native libraries, and
inherited helpers. It is not equivalent to a Rust-only `tikv-jemallocator` build.
Verify dynamic malloc symbol binding separately; a mapped library alone does not
prove allocator replacement. `--jemalloc-arenas 4` passes only `MALLOC_CONF=narenas:4`
to jemalloc runs. Use `--jemalloc-arenas 8` with a fresh output directory for the
separate eight-arena comparison. Omit this option to reproduce untuned jemalloc.
These values select the automatic-arena policy, not a strict total arena count.
The separate diagnostics for this jemalloc library reported `opt.narenas` 4/8
and `arenas.narenas` 5/9; the extra arena's purpose was not established here.
Other allocator options remain unchanged; the command and results record the override.
The GNU CLI already caps glibc arenas at four. Its `mallopt` setting does not
configure preloaded jemalloc, and Cargo's musl jemalloc build-time configuration
does not configure a prebuilt system library. Verify effective jemalloc options
in a separate diagnostic process before timing; do not enable stats printing in runs.

Raw RPCs, model response records, mapping samples, commands and outcomes are saved.
New runs also save monotonic round boundaries and the root turn ID. RPC logs and
in-memory events use the same receive timestamp. New timing records can be checked
against the saved provider intervals without reconstructing a start from a duration.
Derived durations are finalized after all mock request handlers have completed,
so late-recorded child intervals cannot be missed by an earlier snapshot.
Historical runs lack those raw boundaries and remain marked as legacy evidence;
their missing timestamps cannot be recovered by updating the analyzer.

Only a narrow allocator-environment allowlist is recorded, before and after filtering,
with absent values represented explicitly. This is not a complete environment dump.
The harness removes inherited `LD_PRELOAD`, `MALLOC_CONF` and `MALLOC_ARENA_MAX`,
then applies the requested jemalloc settings. It does not remove `GLIBC_TUNABLES`
or every other allocator setting; their recorded values must be checked when comparing
new runs. Historical runs did not capture these values, so ambient equality cannot
be verified retrospectively. No actual confound was demonstrated in that evidence.

`allocator_matrix.py --output PATH --analyze-only` validates completed turns,
expected READY messages, child closure, and nonzero valid samples before summarizing.
Invalid trials remain evidence; use a fresh output directory after correcting a
harness failure. Keep CPU profiling in a separate run, never in measured A/B trials.
