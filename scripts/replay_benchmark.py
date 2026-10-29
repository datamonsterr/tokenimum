#!/usr/bin/env python3
"""Private live runner for `tkm bench history`; invoked only by workflow_dispatch.

All subprocess output stays private. Only the final Rust-written aggregate report
is uploaded. This driver never reads the invoking developer's Claude home.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
import uuid


def run(argv, *, cwd=None, env=None, data=None, timeout=3600, check=True):
    return subprocess.run([str(v) for v in argv], cwd=cwd, env=env, input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout, check=check)


def git(repo, *args, **kwargs):
    return run(["git", "-C", repo, *args], **kwargs).stdout.decode().strip()


def json_write(path, value):
    path.write_text(json.dumps(value, indent=2))
    path.chmod(0o600)


def home_env(home):
    # Explicit per-process HOME; never mutate the caller's HOME or inspect it.
    env = os.environ.copy()
    for name in list(env):
        if name.startswith(("CLAUDE_", "TKM_", "XDG_", "SUPERORCHESTRATOR_")):
            env.pop(name)
    for name in ("ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"):
        if os.environ.get(name):
            env[name] = os.environ[name]
    env.update(HOME=str(home), XDG_DATA_HOME=str(home / ".local/share"), XDG_CONFIG_HOME=str(home / ".config"), XDG_CACHE_HOME=str(home / ".cache"), XDG_RUNTIME_DIR=str(home / ".runtime"), DISABLE_AUTOUPDATER="1", CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1", GIT_TERMINAL_PROMPT="0")
    (home / ".runtime").mkdir(mode=0o700)
    return env


def snapshot(home, repo, destination):
    destination.mkdir()
    for name in (".claude", ".claude.json", ".config/tokenimum/config.toml", ".config/systemd/user/tkmd.service"):
        source = home / name
        if source.exists():
            target = destination / name
            target.parent.mkdir(parents=True, exist_ok=True)
            if source.is_dir():
                shutil.copytree(source, target, symlinks=True)
            else:
                shutil.copyfile(source, target)
    for name in ("CLAUDE.md", ".claude", ".mcp.json", ".otherskill", "otherskill.config.json", "AGENTS.md"):
        source = repo / name
        if source.exists():
            target = destination / "project" / name
            target.parent.mkdir(parents=True, exist_ok=True)
            if source.is_dir():
                shutil.copytree(source, target, symlinks=True)
            else:
                shutil.copyfile(source, target)


def checkout(repo, start, empty):
    git(repo, "reset", "--hard", start or empty)
    git(repo, "clean", "-fdx")


def changed_files(repo, start, end):
    # NUL delimiters preserve spaces/newlines in filenames; names never leave private state.
    return set(run(["git", "-C", repo, "diff", "--name-only", "-z", start, end]).stdout.split(b"\0")) - {b""}


def overlap(original, replay):
    union = original | replay
    return len(original & replay) / len(union) if union else 1.0


def preflight(manifest, plan, reference):
    if not re.fullmatch(r"\d+\.\d+\.\d+", manifest["claude_version"]):
        raise ValueError("Claude release must be pinned")
    version = run(["claude", "--version"]).stdout.decode()
    if version.split()[0] != manifest["claude_version"]:
        raise ValueError("installed Claude release differs from bundle")
    if not manifest["tests"] or any(not command or not all(isinstance(arg, str) for arg in command) for command in manifest["tests"]):
        raise ValueError("CI-equivalent project test commands are required")
    if len(plan["control"]) < 10 or len({session["id"] for session in plan["control"]}) < 10:
        raise ValueError("ten distinct controls are required")
    for session in plan["replay"] + plan["control"]:
        for commit in (session["start_commit"], session["end_commit"]):
            if commit:
                git(reference, "cat-file", "-e", commit + "^{commit}")


def measure_transcripts(tkm, manifest, home, session_id, private, start, env):
    paths = sorted((home / ".claude/projects").rglob("*.jsonl"))
    roots = [path for path in paths if path.stem == session_id]
    if len(roots) != 1:
        raise ValueError("missing or ambiguous persisted replay transcript")
    specs = []
    for index, path in enumerate(paths):
        is_root = path == roots[0]
        specs.append({"id": session_id if is_root else f"so/{index}", "path": str(path), "parent": None if is_root else session_id, "start_commit": start})
    measured_manifest = {**manifest, "sessions": specs}
    manifest_path = private / "measured-manifest.json"
    metrics_path = private / "arm-baseline.json"
    json_write(manifest_path, measured_manifest)
    run([tkm, "bench", "history", "baseline", "--manifest", manifest_path, "--out", metrics_path], env=env)
    measured = json.loads(metrics_path.read_text())
    # Losing one agent transcript would make the arm look artificially cheap.
    if measured["skipped"] or set(measured["sessions"]) != {session_id}:
        raise ValueError("incomplete replay or agent transcript")
    return measured["sessions"][session_id]


def wait_for_socket(home, daemon):
    socket = home / ".runtime/tokenimum/tkmd.sock"
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if socket.exists():
            return
        if daemon.poll() is not None:
            raise ValueError("tokenimum daemon did not start")
        time.sleep(0.1)
    raise ValueError("tokenimum daemon startup timeout")


def export_overhead(tkm, repo, env, daemon):
    # Flush ingestion/folding before reading the normal redacted export.
    daemon.terminate()
    daemon.wait(timeout=60)
    exported = json.loads(run([tkm, "export"], cwd=repo, env=env).stdout)
    if not exported["sessions"]:
        raise ValueError("tokenimum did not observe the replay")
    if any(session["fold_status"] in {"open", "pending", "error"} for session in exported["sessions"]):
        raise ValueError("tokenimum data was not completely folded")
    return {
        "decision_tokens": sum(ledger["overhead_tokens"] for ledger in exported["ledger"]),
        "decision_cost_usd": sum(decision["cost_usd"] for decision in exported["decisions"]),
        "decisions": len(exported["decisions"]),
        "redirects": exported["activity"]["redirects"],
        "folds": exported["activity"]["folds"],
    }


def replay_session(args, manifest, session, repo, empty, arm, index, anchored):
    private = args.work / f"{arm}-{index:04}"
    private.mkdir()
    home = private / "home"
    shutil.copytree(args.bundle / "home-template", home)
    env = home_env(home)
    if anchored or index == 0:
        checkout(repo, session["start_commit"], empty)
    start = git(repo, "rev-parse", "HEAD")
    before = private / "config-a"
    after = private / "config-b"
    snapshot(home, repo, before)
    # Verify the template itself has no tokenimum installation.
    for path in before.rglob("*"):
        if path.is_file() and (path.name == "SKILL.md" and path.parent.name == "tokenimum" or path.suffix == ".json" and b"tkm-hook" in path.read_bytes()):
            raise ValueError("baseline template already contains tokenimum")
    daemon = None
    try:
        if arm == "replay":
            # Install fully, while keeping service-manager side effects inside this run.
            install_bin = private / "install-bin"
            install_bin.mkdir()
            for name, status in (("systemctl", 1), ("tkmd", 0)):
                shim = install_bin / name
                shim.write_text(f"#!/bin/sh\nexit {status}\n")
                shim.chmod(0o700)
            init_env = {**env, "PATH": str(install_bin) + os.pathsep + env["PATH"], "TKM_TKMD_BIN": str(install_bin / "tkmd")}
            run([args.tkm, "init", "--host", "claude-code", "--home", home / ".claude", "--yes"], cwd=repo, env=init_env)
        snapshot(home, repo, after)
        run([args.tkm, "bench", "history", "config-check", "--a", before, "--b", after], env=env)
        # Both installed and control config are checked before the first live prompt.
        if arm == "replay":
            daemon = subprocess.Popen([str(args.tkm.parent / "tkmd")], cwd=repo, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            wait_for_socket(home, daemon)
        session_id = str(uuid.uuid4())
        successful = True
        for turn, prompt in enumerate(session["prompts"]):
            argv = ["claude", "-p", "--output-format", "stream-json", "--verbose", "--model", prompt["model"], "--effort", manifest["effort"], "--permission-mode", manifest["permission_mode"]]
            argv += ["--session-id", session_id] if turn == 0 else ["--resume", session_id]
            result = run(argv, cwd=repo, env=env, data=prompt["text"].encode(), check=False)
            (private / f"turn-{turn:04}.jsonl").write_bytes(result.stdout)
            (private / f"turn-{turn:04}.stderr").write_bytes(result.stderr)
            events = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
            final = [event for event in events if event.get("type") == "result"]
            if result.returncode or not final or final[-1].get("is_error") or final[-1].get("subtype") != "success":
                successful = False
                break
        metrics = measure_transcripts(args.tkm, manifest, home, session_id, args.work, start, env)
        # Test the final replay tree, including committed and untracked edits.
        tests_passed = successful
        for command in manifest["tests"]:
            result = run(command, cwd=repo, env=env, check=False)
            tests_passed = tests_passed and result.returncode == 0
        git(repo, "add", "-A")
        tree = git(repo, "write-tree")
        original = changed_files(repo, session["start_commit"] or empty, session["end_commit"])
        replayed = changed_files(repo, session["start_commit"] or empty, tree)
        result = {"metrics": metrics, "outcome": {"tests_passed": tests_passed, "changed_file_overlap": overlap(original, replayed)}, "decision_tokens": 0, "decision_cost_usd": 0.0, "decisions": 0, "redirects": 0, "folds": 0}
        if daemon:
            result.update(export_overhead(args.tkm, repo, env, daemon))
        return result
    finally:
        if daemon and daemon.poll() is None:
            daemon.terminate()
            daemon.wait(timeout=60)


def clone(reference, destination):
    run(["git", "clone", "--no-hardlinks", reference, destination])
    git(destination, "remote", "remove", "origin")
    git(destination, "config", "user.name", "Replay benchmark")
    git(destination, "config", "user.email", "benchmark@example.invalid")
    git(destination, "checkout", "--orphan", "replay-empty")
    git(destination, "rm", "-rf", "--ignore-unmatch", ".")
    git(destination, "clean", "-fdx")
    git(destination, "commit", "--allow-empty", "-m", "Empty replay starting state")
    return git(destination, "rev-parse", "HEAD")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--tkm", type=Path, required=True)
    parser.add_argument("--subset", choices=["pilot", "sample", "full"], default="pilot")
    parser.add_argument("--confirm-full", action="store_true")
    parser.add_argument("--free-running", action="store_true")
    args = parser.parse_args()
    if args.subset == "full" and not args.confirm_full:
        parser.error("full requires --confirm-full")
    if not os.environ.get("ANTHROPIC_API_KEY") and not os.environ.get("CLAUDE_CODE_OAUTH_TOKEN"):
        raise ValueError("Claude repository secret missing")
    if not os.environ.get("OPENROUTER_API_KEY"):
        raise ValueError("decision-model repository secret missing")
    os.umask(0o077)
    args.bundle = args.bundle.resolve(strict=True)
    args.reference = args.reference.resolve(strict=True)
    args.tkm = args.tkm.resolve(strict=True)
    args.work = args.work.resolve()
    args.work.mkdir(mode=0o700)
    manifest_path = args.bundle / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    baseline_path = args.work / "baseline.json"
    plan_path = args.work / "plan.json"
    run([args.tkm, "bench", "history", "baseline", "--manifest", manifest_path, "--out", baseline_path])
    argv = [args.tkm, "bench", "history", "plan", "--manifest", manifest_path, "--out", plan_path, "--subset", args.subset]
    if args.confirm_full:
        argv.append("--confirm-full")
    run(argv)
    baseline = json.loads(baseline_path.read_text())
    plan = json.loads(plan_path.read_text())
    estimate = {arm: sum(baseline["sessions"][session["id"]]["cost_usd"] for session in sessions) for arm, sessions in plan.items()}
    print("Cost estimate from baseline.json (historical USD, decision overhead extra): " + json.dumps(estimate), flush=True)
    preflight(manifest, plan, args.reference)
    measurements = {"mode": "free-running" if args.free_running else "anchored", "replay": {}, "control": {}}
    for arm in ("replay", "control"):
        repo = args.work / ("taphoa-replay" if arm == "replay" else "taphoa-control")
        empty = clone(args.reference, repo)
        for index, session in enumerate(plan[arm]):
            measurements[arm][session["id"]] = replay_session(args, manifest, session, repo, empty, arm, index, arm == "control" or not args.free_running)
            json_write(args.work / "measurements.json", measurements)
            print(f"Completed {arm} {index + 1}/{len(plan[arm])}", flush=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    run([args.tkm, "bench", "history", "report", "--baseline", baseline_path, "--measurements", args.work / "measurements.json", "--out", args.report])
    print("Aggregate report written; private run files are excluded from artifacts.")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, subprocess.SubprocessError):
        raise SystemExit("Replay stopped; check the private inputs or runner state (contents suppressed).") from None
