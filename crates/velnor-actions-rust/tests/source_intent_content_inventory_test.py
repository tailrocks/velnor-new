"""Provenance-free archive content proof; no tool, source, or network execution."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile
import unittest
from unittest.mock import patch


class ReconcileError(ValueError):
    pass


def require(condition, reason):
    if not condition:
        raise ReconcileError(reason)


ROOT = Path(__file__).resolve().parents[1] / "src"
NS = {"__name__": "source_intent_content_test", "require": require,
      "ReconcileError": ReconcileError, "decode_json": json.loads}
exec(compile((ROOT / "release_reconcile_cargo.py").read_text(), "release_reconcile_cargo.py", "exec"), NS)


def package(manifest=None, files=(), nonregular=None, prefix="demo-1.0.0/"):
    manifest = manifest or b'[package]\nname="demo"\nversion="1.0.0"\n[features]\ndefault=[]\n'
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w:gz") as archive:
        for name, data in [("Cargo.toml", manifest), ("src/lib.rs", b"pub fn fixture() {}\n"), *files]:
            member = tarfile.TarInfo(prefix + name)
            member.size = len(data)
            archive.addfile(member, io.BytesIO(data))
        if nonregular is not None:
            member = tarfile.TarInfo(prefix + "link")
            member.type = nonregular
            member.linkname = "../outside"
            archive.addfile(member)
    return stream.getvalue()


def inventory(data):
    return NS["source_intent_content_inventory"](data, "demo", "1.0.0")


class ContentInventoryTests(unittest.TestCase):
    def test_no_vcs_file_required_and_no_source_authority_returned(self):
        value = inventory(package())
        self.assertEqual(set(value), {"files", "features"})
        self.assertEqual(set(value["files"]), {"Cargo.toml", "src/lib.rs"})
        self.assertEqual(value["features"], {"default": []})

    def test_vcs_is_ordinary_content_even_if_not_json(self):
        data = b"untrusted text claiming any source"
        value = inventory(package(files=[(".cargo_vcs_info.json", data)]))
        self.assertEqual(value["files"][".cargo_vcs_info.json"],
                         {"sha256": hashlib.sha256(data).hexdigest(), "size": len(data)})
        other = inventory(package(files=[(".cargo_vcs_info.json", b"different claim")]))
        self.assertNotEqual(value, other)

    def test_toml_layout_and_compression_headers_are_normalized(self):
        initial = package()
        alternate = b'[features]\ndefault = []\n[package]\nversion = "1.0.0"\nname = "demo"\n'
        self.assertEqual(inventory(initial), inventory(package(manifest=alternate)))
        self.assertEqual(inventory(initial), inventory(gzip.compress(gzip.decompress(initial), mtime=7)))

    def test_unsafe_members_duplicates_and_identity_rejected(self):
        cases = [package(files=[("../escape", b"x")]), package(files=[("src/lib.rs", b"duplicate")]),
                 package(files=[("./unexpected", b"x")]), package(nonregular=tarfile.SYMTYPE),
                 package(nonregular=tarfile.LNKTYPE), package(prefix="other-1.0.0/"),
                 package(manifest=b'[package]\nname="wrong"\nversion="1.0.0"\n')]
        for data in cases:
            with self.subTest(size=len(data)), self.assertRaises(ReconcileError):
                inventory(data)

    def test_total_decompression_bound_applies_before_tar_parse(self):
        class Sized:
            def __len__(self):
                return 128 * 1024 * 1024 + 1
        class Bomb:
            def __enter__(self):
                return self
            def __exit__(self, *args):
                return False
            def read(self, bound):
                return Sized()
        with patch.object(NS["gzip"], "GzipFile", return_value=Bomb()), \
             self.assertRaisesRegex(ReconcileError, "archive_total_expansion"):
            inventory(b"bounded compressed input")

    def test_immutable_bytes_and_closed_package_identity_required(self):
        for data, name, version in ((bytearray(package()), "demo", "1.0.0"),
                                    (package(), "../demo", "1.0.0"),
                                    (package(), "demo", "../1.0.0")):
            with self.assertRaises(ReconcileError):
                NS["source_intent_content_inventory"](data, name, version)

    def test_closed_package_and_feature_types(self):
        for manifest in (b'package="invalid shape"\n',
                         b'[package]\nname="demo"\nversion="1.0.0"\n[features]\ndefault=true\n'):
            with self.assertRaises(ReconcileError):
                inventory(package(manifest=manifest))

    def test_entire_namespace_rejects_case_unicode_and_file_parent_aliases(self):
        collisions = [("A/x", "a/y"), ("A", "a/x"), ("a/x", "A"),
                      ("caf\u00e9/x", "cafe\u0301/y"), ("Stra\u00dfe/x", "STRASSE/y"),
                      ("Cargo.toml", "cargo.toml"), ("parent", "parent/child"),
                      ("parent/child", "parent")]
        for first, second in collisions:
            with self.subTest(paths=(first, second)), self.assertRaises(ReconcileError):
                inventory(package(files=[(first, b"one"), (second, b"two")]))

    def test_shared_exact_directory_namespace_is_valid(self):
        value = inventory(package(files=[("directory/a", b"one"), ("directory/b", b"two")]))
        self.assertIn("directory/a", value["files"])
        self.assertIn("directory/b", value["files"])

    def test_pax_deep_path_and_namespace_allocation_bounded(self):
        with self.assertRaisesRegex(ReconcileError, "archive_path_size_or_characters"):
            inventory(package(files=[("a/" * 3000 + "leaf", b"x")]))
        with patch.dict(NS, {"SOURCE_INTENT_MAX_NAMESPACE_NODES": 5}), \
             self.assertRaisesRegex(ReconcileError, "archive_namespace_size"):
            inventory(package(files=[("a/b/c/d", b"x")]))

    def test_invalid_platform_components_rejected_before_return(self):
        for path in ("directory/line\nbreak", "directory/\x7f", "x" * 256):
            with self.subTest(path=repr(path)), self.assertRaises(ReconcileError):
                inventory(package(files=[(path, b"x")]))


if __name__ == "__main__":
    unittest.main()
