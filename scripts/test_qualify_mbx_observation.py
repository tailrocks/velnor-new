"""Real MBX container admission, mocked smoke; no tool or compiler executes."""

import hashlib
import json
import os
from pathlib import Path
import platform
import unittest
from unittest.mock import patch

import test_qualify_owned_tool as T
from owned_mbx_observation import SMOKE_CASES, UNAVAILABLE
from owned_tool_source import BASES
from owned_tool_qualification_evidence import validate_qualified


class MbxDispatchTests(unittest.TestCase):
    def setUp(self):
        self.fixture = T.QualifierAdmissionTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        f = self.fixture
        f.source.update(tool="mbx", version="1.21.1-owned-cache-transport",
                        upstream_base_commit=BASES["mbx"][1])
        source_receipt = json.loads(f.source_receipt)
        source_receipt.update(tool="mbx", upstream_repository="https://github.com/jdx/mr-boxington",
                              upstream_base_commit=BASES["mbx"][1])
        f.source_receipt = json.dumps(source_receipt, sort_keys=True).encode()
        f.source["receipt_sha256"] = T.sha(f.source_receipt)
        os.environ["OWNED_TOOL_SOURCE_JSON"] = json.dumps(f.source)
        build = f.root / "mbx-build"
        build.mkdir()
        bootstrap = {tool: f.root / ("verified-bootstrap-" + tool) for tool in ("mise", "mbx")}
        def acquire(spec, source, environment):
            self.assertEqual(spec, f.source)
            source.mkdir()
            (source / "LICENSE").write_bytes(b"fixture license\n")
            binary = source.parent / "target/release/mbx"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(T.native_header(f.target))
            return f.source_receipt
        with patch.object(T.BUILDER, "admit", acquire), \
                patch.object(T.BUILDER, "run", f.build_execution_fixture):
            name, archive, f.receipt, evidence = T.BUILDER.candidate(
                f.source, bootstrap, build, T.BUILDER.workflow_identity())
        f.directory = f.root / "mbx-candidate"
        f.directory.mkdir()
        (f.directory / name).write_bytes(archive)
        (f.directory / "source-receipt.json").write_bytes(evidence)
        for key, filename in T.API_EVIDENCE_FILES.items():
            (f.directory / filename).write_bytes(f.api_documents[key])
        f.arguments.tool, f.arguments.candidate_directory = "mbx", f.directory
        f.save_candidate()
        f.admission.update(tool="mbx", artifact_name=f"owned-candidate-11-2-mbx-{f.target}")
        f.save_admission()

    def observation(self, binary, root, receipt, input_digest):
        return {"schema": 1, "status": "OBSERVED_MBX_SMOKE_ONLY",
            "source": receipt["source"], "target": receipt["target"],
            "host": {"system": platform.system(), "machine": platform.machine()},
            "artifact": receipt["artifact"], "candidate_receipt_sha256": input_digest,
            "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "version": receipt["version_banner"].strip(), "version_is_distinct": True,
            "environment": {}, "store_before": [], "store_after": [],
            "native_qualification": {"status": "unavailable", "reason": UNAVAILABLE},
            "results": [{"case": name, "returncode": 0, "timed_out": False,
                         "observation": "matched"} for name in SMOKE_CASES],
            "passed": False, "abi": None, "native_authority": None}

    def qualify(self, observer=None):
        with patch.object(T.QUALIFIER, "observe_mbx", observer or self.observation):
            T.QUALIFIER.qualify(self.fixture.arguments)

    def test_admitted_smoke_preserves_candidate_and_unavailable_evidence(self):
        f = self.fixture
        original = (f.directory / "candidate-receipt.json").read_bytes()
        with self.assertRaisesRegex(ValueError, "unavailable; observations preserved"):
            self.qualify()
        envelope = json.loads(f.arguments.receipt.read_bytes())
        self.assertEqual(envelope["status"], "MBX_NATIVE_QUALIFICATION_UNAVAILABLE")
        self.assertIsNone(envelope["candidate"]["behavioral_qualification"])
        self.assertEqual((f.directory / "candidate-receipt.json").read_bytes(), original)
        self.assertEqual(envelope["observation_report_sha256"], T.sha(f.arguments.report.read_bytes()))
        self.assertIs(envelope["passed"], False)
        self.assertIsNone(envelope["abi"])
        self.assertIsNone(envelope["native_authority"])
        with self.assertRaisesRegex(ValueError, "unexpected or missing"):
            validate_qualified(envelope, {}, {}, {"qualification": {}})

    def test_wrong_report_identity_preserved_but_rejected(self):
        def incorrect(*arguments):
            report = self.observation(*arguments)
            report["source"] = {"commit": "0" * 40}
            return report
        with self.assertRaisesRegex(ValueError, "observation rejected"):
            self.qualify(incorrect)
        envelope = json.loads(self.fixture.arguments.receipt.read_bytes())
        self.assertEqual(envelope["status"], "MBX_OBSERVATION_REJECTED")

    def test_failed_smoke_preserved_but_rejected(self):
        def failed(*arguments):
            report = self.observation(*arguments)
            report["results"][-1]["observation"] = "mismatched"
            return report
        with self.assertRaisesRegex(ValueError, "observation rejected"):
            self.qualify(failed)
        envelope = json.loads(self.fixture.arguments.receipt.read_bytes())
        self.assertEqual(envelope["status"], "MBX_OBSERVATION_REJECTED")

    def test_wrong_source_blocks_smoke_before_execution(self):
        f = self.fixture
        f.receipt["source"]["commit"] = "0" * 40
        f.save_candidate()
        with patch.object(T.QUALIFIER, "observe_mbx") as observe:
            with self.assertRaisesRegex(ValueError, "source identity mismatch"):
                T.QUALIFIER.qualify(f.arguments)
            observe.assert_not_called()

    def test_wrong_host_blocks_smoke_before_execution(self):
        f = self.fixture
        f.arguments.target = "unavailable-host"
        with patch.object(T.QUALIFIER, "observe_mbx") as observe:
            with self.assertRaisesRegex(ValueError, "native host mismatch"):
                T.QUALIFIER.qualify(f.arguments)
            observe.assert_not_called()

    def test_wrong_binary_archive_blocks_smoke_before_execution(self):
        f = self.fixture
        f.receipt["artifact"]["binary_sha256"] = "0" * 64
        f.save_candidate()
        with patch.object(T.QUALIFIER, "observe_mbx") as observe:
            with self.assertRaisesRegex(ValueError, "binary digest mismatch"):
                T.QUALIFIER.qualify(f.arguments)
            observe.assert_not_called()


if __name__ == "__main__":
    unittest.main()
