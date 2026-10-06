"""Offline bounded materialization proofs; no repository or tool execution."""
import errno
import os
from pathlib import Path
import stat
import tempfile
from types import MappingProxyType, SimpleNamespace
import unittest
from unittest.mock import patch

import release_source_snapshot_test as snapshot_fixture


SOURCE = Path(__file__).resolve().parents[1] / "src" / "release_source_materialize.py"
SourceError = snapshot_fixture.SourceError


class SourceMaterializeTest(unittest.TestCase):
    def setUp(self):
        self.fixture = snapshot_fixture.SourceSnapshotTest()
        self.fixture.setUp()
        self.ns = self.fixture.ns
        exec(compile(SOURCE.read_bytes(), str(SOURCE), "exec"), self.ns)
        self.snapshot = self.fixture.decode()
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.parent = Path(self.temporary.name).resolve()
        self.destination = self.parent / "source"

    def materialize(self, snapshot=None, destination=None):
        return self.ns["materialize_source_snapshot"](
            self.snapshot if snapshot is None else snapshot,
            self.destination if destination is None else destination)

    def snapshot_for(self, files):
        source_fixture = self.fixture.source_fixture
        source_fixture.commit, source_fixture.tree, source_fixture.blobs = snapshot_fixture.fixture(files)
        source = source_fixture.source()
        raw = self.ns["serialize_source_snapshot"](source)
        return self.fixture.decode(raw)

    def test_exact_regular_content_private_root_and_no_api_calls(self):
        self.fixture.source_fixture.calls.clear()
        result = self.materialize()
        self.assertEqual(result, self.destination)
        self.assertEqual(stat.S_IMODE(result.stat().st_mode), 0o700)
        actual = {str(path.relative_to(result)) for path in result.rglob("*")}
        self.assertEqual(actual, set(self.snapshot.entries))
        for name, entry in self.snapshot.entries.items():
            path = result / name
            self.assertFalse(path.is_symlink())
            if entry["type"] == "blob":
                self.assertEqual(path.read_bytes(), self.snapshot.blobs[entry["sha"]])
                self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o644)
            else:
                self.assertTrue(path.is_dir())
        self.assertEqual(self.fixture.source_fixture.calls, [])

    def test_executable_binary_empty_tree_and_short_writes(self):
        snapshot = self.snapshot_for({"bin/run": b"\xff\0\xfe"})
        source_fixture = self.fixture.source_fixture
        source_fixture.tree["tree"][0]["mode"] = "100755"
        blob_sha = source_fixture.tree["tree"][0]["sha"]
        inner = b"100755 run\0" + bytes.fromhex(blob_sha)
        tree_sha = snapshot_fixture.tree_fixture.git_hash(b"tree", inner)
        source_fixture.tree["tree"][1]["sha"] = tree_sha
        root = snapshot_fixture.tree_fixture.git_hash(b"tree", b"40000 bin\0" + bytes.fromhex(tree_sha))
        source_fixture.commit["tree"]["sha"] = root
        source_fixture.tree["sha"] = root
        snapshot = self.fixture.decode(self.ns["serialize_source_snapshot"](source_fixture.source()))
        write = os.write
        with patch.object(os, "write", side_effect=lambda fd, raw: write(fd, raw[:1])):
            self.materialize(snapshot)
        self.assertEqual((self.destination / "bin/run").read_bytes(), b"\xff\0\xfe")
        self.assertEqual(stat.S_IMODE((self.destination / "bin/run").stat().st_mode), 0o755)
        empty = self.snapshot_for({})
        self.materialize(empty, self.parent / "empty")
        self.assertEqual(list((self.parent / "empty").iterdir()), [])
        source_fixture.test_executable_and_nested_empty_tree()
        nested = self.fixture.decode(self.ns["serialize_source_snapshot"](source_fixture.source()))
        self.materialize(nested, self.parent / "nested")
        self.assertTrue((self.parent / "nested/empty").is_dir())
        self.assertEqual(list((self.parent / "nested/empty").iterdir()), [])

    def test_only_exact_pure_snapshot_type(self):
        impostors = [self.fixture.source, {}, SimpleNamespace(**{
            name: getattr(self.snapshot, name) for name in self.snapshot.__slots__})]
        for value in impostors:
            with self.assertRaisesRegex(SourceError, "source_snapshot_content"):
                self.materialize(value)
        self.assertFalse(self.destination.exists())

    def test_defensive_metadata_and_topology_validation(self):
        original = self.snapshot.entries
        mutations = [lambda entries: entries["Cargo.toml"].update(mode="120000"),
                     lambda entries: entries["Cargo.toml"].update(type="commit", mode="160000"),
                     lambda entries: entries.pop("foo"),
                     lambda entries: entries.update({".git/config": entries.pop("Cargo.toml")}),
                     lambda entries: entries.update({"../escape": entries.pop("Cargo.toml")})]
        for mutation in mutations:
            entries = {name: dict(entry) for name, entry in original.items()}
            mutation(entries)
            object.__setattr__(self.snapshot, "entries", MappingProxyType(entries))
            with patch.object(os, "mkdir", side_effect=AssertionError("preflight wrote")):
                with self.assertRaises(SourceError):
                    self.materialize()
        object.__setattr__(self.snapshot, "entries", original)

    def test_whole_tree_host_collisions_rejected_before_mkdir(self):
        cases = [{"Readme": b"a", "README": b"b"},
                 {"caf\u00e9": b"a", "cafe\u0301": b"b"},
                 {"DIR/a": b"a", "dir/b": b"b"},
                 {"A": b"a", "a/b": b"b"},
                 {"stra\u00dfe": b"a", "STRASSE": b"b"}]
        for files in cases:
            snapshot = self.snapshot_for(files)
            with patch.object(os, "mkdir", side_effect=AssertionError("preflight wrote")):
                with self.assertRaisesRegex(SourceError, "source_materialize_host_collision"):
                    self.materialize(snapshot)

    def test_long_components_blob_corruption_and_coverage_before_creation(self):
        snapshot = self.snapshot_for({"a" * 256: b"long"})
        with self.assertRaisesRegex(SourceError, "source_materialize_component_size"):
            self.materialize(snapshot)
        for raw in (b"x" * 43, bytearray(b"x")):
            object.__setattr__(self.snapshot, "blobs", MappingProxyType({
                **self.snapshot.blobs, self.snapshot.entries["Cargo.toml"]["sha"]: raw}))
            with self.assertRaises(SourceError):
                self.materialize()
        self.assertFalse(self.destination.exists())

    def test_rehash_at_write_and_cleanup(self):
        original = self.ns["_materialize_blob"]
        calls = 0

        def changed(snapshot, entry):
            nonlocal calls
            calls += 1
            if calls > sum(item["type"] == "blob" for item in snapshot.entries.values()):
                object.__setattr__(snapshot, "blobs", MappingProxyType({
                    **snapshot.blobs, entry["sha"]: b"x" * entry["size"]}))
            return original(snapshot, entry)

        with patch.dict(self.ns, {"_materialize_blob": changed}):
            with self.assertRaisesRegex(SourceError, "source_materialize_blob_digest"):
                self.materialize()
        self.assertFalse(self.destination.exists())

    def test_reject_existing_destination_and_preserve_foreign_content(self):
        self.destination.mkdir()
        sentinel = self.destination / "foreign"
        sentinel.write_bytes(b"keep")
        with self.assertRaises(FileExistsError):
            self.materialize()
        self.assertEqual(sentinel.read_bytes(), b"keep")
        link = self.parent / "link"
        link.symlink_to(self.destination, target_is_directory=True)
        with self.assertRaises(FileExistsError):
            self.materialize(destination=link)
        self.assertTrue(link.is_symlink())

    def test_reject_symlink_ancestor_unowned_parent_and_dotdot(self):
        alias = self.parent / "alias"
        alias.symlink_to(self.parent, target_is_directory=True)
        with self.assertRaises(OSError):
            self.materialize(destination=alias / "source")
        self.parent.chmod(0o777)
        try:
            with self.assertRaisesRegex(SourceError, "source_materialize_parent_private"):
                self.materialize()
        finally:
            self.parent.chmod(0o700)
        for suffix in ("../escape", "a\\b/source", "a\n/source", "./source", ".git/source"):
            with self.assertRaises(SourceError):
                self.materialize(destination=str(self.parent) + "/" + suffix)
        self.assertFalse(self.destination.exists())

    def test_write_and_chmod_failures_cleanup_only_owned_destination(self):
        sentinel = self.parent / "foreign"
        sentinel.write_bytes(b"keep")
        for operation in ("write", "fchmod"):
            with patch.object(os, operation, side_effect=OSError(errno.ENOSPC, "injected")):
                with self.assertRaises(OSError):
                    self.materialize()
            self.assertFalse(self.destination.exists())
            self.assertEqual(sentinel.read_bytes(), b"keep")
        with patch.object(os, "write", return_value=0):
            with self.assertRaisesRegex(SourceError, "source_materialize_short_write"):
                self.materialize()
        self.assertFalse(self.destination.exists())

    def test_foreign_file_in_root_never_deleted_on_failure(self):
        def failed_write(_descriptor, _raw):
            (self.destination / "foreign").write_bytes(b"keep")
            raise OSError("injected")

        with patch.object(os, "write", side_effect=failed_write):
            with self.assertRaisesRegex(SourceError, "source_materialize_cleanup"):
                self.materialize()
        self.assertEqual((self.destination / "foreign").read_bytes(), b"keep")
        self.assertFalse((self.destination / "CHANGELOG.md").exists())

    def test_replaced_child_directory_never_receives_write_or_cleanup(self):
        original = self.ns["_materialize_write"]

        def swapped(root, path, entry, snapshot, owned):
            original(root, path, entry, snapshot, owned)
            if path == "foo":
                (self.destination / "foo").rename(self.destination / "saved")
                (self.destination / "foo").mkdir()
                (self.destination / "foo/foreign").write_bytes(b"keep")

        with patch.dict(self.ns, {"_materialize_write": swapped}):
            with self.assertRaisesRegex(SourceError, "source_materialize_cleanup"):
                self.materialize()
        self.assertEqual((self.destination / "foo/foreign").read_bytes(), b"keep")
        self.assertFalse((self.destination / "foo/lib.rs").exists())
        self.assertFalse((self.destination / "saved/lib.rs").exists())


if __name__ == "__main__":
    unittest.main()
