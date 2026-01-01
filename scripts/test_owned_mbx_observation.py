"""Mocked smoke observations prove refusal boundaries; no native tool executes."""

import hashlib
import json
from pathlib import Path
import platform
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import owned_mbx_observation as O
from owned_tool_behavior import native_host_target
from owned_tool_qualification_evidence import validate_claim
from owned_tool_execution_test_fixtures import fixture


class ObservationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="mbx-observation-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.binary = self.root / "mbx"
        self.binary.write_bytes(b"fixture executable bytes; never run")
        self.candidate = {"tool": "mbx", "target": native_host_target(
            {"system": platform.system(), "machine": platform.machine()}),
            "version": "1.21.1-owned-cache-transport",
            "version_banner": "mbx 1.21.1-owned-cache-transport",
            "artifact": {"name": "mbx-fixture.tar.gz", "archive_sha256": "c" * 64,
                         "binary_sha256": hashlib.sha256(self.binary.read_bytes()).hexdigest()},
            "source": {"commit": "a" * 40, "tree": "b" * 40}}
        self.commands = []

    def execute(self, argv, **arguments):
        self.commands.append((argv, arguments))
        if argv[1:] == ["--version"]:
            data = self.candidate["version_banner"].encode() + b"\n"
        elif argv[1:] == ["cache", "dir", "--json"]:
            data = json.dumps({"version": 1, "store": str(self.root / "cache/actions")}).encode()
        elif argv[1:] == ["cache", "verify"]:
            data = b"verified 0 objects and 0 action results\n"
        else:
            self.fail("unreviewed smoke argv")
        arguments["stdout"].write(data)
        return SimpleNamespace(wait=lambda **kwargs: 0)

    def observe(self, execute=None):
        with patch.object(O.subprocess, "Popen", execute or self.execute):
            return O.observe(self.binary, self.root, self.candidate, "d" * 64)

    def test_exact_smoke_is_observed_only_with_bound_raw_bytes(self):
        with patch.dict("os.environ", MBX_REMOTE_URL="https://untrusted", RUSTC_WRAPPER="poison"):
            report = self.observe()
        self.assertEqual(len(self.commands), 3)
        self.assertEqual(report["status"], "OBSERVED_MBX_SMOKE_ONLY")
        self.assertIs(report["passed"], False)
        self.assertIsNone(report["abi"])
        self.assertIsNone(report["native_authority"])
        self.assertEqual(report["native_qualification"]["status"], "unavailable")
        self.assertEqual(report["source"], self.candidate["source"])
        self.assertEqual(report["store_before"], [])
        self.assertTrue(all(case["observation"] == "matched" for case in report["results"]))
        for argv, arguments in self.commands:
            self.assertEqual(argv[0], str(self.binary))
            self.assertEqual(arguments["cwd"], self.root)
            self.assertNotIn("MBX_REMOTE_URL", arguments["env"])
            self.assertNotIn("RUSTC_WRAPPER", arguments["env"])
        self.assertTrue(all(case["stdout"]["size"] > 0 for case in report["results"]))

    def test_wrong_binary_digest_rejected_before_execution(self):
        self.candidate["artifact"]["binary_sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "binary mismatch"):
            self.observe()
        self.assertEqual(self.commands, [])

    def test_wrong_host_rejected_before_execution(self):
        self.candidate["target"] = "not-a-native-target"
        with self.assertRaisesRegex(ValueError, "host/tool mismatch"):
            self.observe()
        self.assertEqual(self.commands, [])

    def test_wrong_version_preserved_without_cache_execution(self):
        def incorrect(argv, **arguments):
            self.commands.append((argv, arguments))
            arguments["stdout"].write(b"mbx 1.21.1\n")
            return SimpleNamespace(wait=lambda **kwargs: 0)
        report = self.observe(incorrect)
        self.assertEqual(len(self.commands), 1)
        self.assertEqual(report["results"][0]["observation"], "mismatched")
        self.assertIs(report["passed"], False)

    def test_wrong_store_report_preserved_without_verification(self):
        def incorrect(argv, **arguments):
            if argv[1:] == ["cache", "dir", "--json"]:
                self.commands.append((argv, arguments))
                arguments["stdout"].write(b'{"version":1,"store":"<PRIVATE_TMP>"}')
                return SimpleNamespace(wait=lambda **kwargs: 0)
            return self.execute(argv, **arguments)
        report = self.observe(incorrect)
        self.assertEqual(len(self.commands), 2)
        self.assertEqual(report["results"][-1]["observation"], "mismatched")

    def test_wrong_tool_rejected_before_execution(self):
        self.candidate["tool"] = "mise"
        with self.assertRaisesRegex(ValueError, "host/tool mismatch"):
            self.observe()
        self.assertEqual(self.commands, [])

    def test_publisher_denies_smoke_even_with_forged_passed_flag(self):
        manifest, _ = fixture("mbx")
        claim = manifest["artifacts"][0]["qualification"]
        for passed in (False, True):
            claim.update(passed=passed, abi=None, cases=3)
            with self.assertRaisesRegex(ValueError, "ABI unavailable"):
                validate_claim(claim, "mbx")

    def test_preexisting_cache_blocks_before_execution(self):
        (self.root / "cache").mkdir()
        with self.assertRaisesRegex(ValueError, "fresh observation"):
            self.observe()
        self.assertEqual(self.commands, [])

    def test_store_symlink_rejected_without_reading_target(self):
        (self.root / "cache").symlink_to(self.root, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "store symlink"):
            O.inventory(self.root / "cache")

    def test_nonprivate_root_blocks_before_execution(self):
        self.root.chmod(0o755)
        with self.assertRaisesRegex(ValueError, "private owned"):
            self.observe()
        self.assertEqual(self.commands, [])

    def test_output_bound_and_symlink_are_rejected(self):
        path = self.root / "large-output"
        path.write_bytes(b"x" * (O.MAX_OUTPUT + 1))
        with self.assertRaisesRegex(ValueError, "bounded regular"):
            O.output(path)
        link = self.root / "output-link"
        link.symlink_to(path)
        with self.assertRaises(OSError):
            O.output(link)

    def test_timeout_kills_process_group_and_preserves_partial_output(self):
        def timed_out(argv, **arguments):
            arguments["stdout"].write(b"partial")
            wait = unittest.mock.Mock(side_effect=[subprocess.TimeoutExpired(argv, 60), 9])
            return SimpleNamespace(wait=wait, pid=54321)
        with patch.object(O.os, "killpg") as kill:
            report = self.observe(timed_out)
        kill.assert_called_once_with(54321, O.signal.SIGKILL)
        self.assertTrue(report["results"][0]["timed_out"])
        self.assertEqual(report["results"][0]["stdout"]["size"], len(b"partial"))
        self.assertIs(report["passed"], False)


if __name__ == "__main__":
    unittest.main()
