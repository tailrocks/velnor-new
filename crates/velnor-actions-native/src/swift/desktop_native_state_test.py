from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError, URLError

import desktop_native_state as subject


class NativeStateTests(unittest.TestCase):
    def test_exact_profile_assets_required(self):
        asset = "other-desk-1.2.3-aarch64-apple-darwin.zip"
        release = {"assets": [{"name": asset + suffix}
                              for suffix in ("", ".sha256", ".bundle", ".sbom.json")]}
        self.assertTrue(subject.compute_state(asset, release)["complete"])
        self.assertFalse(subject.compute_state("wrong-desk.zip", release)["complete"])
        self.assertFalse(subject.compute_state(asset, None)["release_exists"])

    def test_network_failure_not_missing_release(self):
        with patch.object(subject, "urlopen", side_effect=URLError("network failed")):
            with self.assertRaises(URLError):
                subject.fetch_release("other/project", "1.2.3")

    def test_only_explicit_404_is_missing(self):
        for code in (403, 429, 500):
            with self.subTest(code=code):
                error = HTTPError("https://api.github.com", code, "failed", {}, None)
                with patch.object(subject, "urlopen", side_effect=error):
                    with self.assertRaises(RuntimeError):
                        subject.fetch_release("other/project", "1.2.3")
        error = HTTPError("https://api.github.com", 404, "missing", {}, None)
        with patch.object(subject, "urlopen", side_effect=error):
            self.assertIsNone(subject.fetch_release("other/project", "1.2.3"))

    def test_repository_injection_rejected(self):
        for value in ("other/project/extra", "--repo/evil", "other/project?x=1", "other/project\n"):
            with self.subTest(value=value), self.assertRaises(RuntimeError):
                subject.validate_repository(value)

    def test_redirects_rejected_without_sending_credentials_elsewhere(self):
        with self.assertRaisesRegex(RuntimeError, "redirects"):
            subject.NoRedirect().redirect_request(None, None, 302, "redirect", {},
                                                  "https://other.example")

    def test_release_state_uses_profile_asset(self):
        with tempfile.TemporaryDirectory() as directory:
            profile = {"_root": Path(directory).resolve(), "apple": {
                "app_path": "native/dist/OtherDesk.app", "archive_name_prefix": "other-desk"}}
            with patch.object(subject, "fetch_release", return_value=None) as fetch:
                state = subject.release_state(profile, "1.2.3", "other/project")
            self.assertEqual(state["asset"], "other-desk-1.2.3-aarch64-apple-darwin.zip")
            fetch.assert_called_once_with("other/project", "1.2.3")


if __name__ == "__main__":
    unittest.main()
