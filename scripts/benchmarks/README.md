# Isolated daemon allocator benchmark

Linux-only end-to-end experiment. It does not change installed binaries, personal
configuration, system allocator defaults, or the personal app-server. Use a verified
optimized CLI/package matching the source revision under investigation.

Create a private Python environment with `websocket-client==1.9.2`, then run:

```sh
python scripts/benchmarks/allocator_matrix.py \
  --binary /absolute/private/package/bin/codex \
  --jemalloc /absolute/private/libjemalloc.so.2 \
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

Non-model latency is the wall duration of a root/children round minus the **union**
of overlapping local provider-service intervals. It includes loopback transport,
client scheduling, and notification-delivery overhead; it is not pure Rust time.
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
prove allocator replacement. Neither arm receives allocator tuning overrides.

Raw RPCs, model response records, mapping samples, commands and outcomes are saved.
`allocator_matrix.py --output PATH --analyze-only` validates completed turns,
expected READY messages, child closure, and nonzero valid samples before summarizing.
Invalid trials remain evidence; use a fresh output directory after correcting a
harness failure. Keep CPU profiling in a separate run, never in measured A/B trials.
