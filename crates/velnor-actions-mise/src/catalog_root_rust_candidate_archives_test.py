"""Network-free tests for the closed RootLinux Rust candidate archive boundary."""

import copy
import hashlib
import io
import os
from pathlib import Path
import stat
import tarfile
import tempfile
import unittest
from unittest.mock import Mock, patch

import catalog_root_rust_candidate_archives as candidate


MANIFEST = b"fixture official manifest\n"


def archive_for(component, records, link=False, file_mode=0o755):
    output = io.BytesIO()
    root = candidate._COMPONENT_FILES[component][:-len(".tar.xz")]
    folder = candidate._COMPONENT_FOLDERS[component]
    files = {item["archive_member"]: (item["path"].encode("ascii") + b" bytes")
             for item in records}
    manifest_name = root + "/" + folder + "/manifest.in"
    manifest = b"".join((b"file:" + item["path"].encode("ascii") + b"\n")
                         for item in records)
    with tarfile.open(fileobj=output, mode="w:xz") as tar:
        for name in (root, root + "/" + folder, root + "/" + folder + "/bin"):
            entry = tarfile.TarInfo(name)
            entry.type = tarfile.DIRTYPE
            entry.mode = file_mode
            tar.addfile(entry)
        for name, data in files.items():
            entry = tarfile.TarInfo(name)
            entry.type = tarfile.REGTYPE
            entry.mode = 0o755
            entry.size = len(data)
            tar.addfile(entry, io.BytesIO(data))
        entry = tarfile.TarInfo(manifest_name)
        entry.type = tarfile.REGTYPE
        entry.mode = 0o644
        entry.size = len(manifest)
        tar.addfile(entry, io.BytesIO(manifest))
        if link:
            entry = tarfile.TarInfo(root + "/" + folder + "/bin/link")
            entry.type = tarfile.SYMTYPE
            entry.linkname = "../../outside"
            tar.addfile(entry)
    return output.getvalue()


def fixture():
    components, payload = [], []
    for component in candidate.COMPONENTS:
        path = "bin/" + component.replace("-preview", "")
        root = candidate._COMPONENT_FILES[component][:-len(".tar.xz")]
        folder = candidate._COMPONENT_FOLDERS[component]
        data = (path + " bytes").encode("ascii")
        item = {"path": path, "component": component,
                "archive_member": root + "/" + folder + "/" + path,
                "size": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                "mode": 0o755}
        payload.append(item)
        archive = archive_for(component, [item])
        components.append({"component": component,
                           "xz_url": "https://static.rust-lang.org/dist/" +
                           candidate.RELEASE_DATE + "/" + candidate._COMPONENT_FILES[component],
                           "xz_sha256": hashlib.sha256(archive).hexdigest()})
    config = {"manifest": {"version": candidate.VERSION, "target": candidate.TARGET,
                           "manifest_url": candidate.MANIFEST_URL,
                           "manifest_sha256": hashlib.sha256(MANIFEST).hexdigest(),
                           "components": components}, "payload": payload}
    archives = {component: archive_for(component, [payload[index]])
                for index, component in enumerate(candidate.COMPONENTS)}
    return config, archives


