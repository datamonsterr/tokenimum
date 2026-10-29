import importlib.util
import pathlib
import unittest

SPEC = importlib.util.spec_from_file_location(
    "check_bench_budget", pathlib.Path(__file__).resolve().parents[1] / "check_bench_budget.py"
)
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)

OUTPUT = """running 1 tests
test tool_done_claude_posttooluse ... bench:      12,345 ns/iter (+/- 1,200)
"""


class BenchBudgetTests(unittest.TestCase):
    def test_parses_bencher_lines_with_thousands_separators(self):
        self.assertEqual(module.parse(OUTPUT), {"tool_done_claude_posttooluse": (12345, 1200)})

    def test_passes_under_the_limit_using_the_upper_estimate(self):
        mean, upper, ok = module.check(OUTPUT, "tool_done_claude_posttooluse", 200_000)
        self.assertEqual((mean, upper, ok), (12345, 13545, True))

    def test_fails_when_mean_plus_deviation_exceeds_the_limit(self):
        _, _, ok = module.check(OUTPUT, "tool_done_claude_posttooluse", 13_000)
        self.assertFalse(ok)

    def test_missing_benchmark_is_an_error(self):
        with self.assertRaises(ValueError):
            module.check(OUTPUT, "nope", 1)

    def test_all_tool_done_cases_are_checked(self):
        output = OUTPUT + "test tool_done_gate_open ... bench: 199,999 ns/iter (+/- 2)\n"
        results = module.check_prefix(output, "tool_done_", 200_000)
        self.assertTrue(results["tool_done_claude_posttooluse"][2])
        self.assertFalse(results["tool_done_gate_open"][2])

    def test_missing_prefix_is_an_error(self):
        with self.assertRaises(ValueError):
            module.check_prefix(OUTPUT, "missing_", 200_000)


if __name__ == "__main__":
    unittest.main()
