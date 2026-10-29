#!/usr/bin/env python3
"""The regression series must contain both measured round-trip percentiles."""
import importlib.util
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('roundtrip', ROOT / 'scripts/roundtrip_to_benchmark.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class RoundtripBenchmarkTest(unittest.TestCase):
    def test_both_percentiles_are_exported(self):
        data = {'results': [{'times': [0.001, 0.002, 0.003]}]}
        self.assertEqual(module.convert(data), [
            {'name': 'hook round trip p50', 'unit': 'ms', 'value': 2.0},
            {'name': 'hook round trip p99', 'unit': 'ms', 'value': 3.0},
        ])


if __name__ == '__main__':
    unittest.main()
