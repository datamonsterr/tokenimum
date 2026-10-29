#!/usr/bin/env python3
"""Check that the performance gate inventory does not admit placeholders."""
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class GatesTest(unittest.TestCase):
    def test_no_pending_gates_or_placeholder(self):
        adr = (ROOT / 'docs/adr/0046-performance-gates.md').read_text()
        workflow = (ROOT / '.github/workflows/bench.yml').read_text()
        self.assertNotIn('| Pending |', adr)
        self.assertNotIn('--bench placeholder', workflow)
        for required in ('prompt/scored', 'prompt/cached', 'fold_session', 'shim-opencode', 'shim-pi'):
            self.assertIn(required, workflow)


if __name__ == '__main__':
    unittest.main()
