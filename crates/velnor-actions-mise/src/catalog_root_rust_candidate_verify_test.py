"""Negative and mutation tests for pre-install RootLinux Rust mirror verification."""

import os
import stat
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import catalog_root_rust_candidate_archives as archive
from catalog_root_rust_candidate_archives_test import MANIFEST, archive_for


SOURCE = Path(__file__).with_name("catalog_root_rust_candidate_archives.py").read_text()
VERIFY = Path(__file__).with_name("catalog_root_rust_candidate_verify.py").read_text()


class CandidateMirrorVerifyTests(unittest.TestCase):
    def setUp(self):
        self.sandbox = tempfile.TemporaryDirectory()
        self.temp = Path(self.sandbox.name).resolve()
        self.root = self.temp / "velnor-control" / "root-rust-candidate"
        self.root.mkdir(parents=True)
        (self.root / "cargo-home").mkdir()
        (self.root / "rustup-home").mkdir()
        self.config, self.archives = self.fixture()
        archive._publish(str(self.root), MANIFEST, {
            component: (archive._COMPONENT_FILES[component], self.archives[component])
            for component in archive.COMPONENTS
        })

    def tearDown(self):
        native = self.root / "native-dist"
        if native.is_dir() and not stat.S_ISLNK(os.lstat(native).st_mode):
            for directory, _, _ in os.walk(native):
                os.chmod(directory, 0o700)
        self.sandbox.cleanup()

    def fixture(self):
        components, payload = [], []
        for component in archive.COMPONENTS:
            path = "bin/" + component.replace("-preview", "")
            root = archive._COMPONENT_FILES[component][:-len(".tar.xz")]
            folder = archive._COMPONENT_FOLDERS[component]
            data = (path + " bytes").encode("ascii")
            item = {"path": path, "component": component,
                    "archive_member": root + "/" + folder + "/" + path,
                    "size": len(data), "sha256": archive.hashlib.sha256(data).hexdigest(),
                    "mode": 0o755}
            payload.append(item)
            data_archive = archive_for(component, [item])
            components.append({"component": component,
                               "xz_url": "https://static.rust-lang.org/dist/" +
                               archive.RELEASE_DATE + "/" + archive._COMPONENT_FILES[component],
                               "xz_sha256": archive.hashlib.sha256(data_archive).hexdigest()})
        config = {"manifest": {"version": archive.VERSION, "target": archive.TARGET,
                               "manifest_url": archive.MANIFEST_URL,
                               "manifest_sha256": archive.hashlib.sha256(
                                   MANIFEST).hexdigest(),
                               "components": components}, "payload": payload}
        archives = {component: archive_for(component, [payload[index]])
                    for index, component in enumerate(archive.COMPONENTS)}
        return config, archives

    def run_verify(self):
        namespace = {}
        exec(SOURCE + "\n" + VERIFY, namespace)
        namespace["CONFIG"] = self.config
        environment = {"RUNNER_TEMP": str(self.temp),
                       "VELNOR_ROOT_RUST_CANDIDATE_ROOT": str(self.root),
                       "CARGO_HOME": str(self.root / "cargo-home"),
                       "RUSTUP_HOME": str(self.root / "rustup-home")}
        with patch.dict(os.environ, environment, clear=True):
            namespace["candidate_verify_mirror"]()

    def test_exact_mirror_passes_without_network(self):
        with patch.object(archive.urllib.request, "build_opener") as opener:
            self.run_verify()
        opener.assert_not_called()
        native = self.root / "native-dist"
        for directory, _, names in os.walk(native):
            self.assertEqual(os.stat(directory).st_mode & 0o777, 0o555)
            for name in names:
                self.assertEqual(os.stat(Path(directory) / name).st_mode & 0o777, 0o444)

    def test_missing_v2_sidecar_rejects_legacy_only_mirror(self):
        sidecar = self.root / "native-dist/dist/channel-rust-1.98.1.toml.sha256"
        sidecar.unlink()
        with self.assertRaisesRegex(ValueError, "graph"):
            self.run_verify()

    def test_mutated_archive_rejects_before_install(self):
        path = self.root / "native-dist/dist" / archive.RELEASE_DATE / archive._COMPONENT_FILES["rustc"]
        path.write_bytes(path.read_bytes() + b"mutation")
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            self.run_verify()

    def test_native_dist_symlink_rejected(self):
        native = self.root / "native-dist"
        outside = self.temp / "outside"
        outside.mkdir()
        # Preserve the original mirror for cleanup, then replace only the fixed root leaf.
        import shutil
        shutil.rmtree(native)
        native.symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "directory"):
            self.run_verify()

    def test_forged_toolchain_blocks_before_install(self):
        toolchains = self.root / "rustup-home/toolchains"
        toolchains.mkdir()
        (toolchains / "forged").write_bytes(b"compiler")
        with self.assertRaisesRegex(ValueError, "must be empty"):
            self.run_verify()

    def test_cargo_and_rustup_home_bindings_are_exact(self):
        with self.assertRaisesRegex(ValueError, "home binding"):
            namespace = {}
            exec(SOURCE + "\n" + VERIFY, namespace)
            namespace["CONFIG"] = self.config
            environment = {"RUNNER_TEMP": str(self.temp),
                           "VELNOR_ROOT_RUST_CANDIDATE_ROOT": str(self.root),
                           "CARGO_HOME": str(self.root / "foreign"),
                           "RUSTUP_HOME": str(self.root / "rustup-home")}
            with patch.dict(os.environ, environment, clear=True):
                namespace["candidate_verify_mirror"]()

    def test_home_symlink_rejected(self):
        cargo = self.root / "cargo-home"
        cargo.rmdir()
        cargo.symlink_to(self.temp, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "Cargo home must be a directory"):
            self.run_verify()

    def test_home_foreign_filesystem_rejected(self):
        namespace = {}
        exec(SOURCE + "\n" + VERIFY, namespace)
        namespace["CONFIG"] = self.config
        target = str(self.root / "cargo-home")
        real_lstat = namespace["os"].lstat

        class ForeignDevice:
            def __init__(self, info):
                self.info = info
                self.st_dev = info.st_dev + 1

            def __getattr__(self, name):
                return getattr(self.info, name)

        def lstat(path):
            info = real_lstat(path)
            return ForeignDevice(info) if path == target else info

        environment = {"RUNNER_TEMP": str(self.temp),
                       "VELNOR_ROOT_RUST_CANDIDATE_ROOT": str(self.root),
                       "CARGO_HOME": target,
                       "RUSTUP_HOME": str(self.root / "rustup-home")}
        with self.assertRaisesRegex(ValueError, "filesystem binding"), \
                patch.dict(os.environ, environment, clear=True), \
                patch.object(namespace["os"], "lstat", side_effect=lstat):
            namespace["candidate_verify_mirror"]()

    def test_archive_symlink_is_never_followed(self):
        path = self.root / "native-dist/dist" / archive.RELEASE_DATE / archive._COMPONENT_FILES["rustc"]
        outside = self.temp / "outside-archive"
        outside.write_bytes(path.read_bytes())
        path.unlink()
        path.symlink_to(outside)
        with self.assertRaisesRegex(ValueError, "unavailable"):
            self.run_verify()

    def test_extra_graph_entry_rejected(self):
        extra = self.root / "native-dist/dist/extra"
        extra.write_bytes(b"unexpected")
        with self.assertRaisesRegex(ValueError, "graph"):
            self.run_verify()

    def test_native_dist_extra_entry_rejected(self):
        extra = self.root / "native-dist/extra"
        extra.write_bytes(b"unexpected")
        with self.assertRaisesRegex(ValueError, "graph"):
            self.run_verify()


if __name__ == "__main__":
    unittest.main()
