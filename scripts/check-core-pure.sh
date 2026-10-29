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
streams = re.compile(r"\bio\s*::\s*(?:stdin|stdout|stderr)\b")
other = re.compile(r"\b(?:File\s*::|SystemTime\s*::\s*now|Instant\s*::\s*now|tokio|reqwest|rusqlite)\b")
# Parse use trees to distinguish safe std::io item imports from namespace
# imports, self, globs and aliased module paths. No comment removal is performed.
allowed_io = {"Read", "Write", "Error", "ErrorKind", "Result", "Cursor"}
token = re.compile(r"::|[A-Za-z_][A-Za-z_0-9]*|[{},*]")

def unsafe_imports(source):
    for statement in re.finditer(r"\buse\s+([^;]+);", source):
        tokens = token.findall(statement.group(1))
        pos = 0
        paths = []
        def walk(prefix):
            nonlocal pos
            if pos >= len(tokens):
                return
            if tokens[pos] == "{":
                pos += 1
                while pos < len(tokens) and tokens[pos] != "}":
                    if tokens[pos] == ",":
                        pos += 1
                    else:
                        walk(prefix)
                pos += 1
                return
            part = tokens[pos]
            pos += 1
            path = prefix + [part]
            if pos < len(tokens) and tokens[pos] == "::":
                pos += 1
                walk(path)
                return
            if pos < len(tokens) and tokens[pos] == "as":
                pos += 2
            paths.append(path)
        while pos < len(tokens):
            if tokens[pos] == ",":
                pos += 1
            else:
                walk([])
        for path in paths:
            if path in (["std"], ["std", "self"]):
                return True
            if path[:2] == ["std", "io"] and (len(path) != 3 or path[2] not in allowed_io):
                return True
    return False

bad = False
for path in Path("crates/tkm-core/src").rglob("*.rs"):
    source = path.read_text(encoding="utf-8")
    if direct.search(source) or other.search(source) or streams.search(source) or namespace_import.search(source) or unsafe_imports(source) or any(forbidden_group.search(match.group()) or namespace_group.search(match.group()) for match in groups.finditer(source)):
        print(f"forbidden I/O reference in {path}", file=sys.stderr)
        bad = True
if bad:
    sys.exit(1)
PYCODE
