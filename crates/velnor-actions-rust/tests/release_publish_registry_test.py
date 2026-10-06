"""Immutable artifact, publication visibility, and durable recovery proofs."""
from contextlib import contextmanager
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

FIXTURE = {"__file__": str(Path(__file__).with_name("release_publish_test_fixture.py"))}
exec(compile(Path(FIXTURE["__file__"]).read_text(), "release_publish_test_fixture.py", "exec"), FIXTURE)
NS, POLICY = FIXTURE["NS"], FIXTURE["POLICY"]
ReconcileError = FIXTURE["ReconcileError"]


class ArtifactTests(unittest.TestCase):
    def setUp(self):
        self.proof, self.data = FIXTURE["package"]()

    def validate(self, blob):
        with patch.dict(NS["os"].environ, FIXTURE["ENVIRONMENT"]):
            return NS["validate_package_artifact"](blob, POLICY)

    def test_exact_artifact_returns_same_immutable_archive_bytes(self):
        blob = FIXTURE["artifact"]({"demo": self.proof}, {"demo": self.data})
        candidate, archives = self.validate(blob)
        self.assertEqual(archives["demo"], self.data)
        self.assertEqual(candidate["publication_order"], ["demo"])

    def test_archive_tamper_extra_members_run_scope_and_order_fail(self):
        cases = [FIXTURE["artifact"]({"demo": self.proof}, {"demo": self.data + b"tamper"}),
                 FIXTURE["artifact"]({"demo": self.proof}, {"demo": self.data},
                                     extra=("../hostile.py", "raise Exception()")),
                 FIXTURE["artifact"]({"demo": self.proof}, {"demo": self.data},
                                     overrides={"run_attempt": "1"}),
                 FIXTURE["artifact"]({"demo": self.proof}, {"demo": self.data},
                                     overrides={"publication_order": []})]
        for blob in cases:
            with self.subTest(size=len(blob)), self.assertRaises(ReconcileError):
                self.validate(blob)

    def test_boolean_top_schema_and_nested_policy_schema_fail_closed(self):
        blob = FIXTURE["artifact"]({"demo": self.proof}, {"demo": self.data},
                                  overrides={"schema": True})
        with self.assertRaisesRegex(ReconcileError, "package_artifact_fields"):
            self.validate(blob)
        approved = {"schema": 1, **POLICY}
        blob = FIXTURE["artifact"]({"demo": self.proof}, {"demo": self.data}, approved=approved,
                                  overrides={"policy": {**approved, "schema": True}})
        with patch.dict(NS["os"].environ, FIXTURE["ENVIRONMENT"]), \
             self.assertRaisesRegex(ReconcileError, "package_artifact_fields"):
            NS["validate_package_artifact"](blob, approved)

    def test_forged_selected_dependency_order_rejected_before_credentials(self):
        proof = copy.deepcopy(self.proof)
        proof["dependencies"] = ["demo"]
        with self.assertRaisesRegex(ReconcileError, "publish_dependency_order_proof"):
            NS["validate_registry_inputs"](POLICY, {"demo": proof}, {"demo": self.data})

    def test_valid_shape_metadata_substitution_fails_archive_rederivation(self):
        proof = copy.deepcopy(self.proof)
        proof["publish_metadata"]["description"] = "substituted description"
        with self.assertRaisesRegex(ReconcileError, "publish_manifest_field"):
            NS["validate_registry_inputs"](POLICY, {"demo": proof}, {"demo": self.data})


class PublishTests(unittest.TestCase):
    def setUp(self):
        self.proof, self.data = FIXTURE["package"]()
        self.saved = []

    def save(self, receipt, path):
        self.assertEqual(path, "release-registry/receipt.json")
        self.saved.append(copy.deepcopy(receipt))

    def publish(self, patches):
        with patch.dict(NS["os"].environ, FIXTURE["ENVIRONMENT"]), \
             patch.dict(NS, {"save_receipt": self.save, **patches}):
            return NS["publish_registry"](POLICY, {"demo": self.proof}, {"demo": self.data})

    def test_existing_matching_version_recovered_without_credential(self):
        def forbidden(*args):
            self.fail("credential requested for existing immutable version")
        receipt = self.publish({"fetch": lambda *args: b"existing",
                "registry_token": forbidden,
                "verify_published_package": lambda *args: FIXTURE["registry_proof"](self.proof, "b" * 64)})
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(receipt["operations"]["demo"]["relation"], "existing-normalized")

    def test_uncertain_upload_requires_full_proof_and_retains_intent(self):
        @contextmanager
        def token(*args):
            yield "fixed-token"
        def uncertain(*args):
            raise ReconcileError("publish_transport_uncertain")
        receipt = self.publish({"fetch": lambda *args: None, "_check_missing_version": lambda *args: None,
                "registry_token": token, "upload_package": uncertain,
                "verify_published_package": lambda *args: FIXTURE["registry_proof"](self.proof)})
        self.assertEqual(receipt["status"], "verified")
        states = [saved["operations"]["demo"]["status"] for saved in self.saved]
        self.assertIn("uploading", states)
        self.assertIn("upload-uncertain", states)

    def test_absent_crate_bootstrap_has_no_provisioned_principal(self):
        with patch.dict(NS, {"fetch": lambda *args: None}):
            with self.assertRaisesRegex(ReconcileError, "publish_bootstrap_principal_unproven"):
                NS["_check_missing_version"](POLICY, "demo")

    def test_disconnect_after_commit_uses_full_registry_proof(self):
        @contextmanager
        def token(*args):
            yield "fixed-token"
        def disconnected(*args, **kwargs):
            raise NS["http"].client.RemoteDisconnected("server committed then disconnected")
        opener = type("Opener", (), {"open": disconnected})()
        verified = []
        def proof(*args):
            verified.append(1)
            return FIXTURE["registry_proof"](self.proof)
        with patch.object(NS["urllib"].request, "build_opener", return_value=opener):
            receipt = self.publish({"fetch": lambda *args: None, "_check_missing_version": lambda *args: None,
                    "registry_token": token, "verify_published_package": proof})
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(verified, [1])
        self.assertEqual(receipt["operations"]["demo"]["relation"], "submitted-exact")

    def test_attempted_upload_requires_exact_compressed_checksum(self):
        @contextmanager
        def token(*args):
            yield "fixed-token"
        receipt = self.publish({"fetch": lambda *args: None, "_check_missing_version": lambda *args: None,
                "registry_token": token, "upload_package": lambda *args: None,
                "verify_published_package": lambda *args: {"status": "verified", "registry_checksum": "b" * 64}})
        self.assertEqual(receipt["status"], "incomplete")
        self.assertEqual(receipt["operations"]["demo"]["status"], "failed")
        self.assertNotIn("relation", receipt["operations"]["demo"])

    def test_revoke_failure_retains_submitted_and_fails_receipt(self):
        @contextmanager
        def token(*args):
            yield "fixed-token"
            raise ReconcileError("publish_auth_http:503")
        receipt = self.publish({"fetch": lambda *args: None, "_check_missing_version": lambda *args: None,
                "registry_token": token, "upload_package": lambda *args: None})
        self.assertEqual(receipt["status"], "incomplete")
        self.assertEqual(receipt["operations"]["demo"]["status"], "failed")
        self.assertIn("submitted", [value["operations"]["demo"]["status"] for value in self.saved])


