import importlib.util
import pathlib
import unittest

SPEC = importlib.util.spec_from_file_location(
    "check_hook_latency", pathlib.Path(__file__).resolve().parents[1] / "check_hook_latency.py"
)
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)


def export(times):
    return {"results": [{"command": "tkm-hook", "times": times}]}


class HookLatencyTests(unittest.TestCase):
    def test_nearest_rank_percentiles(self):
        stats = module.summarize(export([i / 1000 for i in range(1, 101)]))
        self.assertEqual(stats["runs"], 100)
        self.assertAlmostEqual(stats["p50"], 50.0)
        self.assertAlmostEqual(stats["p99"], 99.0)

    def test_passes_inside_the_limits(self):
        stats = module.summarize(export([0.001] * 99 + [0.004]))
        text, ok = module.render(stats, 3.0, 10.0)
        self.assertTrue(ok)
        self.assertIn("| p50 |", text)

    def test_fails_when_p50_reaches_the_limit(self):
        stats = module.summarize(export([0.003] * 10))
        _, ok = module.render(stats, 3.0, 10.0)
        self.assertFalse(ok)

    def test_fails_when_p99_reaches_the_limit(self):
        stats = module.summarize(export([0.001] * 90 + [0.012] * 10))
        _, ok = module.render(stats, 3.0, 10.0)
        self.assertFalse(ok)

    def test_reads_hyperfine_2_measurements(self):
        measurements = [{"time_wall_clock": {"value": i / 1000}} for i in range(1, 101)]
        stats = module.summarize({"results": [{"measurements": measurements}]})
        self.assertAlmostEqual(stats["p50"], 50.0)
        self.assertAlmostEqual(stats["p99"], 99.0)

    def test_rejects_exports_without_times(self):
        with self.assertRaises(ValueError):
            module.summarize({"results": [{"command": "x"}]})


if __name__ == "__main__":
    unittest.main()
