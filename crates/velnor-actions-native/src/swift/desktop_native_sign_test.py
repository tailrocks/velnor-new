import hashlib
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import desktop_native_sign as subject


class NativeSignTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.app = self.root / "native/dist/OtherDesk.app"
        self.app.mkdir(parents=True)
        self.profile = {"_root": self.root, "apple": {
            "app_path": "native/dist/OtherDesk.app", "archive_name_prefix": "other-desk",
            "bundle_identifier": "org.other.desktop"}}
        self.cert = b"verified certificate"
        self.env = {"EXPECTED_TEAM_ID": "AB12CD34EF",
                    "EXPECTED_CERT_SHA256": hashlib.sha256(self.cert).hexdigest(),
                    "DEVELOPER_ID_APPLICATION": "Developer ID Application: Other (AB12CD34EF)",
                    "RUNNER_TEMP": str(self.root),
                    "APP_STORE_CONNECT_API_KEY_PATH": str(self.root / "AuthKey.p8"),
                    "APP_STORE_CONNECT_KEY_ID": "AB12CD34EF",
                    "APP_STORE_CONNECT_ISSUER_ID": "01234567-0123-0123-0123-0123456789ab"}
        (self.root / "AuthKey.p8").write_text("key")
        self.addCleanup(patch.stopall)
        patch.dict(os.environ, self.env).start()
        self.commands = []
        self.executor = patch.object(subject, "run", side_effect=self.execute).start()
        self.verifier = patch.object(subject, "verify_app").start()

    def execute(self, argv, **kwargs):
        self.commands.append(argv)
        if "--extract-certificates=" in " ".join(argv):
            prefix = next(arg.split("=", 1)[1] for arg in argv
                          if arg.startswith("--extract-certificates="))
            Path(prefix + "0").write_bytes(self.cert)
        elif argv[:2] == ["codesign", "-dv"]:
            return "TeamIdentifier=AB12CD34EF\n"
        elif "--entitlements" in argv:
            return plistlib.dumps({}).decode()
        elif argv[0] == "ditto":
            Path(argv[-1]).write_bytes(b"zip")
        elif argv[:3] == ["xcrun", "notarytool", "submit"]:
            return '{"status":"Accepted","id":"request"}'
        return ""

    def test_missing_and_malformed_expectations_precede_signing(self):
        for name, value in [("EXPECTED_TEAM_ID", ""), ("EXPECTED_TEAM_ID", "lowercase1"),
                            ("EXPECTED_CERT_SHA256", "bad")]:
            with self.subTest(name=name, value=value), patch.dict(os.environ, {name: value}):
                with self.assertRaises(RuntimeError):
                    subject.sign(self.profile, "1.2.3", "123")
                self.executor.assert_not_called()

    def test_wrong_team_stops_before_notarization(self):
        def wrong_team(argv, **kwargs):
            if argv[:2] == ["codesign", "-dv"]:
                return "TeamIdentifier=ZZZZZZZZZZ\n"
            return self.execute(argv, **kwargs)
        self.executor.side_effect = wrong_team
        with self.assertRaisesRegex(RuntimeError, "TeamIdentifier"):
            subject.sign(self.profile, "1.2.3", "123")
        self.assertFalse(any("notarytool" in command for command in self.commands))

    def test_team_inspection_exit_failure_propagates(self):
        def failing(argv, **kwargs):
            if argv[:2] == ["codesign", "-dv"]:
                raise subprocess.CalledProcessError(1, argv)
            return self.execute(argv, **kwargs)
        self.executor.side_effect = failing
        with self.assertRaises(subprocess.CalledProcessError):
            subject.sign(self.profile, "1.2.3", "123")
        self.assertFalse(any("notarytool" in command for command in self.commands))

    def test_certificate_mismatch_stops_before_notarization(self):
        self.env["EXPECTED_CERT_SHA256"] = "0" * 64
        with patch.dict(os.environ, self.env):
            with self.assertRaisesRegex(RuntimeError, "certificate SHA-256 mismatch"):
                subject.sign(self.profile, "1.2.3", "123")
        self.assertFalse(any("notarytool" in command for command in self.commands))
        self.assertFalse(list(self.root.glob("velnor-native-sign-*")))

    def test_rejected_notarization_cleans_submission(self):
        def rejected(argv, **kwargs):
            if argv[:3] == ["xcrun", "notarytool", "submit"]:
                return '{"status":"Invalid"}'
            return self.execute(argv, **kwargs)
        self.executor.side_effect = rejected
        with self.assertRaisesRegex(RuntimeError, "Accepted"):
            subject.sign(self.profile, "1.2.3", "123")
        self.assertFalse(list(self.root.glob("velnor-native-sign-*")))
        self.assertFalse(subject.archive_path(self.profile, "1.2.3").exists())

    def test_entitlement_inspection_exit_failure_propagates(self):
        def failing(argv, **kwargs):
            if "--entitlements" in argv:
                raise subprocess.CalledProcessError(1, argv)
            return self.execute(argv, **kwargs)
        self.executor.side_effect = failing
        with self.assertRaises(subprocess.CalledProcessError):
            subject.sign(self.profile, "1.2.3", "123")
        self.assertFalse(any("notarytool" in command for command in self.commands))

    def test_forbidden_entitlement_rejected(self):
        self.executor.side_effect = None
        self.executor.return_value = plistlib.dumps({"com.apple.security.get-task-allow": True}).decode()
        with self.assertRaisesRegex(RuntimeError, "get-task-allow"):
            subject.check_entitlements(self.app)

    def test_empty_success_means_no_entitlements(self):
        self.executor.side_effect = None
        for output in ("", f"Executable={self.app}\n"):
            self.executor.return_value = output
            subject.check_entitlements(self.app)
        self.executor.return_value = "unexpected malformed response"
        with self.assertRaisesRegex(RuntimeError, "parse signing entitlements"):
            subject.check_entitlements(self.app)

    def test_profile_owns_app_and_exact_final_zip(self):
        output = subject.sign(self.profile, "1.2.3", "123")
        self.assertEqual(output, self.app.parent / "other-desk-1.2.3-aarch64-apple-darwin.zip")
        self.assertEqual(self.verifier.call_args.kwargs["zip_path"], output)
        self.assertEqual(self.verifier.call_args.kwargs["app"], self.app)
        self.assertIs(self.verifier.call_args.args[0], self.profile)
        self.assertEqual(self.verifier.call_count, 3)
        self.assertFalse(list(self.root.glob("velnor-native-sign-*")))
        signing = next(command for command in self.commands if "--sign" in command)
        self.assertNotIn("--deep", signing)
        self.assertEqual(signing[-1], str(self.app))

    def test_wrong_bundle_fails_verifier_before_codesign(self):
        self.verifier.side_effect = RuntimeError("bundle identifier mismatch")
        with self.assertRaisesRegex(RuntimeError, "bundle identifier"):
            subject.sign(self.profile, "1.2.3", "123")
        self.executor.assert_not_called()

    def test_key_outside_runner_temp_rejected_before_codesign(self):
        with patch.dict(os.environ, {"APP_STORE_CONNECT_API_KEY_PATH": "/etc/hosts"}):
            with self.assertRaises(RuntimeError):
                subject.sign(self.profile, "1.2.3", "123")
        self.executor.assert_not_called()


if __name__ == "__main__":
    unittest.main()
