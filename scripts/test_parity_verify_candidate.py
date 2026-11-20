# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("parity-verify-candidate.py")
spec = importlib.util.spec_from_file_location("parity_verify_candidate", SCRIPT)
parity = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parity)


class BindingTests(unittest.TestCase):
    def archive_with(self, tmp, raw):
        path = Path(tmp) / "candidate.oci.tar"
        path.write_bytes(raw)
        return path

    def test_unparseable_archive_binds_to_nothing(self):
        with tempfile.TemporaryDirectory(prefix="parity-test-") as tmp:
            archive = self.archive_with(tmp, b"not a tar" * 100)
            self.assertEqual(parity.bind_oci(archive), parity.UNBOUND_OCI)

    def test_unparseable_archive_never_binds_the_reference_graph(self):
        with tempfile.TemporaryDirectory(prefix="parity-test-") as tmp:
            archive = self.archive_with(tmp, b"")
            reference_path = Path(tmp) / "ref.oci.tar"
            reference_path.write_bytes(parity.tar_of(parity.base_entries()))
            self.assertNotEqual(parity.bind_oci(archive), parity.legacy.verify_oci_archive(reference_path))

    def test_parseable_archive_binds_its_own_graph(self):
        with tempfile.TemporaryDirectory(prefix="parity-test-") as tmp:
            archive = self.archive_with(tmp, parity.tar_of(parity.base_entries()))
            self.assertEqual(parity.bind_oci(archive), parity.legacy.verify_oci_archive(archive))


class MessageDiffTests(unittest.TestCase):
    RUST_MESSAGES = {
        "layer member size base256 big": "Tar archive has an incorrect member size.",
        "outer truncated": "Cannot read OCI archive.",
        "root input missing Dockerfile": "/work/root/images/job/ubuntu-26.04/Dockerfile: No such file or directory (os error 2)",
        "root input missing README.md": "/work/root/images/job/ubuntu-26.04/README.md: No such file or directory (os error 2)",
        "root input missing build-test.sh": "/work/root/images/job/ubuntu-26.04/build-test.sh: No such file or directory (os error 2)",
        "root input missing candidate/candidate.py": "/work/root/images/job/ubuntu-26.04/candidate/candidate.py: No such file or directory (os error 2)",
        "root input missing runner.lock": "/work/root/.velnor/runner.lock: No such file or directory (os error 2)",
    }

    def test_unexpected_message_diff_is_not_allowed(self):
        self.assertFalse(parity.is_allowed_message_diff("layer not a tar", "anything"))

    def test_every_allowlisted_case_has_a_recorded_rust_message(self):
        self.assertEqual(set(parity.MESSAGE_DIFFS), set(self.RUST_MESSAGES))

    def test_allowlisted_case_with_expected_message_is_allowed(self):
        for name, message in self.RUST_MESSAGES.items():
            with self.subTest(name=name):
                self.assertTrue(parity.is_allowed_message_diff(name, message))

    def test_allowlisted_case_with_unrelated_message_is_not_allowed(self):
        for name in self.RUST_MESSAGES:
            with self.subTest(name=name):
                self.assertFalse(parity.is_allowed_message_diff(name, "unrelated text"))

    def test_allowlisted_case_with_crash_or_empty_message_is_not_allowed(self):
        for name in self.RUST_MESSAGES:
            with self.subTest(name=name):
                self.assertFalse(parity.is_allowed_message_diff(name, None))
                self.assertFalse(parity.is_allowed_message_diff(name, ""))


if __name__ == "__main__":
    unittest.main()
