import importlib.util
import pathlib
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "compare_bench", pathlib.Path(__file__).resolve().parents[1] / "compare_bench.py"
)
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)


def sample(**benches):
    return "\n".join(f"test {n} ... bench: {v:,} ns/iter (+/- 5)" for n, v in benches.items()) + "\n"


def samples(name, values):
    return [sample(**{name: v}) for v in values]


class CompareBenchTests(unittest.TestCase):
    def test_collect_groups_values_per_bench(self):
        got = module.collect([sample(a=10, b=20), sample(a=12)])
        self.assertEqual(got, {"a": [10, 12], "b": [20]})

    def test_median_of_samples(self):
        self.assertEqual(module.median([5, 1, 3]), 3)
        self.assertEqual(module.median([1, 2, 3, 4]), 2.5)

    def test_identical_runs_pass(self):
        rows = module.compare(samples("a", [100, 100, 100]), samples("a", [100, 100, 100]), 1.2)
        self.assertEqual(rows[0]["status"], "ok")

    def test_single_outlier_does_not_move_median(self):
        head = samples("a", [100, 101, 99, 400, 100])
        base = samples("a", [100, 100, 100, 100, 100])
        row = module.compare(head, base, 1.2)[0]
        self.assertEqual(row["ratio"], 1.0)
        self.assertEqual(row["status"], "ok")

    def test_real_regression_fails(self):
        head = samples("a", [150, 151, 149, 150, 150])
        base = samples("a", [100, 101, 99, 100, 100])
        row = module.compare(head, base, 1.2)[0]
        self.assertEqual(row["status"], "regression")
        self.assertAlmostEqual(row["ratio"], 1.5, places=2)

    def test_over_threshold_inside_noise_passes(self):
        # Head median is 1.3x base, but head samples span far more than 30%: not distinguishable.
        head = samples("a", [90, 130, 200, 130, 100])
        base = samples("a", [100, 100, 100, 100, 100])
        row = module.compare(head, base, 1.2)[0]
        self.assertGreater(row["ratio"], 1.2)
        self.assertEqual(row["status"], "noisy")

    def test_noise_in_base_also_widens_margin(self):
        head = samples("a", [130, 130, 130])
        base = samples("a", [60, 100, 140])
        self.assertEqual(module.compare(head, base, 1.2)[0]["status"], "noisy")

    def test_threshold_is_strict(self):
        head = samples("a", [120, 120, 120])
        base = samples("a", [100, 100, 100])
        self.assertEqual(module.compare(head, base, 1.2)[0]["status"], "ok")

    def test_bench_in_one_side_only_is_not_a_regression(self):
        rows = module.compare(samples("a", [1, 1]), samples("b", [1, 1]), 1.2)
        self.assertEqual({r["name"]: r["status"] for r in rows}, {"a": "new", "b": "removed"})

    def test_main_exit_codes(self):
        with tempfile.TemporaryDirectory() as d:
            d = pathlib.Path(d)
            for i in range(3):
                (d / f"h{i}").write_text(sample(a=150))
                (d / f"b{i}").write_text(sample(a=100))
            heads = [str(d / f"h{i}") for i in range(3)]
            bases = [str(d / f"b{i}") for i in range(3)]
            self.assertEqual(module.main(["x", "--threshold", "1.2", "--head", *heads, "--base", *bases]), 1)
            self.assertEqual(module.main(["x", "--threshold", "2", "--head", *heads, "--base", *bases]), 0)
            self.assertEqual(module.main(["x", "--head", *heads, "--base", str(d / "none")]), 2)


if __name__ == "__main__":
    unittest.main()
