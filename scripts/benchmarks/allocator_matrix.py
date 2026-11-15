#!/usr/bin/env python3
"""Run and summarize a balanced, isolated daemon allocator comparison."""

import argparse
import json
import math
from pathlib import Path
import statistics
import subprocess
import sys


def summarize(output):
    rows = []
    for directory in sorted(output.glob("balanced-*")):
        result = json.loads((directory / "result.json").read_text())
        samples = json.loads((directory / "samples.json").read_text())
        assert all(x.get("valid", True) and x["rss"] > 0 for x in samples)
        raw_timing = "clock" in result or any(
            {"round_start", "round_completed", "root_turn_id"} & turn.keys()
            for turn in result["turns"]
        )
        if raw_timing:
            assert result["clock"] == {"source": "time.monotonic", "unit": "seconds"}
            requests = json.loads((directory / "requests.json").read_text())
            for turn in result["turns"]:
                begin, end = turn["round_start"], turn["round_completed"]
                assert math.isfinite(begin) and math.isfinite(end) and end >= begin
                assert turn["root_turn_id"]
                intervals = sorted(
                    (max(begin, request["begin"]), min(end, request["end"]))
                    for request in requests
                    if request["end"] > begin and request["begin"] < end
                )
                union, edge = 0.0, begin
                for low, high in intervals:
                    union += max(0, high - max(low, edge))
                    edge = max(edge, high)
                measured = {
                    "wall_ms": (end - begin) * 1000,
                    "provider_union_ms": union * 1000,
                    "non_model_ms": (end - begin - union) * 1000,
                }
                for key, value in measured.items():
                    assert math.isclose(turn[key], value, rel_tol=0, abs_tol=1e-6), (
                        directory,
                        turn["index"],
                        key,
                        turn[key],
                        value,
                    )
        turns = [x["non_model_ms"] for x in result["turns"][4:]]
        steady = [x for x in samples if x["phase"] == "steady_hold"]
        assert result["stability"]["stable"]
        steady = [
            x
            for x in steady
            if result["stability"]["window_start"]
            <= x["time"]
            < result["stability"]["window_end"]
        ]
        assert len(steady) >= 90, "insufficient steady-state samples"
        idle = [x for x in samples if x["phase"] == "idle"][-10:]
        rpc = [
            json.loads(line)["message"]
            for line in (directory / "rpc.jsonl").read_text().splitlines()
        ]
        completed = [m for m in rpc if m.get("method") == "turn/completed"]
        assert all(m["params"]["turn"]["status"] == "completed" for m in completed)
        messages = [
            m["params"]["item"]
            for m in rpc
            if m.get("method") == "item/completed"
            and m["params"]["item"]["type"] == "agentMessage"
        ]
        assert messages and all(m["text"] == "READY" for m in messages), messages
        assert set(result["children"]) <= set(result["closed_after_idle"])
        row = {
            "run": directory.name,
            "allocator": "glibc" if directory.name.endswith("-a") else "jemalloc",
            "allocator_config": result.get("allocator_config", {}),
            "timing_audit": "raw_boundaries_verified"
            if raw_timing
            else "legacy_unverified",
            "environment_audit": (
                "recorded" if "allocator_environment" in result else "legacy_unrecorded"
            ),
            "latency_median_ms": statistics.median(turns),
            "latency_p95_ms": sorted(turns)[int(0.95 * (len(turns) - 1))],
            "fork_ms": result["fork_ms"],
            "resume_three_ms": result["resume_ms"],
            "completed_turn_notifications": len(completed),
            "steady_samples": len(steady),
            "steady_slope_mib_s": result["stability"]["slope_kib_s"] / 1024,
            "steady_hold_seconds": result["stability"]["hold_seconds"],
        }
        for key in [
            "rss",
            "pss",
            "anonymous",
            "executable_file",
            "other_file",
            "private",
        ]:
            row["steady_" + key + "_mib"] = (
                statistics.median(x[key] for x in steady) / 1024
            )
            row["idle_" + key + "_mib"] = statistics.median(x[key] for x in idle) / 1024
        rows.append(row)
    groups = {}
    for name in ["glibc", "jemalloc"]:
        subset = [r for r in rows if r["allocator"] == name]
        groups[name] = {
            key: {
                "median": statistics.median(r[key] for r in subset),
                "min": min(r[key] for r in subset),
                "max": max(r[key] for r in subset),
            }
            for key in rows[0]
            if isinstance(rows[0][key], (int, float))
        }
    changes = {
        key: (1 - groups["jemalloc"][key]["median"] / groups["glibc"][key]["median"])
        * 100
        for key in [
            "latency_median_ms",
            "steady_rss_mib",
            "steady_anonymous_mib",
            "idle_rss_mib",
            "fork_ms",
            "resume_three_ms",
        ]
    }
    report = {"runs": rows, "groups": groups, "percent_reduction": changes}
    (output / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--jemalloc", type=Path)
    parser.add_argument("--jemalloc-arenas", type=int, choices=[4, 8])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--analyze-only", action="store_true")
    args = parser.parse_args()
    if not args.analyze_only:
        if args.binary is None or args.jemalloc is None:
            parser.error("--binary and --jemalloc are required for runs")
        for i, mode in enumerate("abbaab"):
            command = [
                sys.executable,
                str(Path(__file__).with_name("daemon_allocator.py")),
                "--binary",
                str(args.binary),
                "--output",
                str(args.output / f"balanced-{i}-{mode}"),
            ]
            if mode == "b":
                command += ["--jemalloc", str(args.jemalloc)]
                if args.jemalloc_arenas is not None:
                    command += ["--jemalloc-arenas", str(args.jemalloc_arenas)]
            subprocess.run(command, check=True)
    summarize(args.output)


if __name__ == "__main__":
    main()
