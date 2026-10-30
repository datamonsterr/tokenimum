import importlib.util
from pathlib import Path
import tempfile
import unittest

MODULE = Path(__file__).resolve().parents[1] / 'check_spec_conformance.py'
spec = importlib.util.spec_from_file_location('check_spec_conformance', MODULE)
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class CheckConformance(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / 'docs').mkdir()
        (self.root / 'evidence.rs').touch()
        self.spec = '## 1. Section\n### 1.1 Subsection\n'
        self.header = '| ID | Requirement | Status | Evidence |\n|---|---|---|---|\n'
        self.row = '| S1-1 | Requirement | met | [source](../evidence.rs) |\n'
        self.audit = '## 1. Section\n' + self.header + self.row + '### 1.1 Subsection\n' + self.header + self.row.replace('S1-1', 'S1.1-1')

    def errors(self, audit):
        return checker.check(self.spec, audit, self.root)

    def test_valid(self):
        self.assertEqual(self.errors(self.audit), [])

    def test_missing_evidence(self):
        self.assertTrue(any('missing evidence' in e for e in self.errors(self.audit.replace('[source](../evidence.rs)', ''))))

    def test_missing_path(self):
        self.assertTrue(any('does not exist' in e for e in self.errors(self.audit.replace('evidence.rs', 'missing.rs'))))

    def test_missing_inline_path(self):
        audit = self.audit.replace('[source](../evidence.rs)', '[source](../evidence.rs); `crates/missing.rs`', 1)
        self.assertTrue(any('does not exist' in e for e in self.errors(audit)))

    def test_missing_plain_path(self):
        audit = self.audit.replace('[source](../evidence.rs)', '[source](../evidence.rs); crates/missing.rs', 1)
        self.assertTrue(any('does not exist' in e for e in self.errors(audit)))

    def test_missing_arbitrary_test_file(self):
        audit = self.audit.replace('[source](../evidence.rs)', '[source](../evidence.rs); tests/missing.rs', 1)
        self.assertTrue(any('does not exist' in e for e in self.errors(audit)))

    def test_file_outside_repo_is_rejected(self):
        audit = self.audit.replace('../evidence.rs', '../../outside.rs', 1)
        self.assertTrue(any('does not exist' in e for e in self.errors(audit)))

    def test_missing_heading_table(self):
        self.assertTrue(any('missing table' in e for e in self.errors(self.audit.split('###')[0])))

    def test_deferred_requires_docs(self):
        self.assertTrue(any('deferred requires' in e for e in self.errors(self.audit.replace('met', 'deferred', 1))))

    def test_not_applicable_requires_reason(self):
        self.assertTrue(any('requires a reason' in e for e in self.errors(self.audit.replace('met', 'not applicable', 1))))

    def test_escaped_pipe(self):
        self.assertEqual(checker.cells(r'| A | B\|C |'), ['A', 'B|C'])


if __name__ == '__main__':
    unittest.main()
