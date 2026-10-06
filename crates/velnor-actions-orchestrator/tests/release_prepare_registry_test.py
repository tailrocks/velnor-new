"""Local, mocked registry proof and hostile archive regression checks."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import tarfile
import urllib.request


DIRECTORY = Path(__file__).parent.parent / "src"
NAMESPACE = {}
for filename in ("release_reconcile_common.py", "release_prepare_bytes.py", "release_prepare_registry.py"):
    exec(compile((DIRECTORY / filename).read_text(), filename, "exec"), NAMESPACE)
ACQUIRE = NAMESPACE["acquire_registry_baseline"]
ERROR = NAMESPACE["ReconcileError"]


def archive(entries=None, manifest=None):
    prefix = "demo-crate-1.2.3"
    manifest = manifest or b'[package]\nname="demo-crate"\nversion="1.2.3"\n'
    entries = entries if entries is not None else [
        (prefix + "/Cargo.toml", manifest, tarfile.REGTYPE),
        (prefix + "/src/lib.rs", b"pub fn demo() {}", tarfile.REGTYPE)]
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.USTAR_FORMAT) as stream:
        for entry in entries:
            name, data, kind = entry[:3]
            member = tarfile.TarInfo(name)
            member.type = kind
            member.mode = entry[3] if len(entry) == 4 else 0o644
            member.size = len(data) if kind == tarfile.REGTYPE else 0
            if kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
                member.linkname = "../../outside"
            stream.addfile(member, io.BytesIO(data))
    return gzip.compress(raw.getvalue())


class RegistryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.destination = Path(self.temporary.name).resolve() / "baseline"
        self.raw = archive()
        self.entry = {"name": "demo-crate", "vers": "1.2.3", "yanked": False,
                      "cksum": hashlib.sha256(self.raw).hexdigest()}
        self.calls = []

    def fetch(self, url, limit):
        self.calls.append((url, limit))
        if url.startswith("https://index.crates.io/"):
            return json.dumps(self.entry).encode() + b"\n"
        return self.raw

    def acquire(self):
        return ACQUIRE("demo-crate", "1.2.3", self.destination, self.fetch)

    def hostile(self, entries):
        self.raw = archive(entries)
        self.entry["cksum"] = hashlib.sha256(self.raw).hexdigest()
        with self.assertRaises(ERROR):
            self.acquire()
        self.assertFalse(self.destination.exists())
        self.assertEqual(list(self.destination.parent.iterdir()), [])

    def test_authenticated_exact_baseline_and_inventory(self):
        evidence = self.acquire()
        root = Path(evidence["package_root"])
        self.assertEqual(evidence["registry"], "crates-io")
        self.assertEqual(evidence["checksum"], evidence["archive_sha256"])
        self.assertEqual(Path(evidence["archive_path"]).read_bytes(), self.raw)
        self.assertEqual(Path(evidence["archive_path"]).parent, root.parent)
        self.assertEqual(evidence["manifest_sha256"],
                         hashlib.sha256((root / "Cargo.toml").read_bytes()).hexdigest())
        inventory = NAMESPACE["registry_inventory"](root)
        expected = hashlib.sha256(json.dumps(inventory, sort_keys=True,
                                  separators=(",", ":")).encode()).hexdigest()
        self.assertEqual(evidence["inventory_sha256"], expected)
        self.assertEqual(self.calls[0][0], "https://index.crates.io/de/mo/demo-crate")
        self.assertEqual(self.calls[1][0],
                         "https://static.crates.io/crates/demo-crate/demo-crate-1.2.3.crate")

    def test_selected_index_version_retains_native_fields(self):
        self.entry.update(v=2, features={"default": []}, features2={"extra": ["dep:x"]},
                          links="native", deps=[{"name": "x", "req": "^1", "optional": True}])
        evidence = self.acquire()
        raw = Path(evidence["index_version_path"]).read_bytes()
        self.assertEqual(raw, json.dumps(self.entry).encode())
        self.assertEqual(json.loads(raw), self.entry)
        self.assertEqual(evidence["index_version_sha256"], hashlib.sha256(raw).hexdigest())
        self.assertNotIn("index-version.json", NAMESPACE["registry_inventory"](
            Path(evidence["package_root"])))

    def test_yanked_identity_preserved(self):
        self.entry["yanked"] = True
        self.assertTrue(self.acquire()["yanked"])

    def test_authenticated_ordinary_file_modes_preserved(self):
        prefix = "demo-crate-1.2.3/"
        entries = [(prefix + "Cargo.toml", b'[package]\nname="demo-crate"\nversion="1.2.3"\n',
                    tarfile.REGTYPE, 0o640)]
        modes = (0o600, 0o644, 0o711, 0o444)
        entries.extend((prefix + str(mode), b"data", tarfile.REGTYPE, mode) for mode in modes)
        self.raw = archive(entries)
        self.entry["cksum"] = hashlib.sha256(self.raw).hexdigest()
        root = Path(self.acquire()["package_root"])
        self.assertEqual((root / "Cargo.toml").stat().st_mode & 0o777, 0o640)
        for mode in modes:
            self.assertEqual((root / str(mode)).stat().st_mode & 0o777, mode)

    def test_directory_and_root_modes_deferred_until_children_written(self):
        prefix = "demo-crate-1.2.3"
        self.raw = archive([
            (prefix, b"", tarfile.DIRTYPE, 0o550),
            (prefix + "/src", b"", tarfile.DIRTYPE, 0o555),
            (prefix + "/src/nested", b"", tarfile.DIRTYPE, 0o510),
            (prefix + "/src/nested/lib.rs", b"content", tarfile.REGTYPE, 0o640),
            (prefix + "/Cargo.toml", b"manifest", tarfile.REGTYPE, 0o600)])
        staging = self.destination.parent / "staging"
        staging.mkdir()
        self.addCleanup(NAMESPACE["registry_remove_staging"], staging)
        NAMESPACE["registry_extract"](self.raw, staging, prefix)
        root = staging / prefix
        self.assertEqual(root.stat().st_mode & 0o777, 0o550)
        self.assertEqual((root / "src").stat().st_mode & 0o777, 0o555)
        self.assertEqual((root / "src/nested").stat().st_mode & 0o777, 0o510)
        self.assertEqual((root / "src/nested/lib.rs").read_bytes(), b"content")

    def test_implicit_parents_keep_native_mkdir_permissions(self):
        probe = self.destination.parent / "probe"
        probe.mkdir()
        expected = probe.stat().st_mode & 0o777
        root = Path(self.acquire()["package_root"])
        self.assertEqual(root.stat().st_mode & 0o777, expected)
        self.assertEqual((root / "src").stat().st_mode & 0o777, expected)

    def test_restrictive_staging_cleanup_after_manifest_rejection(self):
        prefix = "demo-crate-1.2.3"
        self.hostile([(prefix, b"", tarfile.DIRTYPE, 0o500),
                      (prefix + "/Cargo.toml", b"invalid manifest", tarfile.REGTYPE, 0o400)])

    def test_unreadable_ordinary_modes_preserved_and_acquisition_fails_closed(self):
        prefix = "demo-crate-1.2.3"
        entries = [(prefix, b"", tarfile.DIRTYPE, 0o000),
                   (prefix + "/src/lib.rs", b"content", tarfile.REGTYPE, 0o000)]
        staging = self.destination.parent / "staging"
        staging.mkdir()
        NAMESPACE["registry_extract"](archive(entries), staging, prefix)
        root = staging / prefix
        self.assertEqual(root.stat().st_mode & 0o777, 0o000)
        root.chmod(0o700)
        self.assertEqual((root / "src/lib.rs").stat().st_mode & 0o777, 0o000)
        NAMESPACE["registry_remove_staging"](staging)
        entries = [(prefix + "/Cargo.toml",
                    b'[package]\nname="demo-crate"\nversion="1.2.3"\n', tarfile.REGTYPE),
                   (prefix + "/src/lib.rs", b"content", tarfile.REGTYPE, 0o000)]
        self.raw = archive(entries)
        self.entry["cksum"] = hashlib.sha256(self.raw).hexdigest()
        original_read = Path.read_bytes

        def deny_unreadable(path):
            if path.name == "lib.rs":
                raise PermissionError("authenticated archive payload unreadable")
            return original_read(path)

        with patch.object(Path, "read_bytes", deny_unreadable):
            with self.assertRaises(PermissionError):
                self.acquire()
        self.assertFalse(self.destination.exists())
        self.assertEqual(list(self.destination.parent.iterdir()), [])

    def test_special_permissions_rejected_for_files_and_directories(self):
        for kind in (tarfile.REGTYPE, tarfile.DIRTYPE):
            for mode in (0o1644, 0o2644, 0o4644):
                self.hostile([("demo-crate-1.2.3/file", b"data", kind, mode)])

    def test_checksum_mismatch(self):
        self.entry["cksum"] = "0" * 64
        with self.assertRaisesRegex(ERROR, "checksum"):
            self.acquire()
        self.assertFalse(self.destination.exists())

    def test_alias_cannot_claim_canonical_identity(self):
        self.entry["name"] = "demo_crate"
        with self.assertRaisesRegex(ERROR, "canonical_name"):
            self.acquire()

    def test_forged_authority_boolean_and_nonboolean_yanked_rejected(self):
        self.entry["authenticated"] = True
        self.entry["yanked"] = 0
        with self.assertRaisesRegex(ERROR, "index_identity"):
            self.acquire()

    def test_duplicate_index_version_rejected(self):
        raw = json.dumps(self.entry).encode()
        with self.assertRaisesRegex(ERROR, "missing_or_duplicate"):
            NAMESPACE["registry_index_entry"](raw + b"\n" + raw, "demo-crate", "1.2.3")

    def test_duplicate_json_key_rejected(self):
        with self.assertRaisesRegex(ERROR, "duplicate_json_key"):
            NAMESPACE["registry_index_entry"](b'{"name":"a","name":"b"}', "a", "1.2.3")

    def test_missing_version_is_error(self):
        self.entry["vers"] = "1.2.2"
        with self.assertRaisesRegex(ERROR, "missing_or_duplicate"):
            self.acquire()

    def test_manifest_name_and_version_bound(self):
        for manifest in (b'[package]\nname="evil"\nversion="1.2.3"',
                         b'[package]\nname="demo-crate"\nversion="1.2.4"'):
            self.hostile([("demo-crate-1.2.3/Cargo.toml", manifest, tarfile.REGTYPE)])

    def test_path_escape_and_wrong_prefix_rejected(self):
        for path in ("demo-crate-1.2.3/../outside", "/demo-crate-1.2.3/file",
                     "evil-1.2.3/file", "demo-crate-1.2.3/dir\\file"):
            self.hostile([(path, b"data", tarfile.REGTYPE)])

    def test_links_special_files_rejected(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.CHRTYPE, tarfile.FIFOTYPE):
            self.hostile([("demo-crate-1.2.3/file", b"", kind)])

    def test_duplicate_archive_path_rejected(self):
        entry = ("demo-crate-1.2.3/file", b"data", tarfile.REGTYPE)
        self.hostile([entry, entry])

    def test_bounded_gzip_and_file_size(self):
        with patch.dict(NAMESPACE, REGISTRY_TAR_LIMIT=50):
            with self.assertRaisesRegex(ERROR, "tar_size"):
                self.acquire()
        with patch.dict(NAMESPACE, REGISTRY_FILE_LIMIT=2):
            with self.assertRaisesRegex(ERROR, "file_size"):
                self.acquire()

    def test_gzip_trailing_payload_rejected(self):
        self.raw += gzip.compress(b"extra")
        self.entry["cksum"] = hashlib.sha256(self.raw).hexdigest()
        with self.assertRaisesRegex(ERROR, "tar_size"):
            self.acquire()

    def test_destination_symlink_and_existing_rejected(self):
        self.destination.symlink_to(self.destination.parent, target_is_directory=True)
        with self.assertRaisesRegex(ERROR, "destination_exists"):
            self.acquire()
        self.destination.unlink()
        self.destination.mkdir()
        with self.assertRaisesRegex(ERROR, "destination_exists"):
            self.acquire()

    def test_fixed_transport_rejects_external_url_and_redirects(self):
        with self.assertRaisesRegex(ERROR, "fetch_url"):
            NAMESPACE["registry_fetch"]("https://evil.invalid/hello", 100)
        with self.assertRaisesRegex(ERROR, "redirect"):
            NAMESPACE["RegistryNoRedirect"]().redirect_request(
                None, None, 302, "redirect", {}, "https://evil.invalid/")

    def test_pax_unicode_long_paths_and_executable_mode(self):
        payload = io.BytesIO()
        with tarfile.open(fileobj=payload, mode="w", format=tarfile.PAX_FORMAT) as stream:
            for path, data, mode in (
                ("Cargo.toml", b'[package]\nname="demo-crate"\nversion="1.2.3"\n', 0o644),
                ("long/" + "界" * 100 + ".sh", b"#!/bin/sh\n", 0o755)):
                member = tarfile.TarInfo("demo-crate-1.2.3/" + path)
                member.size, member.mode = len(data), mode
                stream.addfile(member, io.BytesIO(data))
        self.raw = gzip.compress(payload.getvalue())
        self.entry["cksum"] = hashlib.sha256(self.raw).hexdigest()
        evidence = self.acquire()
        inventory = NAMESPACE["registry_inventory"](Path(evidence["package_root"]))
        self.assertEqual(inventory["long/" + "界" * 100 + ".sh"]["mode"], "100755")

    def test_invalid_semver_rejected_before_fetch(self):
        for version in ("1.2.3-01", "1.2.3-a..b", "1.2.3+a..b", "01.2.3"):
            with self.assertRaises(ERROR):
                ACQUIRE("demo-crate", version, self.destination, self.fetch)
        self.assertEqual(self.calls, [])

    def test_registry_404_separate_from_unpublished_authority(self):
        url = "https://index.crates.io/de/mo/demo-crate"
        response = io.BytesIO(b"missing")
        failure = NAMESPACE["urllib"].error.HTTPError(url, 404, "missing", {}, response)
        with patch.object(urllib.request.OpenerDirector, "open", side_effect=failure):
            with self.assertRaises(NAMESPACE["RegistryNotFound"]) as caught:
                NAMESPACE["registry_fetch"](url, 100)
        self.assertEqual(caught.exception.url, url)
        self.assertTrue(response.closed)
        self.assertIs(caught.exception.__cause__, failure)

    def test_non404_http_error_response_closed(self):
        url = "https://index.crates.io/de/mo/demo-crate"
        response = io.BytesIO(b"server error")
        failure = NAMESPACE["urllib"].error.HTTPError(url, 503, "unavailable", {}, response)
        with patch.object(urllib.request.OpenerDirector, "open", side_effect=failure):
            with self.assertRaisesRegex(ERROR, "registry_fetch_failed") as caught:
                NAMESPACE["registry_fetch"](url, 100)
        self.assertTrue(response.closed)
        self.assertIs(caught.exception.__cause__, failure)

    def test_sparse_paths(self):
        for name, expected in (("a", "1/a"), ("ab", "2/ab"), ("ABC", "3/a/abc"),
                               ("demo_crate", "de/mo/demo_crate")):
            self.assertEqual(NAMESPACE["registry_index_path"](name), expected)


if __name__ == "__main__":
    unittest.main()
