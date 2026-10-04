"""Pure mocked frozen local-registry acquisition; no native solver claims."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch


DIRECTORY = Path(__file__).parent.parent / "src"
NAMESPACE = {}
for filename in ("release_reconcile_common.py", "release_prepare_bytes.py",
                 "release_prepare_registry.py", "release_prepare_universe.py"):
    exec(compile((DIRECTORY / filename).read_text(), filename, "exec"), NAMESPACE)
MATERIALIZER = NAMESPACE["FrozenRegistryMaterializer"]
ERROR = NAMESPACE["ReconcileError"]


def archive(version="1.0.0", marker=False):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w") as output:
        entries = {"Cargo.toml": f'[package]\nname="dep"\nversion="{version}"\n'.encode()}
        if marker:
            entries[".cargo-ok"] = b"{}"
        for path, raw in entries.items():
            member = tarfile.TarInfo(f"dep-{version}/{path}")
            member.size = len(raw)
            output.addfile(member, io.BytesIO(raw))
    return gzip.compress(stream.getvalue())


class UniverseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve() / "source"
        self.addCleanup(self.cleanup)
        self.archives = {version: archive(version) for version in ("1.0.0", "1.1.0")}
        self.entries = [{"name": "dep", "vers": version, "yanked": False,
                         "cksum": hashlib.sha256(raw).hexdigest(),
                         "features2": {"optional": ["dep:hidden"]}, "v": 2}
                        for version, raw in self.archives.items()]
        self.raw = b"\r\n".join(json.dumps(entry).encode() for entry in self.entries) + b"\r\n"
        self.calls = []
        self.source = MATERIALIZER(self.root, self.fetch)

    def cleanup(self):
        if self.root.exists():
            for path in [self.root, *self.root.rglob("*")]:
                if not path.is_symlink():
                    path.chmod(0o755 if path.is_dir() else 0o644)
        self.temporary.cleanup()

    def fetch(self, url, limit):
        self.calls.append((url, limit))
        if url == "https://index.crates.io/3/d/dep":
            return self.raw
        for version, raw in self.archives.items():
            if url == f"https://static.crates.io/crates/dep/dep-{version}.crate":
                return raw
        raise ERROR("mock_unknown_request")

    def test_complete_index_bytes_and_local_registry_archive_layout(self):
        self.source.fetch_index("dep")
        self.source.fetch_archive("dep", "1.0.0")
        snapshot = self.source.freeze()
        self.assertEqual((self.root / "index/3/d/dep").read_bytes(), self.raw)
        self.assertEqual((self.root / "dep-1.0.0.crate").read_bytes(), self.archives["1.0.0"])
        self.assertFalse((self.root / "dep-1.1.0.crate").exists())
        self.assertEqual(snapshot.local_registry, self.root)
        self.assertFalse(hasattr(snapshot, "complete"))
        self.assertFalse(hasattr(snapshot, "activate_native_source"))
        snapshot.verify()
        self.assertEqual(self.root.stat().st_mode & 0o777, 0o555)
        with self.assertRaisesRegex(ERROR, "already_frozen"):
            self.source.fetch_archive("dep", "1.1.0")

    def test_missing_newer_archive_is_never_hidden_by_index_filtering(self):
        self.source.fetch_index("dep")
        del self.archives["1.1.0"]
        with self.assertRaisesRegex(ERROR, "unknown_request"):
            self.source.fetch_archive("dep", "1.1.0")
        self.assertEqual((self.root / "index/3/d/dep").read_bytes(), self.raw)
        self.assertFalse((self.root / "dep-1.1.0.crate").exists())

    def test_unknown_index_or_version_fails_before_archive_fetch(self):
        with self.assertRaisesRegex(ERROR, "unknown_index"):
            self.source.fetch_archive("dep", "1.0.0")
        self.source.fetch_index("dep")
        with self.assertRaisesRegex(ERROR, "unknown_index"):
            self.source.fetch_archive("dep", "2.0.0")
        self.assertEqual(len(self.calls), 1)

    def test_mismatched_name_alias_duplicate_and_missing_response_rejected(self):
        original = self.raw
        for raw in (b"", original.replace(b'"dep"', b'"other"'),
                    original + original, original.replace(b'"dep"', b'"Dep"')):
            self.raw = raw
            with self.assertRaises(ERROR):
                self.source.fetch_index("dep")
        self.assertFalse((self.root / "index/3/d/dep").exists())

    def test_archive_checksum_and_manifest_binding(self):
        self.source.fetch_index("dep")
        self.archives["1.0.0"] = archive("9.0.0")
        with self.assertRaisesRegex(ERROR, "checksum"):
            self.source.fetch_archive("dep", "1.0.0")
        self.assertFalse((self.root / "dep-1.0.0.crate").exists())

    def test_authenticated_archive_marker_does_not_claim_native_cache_authority(self):
        self.archives["1.0.0"] = archive(marker=True)
        self.entries[0]["cksum"] = hashlib.sha256(self.archives["1.0.0"]).hexdigest()
        self.raw = b"\n".join(json.dumps(entry).encode() for entry in self.entries)
        self.source.fetch_index("dep")
        self.source.fetch_archive("dep", "1.0.0")
        self.assertFalse((self.root / ".cargo-ok").exists())
        self.source.freeze().verify()

    def test_prefreeze_mutation_rejected(self):
        self.source.fetch_index("dep")
        (self.root / "index/3/d/dep").write_bytes(b"forged")
        with self.assertRaisesRegex(ERROR, "source_changed"):
            self.source.freeze()

    def test_frozen_byte_mode_path_and_cache_mutations_rejected(self):
        self.source.fetch_index("dep")
        snapshot = self.source.freeze()
        target = self.root / "index/3/d/dep"
        target.chmod(0o644)
        with self.assertRaisesRegex(ERROR, "source_changed"):
            snapshot.verify()
        target.write_bytes(b"forged")
        target.chmod(0o444)
        with self.assertRaisesRegex(ERROR, "source_changed"):
            snapshot.verify()
        self.root.chmod(0o755)
        (self.root / ".cargo-ok").write_bytes(b"{}")
        with self.assertRaisesRegex(ERROR, "cache_marker"):
            snapshot.verify()

    def test_no_caller_completeness_authority(self):
        with self.assertRaises(TypeError):
            MATERIALIZER(self.root.parent / "other", complete=True)

    def test_alias_index_identity_and_nonbytes_response_rejected(self):
        entry = dict(self.entries[0], name="demo_crate")
        with self.assertRaisesRegex(ERROR, "index_identity"):
            NAMESPACE["universe_index_records"](json.dumps(entry).encode(), "demo-crate")
        self.raw = None
        with self.assertRaisesRegex(ERROR, "index_size"):
            self.source.fetch_index("dep")

    def test_added_symlink_rejected(self):
        self.source.fetch_index("dep")
        snapshot = self.source.freeze()
        self.root.chmod(0o755)
        (self.root / "escape").symlink_to(self.root.parent)
        with self.assertRaisesRegex(ERROR, "source_type"):
            snapshot.verify()

    def test_unreadable_directory_never_omitted_from_inventory(self):
        directory = self.root / "blocked"
        directory.mkdir()
        (directory / "data").write_bytes(b"must not disappear")
        directory.chmod(0)
        try:
            with self.assertRaisesRegex(ERROR, "source_unreadable"):
                NAMESPACE["universe_tree"](self.root)
            with self.assertRaisesRegex(ERROR, "source_unreadable"):
                self.source.freeze()
        finally:
            directory.chmod(0o755)

    def test_os_walk_error_is_fatal(self):
        def failed_walk(root, onerror, followlinks):
            onerror(PermissionError("blocked"))
            return iter(())

        with patch.object(NAMESPACE["os"], "walk", side_effect=failed_walk):
            with self.assertRaisesRegex(ERROR, "source_unreadable"):
                NAMESPACE["universe_tree"](self.root)

    def test_native_entry_derived_from_bytes_and_copies_mappings(self):
        self.source.fetch_index("dep")
        self.source.fetch_archive("dep", "1.0.0")
        snapshot = self.source.freeze()
        expected = {"source_id": "registry+https://github.com/rust-lang/crates.io-index",
                    "root": str(self.root),
                    "index": {"3/d/dep": hashlib.sha256(self.raw).hexdigest()},
                    "archives": {"dep-1.0.0.crate":
                                 hashlib.sha256(self.archives["1.0.0"]).hexdigest()}}
        exported = snapshot.native_registry_entry()
        self.assertEqual(exported, expected)
        exported["index"]["3/d/dep"] = None
        self.assertEqual(snapshot.native_registry_entry(), expected)

    def test_only_exact_fixed_url_404_records_negative(self):
        url = "https://index.crates.io/3/d/dep"
        missing = NAMESPACE["RegistryNotFound"]
        self.source._fetch = lambda requested, limit: (_ for _ in ()).throw(missing(url))
        self.assertIsNone(self.source.fetch_index("dep"))
        snapshot = self.source.freeze()
        self.assertEqual(snapshot.native_registry_entry()["index"], {"3/d/dep": None})
        self.assertFalse((self.root / "index/3/d/dep").exists())

    def test_wrong_url_404_and_other_network_failures_never_record_negative(self):
        missing = NAMESPACE["RegistryNotFound"]
        for error in (missing("https://index.crates.io/3/d/other"), ERROR("fetch_failed")):
            def failed_fetch(url, limit):
                raise error

            self.source._fetch = failed_fetch
            with self.assertRaises(ERROR):
                self.source.fetch_index("dep")
            self.assertEqual(self.source._index_observations, {})
        with self.assertRaisesRegex(ERROR, "source_empty"):
            self.source.freeze()

    def test_archive_callback_cannot_rebaseline_mutated_index(self):
        self.source.fetch_index("dep")
        def mutated_fetch(url, limit):
            (self.root / "index/3/d/dep").write_bytes(b"forged")
            return self.archives["1.0.0"]

        self.source._fetch = mutated_fetch
        with self.assertRaisesRegex(ERROR, "source_changed"):
            self.source.fetch_archive("dep", "1.0.0")
        self.assertFalse((self.root / "dep-1.0.0.crate").exists())
        with self.assertRaisesRegex(ERROR, "source_changed"):
            self.source.freeze()

    def test_archive_callback_cannot_rebaseline_prior_archive(self):
        self.source.fetch_index("dep")
        self.source.fetch_archive("dep", "1.0.0")
        def mutated_fetch(url, limit):
            (self.root / "dep-1.0.0.crate").write_bytes(b"forged")
            return self.archives["1.1.0"]

        self.source._fetch = mutated_fetch
        with self.assertRaisesRegex(ERROR, "source_changed"):
            self.source.fetch_archive("dep", "1.1.0")
        self.assertFalse((self.root / "dep-1.1.0.crate").exists())
        with self.assertRaisesRegex(ERROR, "source_changed"):
            self.source.freeze()


if __name__ == "__main__":
    unittest.main()
