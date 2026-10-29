#!/usr/bin/env python3
"""Fail when the tkm-hook round trip exceeds p50/p99 limits (hyperfine JSON `times`, seconds).

Usage: check_hook_latency.py <hyperfine.json> [--p50-ms 3] [--p99-ms 10] [--summary FILE]
"""
import argparse
import json
import math
import sys


def percentile(sorted_values, fraction):
    """Nearest-rank percentile of an ascending list."""
    if not sorted_values:
        raise ValueError("no samples")
    rank = max(1, math.ceil(fraction * len(sorted_values)))
    return sorted_values[rank - 1]


def summarize(export):
    results = export["results"]
    if len(results) != 1:
        raise ValueError(f"expected exactly one hyperfine result, got {len(results)}")
    times = results[0].get("times")
    if not times:
        raise ValueError("hyperfine export has no `times` array")
    ordered = sorted(float(t) * 1000.0 for t in times)
    return {"runs": len(ordered), "p50": percentile(ordered, 0.50), "p99": percentile(ordered, 0.99)}


def render(stats, p50_limit, p99_limit):
    ok50 = stats["p50"] < p50_limit
    ok99 = stats["p99"] < p99_limit
    lines = [
        "### tkm-hook round trip (tool_done)",
        "",
        "| metric | value | limit | status |",
        "|---|---|---|---|",
        f"| p50 | {stats['p50']:.3f} ms | < {p50_limit} ms | {'pass' if ok50 else 'FAIL'} |",
        f"| p99 | {stats['p99']:.3f} ms | < {p99_limit} ms | {'pass' if ok99 else 'FAIL'} |",
        f"| runs | {stats['runs']} | | |",
    ]
    return "\n".join(lines) + "\n", ok50 and ok99


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("export")
    parser.add_argument("--p50-ms", type=float, default=3.0)
    parser.add_argument("--p99-ms", type=float, default=10.0)
    parser.add_argument("--summary")
    args = parser.parse_args(argv[1:])
    with open(args.export, encoding="utf-8") as source:
        stats = summarize(json.load(source))
    text, ok = render(stats, args.p50_ms, args.p99_ms)
    print(text)
    if args.summary:
        with open(args.summary, "a", encoding="utf-8") as summary:
            summary.write(text)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
