#!/usr/bin/env python3
"""Exercise README examples with installed Linux binaries in an isolated HOME.

Run from a checkout: python3 scripts/verify_readme.py --bin-dir /path/to/binaries
No credentials or real agent configuration are inherited. No Rust build is run.
"""
import argparse
import json
import os
from pathlib import Path
import pty
import select
import signal
import subprocess
import tempfile
import time

from fresh_install import fresh_env

ROOT = Path(__file__).resolve().parents[1]


def stop_daemon(env):
    # init can start a detached daemon. Stop only processes whose environment
    # names this temporary HOME; never signal an existing user daemon.
    for entry in Path('/proc').iterdir():
        if not entry.name.isdecimal() or int(entry.name) == os.getpid():
            continue
        try:
            values = (entry / 'environ').read_bytes().split(b'\0')
            executable = (entry / 'comm').read_text().strip()
            if executable == 'tkmd' and f'HOME={env["HOME"]}'.encode() in values:
                os.kill(int(entry.name), signal.SIGTERM)
        except (OSError, ProcessLookupError):
            pass
    deadline = time.monotonic() + 5
    for entry in Path('/proc').iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            while (entry / 'comm').read_text().strip() == 'tkmd':
                if f'HOME={env["HOME"]}'.encode() not in (entry / 'environ').read_bytes().split(b'\0'):
                    break
                if time.monotonic() >= deadline:
                    raise RuntimeError('temporary daemon did not stop')
                time.sleep(0.05)
        except OSError:
            pass


def verify(bin_dir, base):
    env = fresh_env(base, bin_dir)
    # Exercise the detached-daemon path without controlling the runner's user services.
    managers = Path(base) / 'service-managers'
    managers.mkdir()
    systemctl = managers / 'systemctl'
    systemctl.write_text('#!/bin/sh\nexit 1\n')
    systemctl.chmod(0o755)
    env['PATH'] = f'{managers}:{env["PATH"]}'
    project = Path(base) / 'project'
    project.mkdir()
    (Path(env['HOME']) / '.claude').mkdir(exist_ok=True)
    subprocess.run(['git', 'init', '-q', str(project)], check=True, env=env)
    count = 0

    def run(*args, expected=0):
        nonlocal count
        result = subprocess.run(['tkm', *args], env=env, cwd=project,
                                text=True, capture_output=True, timeout=60)
        if result.returncode != expected:
            raise RuntimeError(f'tkm {" ".join(args)}: {result.returncode}\n'
                               f'{result.stdout}\n{result.stderr}')
        if expected == 1 and 'unknown action event' not in result.stderr:
            raise RuntimeError('missing expected no-action diagnostic')
        print(f'PASS tkm {" ".join(args)}')
        count += 1
        return result.stdout

    try:
        run('--help')
        run('init', '--help')
        run('init', '--dry-run', '--preset', 'offline')
        run('init', '--yes', '--preset', 'offline')
        run('doctor')
        run('status')
        run('init', '--yes', '--only', 'claude', '--preset', 'offline')
        run('facts', '--brief')
        run('why', 'last', expected=1)  # No action exists in this fresh repository.
        run('prune')
        run('config', 'show', '--effective')
        exported = run('export')
        json.loads(exported)
        (project / 'tokenimum-state.json').write_text(exported)
        run('config', 'preset', 'offline')
        run('replay', '--bench', str(ROOT / 'fixtures/bench'))
        # Give the actual interactive monitor a PTY; wait for a frame before quitting.
        master, slave = pty.openpty()
        monitor_env = dict(env, TERM='xterm-256color')
        proc = subprocess.Popen(['tkm', 'top'], cwd=project, env=monitor_env,
                                stdin=slave, stdout=slave, stderr=slave)
        os.close(slave)
        try:
            ready, _, _ = select.select([master], [], [], 10)
            if not ready or not os.read(master, 65536):
                raise RuntimeError('tkm top did not render')
            time.sleep(0.5)
            os.write(master, b'q')
            if proc.wait(timeout=10):
                raise RuntimeError('tkm top failed')
            print('PASS tkm top (interactive PTY, quit with q)')
            count += 1
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.wait()
            os.close(master)
        run('uninstall', '--help')
        run('uninstall', '--dry-run')
        stop_daemon(env)
        run('uninstall')
    finally:
        stop_daemon(env)
    print(f'{count} README command checks passed')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', required=True, type=Path)
    args = parser.parse_args()
    bin_dir = args.bin_dir.resolve()
    for name in ('tkm', 'tkm-hook', 'tkmd'):
        if not os.access(bin_dir / name, os.X_OK):
            parser.error(f'{bin_dir / name} must be executable')
    with tempfile.TemporaryDirectory(prefix='tkm-readme-') as base:
        verify(bin_dir, base)


if __name__ == '__main__':
    main()
