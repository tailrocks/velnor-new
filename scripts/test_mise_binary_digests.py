# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
"""scripts/mise-binary-digests.sh trusts SHASUMS256.txt only after the real `minisign` verifies it.

The tests run the pinned tool (mise.toml, `minisign`), never a stand-in. Without it on PATH they
skip, unless VELNOR_REQUIRE_MINISIGN is set, as the `test-maintenance-helpers` task sets it: then
a missing tool is a failure, so CI cannot pass by skipping.
"""

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
SCRIPT = SCRIPTS / "mise-binary-digests.sh"
FIXTURE = SCRIPTS / "fixtures" / "mise-2026.11.11"
OLDER = SCRIPTS / "fixtures" / "mise-2026.11.10"
VERSION = "2026.11.11"
OLDER_VERSION = "2026.11.10"
LINUX_X64 = "be6f46a4cc3d5532acf359f163cf7ce80ceb43e5718d7b9049165a340fb02e2d"
MINISIGN = shutil.which("minisign")


def run(*args, env=None, version=VERSION):
    return subprocess.run(
        ["/bin/sh", str(SCRIPT), version, *args],
        capture_output=True,
        text=True,
        timeout=120,
        env=env,
    )


def setUpModule():
    if MINISIGN is None and os.environ.get("VELNOR_REQUIRE_MINISIGN"):
        raise AssertionError("minisign is not on PATH: run `mise install` (it is pinned in mise.toml)")


@unittest.skipIf(MINISIGN is None, "minisign is not installed: run `mise install` (pinned in mise.toml)")
class SignatureGate(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="mise-digests-test."))
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)
        self.sums = self.dir / "SHASUMS256.txt"
        self.sig = self.dir / "SHASUMS256.txt.minisig"
        shutil.copy(FIXTURE / "SHASUMS256.txt", self.sums)
        shutil.copy(FIXTURE / "SHASUMS256.txt.minisig", self.sig)

    def refused(self, *extra, env=None):
        result = run("--sums", str(self.sums), *extra, env=env)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(result.stdout, "", "no digest may be printed before the signature checks")
        return result.stderr

    def test_official_manifest_with_its_signature_is_trusted(self):
        result = run("--sums", str(self.sums))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(LINUX_X64, result.stdout)

    def test_older_release_replayed_as_newer_is_refused(self):
        older_sums, older_sig = OLDER / "SHASUMS256.txt", OLDER / "SHASUMS256.txt.minisig"
        genuine = run("--sums", str(older_sums), "--sig", str(older_sig), version=OLDER_VERSION)
        self.assertEqual(genuine.returncode, 0, genuine.stderr)
        replayed = run("--sums", str(older_sums), "--sig", str(older_sig))
        self.assertNotEqual(replayed.returncode, 0, replayed.stdout)
        self.assertEqual(replayed.stdout, "", "no digest may be printed for a replayed release")

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

    def test_corrupted_signature_is_refused(self):
        lines = self.sig.read_text().splitlines()
        lines[1] = lines[1][:-8] + "AAAAAAAA"
        self.sig.write_text("\n".join(lines) + "\n")
        self.assertIn("signature", self.refused())

    def test_tampered_trusted_comment_is_refused(self):
        text = self.sig.read_text()
        self.assertIn("file:SHASUMS256.txt", text)
        self.sig.write_text(text.replace("file:SHASUMS256.txt", "file:SHASUMS512.txt"))
        self.assertIn("signature", self.refused())

    def test_valid_signature_of_a_foreign_key_is_refused(self):
        key, pub = self.dir / "foreign.key", self.dir / "foreign.pub"
        subprocess.run(
            [MINISIGN, "-G", "-W", "-f", "-s", str(key), "-p", str(pub)],
            check=True,
            capture_output=True,
            timeout=60,
        )
        subprocess.run(
            [MINISIGN, "-S", "-s", str(key), "-m", str(self.sums), "-x", str(self.sig)],
            check=True,
            capture_output=True,
            timeout=60,
        )
        # The foreign signature is genuine for its own key: minisign accepts it with that key,
        # and only the pinned key makes the script refuse it.
        own = subprocess.run(
            [MINISIGN, "-V", "-m", str(self.sums), "-x", str(self.sig), "-p", str(pub)],
            capture_output=True,
            timeout=60,
        )
        self.assertEqual(own.returncode, 0, own.stderr)
        self.assertIn("signature", self.refused())

    def test_missing_minisign_is_refused(self):
        bare = self.dir / "bin"
        bare.mkdir()
        (bare / "python3").symlink_to(os.path.realpath(sys.executable))
        self.assertNotIn("minisign", {p.name for p in bare.iterdir()})
        error = self.refused(env={"PATH": str(bare)})
        self.assertIn("minisign is not installed", error)

    def test_explicit_signature_path_is_used(self):
        other = self.dir / "elsewhere.minisig"
        self.sig.rename(other)
        self.assertNotEqual(run("--sums", str(self.sums)).returncode, 0)
        result = run("--sums", str(self.sums), "--sig", str(other))
        self.assertEqual(result.returncode, 0, result.stderr)


@unittest.skipIf(MINISIGN is None, "minisign is not installed: run `mise install` (pinned in mise.toml)")
class MinisignResolution(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="mise-minisign-path."))
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)
        self.bin = self.dir / "bin"
        self.bin.mkdir()
        (self.bin / "python3").symlink_to(os.path.realpath(sys.executable))
        self.used = self.dir / "used"
        self.sums = self.dir / "SHASUMS256.txt"
        self.sig = self.dir / "SHASUMS256.txt.minisig"
        shutil.copy(FIXTURE / "SHASUMS256.txt", self.sums)
        shutil.copy(FIXTURE / "SHASUMS256.txt.minisig", self.sig)

    def shim(self, path):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f'#!/bin/sh\necho "$0" >> "{self.used}"\nexec "{MINISIGN}" "$@"\n')
        path.chmod(0o755)
        return str(path)

    def fake_mise(self, resolved):
        mise = self.bin / "mise"
        if resolved is None:
            mise.write_text("#!/bin/sh\nexit 1\n")
        else:
            mise.write_text(
                '#!/bin/sh\nif [ "$1 $2" = "which minisign" ]; then echo "'
                + resolved
                + '"; exit 0; fi\nexit 2\n'
            )
        mise.chmod(0o755)

    def run_with_path(self):
        return run("--sums", str(self.sums), env={"PATH": f"{self.bin}:/usr/bin:/bin"})

    def test_mise_pinned_minisign_is_used(self):
        pinned = self.shim(self.dir / "mise-install" / "minisign")
        self.shim(self.bin / "minisign")
        self.fake_mise(pinned)
        result = self.run_with_path()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.used.read_text().split(), [pinned])

    def test_path_minisign_is_the_fallback_when_mise_cannot_resolve_it(self):
        path_tool = self.shim(self.bin / "minisign")
        self.fake_mise(None)
        result = self.run_with_path()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.used.read_text().split(), [path_tool])


if __name__ == "__main__":
    unittest.main()
