"""Adversarial Cargo archive and public registry proof fixtures."""
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
NS = {"__name__": "cargo_registry_test", "require": require,
      "ReconcileError": ReconcileError, "decode_json": json.loads}
for filename in ("release_reconcile_cargo.py", "release_package_contract.py", "release_reconcile_registry.py"):
    exec(compile((ROOT / filename).read_text(), filename, "exec"), NS)
SHA = "a" * 40
POLICY = {"source_sha": SHA, "owners": {"demo": ["team:2", "user:1"]}}


def archive(sha=SHA, dirty=False, manifest=None, extras=()):
    files = [("Cargo.toml", manifest or b'[package]\nname = "demo"\nversion = "1.0.0"\n[features]\ndefault = []\n'),
             (".cargo_vcs_info.json", json.dumps({"git": {"sha1": sha, "dirty": dirty},
                                                  "path_in_vcs": "crates/demo"}).encode()),
             ("src/lib.rs", b"pub fn example() {}\n")]
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as stream:
        for name, value in [*files, *extras]:
            entry = tarfile.TarInfo("demo-1.0.0/" + name)
            entry.size = len(value)
            stream.addfile(entry, io.BytesIO(value))
    return output.getvalue()


class ArchiveTests(unittest.TestCase):
    def test_normalizes_toml_and_validated_vcs(self):
        first = NS["inventory"](archive(), "demo", "1.0.0", SHA)
        alternate = b'[package]\nversion="1.0.0"\nname="demo"\n# comment\n[features]\ndefault=[]\n'
        second = NS["inventory"](archive(manifest=alternate), "demo", "1.0.0", SHA)
        self.assertEqual(first, second)
        self.assertIn(".cargo_vcs_info.json", first["files"])

    def test_wrong_sha_dirty_duplicates_and_traversal_fail(self):
        for value in (archive(sha="b" * 40), archive(dirty=True),
                      archive(extras=[("src/lib.rs", b"other")]),
                      archive(extras=[("../escape", b"x")])):
            with self.subTest(size=len(value)), self.assertRaises(NS["ReconcileError"]):
                NS["inventory"](value, "demo", "1.0.0", SHA)

    def test_total_decompression_is_bounded_before_tar(self):
        class Bomb:
            def __enter__(self):
                return self
            def __exit__(self, *_):
                return False
            def read(self, bound):
                self.bound = bound
                return Sized(bound)
        class Sized:
            def __init__(self, size):
                self.size = size
            def __len__(self):
                return self.size
        with patch.object(NS["gzip"], "GzipFile", return_value=Bomb()), self.assertRaisesRegex(
                NS["ReconcileError"], "archive_total_expansion"):
            NS["inventory"](b"compressed", "demo", "1.0.0", SHA)


class RegistryTests(unittest.TestCase):
    def setUp(self):
        self.data = archive()
        self.checksum = hashlib.sha256(self.data).hexdigest()
        self.expected = NS["inventory"](self.data, "demo", "1.0.0", SHA)
        self.version = {"crate": "demo", "num": "1.0.0", "checksum": self.checksum,
                        "features": {"default": []}, "yanked": False}
        self.index = {"name": "demo", "vers": "1.0.0", "cksum": self.checksum,
                      "features": {"default": []}, "yanked": False}

    def fetch(self, url, *_):
        if url.endswith("owner_user"):
            return json.dumps({"users": [{"id": 1}]}).encode()
        if url.endswith("owner_team"):
            return json.dumps({"teams": [{"id": 2}]}).encode()
        if url.startswith("https://index.crates.io/"):
            return json.dumps(self.index).encode()
        if url.startswith("https://static.crates.io/"):
            return self.data
        return json.dumps({"version": self.version}).encode()

    def run_registry(self):
        with patch.dict(NS, {"fetch": self.fetch}):
            return NS["registry_package"](POLICY, "demo", "1.0.0", self.expected)

    def test_independent_proof_succeeds(self):
        self.assertEqual(self.run_registry()["status"], "verified")

    def test_yank_checksum_owner_features_and_source_fail(self):
        mutations = [(self.version, "yanked", True), (self.index, "cksum", "b" * 64),
                     (self.version, "features", {"new": []})]
        for target, key, value in mutations:
            original = target[key]
            target[key] = value
            with self.assertRaises(NS["ReconcileError"]):
                self.run_registry()
            target[key] = original
        with patch.dict(NS, {"fetch": self.fetch}):
            for changed in ({**POLICY, "owners": {"demo": ["user:1"]}},
                            {**POLICY, "source_sha": "b" * 40}):
                with self.assertRaises(NS["ReconcileError"]):
                    NS["registry_package"](changed, "demo", "1.0.0", self.expected)
        self.data = self.data + b"tamper"
        with self.assertRaises(NS["ReconcileError"]):
            self.run_registry()



if __name__ == "__main__":
    unittest.main()
