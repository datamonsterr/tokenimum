#!/usr/bin/env python3
"""Convert hyperfine's mean duration to github-action-benchmark's JSON format."""
import json
import math
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    export = json.load(source)

try:
    if export["schema_version"] != 2:
        raise ValueError(f"unsupported hyperfine JSON schema_version: {export['schema_version']!r}")
    results = export["results"]
    if len(results) != 1:
        raise ValueError(f"expected exactly one hyperfine result, got {len(results)}")
    summary = results[0]["summary"]["time_wall_clock"]
    if summary["unit"] != "second":
        raise ValueError(f"expected time_wall_clock unit 'second', got {summary['unit']!r}")
    mean_seconds = summary["mean"]
except (KeyError, TypeError) as error:
    raise ValueError(f"unexpected hyperfine v2 JSON schema: missing or invalid {error}") from error

if not isinstance(mean_seconds, (int, float)) or not math.isfinite(mean_seconds) or mean_seconds <= 0:
    raise ValueError("expected one positive, finite hyperfine wall-clock mean")
with open(sys.argv[2], "w", encoding="utf-8") as destination:
    json.dump([{"name": "tkm-hook startup", "unit": "us", "value": mean_seconds * 1_000_000}], destination)
    destination.write("\n")
