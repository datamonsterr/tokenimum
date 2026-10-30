#!/usr/bin/env python3
"""Fail when a Criterion benchmark (bencher output format) exceeds a time budget.

Usage: check_bench_budget.py <bencher-output> <benchmark-prefix> <limit-ns>

The compared value is the upper estimate: median ns/iter plus the reported standard deviation.
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
    return mean, upper, upper < limit_ns


def check_prefix(text, prefix, limit_ns):
    results = parse(text)
    matches = {name: (mean, mean + deviation, mean + deviation < limit_ns)
               for name, (mean, deviation) in results.items() if name.startswith(prefix)}
    if not matches:
        raise ValueError(f"benchmark prefix {prefix!r} not found in output (found: {sorted(results)})")
    return matches


def main(argv):
    if len(argv) != 4:
        print(__doc__, file=sys.stderr)
        return 2
    with open(argv[1], encoding="utf-8") as source:
        results = check_prefix(source.read(), argv[2], int(argv[3]))
    for name, (mean, upper, _) in results.items():
        print(f"{name}: median {mean} ns, upper {upper} ns, limit {argv[3]} ns")
    if not all(ok for _, _, ok in results.values()):
        print("FAIL: over budget", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
