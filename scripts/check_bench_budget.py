#!/usr/bin/env python3
"""Fail when a Criterion benchmark (bencher output format) exceeds a time budget.

Usage: check_bench_budget.py <bencher-output> <benchmark-name> <limit-ns>

The compared value is the upper estimate: mean ns/iter plus the reported deviation.
"""
import re
import sys

LINE = re.compile(r"^test (?P<name>\S+)\s+\.\.\. bench:\s+(?P<mean>[\d,]+) ns/iter \(\+/- (?P<dev>[\d,]+)\)")


def parse(text):
    results = {}
    for line in text.splitlines():
        match = LINE.match(line.strip())
        if match:
            results[match["name"]] = (
                int(match["mean"].replace(",", "")),
                int(match["dev"].replace(",", "")),
            )
    return results


def check(text, name, limit_ns):
    results = parse(text)
    if name not in results:
        raise ValueError(f"benchmark {name!r} not found in output (found: {sorted(results)})")
    mean, deviation = results[name]
    upper = mean + deviation
    return mean, upper, upper <= limit_ns


def main(argv):
    if len(argv) != 4:
        print(__doc__, file=sys.stderr)
        return 2
    with open(argv[1], encoding="utf-8") as source:
        mean, upper, ok = check(source.read(), argv[2], int(argv[3]))
    print(f"{argv[2]}: mean {mean} ns, upper {upper} ns, limit {argv[3]} ns")
    if not ok:
        print("FAIL: over budget", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
