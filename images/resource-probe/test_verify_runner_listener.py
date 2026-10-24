"""Regression tests for the pinned runner listener image qualification."""

import pathlib
import struct
import subprocess
import tempfile
import unittest

import verify_runner_listener as verifier


def elf64_x86_64(machine=62):
    header = bytearray(64)
    header[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<H", header, 18, machine)
    return bytes(header)


class RunnerListenerTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.binary = pathlib.Path(self.directory.name) / "Runner.Listener"
        self.binary.write_bytes(elf64_x86_64())
        self.binary.chmod(0o755)

    def tearDown(self):
        self.directory.cleanup()

    def result(self, version="2.338.0", returncode=0):
        return subprocess.CompletedProcess(
            args=[str(self.binary), "--version"],
            returncode=returncode,
            stdout=f"{version}\n",
            stderr="",
        )

    def test_accepts_pinned_elf_and_exact_listener_version(self):
        verifier.verify_runner_listener(
            self.binary, "2.338.0", run=lambda *args, **kwargs: self.result()
        )

    def test_rejects_wrong_architecture_before_running_listener(self):
        self.binary.write_bytes(elf64_x86_64(machine=183))
        called = False

        def run(*args, **kwargs):
            nonlocal called
            called = True
            return self.result()

        with self.assertRaisesRegex(ValueError, "ELF64.*x86-64"):
            verifier.verify_runner_listener(self.binary, "2.338.0", run=run)
        self.assertFalse(called)

    def test_rejects_wrong_listener_version(self):
        with self.assertRaisesRegex(ValueError, "--version"):
            verifier.verify_runner_listener(
                self.binary,
                "2.338.0",
                run=lambda *args, **kwargs: self.result(version="2.337.0"),
            )

    def test_rejects_failed_or_malformed_version_probe(self):
        for result in (self.result(returncode=1), self.result(version="2.338.0-extra")):
            with self.subTest(result=result):
                with self.assertRaisesRegex(ValueError, "--version"):
                    verifier.verify_runner_listener(
                        self.binary, "2.338.0", run=lambda *args, **kwargs: result
                    )


if __name__ == "__main__":
    unittest.main()
