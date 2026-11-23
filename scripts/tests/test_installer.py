#!/usr/bin/env python3
"""Exercise release downloads, verification, platform detection, and replacement offline."""

import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
INSTALLER = ROOT / 'install.sh'
BINARIES = ('tkm', 'tkm-hook', 'tkmd')


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.commands = self.base / 'commands'
        self.commands.mkdir()
        self.assets = self.base / 'assets'
        self.assets.mkdir()
        self.destination = self.base / 'bin with spaces'
        self.log = self.base / 'downloads'
        self.env = {
            **os.environ,
            'HOME': str(self.base / 'home'),
            'PATH': f'{self.commands}:{os.environ["PATH"]}',
            'TEST_ASSETS': str(self.assets),
            'TEST_LOG': str(self.log),
            'TEST_OS': 'Linux',
            'TEST_ARCH': 'x86_64',
        }
        for key in ('GH_TOKEN', 'GITHUB_TOKEN', 'TKM_INSTALL_DIR', 'TKM_VERSION'):
            self.env.pop(key, None)
        self.command('uname', '''#!/bin/sh
case "$1" in -s) echo "$TEST_OS";; -m) echo "$TEST_ARCH";; *) exit 1;; esac
''')
        self.command('curl', '''#!/bin/bash
set -eu
printf '%s\\n' "$*" >> "$TEST_LOG"
[ "${TEST_DOWNLOAD_FAIL:-0}" = 0 ] || exit 22
output=
while [ "$#" -gt 0 ]; do
  case "$1" in -o|--output) output=$2; shift 2;; *) url=$1; shift;; esac
done
if [[ "$url" == */releases/assets/* ]]; then
  url="$(cat "$TEST_ASSETS/${url##*/}.name")"
fi
cp "$TEST_ASSETS/${url##*/}" "$output"
''')
        self.command('gh', '''#!/bin/bash
set -eu
printf '%s\\n' "$*" >> "$TEST_LOG"
[ "${TEST_DOWNLOAD_FAIL:-0}" = 0 ] || exit 1
directory=.
while [ "$#" -gt 0 ]; do
  case "$1" in --dir) directory=$2; shift 2;; *) shift;; esac
done
cp "$TEST_ASSETS/"* "$directory/"
''')
        self.make_archive()

    def command(self, name, source):
        path = self.commands / name
        path.write_text(source)
        path.chmod(0o755)

    def make_archive(self, target='x86_64', corrupt_binary=False, platform='unknown-linux-musl'):
        archive = self.assets / f'tokenimum-{target}-{platform}.tar.gz'
        contents = {name: f'#!/bin/sh\necho {name}-fixture\n'.encode() for name in BINARIES}
        sums = ''.join(f'{hashlib.sha256(data).hexdigest()}  {name}\n'
                       for name, data in contents.items()).encode()
        if corrupt_binary:
            contents['tkmd'] += b'# corrupted\n'
        contents['SHA256SUMS'] = sums
        with tarfile.open(archive, 'w:gz') as package:
            for name, data in contents.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                info.mode = 0o755 if name in BINARIES else 0o644
                package.addfile(info, io.BytesIO(data))
        archive.with_suffix('.gz.sha256').write_text(
            f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n')
        return archive

    def run_installer(self, *arguments):
        return subprocess.run(
            ['bash', str(INSTALLER), '--bin-dir', str(self.destination), *arguments],
            env=self.env, text=True, capture_output=True, timeout=15)

    def assert_installed(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for name in BINARIES:
            self.assertTrue(os.access(self.destination / name, os.X_OK))
            output = subprocess.check_output([self.destination / name], text=True)
            self.assertEqual(output.strip(), f'{name}-fixture')

    def test_curl_downloads_latest_and_installs_all_binaries(self):
        self.assert_installed(self.run_installer('--method', 'curl'))
        self.assertIn('/releases/latest/download/', self.log.read_text())
        self.assertIn(str(self.destination), self.run_installer('--help').stdout)

    def test_gh_downloads_explicit_preview(self):
        self.assert_installed(self.run_installer('--method', 'gh', '--version', 'v0.0.0'))
        self.assertIn('release download v0.0.0', self.log.read_text())
        self.assertIn('--repo datamonsterr/tokenimum', self.log.read_text())

    def test_arm64_aliases_select_arm_release(self):
        self.make_archive('aarch64')
        for architecture in ('arm64', 'aarch64'):
            with self.subTest(architecture=architecture):
                self.env['TEST_ARCH'] = architecture
                self.assert_installed(self.run_installer('--method', 'curl', '--version', 'v0.0.0'))
        self.assertIn('/download/v0.0.0/tokenimum-aarch64-', self.log.read_text())

    def test_macos_architectures(self):
        self.env['TEST_OS'] = 'Darwin'
        for architecture, target in (('arm64', 'aarch64'), ('x86_64', 'x86_64')):
            with self.subTest(architecture=architecture):
                self.env['TEST_ARCH'] = architecture
                archive = self.make_archive(target, platform='apple-darwin')
                self.assert_installed(self.run_installer('--archive', str(archive)))

    def test_unsupported_platform_fails_before_download(self):
        for system, architecture in (('Darwin', 'armv7l'), ('MINGW64_NT', 'x86_64'), ('Linux', 'armv7l')):
            with self.subTest(system=system, architecture=architecture):
                self.env.update(TEST_OS=system, TEST_ARCH=architecture)
                result = self.run_installer('--method', 'curl')
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Unsupported', result.stderr)
                self.assertFalse(self.log.exists())

    def test_bad_archive_checksum_preserves_existing_install(self):
        self.destination.mkdir()
        (self.destination / 'tkm').write_text('keep me')
        archive = next(self.assets.glob('*.tar.gz'))
        archive.write_bytes(archive.read_bytes() + b'corrupted')
        result = self.run_installer('--method', 'curl')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.destination / 'tkm').read_text(), 'keep me')
        self.assertFalse((self.destination / 'tkmd').exists())

    def test_bad_binary_checksum_prevents_install(self):
        self.make_archive(corrupt_binary=True)
        result = self.run_installer('--method', 'curl')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.destination / 'tkm').exists())

    def test_authenticated_curl_resolves_private_assets_via_api(self):
        assets = []
        for number, asset in enumerate(sorted(self.assets.iterdir())):
            url = f'https://api.github.com/repos/datamonsterr/tokenimum/releases/assets/{number}'
            assets.append({'name': asset.name, 'url': url})
            (self.assets / f'{number}.name').write_text(asset.name)
        (self.assets / 'v0.0.0').write_text(json.dumps({'assets': assets}))
        self.env['GH_TOKEN'] = 'test-token'
        self.assert_installed(self.run_installer('--method', 'curl', '--version', 'v0.0.0'))
        self.assertIn('/releases/tags/v0.0.0', self.log.read_text())
        self.assertIn('/releases/assets/', self.log.read_text())

    def test_missing_binary_checksum_prevents_install(self):
        archive = next(self.assets.glob('*.tar.gz'))
        with tarfile.open(archive) as package:
            contents = {entry.name: package.extractfile(entry).read() for entry in package}
        contents['SHA256SUMS'] = contents['SHA256SUMS'].splitlines(keepends=True)[0]
        with tarfile.open(archive, 'w:gz') as package:
            for name, data in contents.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                info.mode = 0o755
                package.addfile(info, io.BytesIO(data))
        archive.with_suffix('.gz.sha256').write_text(
            f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n')
        self.assertNotEqual(self.run_installer('--method', 'curl').returncode, 0)
        self.assertFalse((self.destination / 'tkm').exists())

    def test_download_failure_never_installs(self):
        self.env['TEST_DOWNLOAD_FAIL'] = '1'
        for method in ('curl', 'gh'):
            result = self.run_installer('--method', method)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((self.destination / 'tkm').exists())

    def test_update_replaces_a_running_executable(self):
        self.destination.mkdir()
        running = self.destination / 'tkm'
        shutil.copyfile('/bin/bash', running)
        running.chmod(0o755)
        # Keep the copied binary running in a builtin with no child process.
        with subprocess.Popen([running, '-c', 'read -r line'], stdin=subprocess.PIPE) as process:
            try:
                self.assert_installed(self.run_installer('--method', 'curl'))
                self.assertIsNone(process.poll())
            finally:
                process.terminate()
                process.wait(timeout=5)

    def test_local_release_archive_needs_no_download(self):
        archive = next(self.assets.glob('*.tar.gz'))
        self.assert_installed(self.run_installer('--archive', str(archive)))
        self.assertFalse(self.log.exists())

    def test_invalid_arguments_fail_without_downloading(self):
        for arguments in (('--method', 'wget'), ('--version', '../bad'), ('--bin-dir',), ('--unknown',)):
            with self.subTest(arguments=arguments):
                self.assertNotEqual(self.run_installer(*arguments).returncode, 0)
                self.assertFalse(self.log.exists())


if __name__ == '__main__':
    unittest.main()
