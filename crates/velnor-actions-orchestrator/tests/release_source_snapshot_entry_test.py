"""Offline real source codec producer checks; output mocks grant no qualification."""
import hashlib
import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch

import release_source_tree_test as tree_fixture


ROOT = Path(__file__).resolve().parents[1] / "src"
POLICY = {"repository": tree_fixture.REPOSITORY, "source_sha": tree_fixture.COMMIT}


class SourceSnapshotEntryTest(unittest.TestCase):
    def setUp(self):
        self.fixture = tree_fixture.SourceTreeTest()
        self.fixture.setUp()
        self.ns = self.fixture.ns
        self.ns.update({"__name__": "velnor_release_compiled", "policy": lambda: POLICY})
        for name in ("release_source_snapshot.py", "release_original_source_origin.py",
                     "release_source_snapshot_entry.py"):
            path = ROOT / name
            exec(compile(path.read_bytes(), str(path), "exec"), self.ns)
        self.origin = self.ns["authenticate_original_source_origin"]
        # Downstream codec/write units mock the unavailable service boundary.
        # This fixture creates no qualified service context or SourceJob grant.
        self.ns["authenticate_original_source_origin"] = lambda: None
        self.outputs = []
        self.ns["write_source_snapshot_outputs"] = lambda *values: self.outputs.append(values)
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def invoke(self):
        previous = Path.cwd()
        try:
            os.chdir(self.directory)
            self.ns["source_snapshot_main"]()
        finally:
            os.chdir(previous)

    def test_real_authenticated_source_to_single_buffer_snapshot(self):
        serialize = self.ns["serialize_source_snapshot"]
        buffers = []

        def observe(source):
            raw = serialize(source)
            buffers.append(raw)
            return raw

        self.ns["serialize_source_snapshot"] = observe
        self.invoke()
        path = self.directory / "release-source-snapshot/snapshot.zip"
        raw = path.read_bytes()
        self.assertEqual(buffers, [raw])
        decoded = self.ns["decode_source_snapshot"](raw, POLICY)
        self.assertEqual(self.outputs, [(hashlib.sha256(raw).hexdigest(),
                                       tree_fixture.COMMIT, decoded.tree_sha)])
        self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o400)
        self.assertEqual(stat.S_IMODE(path.parent.stat().st_mode), 0o700)
        self.assertEqual(sum(kind == "commit" for kind, _ in self.fixture.calls), 1)

    def test_missing_capability_denies_before_policy_api_or_files(self):
        del self.ns["write_source_snapshot_outputs"]
        self.ns["policy"] = lambda: self.fail("policy called before capability")
        with patch.dict(os.environ, {"GITHUB_OUTPUT": str(self.directory / "ambient")}), \
                self.assertRaisesRegex(tree_fixture.SourceError, "output_unqualified"):
            self.invoke()
        self.assertEqual(self.fixture.calls, [])
        self.assertEqual(list(self.directory.iterdir()), [])

    def test_actual_missing_origin_denies_before_policy_api_or_files(self):
        self.ns["authenticate_original_source_origin"] = self.origin
        self.ns["policy"] = lambda: self.fail("policy called before origin")
        with self.assertRaisesRegex(tree_fixture.SourceError,
                                    "original_source_service_context_missing"):
            self.invoke()
        self.assertEqual(self.fixture.calls, [])
        self.assertEqual(self.outputs, [])
        self.assertEqual(list(self.directory.iterdir()), [])

    def test_substituted_origin_denies_before_policy_api_or_files(self):
        self.ns["authenticate_original_source_origin"] = self.origin
        self.ns["policy"] = lambda: self.fail("policy called before origin")
        for context in ({"repository_id": 1}, object()):
            with self.subTest(context=type(context).__name__):
                self.ns["_compiled_original_source_context"] = lambda: context
                with self.assertRaisesRegex(tree_fixture.SourceError,
                                            "original_source_service_context_unqualified"):
                    self.invoke()
        self.assertEqual(self.fixture.calls, [])
        self.assertEqual(self.outputs, [])
        self.assertEqual(list(self.directory.iterdir()), [])

    def test_noncallable_capability_denies_before_api(self):
        self.ns["write_source_snapshot_outputs"] = "caller-owned-path"
        with self.assertRaisesRegex(tree_fixture.SourceError, "output_unqualified"):
            self.invoke()
        self.assertEqual(self.fixture.calls, [])

    def test_source_verification_failure_writes_nothing(self):
        self.fixture.commit["sha"] = "b" * 40
        with self.assertRaises(tree_fixture.SourceError):
            self.invoke()
        self.assertEqual(self.outputs, [])
        self.assertEqual(list(self.directory.iterdir()), [])

    def test_serialization_failure_writes_nothing(self):
        self.ns["serialize_source_snapshot"] = lambda _: "not bytes"
        with self.assertRaisesRegex(tree_fixture.SourceError, "snapshot_bytes"):
            self.invoke()
        self.assertEqual(self.outputs, [])
        self.assertEqual(list(self.directory.iterdir()), [])

    def test_existing_destination_never_overwritten(self):
        target = self.directory / "release-source-snapshot"
        target.mkdir()
        (target / "snapshot.zip").write_bytes(b"existing")
        with self.assertRaises(FileExistsError):
            self.invoke()
        self.assertEqual((target / "snapshot.zip").read_bytes(), b"existing")
        self.assertEqual(self.outputs, [])

    def test_symlink_destination_never_followed(self):
        outside = self.directory / "outside"
        outside.mkdir()
        (self.directory / "release-source-snapshot").symlink_to(outside, target_is_directory=True)
        with self.assertRaises(FileExistsError):
            self.invoke()
        self.assertEqual(list(outside.iterdir()), [])
        self.assertEqual(self.outputs, [])


if __name__ == "__main__":
    unittest.main()
