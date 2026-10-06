"""Mock final reconciliation without Cargo, Git, or remote calls."""

import copy
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1] / "src"
NAMESPACE = {"__name__": "release_reconcile_entry_test"}
exec(compile((ROOT / "release_reconcile_common.py").read_text(),
             "release_reconcile_common.py", "exec"), NAMESPACE)
exec(compile((ROOT / "release_reconcile_entry.py").read_text(),
             "release_reconcile_entry.py", "exec"), NAMESPACE)
ReconcileError = NAMESPACE["ReconcileError"]

SOURCE = "a" * 40
ENVIRONMENT = {"GITHUB_SHA": SOURCE, "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2"}
POLICY = {
    "schema": 1, "repository": "owner/repo", "registry": "crates-io",
    "source_sha": SOURCE, "packages": {"demo": "1.0.0"},
    "owners": {"demo": ["user:1"]}, "tags": {"demo": "demo-v1.0.0"},
    "authentication": "trusted-publishing",
    "tools": {key: "1.0.0" for key in ("generator", "release-plz", "rust", "python", "gh")},
    "intent_id": "intent-1",
}
ARTIFACT = {"id": 11, "digest": "sha256:" + "1" * 64,
            "name": "unused", "producer_job": {"id": 17, "name": "unused",
                                                    "conclusion": "success"}}


def candidate(policy=POLICY, packages=None, order=None):
    return {
        "schema": 1, "policy": policy,
        "packages": packages or {"demo": {"forge_release": {
            "tag_name": "demo-v1.0.0", "body": "Release notes.",
            "name": "demo-v1.0.0", "draft": False, "prerelease": False,
        }}},
        "publication_order": order or ["demo"],
        "workflow_sha": SOURCE, "run_id": "123", "run_attempt": "2",
        "status": "package-verified",
    }


def terminal(policy=POLICY, order=None, status="incomplete"):
    return {
        "schema": 1, "policy": policy, "status": status,
        "workflow_sha": SOURCE, "run_id": "123", "run_attempt": "2",
        "publication_order": order or ["demo"],
        "operations": {name: {"version": version, "status": "pending"}
                        for name, version in policy["packages"].items()},
    }


