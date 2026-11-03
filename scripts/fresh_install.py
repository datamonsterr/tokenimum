#!/usr/bin/env python3
"""SPEC 15 M3: fresh machine to working install in one command.

Runs the release `tkm`, `tkmd` and `tkm-hook` binaries under a fresh HOME that holds nothing
but an empty Claude home. It runs the SPEC 11.1 one-command install, then asserts that
`tkm doctor` passes and that a recorded Claude hook payload makes a round trip through the
daemon. Nothing outside the temporary directory is touched.

Usage: fresh_install.py [--bin-dir target/release]
"""
import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HOOK_FIXTURE = ROOT / 'fixtures/claude/hooks/UserPromptSubmit.json'


class InstallError(Exception):
    pass


def fresh_env(base, bin_dir):
    """Environment of a machine that has never run tokenimum. Only PATH is inherited."""
    home = Path(base) / 'home'
    for sub in ('.claude', '.config', '.local/share', '.local/state', 'run'):
        (home / sub).mkdir(parents=True, exist_ok=True)
    (home / 'run').chmod(0o700)
    system_path = os.environ.get('PATH', '/usr/bin:/bin')
    return {
        'PATH': f'{Path(bin_dir).resolve()}:{system_path}',
        'HOME': str(home),
        'XDG_CONFIG_HOME': str(home / '.config'),
        'XDG_DATA_HOME': str(home / '.local/share'),
        'XDG_STATE_HOME': str(home / '.local/state'),
        'XDG_RUNTIME_DIR': str(home / 'run'),
        'TKM_TKMD_BIN': str(Path(bin_dir).resolve() / 'tkmd'),
    }


def run(argv, env, stdin=None, timeout=60):
    done = subprocess.run(argv, env=env, input=stdin, capture_output=True, text=True,
                          timeout=timeout, check=False)
    return done


def require(done, what):
    if done.returncode != 0:
        raise InstallError(f'{what} exited {done.returncode}\n{done.stdout}\n{done.stderr}')
    return done.stdout


def wait_for_socket(env, seconds=10):
    socket = Path(env['XDG_RUNTIME_DIR']) / 'tokenimum' / 'tkmd.sock'
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if socket.exists():
            return socket
        time.sleep(0.1)
    raise InstallError(f'daemon socket {socket} never appeared')


def errors_log(env):
    path = Path(env['XDG_DATA_HOME']) / 'tokenimum' / 'errors.log'
    return path.read_text() if path.exists() else ''


def install(bin_dir, base):
    """One-command install, doctor and hook round trip. Returns the hook reply text."""
    env = fresh_env(base, bin_dir)
    require(run(['tkm', 'init', '--yes', '--only', 'claude', '--no-shims', '--preset', 'offline'], env),
            'tkm init')
    settings = Path(env['HOME']) / '.claude/settings.json'
    if 'tkm-hook' not in settings.read_text():
        raise InstallError(f'{settings} has no tokenimum hook entry')
    payload = HOOK_FIXTURE.read_text()
    json.loads(payload)
    hook = ['tkm-hook', '--host', 'claude', '--event', 'prompt']
    # The first hook call finds no daemon, replies with the no-op and starts one.
    require(run(hook, env, payload), 'first hook call')
    wait_for_socket(env)
    reply = require(run(hook, env, payload), 'hook round trip')
    if reply.strip():
        json.loads(reply)
    require(run(['tkm', 'doctor'], env), 'tkm doctor')
    if errors_log(env).strip():
        raise InstallError(f'errors.log is not empty:\n{errors_log(env)}')
    return reply


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', default=str(ROOT / 'target/release'))
    args = parser.parse_args(argv)
    for name in ('tkm', 'tkmd', 'tkm-hook'):
        if not (Path(args.bin_dir) / name).is_file():
            parser.exit(2, f'missing release binary {Path(args.bin_dir) / name}\n')
    with tempfile.TemporaryDirectory(prefix='tkm-fresh-') as base:
        try:
            install(args.bin_dir, base)
        except InstallError as error:
            print(f'fresh install failed: {error}', file=sys.stderr)
            return 1
        finally:
            # Stop the daemon the first hook call started.
            daemon = f'{Path(args.bin_dir).resolve()}/tkmd'
            subprocess.run(['pkill', '-f', daemon], check=False)
            # Let it exit before the temporary home is removed; it may still be writing there.
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline and subprocess.run(
                    ['pgrep', '-f', daemon], capture_output=True, check=False).returncode == 0:
                time.sleep(0.1)
    print('fresh install ok: init, doctor and hook round trip')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
