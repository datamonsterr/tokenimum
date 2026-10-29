#!/usr/bin/env python3
"""User-invoked inventory/build and CI unpack for the private E10.6 bundle.

This script is never invoked against a developer's home by CI. Credentials are
never collected: build accepts an explicitly reviewed configuration template.
"""
import argparse
import getpass
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile


def inside(root, path):
    resolved = (root / path).resolve(strict=True)
    if not resolved.is_relative_to(root.resolve()) or not resolved.is_file():
        raise ValueError("source is not an in-root file")
    return resolved


def inventory(root):
    return sorted(p.relative_to(root).as_posix() for directory in root.glob("-home-dat-dev-taphoa-mini*") for p in directory.rglob("*.jsonl") if p.is_file())


def copy_template(source, target):
    """Only the reviewed AI configuration surface; never ambient credentials."""
    allowed = {"CLAUDE.md", "settings.json", "settings.local.json", ".mcp.json", "skills", "plugins", "agents", "commands"}
    for path in source.rglob("*"):
        if path.is_symlink():
            raise ValueError("template symlinks must be made portable before bundling")
        if not path.is_file():
            continue
        relative = path.relative_to(source)
        parts = relative.parts
        if relative.as_posix() == ".claude.json":
            data = json.loads(path.read_text())
            if set(data) - {"mcpServers"}:
                raise ValueError("reviewed .claude.json may contain only mcpServers")
        elif len(parts) < 2 or parts[0] != ".claude" or parts[1] not in allowed:
            raise ValueError("template contains a file outside the reviewed AI configuration surface")
        if any("credential" in part.lower() or part in {".env", ".ssh", ".auth.json"} for part in parts):
            raise ValueError("credential files must never enter the template")
        destination = target / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, destination)


def build(args):
    output = args.output.expanduser().resolve()
    repository = Path(__file__).resolve().parents[1]
    if output.is_relative_to(repository):
        raise ValueError("write the encrypted bundle outside the checkout")
    metadata_path = args.metadata.resolve(strict=True)
    metadata = json.loads(metadata_path.read_text())
    if not metadata.get("tests") or any(not command for command in metadata["tests"]):
        raise ValueError("CI-equivalent test commands are required")
    root = args.source_root.expanduser().resolve(strict=True)
    discovered = set(inventory(root))
    declared = {entry["path"] for entry in metadata["sessions"]}
    if discovered != declared:
        raise ValueError("metadata must describe every discovered transcript exactly once")
    if len(declared) != len(metadata["sessions"]):
        raise ValueError("duplicate source transcript")
    key = os.environ.get("REPLAY_BUNDLE_KEY") or getpass.getpass("Bundle passphrase: ")
    if not key or "\n" in key:
        raise ValueError("bundle passphrase must be nonempty and contain no newline")
    with tempfile.TemporaryDirectory(prefix="replay-bundle-") as temporary:
        private = Path(temporary)
        bundle = private / "bundle"
        bundle.mkdir(mode=0o700)
        for index, entry in enumerate(metadata["sessions"]):
            source = inside(root, Path(entry["path"]))
            relative = Path("transcripts") / f"{index:04}.jsonl"
            destination = bundle / relative
            destination.parent.mkdir(exist_ok=True)
            shutil.copyfile(source, destination)
            entry["path"] = relative.as_posix()
            if entry.get("parent") is None and not entry.get("start_commit"):
                log = inside(metadata_path.parent, Path(entry["git_log"]))
                relative_log = Path("git-logs") / f"{index:04}.log"
                (bundle / relative_log).parent.mkdir(exist_ok=True)
                shutil.copyfile(log, bundle / relative_log)
                entry["git_log"] = relative_log.as_posix()
        template = bundle / "home-template"
        template.mkdir()
        copy_template(args.config_template.resolve(strict=True), template)
        (bundle / "manifest.json").write_text(json.dumps(metadata, indent=2))
        archive = private / "bundle.tar.gz"
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(bundle, arcname="bundle")
        subprocess.run(["gpg", "--batch", "--yes", "--pinentry-mode", "loopback", "--passphrase-fd", "0", "--symmetric", "--cipher-algo", "AES256", "--output", str(output), str(archive)], input=(key + "\n").encode(), stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, check=True)
    print("Encrypted bundle written outside the checkout; upload it to the private URL.")


def unpack(archive, destination):
    destination.mkdir(parents=True, exist_ok=True, mode=0o700)
    with tarfile.open(archive, "r:gz") as tar:
        # Reject links even when their targets would be in-root: the bundle schema needs files only.
        for member in tar.getmembers():
            path = Path(member.name)
            if path.is_absolute() or ".." in path.parts or not (member.isfile() or member.isdir()):
                raise ValueError("invalid bundle archive member")
        tar.extractall(destination, filter="data")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    listing = sub.add_parser("inventory")
    listing.add_argument("--source-root", type=Path, default=Path.home() / ".claude/projects")
    builder = sub.add_parser("build")
    builder.add_argument("--source-root", type=Path, default=Path.home() / ".claude/projects")
    builder.add_argument("--metadata", type=Path, required=True)
    builder.add_argument("--config-template", type=Path, required=True)
    builder.add_argument("--output", type=Path, required=True)
    extractor = sub.add_parser("unpack")
    extractor.add_argument("--archive", type=Path, required=True)
    extractor.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    if args.command == "inventory":
        print(json.dumps(inventory(args.source_root.expanduser().resolve()), indent=2))
    elif args.command == "build":
        build(args)
    else:
        unpack(args.archive, args.destination)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, subprocess.SubprocessError, tarfile.TarError):
        raise SystemExit("Bundle operation failed; check metadata and private inputs (contents suppressed).") from None
