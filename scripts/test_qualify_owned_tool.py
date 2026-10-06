"""Synthetic native containers test real builder-to-qualifier admission; no builds."""

import hashlib
import importlib.util
import json
import os
import platform
from pathlib import Path
import struct
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from owned_tool_behavior import MISE_CASES
from owned_tool_source import BASES, HOSTS
from source_qualification_execution import API_EVIDENCE_FILES, PREFIX, WORKFLOW_PATH, admit_execution


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


QUALIFIER = load("owned_qualifier_test", "qualify-owned-tool.py")
BUILDER = QUALIFIER.build_helpers()


def sha(data):
    return hashlib.sha256(data).hexdigest()


def native_header(target):
    data = bytearray(64)
    if target == "aarch64-apple-darwin":
        data[:4] = b"\xcf\xfa\xed\xfe"
        struct.pack_into("<I", data, 4, 0x100000c)
        struct.pack_into("<I", data, 12, 2)
    else:
        data[:7] = b"\x7fELF\x02\x01\x01"
        struct.pack_into("<H", data, 16, 2)
        struct.pack_into("<H", data, 18, 62 if target.startswith("x86_64") else 183)
    return bytes(data)


class QualifierAdmissionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="owned-admission-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.target = QUALIFIER.native_target()
        self.env = patch.dict(os.environ, GITHUB_SHA="3" * 40, GITHUB_RUN_ID="11",
                              GITHUB_RUN_ATTEMPT="2", OWNED_TOOL_TARGET=self.target,
                              GITHUB_REF="refs/heads/main", GITHUB_EVENT_NAME="workflow_dispatch",
                              GITHUB_WORKFLOW_SHA="3" * 40,
                              GITHUB_WORKFLOW_REF="tailrocks/velnor-new/" + WORKFLOW_PATH + "@refs/heads/main",
                              ImageOS="fixture-native-image", ImageVersion="fixture-v1")
        self.env.start()
        self.addCleanup(self.env.stop)
        self.source = self.source_descriptor()
        self.source_receipt = self.staged_receipt()
        self.source["receipt_sha256"] = sha(self.source_receipt)
        os.environ["OWNED_TOOL_SOURCE_JSON"] = json.dumps(self.source)
        build = self.root / "build"
        build.mkdir()
        bootstrap = {tool: self.root / ("verified-bootstrap-" + tool) for tool in ("mise", "mbx")}
        for binary in bootstrap.values():
            binary.write_bytes(native_header(self.target))
        self.build_commands = []
        with patch.object(BUILDER, "admit", self.acquire_fixture), \
                patch.object(BUILDER, "run", self.build_execution_fixture):
            name, archive, receipt, evidence = BUILDER.candidate(
                self.source, bootstrap, build, BUILDER.workflow_identity())
        self.directory = self.root / "candidate"
        self.directory.mkdir()
        (self.directory / name).write_bytes(archive)
        (self.directory / "source-receipt.json").write_bytes(evidence)
        self.receipt = receipt
        self.arguments = SimpleNamespace(tool="mise", target=self.target,
            candidate_directory=self.directory, receipt=self.root / "qualified.json",
            report=self.root / "native-report.json")
        self.prepare_execution()
        self.save_candidate()

    def prepare_execution(self):
        repository = {"id": 99, "full_name": "tailrocks/velnor-new", "default_branch": "main"}
        run = {"id": 11, "run_attempt": 2, "head_sha": "3" * 40,
            "event": "workflow_dispatch", "head_branch": "main", "repository": repository,
            "head_repository": repository, "path": WORKFLOW_PATH, "workflow_id": 55}
        workflow = {"id": 55, "path": WORKFLOW_PATH}
        self.api_documents = {"repository": json.dumps(repository).encode(),
                              "run": json.dumps(run).encode(), "workflow": json.dumps(workflow).encode()}
        endpoints = {PREFIX: "repository", PREFIX + "/actions/runs/11": "run",
                     PREFIX + "/actions/workflows/55": "workflow"}
        self.execution = admit_execution(lambda endpoint: self.api_documents[endpoints[endpoint]])
        for key, filename in API_EVIDENCE_FILES.items():
            (self.directory / filename).write_bytes(self.api_documents[key])

    def source_descriptor(self):
        directory = "https://github.com/tailrocks/velnor-new/releases/download/fixture-source"
        return {"tool": "mise", "version": "2026.10.0-owned-cargo-wrapper",
            "source_commit": "1" * 40, "source_tree": "2" * 40,
            "upstream_base_commit": BASES["mise"][1], "archive_url": directory + "/source.tar",
            "archive_sha256": "4" * 64, "receipt_url": directory + "/source-receipt.json",
            "receipt_sha256": "5" * 64, "patch_url": directory + "/base.patch",
            "patch_sha256": "6" * 64, "lockfile_sha256": "7" * 64,
            "license_files": {"LICENSE": sha(b"fixture license\n")}}

    def staged_receipt(self):
        source = self.source
        value = {"schema": 1, "status": "STAGED_SOURCE_ONLY", "tool": "mise",
            "upstream_repository": "https://github.com/jdx/mise",
            "upstream_base_commit": source["upstream_base_commit"],
            "source_commit": source["source_commit"], "source_tree": source["source_tree"],
            "source_archive": {"name": "source.tar", "sha256": source["archive_sha256"]},
            "base_patch": {"name": "base.patch", "sha256": source["patch_sha256"]},
            "lockfile": {"path": "Cargo.lock", "sha256": source["lockfile_sha256"]},
            "license_files": source["license_files"], "required_hosts": HOSTS,
            "publication": None, "behavioral_qualification": None,
            "signed_build_provenance": None}
        return json.dumps(value, sort_keys=True).encode()

    def acquire_fixture(self, spec, source, environment):
        self.assertEqual(spec, self.source)
        source.mkdir()
        (source / "LICENSE").write_bytes(b"fixture license\n")
        binary = source.parent / "target/release/mise"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(native_header(self.target))
        return self.source_receipt

    def build_execution_fixture(self, argv, source, environment):
        self.build_commands.append(argv)
        if argv[-2:] == ["rustc", "-vV"]:
            return f"rustc 1.98.1\nrelease: 1.98.1\nhost: {self.target}"
        if argv[-2:] == ["cc", "--version"]:
            return "fixture linker"
        if argv[-1] == "--version":
            return self.source["version"] + " fixture-native (fixture)"
        return ""

    def save_candidate(self):
        raw = (json.dumps(self.receipt, sort_keys=True, indent=2) + "\n").encode()
        (self.directory / "candidate-receipt.json").write_bytes(raw)
        self.admission = {"schema": 1, "status": "SAME_RUN_ARTIFACT_ADMITTED",
            "artifact_id": 77, "artifact_name": f"owned-candidate-11-2-mise-{self.target}",
            "api_digest": "sha256:" + "8" * 64,
            "workflow": {"commit": "3" * 40, "run_id": "11", "run_attempt": "2"},
            "execution": self.execution,
            "tool": "mise", "target": self.target, "candidate_receipt_sha256": sha(raw),
            "binary_container": self.receipt["artifact"], "behavioral_qualification": None}
        self.save_admission()

    def save_admission(self):
        (self.directory / "artifact-admission.json").write_text(
            json.dumps(self.admission, sort_keys=True, indent=2) + "\n")

    def admit(self):
        return QUALIFIER.admit(self.arguments, QUALIFIER.archive_helpers())

    def test_real_builder_receipt_admits_with_nested_source_digest(self):
        receipt, archive, digest, admission, admission_digest, api_documents = self.admit()
        self.assertNotIn("source_receipt_sha256", receipt)
        self.assertEqual(receipt["source"]["receipt_sha256"], sha(self.source_receipt))
        self.assertEqual(sha(archive), receipt["artifact"]["archive_sha256"])
        self.assertEqual(digest, admission["candidate_receipt_sha256"])
        self.assertEqual(admission_digest, sha((self.directory / "artifact-admission.json").read_bytes()))
        self.assertEqual(api_documents, self.api_documents)
        build = self.build_commands[2]
        self.assertEqual(build[0], str(self.root / "verified-bootstrap-mise"))
        self.assertIn(str(self.root / "verified-bootstrap-mbx"), build)
        self.assertFalse(any("mr-boxington@" in arg for arg in build))

    def test_source_receipt_changed_bytes_rejected(self):
        (self.directory / "source-receipt.json").write_bytes(self.source_receipt + b" ")
        with self.assertRaisesRegex(ValueError, "source receipt digest mismatch"):
            self.admit()

    def test_nested_source_digest_mismatch_rejected(self):
        self.receipt["source"]["receipt_sha256"] = "9" * 64
        self.save_candidate()
        with self.assertRaisesRegex(ValueError, "source identity mismatch"):
            self.admit()

    def test_source_receipt_matching_digest_wrong_semantics_rejected(self):
        value = json.loads(self.source_receipt)
        value["source_tree"] = "a" * 40
        changed = json.dumps(value).encode()
        self.source["receipt_sha256"] = sha(changed)
        os.environ["OWNED_TOOL_SOURCE_JSON"] = json.dumps(self.source)
        self.receipt["source"]["receipt_sha256"] = sha(changed)
        (self.directory / "source-receipt.json").write_bytes(changed)
        self.save_candidate()
        with self.assertRaisesRegex(ValueError, "source receipt does not match"):
            self.admit()

    def test_legacy_top_level_source_hash_rejected(self):
        self.receipt["source_receipt_sha256"] = sha(self.source_receipt)
        self.save_candidate()
        with self.assertRaisesRegex(ValueError, "unexpected or missing"):
            self.admit()

    def test_source_receipt_symlink_rejected(self):
        path = self.directory / "source-receipt.json"
        target = self.root / "linked-source.json"
        path.rename(target)
        path.symlink_to(target)
        with self.assertRaises(OSError):
            self.admit()

    def test_archive_changed_bytes_rejected(self):
        path = self.directory / self.receipt["artifact"]["name"]
        path.write_bytes(path.read_bytes() + b"changed")
        with self.assertRaisesRegex(ValueError, "archive digest mismatch"):
            self.admit()

    def test_artifact_admission_binding_mutations_rejected(self):
        mutations = {"artifact_id": True, "artifact_name": "wrong", "api_digest": "bad",
            "candidate_receipt_sha256": "0" * 64, "target": "wrong", "workflow": {},
            "binary_container": {}, "behavioral_qualification": {"passed": True}}
        original = self.admission.copy()
        for key, value in mutations.items():
            with self.subTest(field=key):
                self.admission = dict(original, **{key: value})
                self.save_admission()
                with self.assertRaises(ValueError):
                    self.admit()

    def test_missing_artifact_admission_rejected(self):
        (self.directory / "artifact-admission.json").unlink()
        with self.assertRaises(FileNotFoundError):
            self.admit()

    def test_exclusive_receipt_preserves_existing_file_and_symlink(self):
        path = self.root / "output.json"
        path.write_bytes(b"keep")
        with self.assertRaises(FileExistsError):
            QUALIFIER.write_exclusive(path, {"replacement": True})
        self.assertEqual(path.read_bytes(), b"keep")
        linked = self.root / "linked.json"
        linked.symlink_to(path)
        with self.assertRaises(FileExistsError):
            QUALIFIER.write_exclusive(linked, {"replacement": True})
        self.assertEqual(path.read_bytes(), b"keep")

    def native_report_fixture(self, command, **kwargs):
        report = {"version_is_distinct": True, "version": self.receipt["version_banner"],
            "binary_sha256": self.receipt["artifact"]["binary_sha256"],
            "source_commit": self.source["source_commit"],
            "source_diff_sha256": self.source["patch_sha256"],
            "upstream_commit": BASES["mise"][1],
            "host": {"system": platform.system(), "machine": platform.machine()},
            "results": [{"case": name, "passed": True} for name in MISE_CASES]}
        self.report_bytes = (json.dumps(report) + "\n").encode()
        Path(command[-1]).write_bytes(self.report_bytes)
        return SimpleNamespace(returncode=0)

    def test_qualified_outputs_retain_raw_report_and_artifact_binding(self):
        with patch.object(QUALIFIER.subprocess, "run", self.native_report_fixture):
            QUALIFIER.qualify(self.arguments)
        self.assertEqual(self.arguments.report.read_bytes(), self.report_bytes)
        qualified = json.loads(self.arguments.receipt.read_bytes())
        behavior = qualified["behavioral_qualification"]
        self.assertTrue(behavior["passed"])
        self.assertEqual(behavior["report_sha256"], sha(self.report_bytes))
        self.assertEqual(behavior["artifact_admission"], self.admission)
        self.assertEqual(behavior["cases"], len(MISE_CASES))
        execution = self.arguments.receipt.parent / behavior["execution_evidence"]["directory"]
        for key, filename in API_EVIDENCE_FILES.items():
            self.assertEqual((execution / filename).read_bytes(), self.api_documents[key])
        self.assertEqual(behavior["execution_evidence"]["api_sha256"], self.execution["api_sha256"])

    def test_preexisting_output_blocks_before_native_execution(self):
        self.arguments.report.write_bytes(b"keep")
        with patch.object(QUALIFIER.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "destination already exists"):
                QUALIFIER.qualify(self.arguments)
            run.assert_not_called()
        self.assertEqual(self.arguments.report.read_bytes(), b"keep")

    def test_wrong_native_inventory_preserves_failed_raw_evidence(self):
        def duplicate_case(command, **kwargs):
            result = self.native_report_fixture(command, **kwargs)
            report = json.loads(self.report_bytes)
            report["results"][-1] = report["results"][0]
            self.report_bytes = (json.dumps(report) + "\n").encode()
            Path(command[-1]).write_bytes(self.report_bytes)
            return result
        with patch.object(QUALIFIER.subprocess, "run", duplicate_case):
            with self.assertRaisesRegex(ValueError, "failed receipt preserved"):
                QUALIFIER.qualify(self.arguments)
        self.assertEqual(self.arguments.report.read_bytes(), self.report_bytes)
        receipt = json.loads(self.arguments.receipt.read_bytes())
        self.assertIs(receipt["behavioral_qualification"]["passed"], False)

    def test_unavailable_mbx_suite_never_executes(self):
        admitted = self.admit()
        self.arguments.tool = "mbx"
        with patch.object(QUALIFIER, "admit", return_value=admitted), \
                patch.object(QUALIFIER, "observe_admitted_mbx", side_effect=ValueError(
                    "MBX native qualification is unavailable")) as observe, \
                patch.object(QUALIFIER.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "MBX native qualification is unavailable"):
                QUALIFIER.qualify(self.arguments)
            run.assert_not_called()
            observe.assert_called_once()
        self.assertFalse(self.arguments.receipt.exists())
        self.assertFalse(self.arguments.report.exists())

    def reject_report_mutation(self, key, value):
        def changed_report(command, **kwargs):
            result = self.native_report_fixture(command, **kwargs)
            report = json.loads(self.report_bytes)
            report[key] = value
            self.report_bytes = (json.dumps(report) + "\n").encode()
            Path(command[-1]).write_bytes(self.report_bytes)
            return result
        with patch.object(QUALIFIER.subprocess, "run", changed_report):
            with self.assertRaisesRegex(ValueError, "failed receipt preserved"):
                QUALIFIER.qualify(self.arguments)
        behavior = json.loads(self.arguments.receipt.read_bytes())["behavioral_qualification"]
        self.assertIs(behavior["passed"], False)

    def test_wrong_report_upstream_base_rejected(self):
        self.reject_report_mutation("upstream_commit", "f" * 40)

    def test_wrong_report_native_host_rejected(self):
        other = {"system": "Linux", "machine": "x86_64"}
        if self.target == "x86_64-unknown-linux-gnu":
            other = {"system": "Darwin", "machine": "arm64"}
        self.reject_report_mutation("host", other)

    def assert_origin_blocks_execution(self, exception=ValueError):
        with patch.object(QUALIFIER.subprocess, "run") as run:
            with self.assertRaises(exception):
                QUALIFIER.qualify(self.arguments)
            run.assert_not_called()
        self.assertFalse(self.arguments.receipt.exists())

    def test_legacy_admission_without_execution_rejected(self):
        del self.admission["execution"]
        self.save_admission()
        self.assert_origin_blocks_execution()

    def test_raw_execution_api_digest_drift_blocks_candidate(self):
        path = self.directory / API_EVIDENCE_FILES["run"]
        path.write_bytes(path.read_bytes() + b" ")
        self.assert_origin_blocks_execution()

    def test_forged_head_repository_with_updated_digest_blocks_candidate(self):
        run = json.loads(self.api_documents["run"])
        run["head_repository"] = {"id": 100, "full_name": "tailrocks/velnor-new"}
        raw = json.dumps(run).encode()
        (self.directory / API_EVIDENCE_FILES["run"]).write_bytes(raw)
        self.admission["execution"]["api_sha256"]["run"] = sha(raw)
        self.save_admission()
        self.assert_origin_blocks_execution()

    def test_missing_raw_execution_api_blocks_candidate(self):
        (self.directory / API_EVIDENCE_FILES["workflow"]).unlink()
        self.assert_origin_blocks_execution(FileNotFoundError)

    def test_stale_execution_attempt_blocks_candidate(self):
        self.admission["execution"]["run_attempt"] = "1"
        self.save_admission()
        self.assert_origin_blocks_execution()

    def test_literal_candidate_push_origin_composes_with_builder_receipt(self):
        ref = "refs/heads/owned-tool-candidates"
        with patch.dict(os.environ, GITHUB_EVENT_NAME="push", GITHUB_REF=ref,
                GITHUB_WORKFLOW_REF="tailrocks/velnor-new/" + WORKFLOW_PATH + "@" + ref):
            run = json.loads(self.api_documents["run"])
            run.update(event="push", head_branch="owned-tool-candidates")
            self.api_documents["run"] = json.dumps(run).encode()
            (self.directory / API_EVIDENCE_FILES["run"]).write_bytes(self.api_documents["run"])
            endpoints = {PREFIX: "repository", PREFIX + "/actions/runs/11": "run",
                         PREFIX + "/actions/workflows/55": "workflow"}
            self.execution = admit_execution(lambda endpoint: self.api_documents[endpoints[endpoint]])
            self.save_candidate()
            self.assertEqual(self.admit()[3]["execution"]["ref"], ref)


if __name__ == "__main__":
    unittest.main()
