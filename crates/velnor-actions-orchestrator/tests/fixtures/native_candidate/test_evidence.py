# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from evidence import read_bounded, write_evidence


class FixtureEvidence(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "candidate"
        self.repo.mkdir()
        subprocess.run(["git", "init", "-q", str(self.repo)], check=True)
        (self.repo / "tracked").write_text("candidate")
        subprocess.run(["git", "-C", str(self.repo), "add", "."], check=True)
        subprocess.run(["git", "-C", str(self.repo), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"], check=True)
        self.sha = subprocess.check_output(["git", "-C", str(self.repo), "rev-parse", "HEAD"], text=True).strip()
        self.runner = self.root / "runner"
        self.runner.mkdir()
        self.workspace = self.root / "workspace"
        (self.workspace / ".github").mkdir(parents=True)
        (self.workspace / ".github/velnor-release-intent.toml").write_bytes(b"effective intent")
        self.supplied = self.root / "supplied.toml"
        self.supplied.write_bytes(b"supplied intent")
        self.info = {"workspace": str(self.workspace), "repository": "owner/repo", "source_sha": "a" * 40, "package": "example", "version": "0.1.1"}
        self.packed = b"fixture crate"
        self.state = {"uploads": 1, "published_sha256": hashlib.sha256(self.packed).hexdigest(), "release": {"tag_name": "example-v0.1.1"}, "refs": {"refs/tags/example-v0.1.1": "a" * 40, "refs/tags/velnor-receipt/12/1/publication-started": "b" * 40, "refs/tags/velnor-receipt/12/1/reconciled": "c" * 40}}
        self.counts = {"oidc": 1, "exchange": 1, "revoke": 1}

    def emit(self, path):
        write_evidence(path, self.runner, self.repo, self.supplied, self.info, self.state, self.counts, self.packed)

    def test_receipt_binds_actual_observations_and_is_exclusive(self):
        path = self.runner / "evidence.json"
        self.emit(path)
        evidence = json.loads(path.read_bytes())
        self.assertEqual(evidence["candidate_sha"], self.sha)
        self.assertEqual(evidence["source_sha"], self.info["source_sha"])
        self.assertEqual(evidence["supplied_intent_sha256"], hashlib.sha256(b"supplied intent").hexdigest())
        self.assertEqual(evidence["effective_intent_sha256"], hashlib.sha256(b"effective intent").hexdigest())
        self.assertEqual(evidence["published_sha256"], self.state["published_sha256"])
        self.assertEqual(evidence["tag_target"], self.info["source_sha"])
        with self.assertRaises(FileExistsError):
            self.emit(path)

    def test_bad_observations_and_unsafe_destination_fail_closed(self):
        for key in ("oidc", "exchange", "revoke"):
            self.counts[key] = 0
            with self.assertRaises(ValueError):
                self.emit(self.runner / "evidence.json")
            self.counts[key] = 1
        self.state["refs"]["refs/tags/example-v0.1.1"] = "d" * 40
        with self.assertRaises(ValueError):
            self.emit(self.runner / "evidence.json")
        self.state["refs"]["refs/tags/example-v0.1.1"] = self.info["source_sha"]
        with self.assertRaises(ValueError):
            self.emit(self.root / "outside.json")
        self.assertFalse((self.root / "outside.json").exists())

    def test_oversized_and_symlink_inputs_are_rejected(self):
        self.supplied.write_bytes(b"x" * (1024 * 1024 + 1))
        with self.assertRaises(ValueError):
            self.emit(self.runner / "evidence.json")
        self.supplied.unlink()
        self.supplied.symlink_to(self.workspace / ".github/velnor-release-intent.toml")
        with self.assertRaises(OSError):
            self.emit(self.runner / "evidence.json")
        package = self.root / "package.crate"
        with package.open("wb") as stream:
            stream.truncate(100 * 1024 * 1024 + 1)
        with self.assertRaises(ValueError):
            read_bounded(package, 100 * 1024 * 1024)


if __name__ == "__main__":
    unittest.main()
