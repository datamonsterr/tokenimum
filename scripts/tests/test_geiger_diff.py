"""Regression tests for Geiger's unscanned file owner and allowlist handling."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'geiger_diff.py'


class GeigerDiffTests(unittest.TestCase):
    def run_report(self, paths, rows):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'docs').mkdir()
            (root / 'docs/unsafe-allowlist.md').write_text(
                '| crate | reason |\n| --- | --- |\n' + rows, encoding='utf-8'
            )
            report = root / 'report.json'
            report.write_text(json.dumps({
                'packages': [],
                'packages_without_metrics': [],
                'used_but_not_scanned_files': paths,
            }), encoding='utf-8')
            return subprocess.run(
                ['python3', str(SCRIPT), str(report)], cwd=root,
                capture_output=True, text=True, check=False,
            )

    def test_build_output_owner_and_separator(self):
        result = self.run_report([
            '/runner/target/debug/build/libsqlite3-sys-13c03cd99ceaa06c/out/bindgen.rs',
            '/runner/target/release/build/serde_core-0123456789abcdef/out/private.rs',
            '/cargo/registry/src/index/rustversion-1.0.0/src/lib.rs',
        ], '| libsqlite3-sys | test |\n| serde_core | test |\n| rustversion | test |\n')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn('unknown owner', result.stdout)
        self.assertNotIn('  ---\n', result.stdout)

    def test_unrecognized_build_path_fails_closed(self):
        result = self.run_report([
            '/runner/target/debug/build/libsqlite3-sys-invalid/out/bindgen.rs',
        ], '| libsqlite3-sys | test |\n')
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn('unknown owner', result.stdout)

    def test_non_rust_data_files_are_not_owners(self):
        result = self.run_report([
            '/runner/work/tokenimum/shims/opencode.ts',
            '/runner/work/tokenimum/crates/tkm-hosts/src/generated.rs',
        ], '')
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertNotIn('opencode.ts', result.stdout)
        self.assertIn('generated.rs', result.stdout)


if __name__ == '__main__':
    unittest.main()
