#!/usr/bin/env python3
"""Check workspace policy and every Cargo target root without compiling."""
import json
import pathlib
import re
import sys
import subprocess
import tomllib

root = pathlib.Path(__file__).resolve().parent.parent
errors = []
workspace = tomllib.loads((root / "Cargo.toml").read_text())
if workspace.get("workspace", {}).get("lints", {}).get("rust", {}).get("unsafe_code") != "forbid":
    errors.append("Cargo.toml: workspace rust unsafe_code must be forbid")

command = ["cargo", "metadata", "--format-version", "1", "--no-deps"]
try:
    result = subprocess.run(command + ["--offline"], cwd=root, capture_output=True, text=True, check=False)
    if result.returncode:
        result = subprocess.run(command, cwd=root, capture_output=True, text=True, check=False)
    if result.returncode:
        sys.exit(f"cargo metadata failed: {result.stderr}")
    metadata = json.loads(result.stdout)
except (OSError, json.JSONDecodeError) as error:
    sys.exit(f"cargo metadata failed: {error}")

members = set(metadata["workspace_members"])
for package_metadata in metadata["packages"]:
    if package_metadata["id"] not in members:
        continue
    manifest = pathlib.Path(package_metadata["manifest_path"])
    package = tomllib.loads(manifest.read_text())
    if package.get("lints", {}).get("workspace") is not True:
        errors.append(f"{manifest}: missing workspace lints")
    directory = manifest.parent
    paths = {pathlib.Path(target["src_path"]) for target in package_metadata["targets"]}
    for path in sorted(paths):
        if not path.exists():
            errors.append(f"{path}: target root does not exist")
            continue
        # Rust inner attributes and documentation may precede the attribute, but items may not.
        text = path.read_text()
        pattern = r'(?m)^#!\[forbid\(unsafe_code\)\]\s*$'
        match = re.search(pattern, text)
        prefix = text[:match.start()] if match else ""
        prefix = re.sub(r'/\*.*?\*/|//[^\n]*|#!\[[^\]]*\]', '', prefix, flags=re.S)
        if not match or prefix.strip():
            errors.append(f"{path}: missing forbid attribute before first item")
    build = directory / "build.rs"
    if build.exists() and re.search(r'\bcc\s*::|\bcc\s*=', build.read_text()):
        errors.append(f"{build}: C compilation is forbidden")
for error in errors:
    print(error, file=sys.stderr)
if errors:
    sys.exit(1)
print("Workspace unsafe policy checks passed.")