class ReconcileEntryTests(unittest.TestCase):
    def bindings(self, candidate_value=None, preflight=None, receipts=None):
        value = candidate_value or candidate()
        proofs = receipts or (terminal(), terminal())
        package_artifact = {**ARTIFACT, "name": "velnor-release-package-r123-a2",
                            "producer_job": {"id": 17, "name": "release-package",
                                             "conclusion": "success"}}
        loaded = (value, {}, package_artifact, b"raw-package-zip")
        calls = []

        def save(receipt, _path="release-receipt/receipt.json"):
            calls.append(copy.deepcopy(receipt))
            destination = Path(_path)
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(json.dumps(receipt, sort_keys=True))

        def artifact_binding(producer):
            return 11, "sha256:" + "1" * 64

        def validate_artifact(*_args):
            return _args[0]

        def verify_registry(*args):
            calls.append(("registry", args[1]))
            return {"status": "verified", "name": args[1]}

        def verify_forge(*args):
            calls.append(("forge", args[1]))
            return {"tag": args[1], "release": "verified"}

        values = {
            "policy": lambda: value["policy"],
            "load_package_input": lambda _approved: loaded,
            "load_publish_receipt": lambda _approved, kind: (proofs[0] if kind == "registry" else proofs[1],
                                                               {**ARTIFACT, "name": f"velnor-release-{kind}-r123-a2",
                                                                "producer_job": {"id": 18, "name": f"release-{kind}-publish",
                                                                                 "conclusion": "success"}}),
            "artifact_evidence": lambda *_args: (preflight, {**ARTIFACT, "name": "velnor-release-preflight-r123-a2",
                                                               "producer_job": {"id": 19, "name": "release-preflight",
                                                                                "conclusion": "success"}}),
            "_artifact_upload_binding": artifact_binding,
            "validate_artifact_identity": validate_artifact,
            "validate_package_shape": lambda _package: None,
            "validate_registry_operation": lambda *_args: None,
            "verify_published_package": verify_registry,
            "verify_forge_package": verify_forge,
            "save_receipt": save,
        }
        return values, calls

    def execute(self, values):
        with tempfile.TemporaryDirectory() as directory, patch.dict(
                os.environ, ENVIRONMENT, clear=True), patch.dict(NAMESPACE, values):
            previous = Path.cwd()
            os.chdir(directory)
            try:
                NAMESPACE["reconcile"]()
                return json.loads(Path("release-receipt/receipt.json").read_text())
            finally:
                os.chdir(previous)

    def test_success_binds_package_and_terminal_artifacts(self):
        values, calls = self.bindings()
        receipt = self.execute(values)
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(receipt["operations"]["demo"]["status"], "verified")
        self.assertEqual(receipt["package_artifact"]["name"], "velnor-release-package-r123-a2")
        self.assertEqual(receipt["registry_artifact"]["name"], "velnor-release-registry-r123-a2")
        self.assertEqual(receipt["forge_artifact"]["name"], "velnor-release-forge-r123-a2")
        self.assertEqual(receipt["preflight_diagnostic"], "artifact-not-produced")
        self.assertNotIn("preflight_artifact", receipt)
        self.assertEqual([item for item in calls if isinstance(item, tuple)],
                         [("registry", "demo"), ("forge", "demo")])

    def test_preflight_is_checked_against_immutable_candidate(self):
        values, _calls = self.bindings(preflight={
            "schema": 1, "policy": POLICY, "packages": {"demo": {}},
            "publication_order": ["demo"], "workflow_sha": SOURCE,
            "run_id": "123", "run_attempt": "2", "status": "publication-incomplete",
            "operations": {"demo": {"version": "1.0.0", "status": "pending"}},
        })
        values["_artifact_upload_binding"] = lambda producer: (
            11, "sha256:" + "1" * 64)
        with patch.dict(os.environ, {**ENVIRONMENT,
                                     "RELEASE_PREFLIGHT_ARTIFACT_ID": "19",
                                     "RELEASE_PREFLIGHT_ARTIFACT_DIGEST": "1" * 64}, clear=True), \
                patch.dict(NAMESPACE, values), self.assertRaisesRegex(
                    ReconcileError, "reconcile_preflight_authority"):
            NAMESPACE["reconcile"]()

    def test_nested_policy_boolean_does_not_equal_integer(self):
        value = copy.deepcopy(candidate())
        value["policy"]["tools"]["generator"] = True
        values, _calls = self.bindings(value)
        values["policy"] = lambda: POLICY
        with self.assertRaisesRegex(ReconcileError,
                                    "reconcile_package_candidate"):
            self.execute(values)

    def test_failed_terminal_producer_cannot_claim_verified(self):
        values, _calls = self.bindings()
        verified = terminal(status="verified")
        verified["operations"]["demo"]["status"] = "verified"
        original_loader = values["load_publish_receipt"]

        def failed_registry_loader(approved, kind):
            receipt, artifact = original_loader(approved, kind)
            if kind == "registry":
                artifact = {**artifact, "producer_job": {
                    **artifact["producer_job"], "conclusion": "failure"}}
                return verified, artifact
            return receipt, artifact

        values["load_publish_receipt"] = failed_registry_loader
        with self.assertRaisesRegex(ReconcileError,
                                    "registry_failed_producer_receipt"):
            self.execute(values)

    def test_later_failure_keeps_earlier_success_and_processes_all_packages(self):
        packages = {
            **candidate()["packages"],
            "second": copy.deepcopy(candidate()["packages"]["demo"]),
        }
        approved = copy.deepcopy(POLICY)
        approved["packages"] = {"demo": "1.0.0", "second": "2.0.0"}
        approved["owners"]["second"] = ["user:1"]
        approved["tags"]["second"] = "second-v2.0.0"
        value = candidate(approved, packages, ["demo", "second"])
        values, calls = self.bindings(value, receipts=(terminal(approved, ["demo", "second"]),
                                                        terminal(approved, ["demo", "second"])))
        original = values["verify_forge_package"]

        def forge(approved_value, name, descriptor):
            if name == "second":
                calls.append(("forge", name))
                raise ReconcileError("forge_collision")
            return original(approved_value, name, descriptor)

        values["verify_forge_package"] = forge
        with self.assertRaisesRegex(ReconcileError, "partial_or_failed_reconciliation"):
            self.execute(values)
        saved = [item for item in calls if isinstance(item, dict) and "operations" in item]
        final = saved[-1]
        self.assertEqual(final["operations"]["demo"]["status"], "verified")
        self.assertEqual(final["operations"]["second"]["status"], "failed")
        self.assertEqual([item for item in calls if isinstance(item, tuple)], [
            ("registry", "demo"), ("forge", "demo"),
            ("registry", "second"), ("forge", "second"),
        ])


if __name__ == "__main__":
    unittest.main()
