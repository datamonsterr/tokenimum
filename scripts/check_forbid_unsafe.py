#!/usr/bin/env python3
"""Check workspace policy and every Cargo target root without compiling."""
import pathlib
import re
import sys
import tomllib

root = pathlib.Path(__file__).resolve().parent.parent
errors = []
workspace = tomllib.loads((root / "Cargo.toml").read_text())
if workspace.get("workspace", {}).get("lints", {}).get("rust", {}).get("unsafe_code") != "forbid":
    errors.append("Cargo.toml: workspace rust unsafe_code must be forbid")

for manifest in sorted((root / "crates").glob("*/Cargo.toml")):
    package = tomllib.loads(manifest.read_text())
    if package.get("lints", {}).get("workspace") is not True:
        errors.append(f"{manifest.relative_to(root)}: missing workspace lints")
    directory = manifest.parent
    paths = set(directory.glob("src/lib.rs")) | set(directory.glob("src/main.rs"))
    paths |= set(directory.glob("src/bin/*.rs")) | set(directory.glob("src/bin/*/main.rs"))
    paths |= set(directory.glob("benches/*.rs")) | set(directory.glob("benches/*/main.rs"))
    for kind in ("bin", "bench", "example", "test"):
        for target in package.get(kind, []):
            if "path" in target:
                paths.add(directory / target["path"])
    paths |= set(directory.glob("examples/*.rs")) | set(directory.glob("tests/*.rs"))
    for path in sorted(paths):
        if not path.exists():
            errors.append(f"{path.relative_to(root)}: target root does not exist")
            continue
        # Rust inner attributes and documentation may precede the attribute, but items may not.
        text = path.read_text()
        pattern = r'(?m)^#!\[forbid\(unsafe_code\)\]\s*$'
        match = re.search(pattern, text)
        prefix = text[:match.start()] if match else ""
        prefix = re.sub(r'/\*.*?\*/|//[^\n]*|#!\[[^\]]*\]', '', prefix, flags=re.S)
        if not match or prefix.strip():
            errors.append(f"{path.relative_to(root)}: missing forbid attribute before first item")
    build = directory / "build.rs"
    if build.exists() and re.search(r'\bcc\s*::|\bcc\s*=', build.read_text()):
        errors.append(f"{build.relative_to(root)}: C compilation is forbidden")
for error in errors:
    print(error, file=sys.stderr)
if errors:
    sys.exit(1)
print("Workspace unsafe policy checks passed.")
