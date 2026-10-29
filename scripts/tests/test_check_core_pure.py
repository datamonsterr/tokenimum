#!/usr/bin/env python3
"""Regression tests for core purity source and dependency checks."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "check-core-pure.sh"

class PurityCheck(unittest.TestCase):
    def check(self, source, dependency=None, kind=None):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "crates/tkm-core/src").mkdir(parents=True)
            (root / "crates/tkm-core/src/lib.rs").write_text(source, encoding="utf-8")
            (root / "bin").mkdir()
            cargo = root / "bin/cargo"
            cargo.write_text('#!/bin/sh\ncat "$METADATA_FIXTURE"\n', encoding="utf-8")
            cargo.chmod(0o755)
            packages = [{"id": "core", "name": "tkm-core"}]
            deps = []
            nodes = [{"id": "core", "deps": deps}]
            if dependency:
                packages.append({"id": "dep", "name": dependency})
                deps.append({"pkg": "dep", "dep_kinds": [{"kind": kind}]})
                nodes.append({"id": "dep", "deps": []})
            data = {"packages": packages, "workspace_members": ["core"], "resolve": {"nodes": nodes}}
            metadata = root / "metadata.json"
            metadata.write_text(json.dumps(data), encoding="utf-8")
            env = dict(os.environ, PATH=f'{root / "bin"}:{os.environ["PATH"]}', METADATA_FIXTURE=str(metadata))
            return subprocess.run(["bash", str(SCRIPT)], cwd=root, env=env, capture_output=True, check=False).returncode

    def test_forbidden_sources(self):
        for source in ["use std::{fs, io};", "use std::{io::stdin};", "use std::io::stdout;", "std::io::stderr();", "use std::thread;", "use std::time::{SystemTime, Instant};", "use std::{net, process, env};", "use std::io as console; console::stdin();", "use std::{time, io};"]:
            with self.subTest(source=source):
                self.assertNotEqual(self.check(source), 0)

    def test_allowed_sources(self):
        for source in ["use std::collections::BTreeMap;", "use std::io::Error;", "use std::time::Duration;"]:
            with self.subTest(source=source):
                self.assertEqual(self.check(source), 0)

    def test_dependencies(self):
        for name in ["rand", "rand_core", "rand_chacha", "getrandom", "fastrand", "rustix", "libc", "nix", "socket2", "foo-sys"]:
            with self.subTest(name=name):
                self.assertNotEqual(self.check("", name), 0)
                self.assertEqual(self.check("", name, "dev"), 0)

if __name__ == "__main__":
    unittest.main()
