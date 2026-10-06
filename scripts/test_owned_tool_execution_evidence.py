"""Unsigned execution-origin fixtures; publication permission stays independent."""

import copy
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import owned_tool_qualification_evidence as Q
import source_qualification_execution as E
from owned_tool_execution_test_fixtures import P, environment, fixture


class ExecutionEvidenceTests(unittest.TestCase):
    def stage(self, manifest, files, route="main"):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            output = root / "snapshot"
            output.mkdir()
            for name, data in files.items():
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_bytes(data)
            Q.stage_qualified_evidence(output, manifest, root, P.read_regular, environment(manifest, route))
            return {path.name: path.read_bytes() for path in output.iterdir()}

    def test_each_host_preserves_exact_three_raw_api_documents(self):
        manifest, files = fixture()
        output = self.stage(manifest, files)
        self.assertEqual(len(output), 15)
        digests = []
        for artifact in manifest["artifacts"]:
            target = artifact["target"]
            for name in E.API_EVIDENCE_FILES.values():
                self.assertEqual(output["execution-" + target + "-" + name],
                                 files["execution-" + target + "/" + name])
            digests.append(artifact["qualification"]["sourceartifact_execution_sha256"])
        self.assertEqual(len(set(digests)), 3)

    def test_branch_origin_is_admitted_but_cannot_authorize_main_publisher(self):
        manifest, files = fixture(route="candidate")
        self.stage(manifest, files, "candidate")
        with patch.dict(os.environ, environment(manifest, "candidate")), patch.object(P, "gh") as command:
            with self.assertRaises(ValueError):
                P.trusted_dispatch(manifest)
            command.assert_not_called()

    def test_execution_receipt_cannot_be_dropped_or_relabelled(self):
        manifest, files = fixture()
        artifact = manifest["artifacts"][0]
        receipt = json.loads(files["qualified-receipt-" + artifact["target"] + ".json"])
        report = json.loads(files["native-report-" + artifact["target"] + ".json"])
        mutations = [lambda r: r["behavioral_qualification"]["artifact_admission"].pop("execution"),
            lambda r: r["behavioral_qualification"]["artifact_admission"]["execution"].update(run_attempt="2"),
            lambda r: r["behavioral_qualification"]["artifact_admission"]["execution"].update(repository="attacker/repo"),
            lambda r: r["behavioral_qualification"]["artifact_admission"]["execution"]["workflow"].update(id=True),
            lambda r: r["behavioral_qualification"]["artifact_admission"]["execution"].update(extra=True),
            lambda r: r["behavioral_qualification"]["execution_evidence"].update(directory="../escape"),
            lambda r: r["behavioral_qualification"]["execution_evidence"]["api_sha256"].update(run="f" * 64)]
        for mutation in mutations:
            candidate = copy.deepcopy(receipt)
            mutation(candidate)
            with self.assertRaises(ValueError):
                Q.validate_qualified(candidate, report, manifest, artifact, environment(manifest))
        claim = copy.deepcopy(artifact["qualification"])
        claim.pop("sourceartifact_execution_sha256")
        with self.assertRaises(ValueError):
            Q.validate_claim(claim, manifest["tool"])

    def test_raw_api_bytes_missing_mutated_or_foreign_fail_replay(self):
        manifest, files = fixture()
        target = manifest["artifacts"][0]["target"]
        for name in E.API_EVIDENCE_FILES.values():
            candidate = dict(files)
            path = "execution-" + target + "/" + name
            candidate[path] += b" "
            with self.assertRaises(ValueError):
                self.stage(manifest, candidate)
        candidate = dict(files)
        candidate.pop("execution-" + target + "/" + E.API_EVIDENCE_FILES["run"])
        with self.assertRaises(OSError):
            self.stage(manifest, candidate)

    def test_main_environment_is_independent_of_manifest_execution(self):
        manifest, files = fixture()
        wrong = environment(manifest)
        wrong["GITHUB_WORKFLOW_SHA"] = "f" * 40
        artifact = manifest["artifacts"][0]
        receipt = json.loads(files["qualified-receipt-" + artifact["target"] + ".json"])
        report = json.loads(files["native-report-" + artifact["target"] + ".json"])
        with self.assertRaises(ValueError):
            Q.validate_qualified(receipt, report, manifest, artifact, wrong)


if __name__ == "__main__":
    unittest.main()
