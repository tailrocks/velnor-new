"""Installation trust boundaries; no network or executable invocation."""
import hashlib
import json
import os
from pathlib import Path
import platform
import tempfile
import unittest
from unittest.mock import Mock, patch

import catalog_mise_acquisition_install as acquisition
from catalog_executable_bounds import executable_limit, archive_limit, stream_sha256
acquisition.executable_limit = executable_limit
acquisition.archive_limit = archive_limit
acquisition.stream_sha256 = stream_sha256

BINARY = b"qualified owned Mise bytes"
DIGEST = hashlib.sha256(BINARY).hexdigest()


class InstallationSecurityTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.temp = Path(self.temporary.name).resolve()
        self.root = self.temp / "velnor/mise"
        self.binary = self.root / "bin/mise"
        self.path_output = self.temp / "github-path"
        self.step_output = self.temp / "github-output"
        environment = {"RUNNER_TEMP": str(self.temp), "MISE_DATA_DIR": str(self.root),
                       "GITHUB_PATH": str(self.path_output), "GITHUB_OUTPUT": str(self.step_output)}
        self.environment = patch.dict(os.environ, environment, clear=True)
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.domains = patch.object(acquisition, "DOMAINS", {"full": "velnor/mise"}, create=True)
        self.domains.start()
        self.addCleanup(self.domains.stop)
        output_purpose = patch.object(acquisition, "OUTPUT_PURPOSE", "workflow", create=True)
        output_purpose.start()
        self.addCleanup(output_purpose.stop)
        self.configuration = {"platform": [platform.system(), platform.machine()],
                              "binary_sha256": DIGEST, "archive_sha256": DIGEST,
                              "asset_url": "https://github.com/owner/mise/releases/download/v1/mise",
                              "format": "binary", "member": "", "qualification": "bound",
                              "policy": "velnor-mise-acquisition-v1"}

    def existing(self, contents=BINARY):
        self.binary.parent.mkdir(parents=True, mode=0o700)
        self.binary.write_bytes(contents)
        self.binary.chmod(0o700)

    def test_source_intent_has_no_workflow_outputs_or_full_namespace(self):
        source_root = self.temp / 'velnor-control/source-intent/mise'
        os.environ['MISE_DATA_DIR'] = str(source_root)
        os.environ.pop('GITHUB_PATH')
        os.environ.pop('GITHUB_OUTPUT')
        with patch.object(acquisition, 'DOMAINS', {'source-intent': 'velnor-control/source-intent/mise'}), \
                patch.object(acquisition, 'OUTPUT_PURPOSE', 'source-intent'), \
                patch.object(acquisition, 'download_asset', lambda *_: BINARY, create=True):
            acquisition.acquire('source-intent', self.configuration)
        self.assertEqual((source_root / 'bin/mise').read_bytes(), BINARY)
        self.assertFalse(self.root.exists())
        self.assertFalse(self.path_output.exists())
        self.assertFalse(self.step_output.exists())

    def acquire(self, downloader):
        with patch.object(acquisition, "download_asset", downloader, create=True):
            acquisition.acquire("full", self.configuration)

    def test_healthy_binary_never_fetches(self):
        self.existing()
        downloader = Mock(side_effect=AssertionError("healthy binary fetched"))
        self.acquire(downloader)
        downloader.assert_not_called()
        self.assertEqual(self.path_output.read_text(), str(self.binary.parent) + "\n")
        receipt = json.loads((self.root / ".velnor-mise-receipt.json").read_text())
        self.assertEqual(receipt, self.configuration)

    def test_damaged_regular_binary_repairs(self):
        self.existing(b"damaged")
        downloader = Mock(return_value=BINARY)
        self.acquire(downloader)
        downloader.assert_called_once_with(self.configuration["asset_url"], DIGEST)
        self.assertEqual(self.binary.read_bytes(), BINARY)
        self.assertEqual(self.binary.stat().st_mode & 0o777, 0o700)

    def test_safe_regular_wrong_permissions_repairs_once(self):
        self.existing()
        for mode in (0o600, 0o720):
            with self.subTest(mode=oct(mode)):
                self.binary.chmod(mode)
                downloader = Mock(return_value=BINARY)
                self.acquire(downloader)
                downloader.assert_called_once_with(self.configuration["asset_url"], DIGEST)
                self.assertEqual(self.binary.read_bytes(), BINARY)
                self.assertEqual(self.binary.stat().st_mode & 0o777, 0o700)

    def test_downloaded_binary_hash_before_install(self):
        self.existing(b"damaged")
        with self.assertRaisesRegex(ValueError, "binary_digest"):
            self.acquire(Mock(return_value=b"unauthenticated"))
        self.assertEqual(self.binary.read_bytes(), b"damaged")
        self.assertFalse(self.path_output.exists())
        self.assertFalse((self.root / ".velnor-mise-receipt.json").exists())

    def test_final_hash_before_publication(self):
        real_atomic = acquisition.atomic_file
        def damage_after_write(parent, name, payload, mode):
            real_atomic(parent, name, b"postwrite damage" if name == "mise" else payload, mode)
        with patch.object(acquisition, "atomic_file", side_effect=damage_after_write):
            with self.assertRaisesRegex(ValueError, "final_digest"):
                self.acquire(Mock(return_value=BINARY))
        self.assertFalse(self.path_output.exists())
        self.assertFalse((self.root / ".velnor-mise-receipt.json").exists())

    def test_symlink_root_components_reject_without_fetch(self):
        for component in ("velnor", "mise", "bin"):
            with self.subTest(component=component):
                target = self.temp / ("outside-" + component)
                target.mkdir(mode=0o700)
                link = {"velnor": self.temp / "velnor", "mise": self.root,
                        "bin": self.binary.parent}[component]
                link.parent.mkdir(parents=True, exist_ok=True)
                link.symlink_to(target, target_is_directory=True)
                downloader = Mock(side_effect=AssertionError("symlink root fetched"))
                with self.assertRaises(OSError):
                    self.acquire(downloader)
                downloader.assert_not_called()
                self.assertEqual(list(target.iterdir()), [])
                link.unlink()

    def test_symlink_binary_rejects_without_fetch(self):
        self.binary.parent.mkdir(parents=True, mode=0o700)
        target = self.temp / "outside-binary"
        target.write_bytes(BINARY)
        target.chmod(0o700)
        self.binary.symlink_to(target)
        downloader = Mock()
        with self.assertRaises(OSError):
            self.acquire(downloader)
        downloader.assert_not_called()

    def test_wrong_platform_rejects_before_root_creation(self):
        self.configuration["platform"] = ["foreign", "foreign"]
        with self.assertRaisesRegex(ValueError, "platform"):
            self.acquire(Mock())
        self.assertFalse(self.root.exists())

    def test_fifo_binary_rejects_without_blocking_or_fetch(self):
        self.binary.parent.mkdir(parents=True, mode=0o700)
        os.mkfifo(self.binary)
        real_open = os.open
        def require_nonblocking(name, flags, *args, **kwargs):
            if name == "mise" and not flags & os.O_DIRECTORY:
                self.assertTrue(flags & os.O_NONBLOCK, "FIFO open would block")
            return real_open(name, flags, *args, **kwargs)
        downloader = Mock()
        with patch.object(acquisition.os, "open", side_effect=require_nonblocking):
            with self.assertRaisesRegex(ValueError, "binary_shape"):
                self.acquire(downloader)
        downloader.assert_not_called()

    def test_temp_control_characters_reject_before_creation(self):
        for character in ("\n", "\r", "\x7f", "\x85", "\x9f"):
            with self.subTest(character=repr(character)):
                os.environ["RUNNER_TEMP"] = str(self.temp) + character + "injected"
                with self.assertRaisesRegex(ValueError, "temp_path"):
                    self.acquire(Mock())
                self.assertFalse(self.root.exists())


if __name__ == "__main__":
    unittest.main()
