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
    if name in {"tokio", "reqwest", "rusqlite", "hyper", "mio", "rand", "rand_core", "rand_chacha", "getrandom", "fastrand", "rustix", "libc", "nix", "socket2"} or name.endswith("-sys"):
        bad.append(name)
    for dep in nodes[package_id]["deps"]:
        if any(kind["kind"] is None for kind in dep["dep_kinds"]):
            stack.append(dep["pkg"])
if bad:
    raise SystemExit("forbidden core dependencies: " + ", ".join(sorted(bad)))
PY
python3 - <<'PYCODE'
from pathlib import Path
import re
import sys

# Match direct paths as well as grouped use trees. Comments are deliberately
# included, as in the original check. Braced trees may span multiple lines.
direct = re.compile(r"\bstd\s*::\s*(?:(?:fs|net|process|env|thread)\b|io\s*::\s*(?:stdin|stdout|stderr)\b|time\s*::\s*(?:SystemTime|Instant)\b)")
groups = re.compile(r"\bstd\s*::[^;]*\{[^;]*;", re.DOTALL)
namespace_import = re.compile(r"\buse\s+std\s*::\s*(?:io|time)\s*(?:as\s+\w+\s*)?;")
namespace_group = re.compile(r"\b(?:io|time)\s*(?:,|}|as\s+\w+)")
forbidden_group = re.compile(r"\b(?:fs|net|process|env|thread|stdin|stdout|stderr|SystemTime|Instant)\b")
other = re.compile(r"\b(?:File\s*::|SystemTime\s*::\s*now|Instant\s*::\s*now|tokio|reqwest|rusqlite)\b")
bad = False
for path in Path("crates/tkm-core/src").rglob("*.rs"):
    source = path.read_text(encoding="utf-8")
    if direct.search(source) or other.search(source) or namespace_import.search(source) or any(forbidden_group.search(match.group()) or namespace_group.search(match.group()) for match in groups.finditer(source)):
        print(f"forbidden I/O reference in {path}", file=sys.stderr)
        bad = True
if bad:
    sys.exit(1)
PYCODE
