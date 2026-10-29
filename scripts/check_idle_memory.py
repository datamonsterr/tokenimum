#!/usr/bin/env python3
"""Check three VmRSS samples in KiB against a strict MiB limit."""
import os
import pathlib
import sys


def check(text, limit_mib):
    """Return the largest sample and whether it is below the limit."""
    samples = text.splitlines()
    if len(samples) != 3 or any(not sample.isascii() or not sample.isdecimal() for sample in samples):
        raise ValueError("expected exactly three nonnegative integer samples in KiB")
    if limit_mib <= 0:
        raise ValueError("limit must be positive")
    maximum = max(map(int, samples))
    return maximum, maximum < limit_mib * 1024


def main(argv):
    if len(argv) != 4:
        print("usage: check_idle_memory.py <samples> <limit_mib> <label>", file=sys.stderr)
        return 2
    try:
        maximum, passed = check(pathlib.Path(argv[1]).read_text(encoding="utf-8"), int(argv[2]))
    except (OSError, ValueError) as error:
        print(f"idle memory: {error}", file=sys.stderr)
        return 2
    result = f"{argv[3]}: peak {maximum / 1024:.2f} MiB; limit < {argv[2]} MiB: {'PASS' if passed else 'FAIL'}"
    print(result)
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary, "a", encoding="utf-8") as output:
            print(result, file=output)
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
