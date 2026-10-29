import importlib.util
import pathlib
import unittest

SPEC = importlib.util.spec_from_file_location(
    "check_idle_memory", pathlib.Path(__file__).resolve().parents[1] / "check_idle_memory.py"
)
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)


class IdleMemoryTests(unittest.TestCase):
    def test_uses_maximum_sample(self):
        self.assertEqual(module.check("1024\n2048\n1536\n", 30), (2048, True))

    def test_equal_to_limit_fails(self):
        self.assertEqual(module.check("30720\n1\n2\n", 30), (30720, False))

    def test_rejects_missing_or_malformed_samples(self):
        for samples in ("", "1\n2\n", "1\n2\nbad\n", "-1\n2\n3\n"):
            with self.subTest(samples=samples), self.assertRaises(ValueError):
                module.check(samples, 30)

    def test_rejects_nonpositive_limit(self):
        with self.assertRaises(ValueError):
            module.check("1\n2\n3\n", 0)


if __name__ == "__main__":
    unittest.main()
