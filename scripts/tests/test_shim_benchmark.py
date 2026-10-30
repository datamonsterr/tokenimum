#!/usr/bin/env python3
"""Shim benchmarks export distinct p50/p99 series and enforce strict limits."""
import importlib.util
import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
spec = importlib.util.spec_from_file_location('shim_benchmark', ROOT / 'scripts/shim_benchmark.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ShimBenchmarkTest(unittest.TestCase):
    def test_percentiles_and_limits(self):
        data = {'results': [{'times': [0.001, 0.002, 0.003]}]}
        metrics = module.convert(data, 'pi')
        self.assertEqual([m['name'] for m in metrics], ['shim pi p50', 'shim pi p99'])
        self.assertEqual([m['value'] for m in metrics], [2.0, 3.0])
        self.assertTrue(module.within_budget(metrics))
        metrics[1]['value'] = 40.0
        self.assertFalse(module.within_budget(metrics))


if __name__ == '__main__':
    unittest.main()
