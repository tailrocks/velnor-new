"""Fixture-only behavioral evidence policy tests; no signing or native execution."""

import copy
import json
from pathlib import Path
import tempfile
import unittest

import owned_tool_qualification_evidence as Q
from owned_tool_execution_test_fixtures import P, fixture, environment


class EvidenceTests(unittest.TestCase):
    def records(self):
        manifest, files = fixture()
        artifact = manifest["artifacts"][0]
        target = artifact["target"]
        receipt = json.loads(files["qualified-receipt-" + target + ".json"])
        report = json.loads(files["native-report-" + target + ".json"])
        return manifest, artifact, receipt, report

    def test_exact_measured_record_and_durable_assets(self):
        manifest, files = fixture()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            output = root / "snapshot"
            output.mkdir()
            for name, data in files.items():
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_bytes(data)
            Q.stage_qualified_evidence(output, manifest, root, P.read_regular, environment(manifest))
            self.assertEqual(len(list(output.iterdir())), 15)
            for path in output.iterdir():
                key = path.name
                if key.startswith("execution-"):
                    key = next(name for name in files if name.replace("/", "-") == key)
                self.assertEqual(path.read_bytes(), files[key])

    def test_claim_requires_real_supported_abi_and_positive_closed_evidence(self):
        manifest, artifact, _, _ = self.records()
        mutations = [lambda c: c.update(passed=1), lambda c: c.update(abi=None),
            lambda c: c.update(abi="unapproved"), lambda c: c.update(cases=0),
            lambda c: c.update(cases=56), lambda c: c.update(cases=True),
            lambda c: c.update(sourceartifact_id=True), lambda c: c.update(sourceartifact_id=0),
            lambda c: c.update(sourceartifact_api_digest="sha256:" + "0" * 64),
            lambda c: c.update(sourceartifact_api_digest="https://attacker"),
            lambda c: c.update(qualified_receipt_sha256="0" * 64),
            lambda c: c.update(arbitrary=True)]
        for mutation in mutations:
            claim = copy.deepcopy(artifact["qualification"])
            mutation(claim)
            with self.subTest(claim=claim), self.assertRaises(ValueError):
                Q.validate_claim(claim, manifest["tool"])
        with self.assertRaises(ValueError):
            P.validate_manifest(fixture("mbx")[0])

    def test_qualified_receipt_binds_source_binary_workflow_admission(self):
        manifest, artifact, receipt, report = self.records()
        mutations = [lambda r: r.update(schema=True), lambda r: r.update(extra=True),
            lambda r: r["workflow"].update(run_attempt="2"),
            lambda r: r["source"].update(commit="f" * 40),
            lambda r: r["artifact"].update(binary_sha256="f" * 64),
            lambda r: r.update(version_banner="wrong owned version"),
            lambda r: r["compiler"].update(rustc_vv="release: 1.98.1\nhost: wrong"),
            lambda r: r["behavioral_qualification"].update(passed=False),
            lambda r: r["behavioral_qualification"].update(candidate_receipt_sha256="f" * 64),
            lambda r: r["behavioral_qualification"]["artifact_admission"].update(artifact_id=True),
            lambda r: r["behavioral_qualification"]["artifact_admission"].update(api_digest="sha256:" + "e" * 64),
            lambda r: r["behavioral_qualification"]["artifact_admission"].update(artifact_name="old-run"),
            lambda r: r["behavioral_qualification"].update(artifact_admission_sha256="f" * 64)]
        for mutation in mutations:
            candidate = copy.deepcopy(receipt)
            mutation(candidate)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                Q.validate_qualified(candidate, report, manifest, artifact, environment(manifest))

    def test_report_exact_cases_and_measured_identity(self):
        _, artifact, receipt, report = self.records()
        mutations = [lambda r: r["results"].pop(), lambda r: r["results"].reverse(),
            lambda r: r["results"][0].update(passed=1),
            lambda r: r["results"][0].update(case=r["results"][1]["case"]),
            lambda r: r.update(source_diff_sha256="e" * 64),
            lambda r: r.update(upstream_commit="f" * 40),
            lambda r: r.update(binary_sha256="f" * 64),
            lambda r: r.update(version_is_distinct=1),
            lambda r: r["host"].update(machine="arm64"), lambda r: r.update(extra=True)]
        for mutation in mutations:
            candidate = copy.deepcopy(report)
            mutation(candidate)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                Q.validate_report(candidate, receipt, artifact["qualification"])

    def test_raw_qualified_and_report_hashes_checked(self):
        for prefix in ("qualified-receipt-", "native-report-"):
            manifest, files = fixture()
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve()
                output = root / "snapshot"
                output.mkdir()
                for name, data in files.items():
                    (root / name).parent.mkdir(parents=True, exist_ok=True)
                    (root / name).write_bytes(data)
                name = prefix + manifest["artifacts"][0]["target"] + ".json"
                (root / name).write_bytes(b"tampered")
                with self.assertRaisesRegex(ValueError, "evidence digest"):
                    Q.stage_qualified_evidence(output, manifest, root, P.read_regular, environment(manifest))


if __name__ == "__main__":
    unittest.main()