class VisibilityTests(unittest.TestCase):
    def test_real_registry_path_observes_stale_200_index_then_new_version(self):
        expected, archive = FIXTURE["package"]()
        checksum = expected["archive_sha256"]
        observations = []
        index = {"name": "demo", "vers": "1.0.0", "cksum": checksum,
                 "features": {}, "deps": [], "yanked": False}
        def fetch(url, *args):
            if url.endswith("owner_user"):
                return b'{"users":[{"id":1}]}'
            if url.endswith("owner_team"):
                return b'{"teams":[]}'
            if url.startswith("https://index.crates.io/"):
                observations.append(1)
                return b'' if len(observations) == 1 else json.dumps(index).encode()
            if url.startswith("https://static.crates.io/"):
                return archive
            if url.endswith("dependencies"):
                return b'{"dependencies":[]}'
            return json.dumps({"version": {"crate": "demo", "num": "1.0.0",
                "checksum": checksum, "features": {}, "yanked": False}}).encode()
        with patch.dict(NS, {"fetch": fetch}), patch.object(NS["time"], "sleep") as delay:
            result = NS["verify_published_package"](POLICY, "demo", "1.0.0", expected)
        self.assertEqual(result["registry_checksum"], checksum)
        self.assertEqual(len(observations), 3)
        delay.assert_called_once_with(1)

    def test_actual_index_and_api_dependency_proofs_preserve_rename(self):
        dependency = {"name": "upstream", "version_req": "^1.0", "features": [],
                      "optional": False, "default_features": True, "target": None,
                      "kind": "normal", "explicit_name_in_toml": "alias"}
        index = {"vers": "1.0.0", "deps": [{"name": "alias", "package": "upstream",
                 "req": "^1.0", "features": [], "optional": False,
                 "default_features": True, "target": None, "kind": "normal"}]}
        api = {"dependencies": [{"crate_id": "upstream", "req": "^1.0", "features": [],
                "optional": False, "default_features": True, "target": None, "kind": "normal"}]}
        def fetch(url, *args):
            return json.dumps(index if url.startswith("https://index") else api).encode()
        patches = {"registry_package": lambda *args: {"status": "verified"}, "fetch": fetch}
        expected = {"publish_metadata": {"deps": [dependency]}}
        with patch.dict(NS, patches):
            result = NS["_verify_published_once"](POLICY, "demo", "1.0.0", expected)
            self.assertEqual(result["status"], "verified")
            index["deps"][0]["name"] = "wrong_alias"
            with self.assertRaisesRegex(ReconcileError, "publish_index_dependency_mismatch"):
                NS["_verify_published_once"](POLICY, "demo", "1.0.0", expected)
            index["deps"][0]["name"] = "alias"
            api["dependencies"][0]["optional"] = 0
            with self.assertRaisesRegex(ReconcileError, "publish_dependency_types"):
                NS["_verify_published_once"](POLICY, "demo", "1.0.0", expected)

    def test_stale_index_200_retried_then_visible(self):
        attempts = []
        def observe(*args):
            attempts.append(1)
            if len(attempts) == 1:
                raise ReconcileError("index_version_missing")
            return {"status": "verified"}
        with patch.dict(NS, {"_verify_published_once": observe}), patch.object(NS["time"], "sleep"):
            result = NS["verify_published_package"](POLICY, "demo", "1.0.0", {})
        self.assertEqual(len(attempts), 2)
        self.assertEqual(result["status"], "verified")

    def test_exhaustion_bounded_owner_mismatch_immediate(self):
        for reason, expected in (("index_version_missing", 5), ("ownership_mismatch", 1)):
            calls = []
            def observe(*args):
                calls.append(1)
                raise ReconcileError(reason)
            with patch.dict(NS, {"_verify_published_once": observe}), \
                 patch.object(NS["time"], "sleep"), self.assertRaises(ReconcileError):
                NS["verify_published_package"](POLICY, "demo", "1.0.0", {})
            self.assertEqual(len(calls), expected)


if __name__ == "__main__":
    unittest.main()
