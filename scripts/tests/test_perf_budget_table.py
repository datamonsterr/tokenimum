#!/usr/bin/env python3
"""Keep ADR 0046's budget inventory synchronized with SPEC section 14."""
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


def rows(text):
    return [tuple(cell.strip() for cell in line.strip().strip('|').split('|'))
            for line in text.splitlines() if line.startswith('|')][2:]


class BudgetTableTest(unittest.TestCase):
    def test_adr_covers_spec_paths_and_exact_budgets(self):
        spec = (ROOT / 'SPEC.md').read_text(encoding='utf-8').split('## 14. Performance budget', 1)[1].split('## 15.', 1)[0]
        adr = (ROOT / 'docs/adr/0046-performance-gates.md').read_text(encoding='utf-8')
        expected = [(row[0], row[1]) for row in rows(spec)]
        actual = [(row[0], row[1]) for row in rows(adr)]
        self.assertEqual(expected, actual)
        self.assertTrue(all(len(row) >= 4 and row[2] and row[3] for row in rows(adr)))


if __name__ == '__main__':
    unittest.main()
