# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
"""scripts/mise-binary-digests.sh trusts SHASUMS256.txt only after its minisign signature checks."""

import base64
import importlib.util
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
SCRIPT = SCRIPTS / "mise-binary-digests.sh"
FIXTURE = SCRIPTS / "fixtures" / "mise-2026.11.11"
VERSION = "2026.11.11"
LINUX_X64 = "be6f46a4cc3d5532acf359f163cf7ce80ceb43e5718d7b9049165a340fb02e2d"


spec = importlib.util.spec_from_file_location("minisign_verify", SCRIPTS / "minisign_verify.py")
minisign = importlib.util.module_from_spec(spec)
spec.loader.exec_module(minisign)


def run(*args):
    return subprocess.run(
        ["sh", str(SCRIPT), VERSION, *args], capture_output=True, text=True, timeout=120
    )


class SignatureGate(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="mise-digests-test."))
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)
        self.sums = self.dir / "SHASUMS256.txt"
        self.sig = self.dir / "SHASUMS256.txt.minisig"
        shutil.copy(FIXTURE / "SHASUMS256.txt", self.sums)
        shutil.copy(FIXTURE / "SHASUMS256.txt.minisig", self.sig)

    def refused(self, *extra):
        result = run("--sums", str(self.sums), *extra)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(result.stdout, "", "no digest may be printed before the signature checks")
        return result.stderr

    def test_official_manifest_with_its_signature_is_trusted(self):
        result = run("--sums", str(self.sums))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(LINUX_X64, result.stdout)

    def test_tampered_digest_is_refused(self):
        text = self.sums.read_text()
        self.assertIn(LINUX_X64, text)
        self.sums.write_text(text.replace(LINUX_X64, "0" * 64))
        self.assertIn("signature", self.refused())

    def test_appended_line_is_refused(self):
        with self.sums.open("a") as handle:
            handle.write("0" * 64 + "  ./mise-v2026.11.11-linux-x64\n")
        self.assertIn("signature", self.refused())

    def test_missing_signature_is_refused(self):
        self.sig.unlink()
        self.assertIn("signature", self.refused())

    def test_empty_signature_is_refused(self):
        self.sig.write_text("")
        self.assertIn("signature", self.refused())

    def test_signature_of_another_key_or_garbage_is_refused(self):
        lines = self.sig.read_text().splitlines()
        lines[1] = lines[1][:-8] + "AAAAAAAA"
        self.sig.write_text("\n".join(lines) + "\n")
        self.assertIn("signature", self.refused())

    def test_explicit_signature_path_is_used(self):
        other = self.dir / "elsewhere.minisig"
        self.sig.rename(other)
        self.assertNotEqual(run("--sums", str(self.sums)).returncode, 0)
        result = run("--sums", str(self.sums), "--sig", str(other))
        self.assertEqual(result.returncode, 0, result.stderr)


class Verifier(unittest.TestCase):
    def setUp(self):
        self.key = (SCRIPTS / "mise-minisign.pub").read_text()
        self.sig = (FIXTURE / "SHASUMS256.txt.minisig").read_text()
        self.message = (FIXTURE / "SHASUMS256.txt").read_bytes()

    def test_official_release_verifies_and_returns_the_trusted_comment(self):
        trusted = minisign.verify(self.key, self.sig, self.message)
        self.assertIn("file:SHASUMS256.txt", trusted)

    def test_other_message_is_refused(self):
        with self.assertRaises(minisign.SignatureError):
            minisign.verify(self.key, self.sig, self.message + b"\n")

    def test_tampered_trusted_comment_is_refused(self):
        forged = self.sig.replace("file:SHASUMS256.txt", "file:SHASUMS512.txt")
        with self.assertRaises(minisign.SignatureError):
            minisign.verify(self.key, forged, self.message)

    def test_flipped_signature_bit_is_refused(self):
        lines = self.sig.splitlines()
        raw = bytearray(base64.b64decode(lines[1]))
        raw[40] ^= 1
        lines[1] = base64.b64encode(bytes(raw)).decode()
        with self.assertRaises(minisign.SignatureError):
            minisign.verify(self.key, "\n".join(lines) + "\n", self.message)

    def test_different_key_is_refused(self):
        raw = bytearray(base64.b64decode(self.key.splitlines()[1]))
        raw[20] ^= 1
        other = "untrusted comment: other\n" + base64.b64encode(bytes(raw)).decode() + "\n"
        with self.assertRaises(minisign.SignatureError):
            minisign.verify(other, self.sig, self.message)

    def test_key_id_mismatch_is_refused(self):
        raw = bytearray(base64.b64decode(self.key.splitlines()[1]))
        raw[3] ^= 1
        other = "untrusted comment: other\n" + base64.b64encode(bytes(raw)).decode() + "\n"
        with self.assertRaises(minisign.SignatureError):
            minisign.verify(other, self.sig, self.message)

    def test_non_prehashed_signature_is_refused(self):
        lines = self.sig.splitlines()
        raw = bytearray(base64.b64decode(lines[1]))
        raw[1] = ord("d")
        lines[1] = base64.b64encode(bytes(raw)).decode()
        with self.assertRaises(minisign.SignatureError):
            minisign.verify(self.key, "\n".join(lines) + "\n", self.message)

    def test_garbage_is_refused(self):
        for sig in ("", "garbage", "a\nb\nc\nd\n", self.sig.replace("trusted comment: ", "")):
            with self.assertRaises(minisign.SignatureError):
                minisign.verify(self.key, sig, self.message)

    def test_scalar_not_below_the_group_order_is_refused(self):
        key = base64.b64decode(self.key.splitlines()[1])[10:]
        raw = base64.b64decode(self.sig.splitlines()[1])[10:]
        s = int.from_bytes(raw[32:], "little") + minisign.L
        forged = raw[:32] + s.to_bytes(32, "little")
        self.assertFalse(minisign.ed25519_verify(key, b"x", forged))

    def test_small_order_public_key_is_refused(self):
        identity = (1).to_bytes(32, "little")
        self.assertFalse(minisign.ed25519_verify(identity, b"x", identity + bytes(32)))


if __name__ == "__main__":
    unittest.main()
