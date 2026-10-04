"""Focused native stage corruption, retention and signed-key regression tests."""
import gzip
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from delivery_apt_core import config_digest, digest, write_json
from delivery_apt_stage import guard, inventory, verified_input
from delivery_apt_stage_feed import check_state, fetch, pool_path, release_hashes, stanzas, verify_signature
from delivery_apt_stage_publish import build_indexes, release_document, rollback, version_order


CONFIG = {"source_repository": "example/source", "consumer_repository": "example/feed",
          "package": "example", "binary": "example", "identity_directory": "example",
          "manifest_schema": "example.manifest/v1", "keyring": "keys/publisher.gpg",
          "signer_fingerprint": "A" * 40, "origin": "Example", "description": "Example APT",
          "feed_url": "https://example.test/apt", "branch": "main", "schedule": "0 * * * *",
          "signer_workflow": ".github/workflows/release.yml",
          "oci_image_repository": "ghcr.io/example/source",
          "oci_signer_workflow": ".github/workflows/delivery-oci.yml"}


class ArtifactFixture(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.previous = Path.cwd()
        os.chdir(self.temporary.name)
        self.environment = patch.dict(os.environ, {"INPUT_SUITE": "stable", "GITHUB_RUN_ID": "42",
                                                  "GITHUB_RUN_ATTEMPT": "1", "GITHUB_SHA": "c" * 40})
        self.environment.start()
        self.candidate_patch = patch("delivery_apt_stage.candidate_hashes",
                                     return_value={"amd64": "a" * 64, "arm64": "b" * 64})
        self.candidate_patch.start()
        self.signer_patch = patch("delivery_apt_stage.signer")
        self.signer_patch.start()

    def tearDown(self):
        self.candidate_patch.stop()
        self.signer_patch.stop()
        self.environment.stop()
        os.chdir(self.previous)
        self.temporary.cleanup()

    def verified(self):
        root = Path("incoming")
        root.mkdir()
        for name in ("manifest.json", "release-record.json", "example-1.2.3-amd64.deb", "example-1.2.3-arm64.deb"):
            (root / name).write_bytes(name.encode())
        files = inventory(root)
        marker = {"schema": "velnor.apt-verified/v1", "config": CONFIG, "config_sha256": config_digest(CONFIG),
                  "suite": "stable", "version": "v1.2.3", "debian_version": "1.2.3", "commit": "f" * 40,
                  "source_ref": "refs/tags/v1.2.3", "files": files, "workflow_run_id": "42",
                  "workflow_run_attempt": "1", "consumer_source_sha": "c" * 40, "source_record_sha256": files["release-record.json"],
                  "manifest_sha256": files["manifest.json"], "packages": [
                      {"arch": arch, "name": f"example-1.2.3-{arch}.deb", "sha256": files[f"example-1.2.3-{arch}.deb"]}
                      for arch in ("amd64", "arm64")]}
        write_json(root / ".apt-verified.json", marker)
        return marker

    def test_verified_accepts_exact_artifact(self):
        expected = self.verified()
        self.assertEqual(verified_input(CONFIG), expected)

    def test_corrupted_manifest_refused_before_tools(self):
        self.verified()
        Path("incoming/manifest.json").write_text("changed")
        with self.assertRaisesRegex(ValueError, "changed after verification"):
            verified_input(CONFIG)

    def test_extra_file_refused(self):
        self.verified()
        Path("incoming/extra.deb").write_text("unverified")
        with self.assertRaisesRegex(ValueError, "changed after verification"):
            verified_input(CONFIG)

    def test_configuration_change_refused(self):
        self.verified()
        with self.assertRaisesRegex(ValueError, "configuration mismatch"):
            verified_input(dict(CONFIG, origin="Other"))

    def test_other_workflow_run_refused(self):
        self.verified()
        with patch.dict(os.environ, {"GITHUB_RUN_ID": "43"}):
            with self.assertRaisesRegex(ValueError, "another workflow run"):
                verified_input(CONFIG)

    def test_other_consumer_source_sha_refused(self):
        self.verified()
        with patch.dict(os.environ, {"GITHUB_SHA": "d" * 40}):
            with self.assertRaisesRegex(ValueError, "consumer source SHA"):
                verified_input(CONFIG)

    def test_symlink_refused(self):
        self.verified()
        target = Path("incoming/manifest.json")
        target.unlink()
        target.symlink_to("release-record.json")
        with self.assertRaisesRegex(ValueError, "symlink"):
            verified_input(CONFIG)

    def test_staged_corruption_refused_before_network(self):
        root = Path("public")
        root.mkdir()
        (root / "last-publish").write_text("v1.2.3\n")
        proof = {"schema": "velnor.apt-stage/v1", "config_sha256": config_digest(CONFIG), "suite": "stable",
                 "version": "v1.2.3", "workflow_run_id": "42", "workflow_run_attempt": "1", "consumer_source_sha": "c" * 40, "files": inventory(root)}
        write_json(root / ".apt-stage.json", proof)
        (root / "last-publish").write_text("v9.9.9\n")
        with patch("delivery_apt_stage.live_suite") as network:
            with self.assertRaisesRegex(ValueError, "corrupted"):
                guard(CONFIG)
            network.assert_not_called()


    def staged(self):
        root = Path("public")
        root.mkdir()
        (root / "last-publish").write_text("v1.2.3\n")
        packages = [{"arch": arch, "name": f"example-1.2.3-{arch}.deb", "sha256": checksum}
                    for arch, checksum in (("amd64", "a" * 64), ("arm64", "b" * 64))]
        state_packages = [{"name": item["name"], "sha256": item["sha256"]} for item in packages]
        write_json(root / "package-state.json", {"source_commit": "f" * 40, "source_ref": "refs/tags/v1.2.3", "packages": state_packages})
        proof = {"schema": "velnor.apt-stage/v1", "config_sha256": config_digest(CONFIG), "suite": "stable",
                 "version": "v1.2.3", "debian_version": "1.2.3", "commit": "f" * 40,
                 "workflow_run_id": "42", "workflow_run_attempt": "1", "consumer_source_sha": "c" * 40, "files": inventory(root),
                 "source_ref": "refs/tags/v1.2.3", "source_record_sha256": "a" * 64, "packages": packages,
                 "live_record": {"crate_version": "1.2.2"}}
        write_json(root / ".apt-stage.json", proof)
        return {"crate_version": "1.2.3", "source_record_sha256": "a" * 64}

    def live_entries(self):
        return {arch: [{"Version": "1.2.3", "SHA256": checksum}]
                for arch, checksum in (("amd64", "a" * 64), ("arm64", "b" * 64))}

    def test_equal_live_source_with_changed_package_refused(self):
        staged = self.staged()
        entries = self.live_entries()
        entries["arm64"][0]["SHA256"] = "c" * 64
        with patch("delivery_apt_stage.local_suite", return_value=staged):
            with patch("delivery_apt_stage.live_suite", return_value={"record": staged, "entries": entries}):
                with self.assertRaisesRegex(ValueError, "different candidate package bytes"):
                    guard(CONFIG)

    def test_candidate_proof_must_match_signed_local_indexes(self):
        staged = self.staged()
        with patch("delivery_apt_stage.local_suite", return_value=staged):
            with patch("delivery_apt_stage.candidate_hashes", return_value={"amd64": "a" * 64, "arm64": "c" * 64}):
                with self.assertRaisesRegex(ValueError, "proof differs from signed indexes"):
                    guard(CONFIG)

    def test_deploy_guard_refuses_newer_live(self):
        staged = self.staged()
        with patch("delivery_apt_stage.local_suite", return_value=staged):
            with patch("delivery_apt_stage.live_suite", return_value={"record": {"crate_version": "1.2.4"}}):
                with self.assertRaisesRegex(ValueError, "older than live"):
                    guard(CONFIG)

    def test_deploy_guard_refuses_intermediate_live_advancement(self):
        staged = self.staged()
        with patch("delivery_apt_stage.local_suite", return_value=staged):
            with patch("delivery_apt_stage.live_suite", return_value={"record": {"crate_version": "1.2.2", "new": "head"}}):
                with self.assertRaisesRegex(ValueError, "rollback head changed"):
                    guard(CONFIG)

    def test_deploy_guard_refuses_equal_version_source_collision(self):
        staged = self.staged()
        live = {"record": {"crate_version": "1.2.3", "source_record_sha256": "b" * 64}}
        with patch("delivery_apt_stage.local_suite", return_value=staged):
            with patch("delivery_apt_stage.live_suite", return_value=live):
                with self.assertRaisesRegex(ValueError, "different immutable source"):
                    guard(CONFIG)

    def test_deploy_guard_refuses_live_disappearance(self):
        staged = self.staged()
        with patch("delivery_apt_stage.local_suite", return_value=staged):
            with patch("delivery_apt_stage.live_suite", return_value=None):
                with self.assertRaisesRegex(ValueError, "disappeared"):
                    guard(CONFIG)

    def preview_bootstrap_guard(self, live=None):
        staged = self.staged()
        root = Path("public")
        (root / "last-publish").unlink()
        version = "1.2.3~preview.1+abcdef0"
        (root / "last-publish-preview").write_text(version + "\n")
        (root / "package-state.json").unlink()
        packages = [{"arch": arch, "name": f"example-preview-{version.replace('~', '.')}-{arch}.deb",
                     "sha256": checksum} for arch, checksum in (("amd64", "a" * 64), ("arm64", "b" * 64))]
        write_json(root / "package-state-preview.json", {"source_commit": "f" * 40, "source_ref": "refs/heads/main",
                   "packages": [{"name": item["name"], "sha256": item["sha256"]} for item in packages]})
        proof = {"schema": "velnor.apt-stage/v1", "config_sha256": config_digest(CONFIG), "suite": "preview",
                 "version": version, "debian_version": version, "commit": "f" * 40, "source_ref": "refs/heads/main",
                 "source_record_sha256": "a" * 64, "packages": packages, "workflow_run_id": "42",
                 "workflow_run_attempt": "1", "consumer_source_sha": "c" * 40, "files": inventory(root, (".apt-stage.json",)), "live_record": None}
        (root / ".apt-stage.json").write_text(json.dumps(proof))
        staged["crate_version"] = version
        with patch.dict(os.environ, {"INPUT_SUITE": "preview"}):
            with patch("delivery_apt_stage.local_suite", return_value=staged):
                with patch("delivery_apt_stage.live_suite", side_effect=[live, None]):
                    guard(CONFIG)

    def test_explicit_preview_bootstrap_allows_absent_live(self):
        self.preview_bootstrap_guard()

    def test_preview_bootstrap_refuses_lower_live_appearance(self):
        live = {"record": {"crate_version": "1.2.2~preview.1+abcdef0"}}
        with patch("delivery_apt_stage.version_order", return_value=1):
            with self.assertRaisesRegex(ValueError, "rollback head changed"):
                self.preview_bootstrap_guard(live)

    def test_preserved_other_suite_disappearance_refused(self):
        staged = self.staged()
        root = Path("public")
        write_json(root / "publication-record-preview.json", {"crate_version": "1.2.3~preview.1+abcdef0"})
        proof = json.loads((root / ".apt-stage.json").read_text())
        preserved = {"crate_version": "1.2.3~preview.1+abcdef0"}
        proof["other_record"] = preserved
        proof["files"] = inventory(root, (".apt-stage.json",))
        (root / ".apt-stage.json").write_text(json.dumps(proof))
        with patch("delivery_apt_stage.local_suite", side_effect=[staged, preserved]):
            with patch("delivery_apt_stage.live_suite", side_effect=[{"record": staged, "entries": self.live_entries()}, None]):
                with self.assertRaisesRegex(ValueError, "other suite disappeared"):
                    guard(CONFIG)

    def test_deploy_guard_accepts_unchanged_live(self):
        staged = self.staged()
        with patch("delivery_apt_stage.local_suite", return_value=staged):
            with patch("delivery_apt_stage.live_suite", side_effect=[{"record": staged, "entries": self.live_entries()}, None]):
                guard(CONFIG)

class RetentionTests(unittest.TestCase):
    def test_old_candidate_refused(self):
        marker = {"suite": "stable", "debian_version": "1.2.3"}
        live = {"record": {"crate_version": "1.2.4"}}
        with self.assertRaisesRegex(ValueError, "roll back"):
            rollback(CONFIG, marker, live)

    def test_stable_bootstrap_refused(self):
        with self.assertRaisesRegex(ValueError, "signed rollback pair"):
            rollback(CONFIG, {"suite": "stable", "debian_version": "1.2.3"}, None)

    def test_preview_absence_initializes_without_rollback(self):
        self.assertEqual(rollback(CONFIG, {"suite": "preview", "debian_version": "1.2.3~preview.1+abcdef0"}, None),
                         (None, None))

    def test_equal_version_source_collision_refused(self):
        marker = {"suite": "stable", "debian_version": "1.2.3", "source_record_sha256": "a" * 64}
        live = {"record": {"crate_version": "1.2.3", "source_record_sha256": "b" * 64}}
        with self.assertRaisesRegex(ValueError, "immutable source"):
            rollback(CONFIG, marker, live)

    def test_http_server_failure_cannot_bootstrap(self):
        import urllib.error
        failure = urllib.error.HTTPError(CONFIG["feed_url"], 503, "unavailable", None, None)
        with patch("urllib.request.OpenerDirector.open", side_effect=failure):
            with self.assertRaisesRegex(RuntimeError, "HTTP failure: 503"):
                fetch(CONFIG, "publication-record-preview.json", absent=True)

    def state_fixture(self, suite):
        version = "1.2.3~preview.5+abcdef0" if suite == "preview" else "1.2.3"
        files = {pool_path(CONFIG, suite, version, arch): arch.encode() for arch in ("amd64", "arm64")}
        entries = {arch: [{"Version": version, "Filename": pool_path(CONFIG, suite, version, arch),
                          "SHA256": hashlib.sha256(arch.encode()).hexdigest()}] for arch in ("amd64", "arm64")}
        state = {"schema": "velnor.apt-package-state.v1", "source_repository": CONFIG["source_repository"],
                 "source_ref": "refs/heads/main" if suite == "preview" else "refs/tags/v" + version,
                 "source_commit": "f" * 40, "version": version if suite == "preview" else "v" + version,
                 "packages": [{"name": (f"example-preview-{version.replace('~', '.')}-{arch}.deb" if suite == "preview"
                                         else f"example-{version}-{arch}.deb"),
                               "sha256": entries[arch][0]["SHA256"]} for arch in ("amd64", "arm64")]}
        payload = {"usr/share/example/build-identity.json": json.dumps({"source_sha": "f" * 40,
                   "crate_version": "1.2.3"}).encode(), "usr/bin/example": b"binary"}
        return version, files, entries, state, payload

    def test_preview_source_ref_independent_of_consumer_branch(self):
        version, files, entries, state, payload = self.state_fixture("preview")
        with patch("delivery_apt_stage_feed.deb_payload", return_value=payload):
            with patch("delivery_apt_stage_feed.elf_identity"):
                check_state(dict(CONFIG, branch="feed-main"), "preview", {"crate_version": version}, state, entries, files)

    def test_consistent_signed_package_identity_passes_both_suites(self):
        for suite in ("stable", "preview"):
            version, files, entries, state, payload = self.state_fixture(suite)
            with patch("delivery_apt_stage_feed.deb_payload", return_value=payload):
                with patch("delivery_apt_stage_feed.elf_identity") as elf:
                    check_state(CONFIG, suite, {"crate_version": version}, state, entries, files)
                    self.assertEqual(elf.call_count, 2)

    def test_unsigned_state_commit_corruption_refused_both_suites(self):
        for suite in ("stable", "preview"):
            version, files, entries, state, payload = self.state_fixture(suite)
            state["source_commit"] = "a" * 40
            with patch("delivery_apt_stage_feed.deb_payload", return_value=payload):
                with self.assertRaisesRegex(ValueError, "differs from signed package identity"):
                    check_state(CONFIG, suite, {"crate_version": version}, state, entries, files)

    def test_stable_numeric_order(self):
        self.assertEqual(version_order("1.10.0", "1.9.9", "stable"), 1)


class NativeIndexTests(unittest.TestCase):
    setUp = ArtifactFixture.setUp
    tearDown = ArtifactFixture.tearDown
    def test_native_indexes_and_release_hashes_cover_exact_pair(self):
        root = Path("public")
        version = "1.2.3"
        for arch in ("amd64", "arm64"):
            path = root / pool_path(CONFIG, "stable", version, arch)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(arch.encode())
        def control(argv):
            arch = "amd64" if "amd64" in argv[-1] else "arm64"
            return f"Package: example\nVersion: {version}\nArchitecture: {arch}\nDescription: fixture\n multiline\n"
        with patch("delivery_apt_stage_publish.run", side_effect=control):
            build_indexes(CONFIG, {"suite": "stable"}, {version}, root)
        release = release_document(CONFIG, "stable", root).decode()
        hashes = release_hashes(release, "stable", CONFIG)
        self.assertEqual(len(hashes), 4)
        for relative, checksum in hashes.items():
            self.assertEqual(digest(root / "dists/stable" / relative), checksum)
        for arch in ("amd64", "arm64"):
            index = root / f"dists/stable/main/binary-{arch}/Packages"
            entries = stanzas(index.read_text())
            self.assertEqual(len(entries), 1)
            self.assertEqual(entries[0]["Filename"], pool_path(CONFIG, "stable", version, arch))
            self.assertEqual(gzip.decompress(Path(str(index) + ".gz").read_bytes()), index.read_bytes())


class SignatureTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("gpg") and shutil.which("gpgv"), "GPG unavailable")
    def test_signing_subkey_resolves_pinned_primary(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            os.chmod(home, 0o700)
            base = ["gpg", "--batch", "--homedir", str(home), "--pinentry-mode", "loopback", "--passphrase", ""]
            subprocess.run(base + ["--quick-generate-key", "APT test <apt@example.test>", "ed25519", "cert", "0"],
                           check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            listing = subprocess.run(base + ["--with-colons", "--list-secret-keys"], check=True,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True).stdout
            fingerprint = next(line.split(":")[9] for line in listing.splitlines() if line.startswith("fpr:"))
            subprocess.run(base + ["--quick-add-key", fingerprint, "ed25519", "sign", "0"], check=True,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            keyring = home / "public.gpg"
            keyring.write_bytes(subprocess.run(base + ["--export", fingerprint], check=True,
                                               stdout=subprocess.PIPE).stdout)
            document = home / "record"
            document.write_bytes(b"authenticated record\n")
            signature = home / "record.sig"
            subprocess.run(base + ["--local-user", fingerprint, "--output", str(signature), "--detach-sign", str(document)],
                           check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            config = dict(CONFIG, keyring=str(keyring), signer_fingerprint=fingerprint)
            verify_signature(config, document.read_bytes(), signature.read_bytes())
            with self.assertRaises(subprocess.CalledProcessError):
                verify_signature(config, b"corrupted", signature.read_bytes())
            subprocess.run(["gpgconf", "--homedir", str(home), "--kill", "gpg-agent"], check=True)


if __name__ == "__main__":
    unittest.main()
