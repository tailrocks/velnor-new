import importlib.util
import hashlib
import io
import json
import tarfile
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace


MODULE_PATH = Path(__file__).with_name("plan-debian-package.py")
SPEC = importlib.util.spec_from_file_location("package_development", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
package_development = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(package_development)


class DebianDevelopmentVersionTests(unittest.TestCase):
    def test_first_qualification_sorts_after_v7_and_before_next_release(self):
        version, utc = package_development.compute_version(
            "0.1.2",
            1,
            "66da8c2fac0cc10c8f9ae3320656d18ebfa6a8b0",
            "2026-10-08T04:31:53+02:00",
            "0.1.1-1",
        )
        self.assertEqual(version, "0.1.2~dev0001+20261008023153+g66da8c2fac0c-1")
        self.assertEqual(utc, "2026-10-08T02:31:53Z")

    def test_sequence_orders_qual_b_even_if_commit_time_is_earlier(self):
        first, _ = package_development.compute_version(
            "0.1.2",
            1,
            "66da8c2fac0cc10c8f9ae3320656d18ebfa6a8b0",
            "2026-10-08T02:31:53Z",
            "0.1.1-1",
        )
        second, _ = package_development.compute_version(
            "0.1.2",
            2,
            "ffffffffffffffffffffffffffffffffffffffff",
            "2026-10-07T23:59:59Z",
            first,
        )
        self.assertTrue(package_development.dpkg_compare(first, "lt", second))
        self.assertTrue(package_development.dpkg_compare(second, "lt", "0.1.2-1"))

    def test_rejects_same_source_as_a_second_qualification_candidate(self):
        first, _ = package_development.compute_version(
            "0.1.2",
            1,
            "66da8c2fac0cc10c8f9ae3320656d18ebfa6a8b0",
            "2026-10-08T02:31:53Z",
            "0.1.1-1",
        )
        with self.assertRaises(package_development.EvidenceError):
            package_development.compute_version(
                "0.1.2",
                2,
                "66da8c2fac0cc10c8f9ae3320656d18ebfa6a8b0",
                "2026-10-08T02:31:53Z",
                first,
            )

    def test_rejects_skipped_sequence(self):
        first, _ = package_development.compute_version(
            "0.1.2",
            1,
            "66da8c2fac0cc10c8f9ae3320656d18ebfa6a8b0",
            "2026-10-08T02:31:53Z",
            "0.1.1-1",
        )
        with self.assertRaises(package_development.EvidenceError):
            package_development.compute_version(
                "0.1.2",
                3,
                "ffffffffffffffffffffffffffffffffffffffff",
                "2026-10-08T03:00:00Z",
                first,
            )

    def test_rejects_time_without_timezone(self):
        with self.assertRaises(package_development.EvidenceError):
            package_development.normalize_utc("2026-10-08T02:31:53")

    def _provenance_fixture(self, use_manifest_default=False):
        temporary = tempfile.TemporaryDirectory(prefix="velnor-package-provenance-test-")
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)

        def digest(path):
            return hashlib.sha256(path.read_bytes()).hexdigest()

        lock_bytes = b"# isolated Cargo.lock fixture\n"
        archive_path = root / "source.tar"
        with tarfile.open(archive_path, "w") as archive:
            info = tarfile.TarInfo("Cargo.lock")
            info.size = len(lock_bytes)
            archive.addfile(info, io.BytesIO(lock_bytes))
        lock_hash = hashlib.sha256(lock_bytes).hexdigest()
        version = "0.1.1-1" if use_manifest_default else "0.1.2~dev0001+20261008023153+g66da8c2fac0c-1"
        target = "x86_64-unknown-linux-gnu"
        manifest = str(root / "source/crates/tools/velnor-runner-cli/Cargo.toml")
        binary_path = root / "velnor-host"
        binary_path.write_bytes(b"test-only binary placeholder")
        build_log = root / "build.log"
        build_log.write_text("test-only build log placeholder\n", encoding="utf-8")
        package_path = root / "output" / f"velnor-host_{version}_amd64.deb"
        package_path.parent.mkdir()
        package_path.write_bytes(b"test-only package placeholder")
        cargo_deb_path = root / "tools/cargo-deb"
        cargo_deb_path.parent.mkdir()
        cargo_deb_path.write_bytes(b"test-only pinned tool placeholder")

        build_command_file = root / "build.command.txt"
        build_command_file.write_text(
            "env -i PATH=/fake/tools:/usr/bin cargo +1.99.0 build --frozen --release "
            f"--target {target} --manifest-path {manifest} --bin velnor-host\n",
            encoding="utf-8",
        )
        package_command_file = root / "package.command.txt"
        version_arg = "" if use_manifest_default else f"--deb-version {version} "
        package_command_file.write_text(
            f"env -i PATH={cargo_deb_path.parent}:/usr/bin cargo +1.99.0 deb --locked "
            f"--manifest-path {manifest} --target {target} --no-build --no-strip "
            f"{version_arg}--output {package_path.parent}\n",
            encoding="utf-8",
        )
        package_log = root / "package.log"
        package_log.write_text("test-only package log placeholder\n", encoding="utf-8")

        plan = {
            "status": "VERSION_PLAN_ONLY_NO_BINARY_OR_DEB",
            "source": {
                "commit": "a" * 40,
                "tree": "b" * 40,
                "archive_path": str(archive_path),
                "archive_sha256": digest(archive_path),
                "cargo_lock_sha256": lock_hash,
            },
            "package": {
                "name": "velnor-host",
                "version": version,
                "architecture": "amd64",
                "rust_target": target,
                "cargo_upstream_version_at_source": "0.1.1",
            },
            "cargo_deb": {
                "binary_path": str(cargo_deb_path),
                "binary_sha256": digest(cargo_deb_path),
            },
        }
        record = {
            "schema": 1,
            "source": {
                "commit": plan["source"]["commit"],
                "tree": plan["source"]["tree"],
                "archive_sha256": plan["source"]["archive_sha256"],
                "Cargo_lock_sha256": lock_hash,
            },
            "build": {
                "exit_code": 0,
                "command_file": str(build_command_file),
                "command_sha256": digest(build_command_file),
                "stdout_stderr_log": str(build_log),
                "stdout_stderr_sha256": digest(build_log),
                "toolchain": {
                    "cargo": "1.99.0 (fixture)",
                    "rustc": "1.99.0 (fixture)",
                    "features": [],
                    "profile": "release",
                    "target": target,
                },
                "binary": {
                    "path": str(binary_path),
                    "sha256": digest(binary_path),
                },
            },
            "package": {
                "architecture": "amd64",
                "command_file": str(package_command_file),
                "command_sha256": digest(package_command_file),
                "exit_code": 0,
                "no_build": True,
                "no_strip": True,
                "tool": "cargo-deb 3.8.0",
                "tool_binary_sha256": digest(cargo_deb_path),
                "stdout_stderr_log": str(package_log),
                "stdout_stderr_sha256": digest(package_log),
                "version": version,
                "path": str(package_path),
                "sha256": digest(package_path),
                "packaged_binary_sha256": digest(binary_path),
            },
        }
        plan_path = root / "plan.json"
        plan_path.write_text(json.dumps(plan), encoding="utf-8")
        record_path = root / "provenance.json"
        record_path.write_text(json.dumps(record), encoding="utf-8")
        args = SimpleNamespace(plan=plan_path, binary_build_record=record_path, deb=root / "not-read.deb")
        return root, plan, record, args, cargo_deb_path

    def test_accepts_complete_provenance_v1_metadata_fixture(self):
        _root, plan, record, _args, cargo_deb_path = self._provenance_fixture()
        verified = package_development.validate_build_provenance(
            record, plan, cargo_deb_path, plan["cargo_deb"]["binary_sha256"]
        )
        self.assertEqual(verified["rustc"], "1.99.0 (fixture)")
        self.assertEqual(verified["profile"], "release")
        self.assertEqual(verified["features"], [])

    def test_accepts_existing_provenance_v1_manifest_default_version(self):
        _root, plan, record, _args, cargo_deb_path = self._provenance_fixture(
            use_manifest_default=True
        )
        verified = package_development.validate_build_provenance(
            record, plan, cargo_deb_path, plan["cargo_deb"]["binary_sha256"]
        )
        self.assertEqual(plan["package"]["version"], "0.1.1-1")
        self.assertEqual(verified["profile"], "release")

    def _assert_verify_rejects(self, mutation, message):
        _root, _plan, record, args, _cargo_deb_path = self._provenance_fixture()
        mutation(record)
        args.binary_build_record.write_text(json.dumps(record), encoding="utf-8")
        with self.assertRaisesRegex(package_development.EvidenceError, message):
            package_development.verify_package(args)

    def test_verify_package_rejects_missing_rustc(self):
        self._assert_verify_rejects(
            lambda record: record["build"]["toolchain"].pop("rustc"),
            "toolchain rustc version must be a non-empty string",
        )

    def test_verify_package_rejects_wrong_type_rustc(self):
        self._assert_verify_rejects(
            lambda record: record["build"]["toolchain"].update(rustc=199),
            "toolchain rustc version must be a non-empty string",
        )

    def test_verify_package_rejects_missing_profile(self):
        self._assert_verify_rejects(
            lambda record: record["build"]["toolchain"].pop("profile"),
            "toolchain profile must be the release profile",
        )

    def test_verify_package_rejects_wrong_type_profile(self):
        self._assert_verify_rejects(
            lambda record: record["build"]["toolchain"].update(profile=["release"]),
            "toolchain profile must be the release profile",
        )

    def test_verify_package_rejects_missing_features(self):
        self._assert_verify_rejects(
            lambda record: record["build"]["toolchain"].pop("features"),
            "toolchain features must be an explicit list",
        )

    def test_verify_package_rejects_wrong_type_features(self):
        self._assert_verify_rejects(
            lambda record: record["build"]["toolchain"].update(features="default"),
            "toolchain features must be an explicit list",
        )

    def test_verify_package_rejects_failed_build_producer_result(self):
        self._assert_verify_rejects(
            lambda record: record["build"].update(exit_code=True),
            "build producer must record integer exit_code 0",
        )


if __name__ == "__main__":
    unittest.main()