class CandidateArchiveTests(unittest.TestCase):
    def setUp(self):
        self.sandbox = tempfile.TemporaryDirectory()
        self.temp = Path(self.sandbox.name).resolve()
        self.root = self.temp / "velnor-control" / "root-rust-candidate"
        self.root.mkdir(parents=True)

    def tearDown(self):
        self.sandbox.cleanup()

    def environment(self):
        return {"RUNNER_TEMP": str(self.temp),
                "VELNOR_ROOT_RUST_CANDIDATE_ROOT": str(self.root)}

    def test_preflight_and_mirror_preserve_verified_bytes(self):
        config, archives = fixture()
        fetched = {config["manifest"]["manifest_url"]: MANIFEST}
        for item, component in zip(config["manifest"]["components"], candidate.COMPONENTS):
            fetched[item["xz_url"]] = archives[component]

        def fetch(url, expected, limit):
            self.assertLessEqual(len(fetched[url]), limit)
            self.assertEqual(hashlib.sha256(fetched[url]).hexdigest(), expected)
            return fetched[url]

        with patch.dict(os.environ, self.environment(), clear=False):
            candidate._acquire(config, fetch)
        native = self.root / "native-dist"
        self.assertEqual((native / "dist/channel-rust-1.98.1.toml").read_bytes(), MANIFEST)
        self.assertEqual((native / "dist/channel-rust-1.98.1.toml.sha256").read_text(),
                         hashlib.sha256(MANIFEST).hexdigest() +
                         "  channel-rust-1.98.1.toml\n")
        for component in candidate.COMPONENTS:
            path = native / "dist" / candidate.RELEASE_DATE / candidate._COMPONENT_FILES[component]
            self.assertEqual(path.read_bytes(), archives[component])
        self.assertEqual(stat.S_IMODE(native.stat().st_mode), 0o700)

    def test_main_uses_only_compiled_global_config(self):
        config, archives = fixture()
        fetched = {config["manifest"]["manifest_url"]: MANIFEST}
        for item, component in zip(config["manifest"]["components"], candidate.COMPONENTS):
            fetched[item["xz_url"]] = archives[component]
        with patch.dict(os.environ, self.environment(), clear=False), \
                patch.object(candidate, "_download_bytes",
                              side_effect=lambda url, expected, limit: fetched[url]):
            candidate.CONFIG = config
            try:
                candidate.candidate_acquire_archives()
            finally:
                del candidate.CONFIG
        self.assertTrue((self.root / "native-dist").is_dir())

    def test_corrupt_archive_fails_before_native_dist_creation(self):
        config, archives = fixture()
        component = candidate.COMPONENTS[0]
        item = config["manifest"]["components"][0]
        changed = dict(archives)
        changed[component] = archives[component] + b"corrupt"

        def fetch(url, expected, limit):
            if url == item["xz_url"]:
                return changed[component]
            return MANIFEST

        with patch.dict(os.environ, self.environment(), clear=False), self.assertRaisesRegex(
                ValueError, "SHA-256"):
            candidate._acquire(config, fetch)
        self.assertFalse((self.root / "native-dist").exists())

    def test_payload_mutation_fails_archive_identity(self):
        config, archives = fixture()
        mutated = copy.deepcopy(config)
        mutated["payload"][0]["sha256"] = "0" * 64
        with patch.dict(os.environ, self.environment(), clear=False), self.assertRaisesRegex(
                ValueError, "payload bytes"):
            candidate._acquire(mutated, lambda url, expected, limit:
                               MANIFEST if url == candidate.MANIFEST_URL else
                               archives[candidate.COMPONENTS[0]])
        self.assertFalse((self.root / "native-dist").exists())

    def test_symlink_native_dist_rejected_without_fetch(self):
        config, _ = fixture()
        outside = self.temp / "outside"
        outside.mkdir()
        (self.root / "native-dist").symlink_to(outside, target_is_directory=True)
        fetch = Mock()
        with patch.dict(os.environ, self.environment(), clear=False), self.assertRaisesRegex(
                ValueError, "fresh"):
            candidate._acquire(config, fetch)
        fetch.assert_not_called()

    def test_archive_links_and_missing_manifest_entry_rejected(self):
        config, _ = fixture()
        component = candidate.COMPONENTS[0]
        item = config["payload"][0]
        linked = archive_for(component, [item], link=True)
        with self.assertRaisesRegex(ValueError, "member type"):
            candidate._validate_archive(linked, component, [item])
        broken = archive_for(component, [], link=False)
        with self.assertRaisesRegex(ValueError, "file set"):
            candidate._validate_archive(broken, component, [item])

    def test_archive_mode_uses_owner_bits_only(self):
        config, _ = fixture()
        component = candidate.COMPONENTS[0]
        item = config["payload"][0]
        special = archive_for(component, [item], file_mode=0o4755)
        candidate._validate_archive(special, component, [item])


if __name__ == "__main__":
    unittest.main()
