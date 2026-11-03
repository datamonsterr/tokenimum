#!/usr/bin/env python3
"""Check that release binaries build tkmd with the web dashboard feature (SPEC 11.4)."""
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class ReleaseWebFeatureTest(unittest.TestCase):
    def test_every_release_tkmd_build_enables_web(self):
        workflow = (ROOT / '.github/workflows/release.yml').read_text()
        builds = [line for line in workflow.splitlines()
                  if re.search(r'cargo (zig)?build\b', line) and '-p tkmd' in line]
        self.assertTrue(builds, 'release.yml builds no tkmd binary')
        for line in builds:
            self.assertIn('--features tkmd/web', line, line.strip())


if __name__ == '__main__':
    unittest.main()
