#!/usr/bin/env python3
"""fresh_install.py isolates HOME, and fails loudly when a step fails."""
import importlib.util
import os
import pathlib
import stat
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('fresh_install', ROOT / 'scripts/fresh_install.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def stub(path, body):
    path.write_text('#!/bin/sh\n' + body)
    path.chmod(path.stat().st_mode | stat.S_IEXEC)


class FreshInstallTest(unittest.TestCase):
    def test_env_is_confined_to_the_temp_dir(self):
        with tempfile.TemporaryDirectory() as base:
            env = module.fresh_env(base, base)
            self.assertEqual(set(env), {'PATH', 'HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME',
                                        'XDG_STATE_HOME', 'XDG_RUNTIME_DIR', 'TKM_TKMD_BIN'})
            for key, value in env.items():
                if key not in ('PATH',):
                    self.assertTrue(value.startswith(base) or os.path.realpath(value).startswith(os.path.realpath(base)), key)
            self.assertEqual(os.listdir(env['HOME'] + '/.claude'), [])

    def test_failed_step_raises_with_output(self):
        with tempfile.TemporaryDirectory() as base:
            bin_dir = pathlib.Path(base) / 'bin'
            bin_dir.mkdir()
            stub(bin_dir / 'tkm', 'echo broken; exit 3\n')
            with self.assertRaises(module.InstallError) as caught:
                module.install(bin_dir, pathlib.Path(base) / 'work')
            self.assertIn('tkm init exited 3', str(caught.exception))
            self.assertIn('broken', str(caught.exception))

    def test_passing_install_with_fake_binaries_returns_the_reply(self):
        with tempfile.TemporaryDirectory() as base:
            bin_dir = pathlib.Path(base) / 'bin'
            bin_dir.mkdir()
            stub(bin_dir / 'tkm', 'if [ "$1" = init ]; then echo \'tkm-hook\' > "$HOME/.claude/settings.json"; fi\n')
            stub(bin_dir / 'tkm-hook',
                 'mkdir -p "$XDG_RUNTIME_DIR/tokenimum"; touch "$XDG_RUNTIME_DIR/tokenimum/tkmd.sock"; echo \'{}\'\n')
            self.assertEqual(module.install(bin_dir, pathlib.Path(base) / 'work').strip(), '{}')


if __name__ == '__main__':
    unittest.main()
