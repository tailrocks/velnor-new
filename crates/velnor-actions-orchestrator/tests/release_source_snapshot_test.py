"""Offline strict snapshot content proofs; producer authority remains separate."""
import io
import json
from pathlib import Path
import stat
import struct
import unittest
import warnings
import zipfile

import release_source_tree_test as tree_fixture


SOURCE = Path(__file__).resolve().parents[1] / "src" / "release_source_snapshot.py"
COMMIT = tree_fixture.COMMIT
SourceError = tree_fixture.SourceError
fixture = tree_fixture.fixture
POLICY = {"repository": tree_fixture.REPOSITORY, "source_sha": COMMIT}


def zip_contents(raw):
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        return [(member.filename, archive.read(member)) for member in archive.infolist()]


def make_zip(members, mode=stat.S_IFREG | 0o400, compression=zipfile.ZIP_STORED):
    buffer = io.BytesIO()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", UserWarning)
        with zipfile.ZipFile(buffer, "w") as archive:
            for name, raw in members:
                info = zipfile.ZipInfo(name)
                info.create_system = 3
                info.external_attr = mode << 16
                info.compress_type = compression
                archive.writestr(info, raw)
    return buffer.getvalue()


class SourceSnapshotTest(unittest.TestCase):
    def setUp(self):
        self.source_fixture = tree_fixture.SourceTreeTest()
        self.source_fixture.setUp()
        self.ns = self.source_fixture.ns
        exec(compile(SOURCE.read_bytes(), str(SOURCE), "exec"), self.ns)
        self.source = self.source_fixture.source()
        self.raw = self.ns["serialize_source_snapshot"](self.source)
        self.members = zip_contents(self.raw)

    def decode(self, raw=None):
        return self.ns["decode_source_snapshot"](self.raw if raw is None else raw, POLICY)

    def changed_manifest(self, mutation):
        value = json.loads(self.members[0][1])
        mutation(value)
        return make_zip([("evidence.json", json.dumps(value).encode()), *self.members[1:]])

    def test_deterministic_roundtrip_and_fresh_live_comparison(self):
        self.assertEqual(self.ns["serialize_source_snapshot"](self.source), self.raw)
        snapshot = self.decode()
        self.assertEqual(snapshot.entries, self.source.entries)
        self.assertEqual(snapshot.source_sha, COMMIT)
        self.ns["compare_source_snapshot"](self.source_fixture.source(), snapshot)
        with self.assertRaises(AttributeError):
            snapshot.source_sha = "b" * 40
        with self.assertRaises(TypeError):
            snapshot.blobs[next(iter(snapshot.blobs))] = b"changed"

    def test_pure_snapshot_cannot_be_live_authority(self):
        snapshot = self.decode()
        with self.assertRaises(SourceError):
            self.ns["serialize_source_snapshot"](snapshot)
        with self.assertRaises(SourceError):
            self.ns["source_file"](snapshot, "Cargo.toml")
        with self.assertRaises(SourceError):
            self.ns["compare_source_snapshot"](self.source, {"entries": snapshot.entries})

    def test_pure_consistent_alternative_tree_has_no_commit_authority(self):
        other = self.source_fixture
        other.commit, other.tree, other.blobs = fixture({"Cargo.toml": b"other source"})
        alternate = self.ns["serialize_source_snapshot"](other.source())
        snapshot = self.decode(alternate)
        with self.assertRaisesRegex(SourceError, "source_snapshot_live_binding"):
            self.ns["compare_source_snapshot"](self.source, snapshot)

    def test_exact_deduplicated_blob_coverage(self):
        other = self.source_fixture
        other.commit, other.tree, other.blobs = fixture({"one": b"same", "two": b"same"})
        raw = self.ns["serialize_source_snapshot"](other.source())
        self.assertEqual(len(zip_contents(raw)), 2)
        self.decode(raw)
        for members in (self.members[:-1], [*self.members, ("blobs/" + "b" * 40, b"extra")],
                        self.members[1:]):
            with self.assertRaises(SourceError):
                self.decode(make_zip(members))

    def test_reject_duplicate_extra_traversal_and_nonregular_members(self):
        for members in ([*self.members, self.members[0]], [*self.members, ("../escape", b"")],
                        [*self.members, ("arbitrary", b"")],
                        [*self.members, ("blobs\\" + "a" * 40, b"")]):
            with self.assertRaises(SourceError):
                self.decode(make_zip(members))
        for mode in (stat.S_IFLNK | 0o777, stat.S_IFDIR | 0o700, stat.S_IFIFO | 0o600):
            with self.assertRaises(SourceError):
                self.decode(make_zip(self.members, mode=mode))

    def test_reject_encryption_and_unsupported_compression(self):
        raw = bytearray(self.raw)
        for signature, offset in ((b"PK\x03\x04", 6), (b"PK\x01\x02", 8)):
            position = raw.index(signature) + offset
            flag = struct.unpack_from("<H", raw, position)[0]
            struct.pack_into("<H", raw, position, flag | 1)
        with self.assertRaises(SourceError):
            self.decode(bytes(raw))
        with self.assertRaises(SourceError):
            self.decode(make_zip(self.members, compression=zipfile.ZIP_BZIP2))

    def test_reject_nul_filename_alias(self):
        raw = make_zip([("evidence.jsonXY", self.members[0][1]), *self.members[1:]])
        raw = raw.replace(b"evidence.jsonXY", b"evidence.json\0Y")
        with self.assertRaisesRegex(SourceError, "source_snapshot_member_path"):
            self.decode(raw)

    def test_malformed_deflate_normalized(self):
        raw = bytearray(make_zip(self.members, compression=zipfile.ZIP_DEFLATED))
        name_length, extra_length = struct.unpack_from("<HH", raw, 26)
        start = 30 + name_length + extra_length
        raw[start:start + 2] = b"\xff\xff"
        with self.assertRaisesRegex(SourceError, "source_snapshot_zip"):
            self.decode(bytes(raw))

    def test_invalid_utf8_zip_filename_normalized(self):
        raw = bytearray(self.raw)
        for signature, flag_offset, name_offset in ((b"PK\x03\x04", 6, 30),
                                                    (b"PK\x01\x02", 8, 46)):
            position = raw.index(signature)
            flags = struct.unpack_from("<H", raw, position + flag_offset)[0]
            struct.pack_into("<H", raw, position + flag_offset, flags | 0x800)
            raw[position + name_offset] = 0xff
        with self.assertRaisesRegex(SourceError, "source_snapshot_zip"):
            self.decode(bytes(raw))

    def test_json_recursion_and_integer_limit_normalized(self):
        for manifest in (b"[" * 2000 + b"0" + b"]" * 2000, b"1" * 10000):
            with self.assertRaisesRegex(SourceError, "source_snapshot_json|source_snapshot_schema"):
                self.decode(make_zip([("evidence.json", manifest), *self.members[1:]]))

    def test_reject_manifest_fields_policy_boolean_and_blob_size(self):
        mutations = [lambda value: value.update(schema=True),
                     lambda value: value.update(extra=True),
                     lambda value: value.update(repository="other/repository"),
                     lambda value: value.update(source_sha="b" * 40),
                     lambda value: value["entries"]["Cargo.toml"].update(size=True),
                     lambda value: value["entries"]["Cargo.toml"].update(size=1),
                     lambda value: value["entries"]["Cargo.toml"].update(extra=True),
                     lambda value: value["entries"]["foo"].update(size=0),
                     lambda value: value["entries"]["Cargo.toml"].update(mode="120000")]
        for mutation in mutations:
            with self.assertRaises(SourceError):
                self.decode(self.changed_manifest(mutation))

    def test_reject_duplicate_json_invalid_utf8_and_broken_zip(self):
        manifest = self.members[0][1]
        for raw in (b"\xff", b"not json", b'{"schema":1,"schema":1}',
                    manifest.replace(b'"schema":1', b'"schema":NaN')):
            with self.assertRaises(SourceError):
                self.decode(make_zip([("evidence.json", raw), *self.members[1:]]))
        for raw in (b"", b"not a ZIP", self.raw[:-10], bytearray(self.raw)):
            with self.assertRaises(SourceError):
                self.decode(raw)

    def test_reject_blob_same_length_tamper_and_tree_tamper(self):
        name, raw = self.members[1]
        with self.assertRaises(SourceError):
            self.decode(make_zip([self.members[0], (name, b"x" * len(raw)), *self.members[2:]]))
        with self.assertRaises(SourceError):
            self.decode(self.changed_manifest(lambda value: value.update(tree_sha="b" * 40)))
        with self.assertRaises(SourceError):
            self.decode(self.changed_manifest(
                lambda value: value["entries"]["foo"].update(sha="b" * 40)))

    def test_reject_conflicting_sizes_for_same_blob(self):
        other = self.source_fixture
        other.commit, other.tree, other.blobs = fixture({"one": b"same", "two": b"same"})
        raw = self.ns["serialize_source_snapshot"](other.source())
        members = zip_contents(raw)
        value = json.loads(members[0][1])
        value["entries"]["two"]["size"] = 5
        with self.assertRaises(SourceError):
            self.decode(make_zip([("evidence.json", json.dumps(value).encode()), *members[1:]]))

    def test_reject_source_cache_tamper_during_serialization(self):
        sha = self.source.entries["Cargo.toml"]["sha"]
        self.source._blobs[sha] = b"x" * self.source.entries["Cargo.toml"]["size"]
        with self.assertRaises(SourceError):
            self.ns["serialize_source_snapshot"](self.source)

    def test_zip_size_member_size_expansion_and_count_limits(self):
        for key, value in (("_SNAPSHOT_MAX_ZIP", 1), ("_SNAPSHOT_MAX_MANIFEST", 1),
                           ("_SNAPSHOT_MAX_EXPANDED", 1)):
            original = self.ns[key]
            self.ns[key] = value
            with self.assertRaises(SourceError):
                self.decode()
            self.ns[key] = original
        with self.assertRaises(SourceError):
            self.decode(make_zip([("evidence.json", b"{}")] * 20002))


if __name__ == "__main__":
    unittest.main()
