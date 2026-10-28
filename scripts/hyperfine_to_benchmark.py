#!/usr/bin/env python3
"""Convert hyperfine's mean duration to github-action-benchmark's JSON format."""
import json
import math
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    results = json.load(source)["results"]
if len(results) != 1 or not math.isfinite(results[0]["mean"]) or results[0]["mean"] <= 0:
    raise ValueError("expected one positive, finite hyperfine mean")
with open(sys.argv[2], "w", encoding="utf-8") as destination:
    json.dump([{"name": "tkm-hook startup", "unit": "seconds", "value": results[0]["mean"]}], destination)
    destination.write("\n")
