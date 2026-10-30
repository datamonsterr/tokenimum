"""Offline tests: invented bundle paths, mocked model calls, no developer HOME access."""
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


def module(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).parents[1] / f"{name}.py")
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


bundle = module("replay_bundle")
runner = module("replay_benchmark")


class BundleTests(unittest.TestCase):
    def test_inventory_only_selects_the_requested_project_prefix(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in ("-home-dat-dev-taphoa-mini/one.jsonl", "-home-dat-dev-taphoa-mini-worktree/child.jsonl", "other/private.jsonl"):
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_text('{}\n')
            self.assertEqual(bundle.inventory(root), ["-home-dat-dev-taphoa-mini-worktree/child.jsonl", "-home-dat-dev-taphoa-mini/one.jsonl"])

    def test_template_never_copies_credential_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            (source / ".claude").mkdir(parents=True)
            (source / ".claude/.credentials.json").write_text('{"sentinel":"synthetic"}')
            with self.assertRaises(ValueError):
                bundle.copy_template(source, root / "target")
            self.assertFalse((root / "target/.claude/.credentials.json").exists())

    def test_template_refuses_account_metadata_in_global_claude_json(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / ".claude.json").write_text('{"oauthAccount":{"sentinel":"synthetic"}}')
            with self.assertRaises(ValueError):
                bundle.copy_template(root, root / "target")

    def test_unpack_refuses_path_traversal_and_links(self):
        for name, kind in (("../escape", tarfile.REGTYPE), ("bundle/link", tarfile.SYMTYPE)):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                archive = root / "bad.tar.gz"
                with tarfile.open(archive, "w:gz") as tar:
                    member = tarfile.TarInfo(name)
                    member.type = kind
                    member.linkname = "/outside"
                    tar.addfile(member, io.BytesIO())
                with self.assertRaises(ValueError):
                    bundle.unpack(archive, root / "out")


class RunnerTests(unittest.TestCase):
    def test_changed_file_overlap_requires_matching_work_and_handles_no_edits(self):
        self.assertEqual(runner.overlap(set(), set()), 1.0)
        self.assertEqual(runner.overlap({b"a"}, {b"b"}), 0.0)
        self.assertEqual(runner.overlap({b"a", b"b"}, {b"b"}), 0.5)

    def test_control_reuses_session_and_sends_prompts_only_on_stdin(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            work = root / "work"
            work.mkdir()
            (root / "home-template/.claude").mkdir(parents=True)
            repo = root / "repo"
            repo.mkdir()
            args = SimpleNamespace(work=work, bundle=root, tkm=root / "tkm")
            session = {"id": "synthetic", "start_commit": "a" * 40, "end_commit": "b" * 40, "prompts": [{"text": "typed sentinel one", "model": "synthetic-model"}, {"text": "typed sentinel two", "model": "synthetic-model"}]}
            calls = []

            def fake_run(argv, **kwargs):
                calls.append((list(map(str, argv)), kwargs))
                payload = b'{"type":"result","subtype":"success","is_error":false}\n' if argv[0] == "claude" else b""
                return subprocess.CompletedProcess(argv, 0, payload, b"")

            with patch.object(runner, "run", side_effect=fake_run), patch.object(runner, "git", return_value="a" * 40), patch.object(runner, "checkout"), patch.object(runner, "changed_files", return_value={b"widget"}), patch.object(runner, "measure_transcripts", return_value={"cost_usd": 0.1}):
                result = runner.replay_session(args, {"tests": [["synthetic-test"]], "effort": "high", "permission_mode": "default"}, session, repo, "c" * 40, "control", 0, True)
            prompts = [call for call in calls if call[0][0] == "claude"]
            self.assertEqual(len(prompts), 2)
            first_id = prompts[0][0][prompts[0][0].index("--session-id") + 1]
            resumed_id = prompts[1][0][prompts[1][0].index("--resume") + 1]
            self.assertEqual(first_id, resumed_id)
            self.assertEqual(prompts[0][1]["data"], b"typed sentinel one")
            self.assertFalse(any("typed sentinel" in arg for argv, _ in calls for arg in argv))
            self.assertFalse(any("init" in argv for argv, _ in calls))
            self.assertTrue(any("config-check" in argv for argv, _ in calls))
            self.assertTrue(result["outcome"]["tests_passed"])
            self.assertEqual(result["decision_cost_usd"], 0.0)


if __name__ == "__main__":
    unittest.main()
