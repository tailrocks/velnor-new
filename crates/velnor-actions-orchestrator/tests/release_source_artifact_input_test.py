"""Pure input bridge negatives; no standalone source authority is minted."""
import hashlib
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import release_source_snapshot_test as snapshot_fixture


SOURCE = Path(__file__).resolve().parents[1] / "src" / "release_source_artifact_input.py"


class SourceArtifactInputTests(unittest.TestCase):
    def setUp(self):
        fixture = snapshot_fixture.SourceSnapshotTest()
        fixture.setUp()
        self.ns, self.raw, self.source = fixture.ns, fixture.raw, fixture.source
        exec(compile(SOURCE.read_bytes(), str(SOURCE), "exec"), self.ns)
        self.temporary = tempfile.TemporaryDirectory(prefix="velnor-source-input-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve(strict=True)
        self.binding = {
            "repository": self.source.repository, "source_sha": self.source.source_sha,
            "tree_sha": self.source.tree_sha, "workflow": ".github/workflows/release.yml",
            "workflow_sha": "d" * 40,
            "ref": "refs/heads/main", "run_id": "123", "attempt": "1",
            "producer_job": "release-source-snapshot", "producer_helper_sha256": "a" * 64,
            "artifact_id": "456", "raw_zip_sha256": "b" * 64,
            "inner_sha256": hashlib.sha256(self.raw).hexdigest(),
            "destination": str(self.root),
        }

    def test_valid_content_is_pure_and_standalone_owner_stays_denied(self):
        snapshot = self.ns["_source_artifact_decode_content"](self.raw, self.binding)
        with self.assertRaisesRegex(snapshot_fixture.SourceError, "source_artifact_authority"):
            self.ns["authenticated_source_snapshot"](snapshot, 1024)
        with patch.dict(os.environ, {"RELEASE_SOURCE_SNAPSHOT_BLOB_SHA256":
                                    self.binding["inner_sha256"], "SOURCE_VERIFIED": "true"}):
            with self.assertRaisesRegex(snapshot_fixture.SourceError, "compiled_input_unqualified"):
                self.ns["load_authenticated_source_snapshot"]()

    def test_expected_tree_is_independent_of_valid_content(self):
        self.binding["tree_sha"] = "c" * 40
        with self.assertRaisesRegex(snapshot_fixture.SourceError, "expected_tree"):
            self.ns["_source_artifact_decode_content"](self.raw, self.binding)

    def test_file_reader_rejects_pure_snapshot_live_api_source_and_paths(self):
        snapshot = self.ns["_source_artifact_decode_content"](self.raw, self.binding)
        for value in (snapshot, self.source, self.root, self.binding):
            with self.subTest(value=type(value).__name__):
                with self.assertRaisesRegex(snapshot_fixture.SourceError,
                                            "source_artifact_authority"):
                    self.ns["authenticated_source_file"](value, "Cargo.toml")

    def test_changed_bytes_or_identity_rejected(self):
        with self.assertRaisesRegex(snapshot_fixture.SourceError, "inner_digest"):
            self.ns["_source_artifact_decode_content"](self.raw + b"x", self.binding)
        for key in ("repository", "source_sha"):
            binding = dict(self.binding)
            binding[key] = "other/repository" if key == "repository" else "c" * 40
            with self.subTest(key=key), self.assertRaises(snapshot_fixture.SourceError):
                self.ns["_source_artifact_decode_content"](self.raw, binding)

    def test_closed_binding_fields_and_producer(self):
        for key, value in (("producer_job", "release-package"), ("workflow", "other.yml"),
                           ("run_id", "0"), ("attempt", "01"), ("artifact_id", "True"),
                           ("inner_sha256", "A" * 64), ("extra", True)):
            binding = dict(self.binding)
            binding[key] = value
            with self.subTest(key=key), self.assertRaises(snapshot_fixture.SourceError):
                self.ns["_source_artifact_content_binding"](binding)

    def test_same_bounded_regular_bytes_read_without_links(self):
        (self.root / "snapshot.zip").write_bytes(self.raw)
        self.assertEqual(self.ns["_source_artifact_read_content"](self.binding), self.raw)
        (self.root / "extra").write_bytes(b"extra")
        with self.assertRaisesRegex(snapshot_fixture.SourceError, "layout"):
            self.ns["_source_artifact_read_content"](self.binding)
        (self.root / "extra").unlink()
        (self.root / "snapshot.zip").unlink()
        target = self.root.parent / (self.root.name + "-target")
        target.write_bytes(self.raw)
        self.addCleanup(target.unlink)
        (self.root / "snapshot.zip").symlink_to(target)
        with self.assertRaises(OSError):
            self.ns["_source_artifact_read_content"](self.binding)

    def test_symlink_ancestor_is_rejected(self):
        alias = self.root / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        self.binding["destination"] = str(alias)
        with self.assertRaises(OSError):
            self.ns["_source_artifact_read_content"](self.binding)

    def test_fifo_never_waits_for_a_writer(self):
        os.mkfifo(self.root / "snapshot.zip")
        with self.assertRaisesRegex(snapshot_fixture.SourceError, "source_artifact_file"):
            self.ns["_source_artifact_read_content"](self.binding)

    def test_portable_descriptor_is_closed_claims_never_authority(self):
        descriptor = {key: value for key, value in self.binding.items() if key != "destination"}
        self.assertIsNone(self.ns["validate_source_snapshot_descriptor"](descriptor))
        with self.assertRaisesRegex(snapshot_fixture.SourceError, "source_artifact_authority"):
            self.ns["authenticated_source_descriptor"](descriptor)
        for change in ({"destination": str(self.root)}, {"extra": "claim"},
                       {"workflow_sha": "bad"}, {"artifact_id": True}):
            mutated = {**descriptor, **change}
            with self.subTest(change=change), self.assertRaises(snapshot_fixture.SourceError):
                self.ns["validate_source_snapshot_descriptor"](mutated)

    def test_descriptor_accessor_rejects_content_paths_and_live_api_source(self):
        snapshot = self.ns["_source_artifact_decode_content"](self.raw, self.binding)
        for value in (snapshot, self.source, self.root):
            with self.subTest(value=type(value).__name__):
                with self.assertRaisesRegex(snapshot_fixture.SourceError,
                                            "source_artifact_authority"):
                    self.ns["authenticated_source_descriptor"](value)


if __name__ == "__main__":
    unittest.main()
