#!/usr/bin/env bash
set -euo pipefail
metadata=$(mktemp)
trap 'rm -f "$metadata"' EXIT
cargo metadata --format-version 1 --locked > "$metadata"
python3 - "$metadata" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)
packages = {p["id"]: p for p in data["packages"]}
nodes = {n["id"]: n for n in data["resolve"]["nodes"]}
roots = [p["id"] for p in data["packages"] if p["name"] == "tkm-core" and p["id"] in data["workspace_members"]]
if len(roots) != 1:
    raise SystemExit("expected exactly one workspace tkm-core")
seen = set()
stack = roots[:]
bad = []
while stack:
    package_id = stack.pop()
    if package_id in seen:
        continue
    seen.add(package_id)
    name = packages[package_id]["name"]
    if name in {"tokio", "reqwest", "rusqlite", "hyper", "mio"} or name.endswith("-sys"):
        bad.append(name)
    for dep in nodes[package_id]["deps"]:
        if any(kind["kind"] is None for kind in dep["dep_kinds"]):
            stack.append(dep["pkg"])
if bad:
    raise SystemExit("forbidden core dependencies: " + ", ".join(sorted(bad)))
PY
if grep -R -n -E 'std::(fs|net|process|env)|File::|SystemTime::now|Instant::now|tokio|reqwest|rusqlite' crates/tkm-core/src; then
    echo 'forbidden I/O reference in tkm-core source' >&2
    exit 1
fi
