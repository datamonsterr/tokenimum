#!/usr/bin/env python3
"""Idle CPU and memory metrics must use strict absolute limits."""
import importlib.util
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('idle_metrics', ROOT / 'scripts/idle_metrics.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class IdleMetricsTest(unittest.TestCase):
    def test_cpu_average_and_strict_budget(self):
        self.assertEqual(module.cpu_percent(100, 124, 100, 30), 0.8)
        self.assertTrue(module.below_budget(0.8, 1.0))
        self.assertFalse(module.below_budget(1.0, 1.0))

    def test_relative_value_uses_one_tick_floor(self):
        floor = 100 / (100 * 30)
        self.assertAlmostEqual(module.regression_cpu(0, 100, 30), floor)
        self.assertAlmostEqual(module.regression_cpu(floor, 100, 30), floor)
        self.assertAlmostEqual(module.regression_cpu(2 * floor, 100, 30), 2 * floor)

    def test_invalid_measurements_fail(self):
        for values in [(10, 9, 100, 30), (0, 1, 0, 30), (0, 1, 100, 0)]:
            with self.assertRaises(ValueError):
                module.cpu_percent(*values)


if __name__ == '__main__':
    unittest.main()
