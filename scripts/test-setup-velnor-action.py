# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ACTION = Path(__file__).resolve().parent.parent / "actions" / "setup-velnor" / "action.yml"
RELEASE_URL = "https://github.com/tailrocks/velnor-new/releases/download/rust-ci-v0.1.79/velnor-ci-linux-x64"
PAYLOAD = b"the released velnor-ci binary\n"
DIGEST_TOOLS = ("sha256sum", "shasum")
SCRIPT_TOOLS = ("bash", "cut", "tr", "mktemp", "rm", "cp", "mkdir", "chmod", "mv")

STUB_CURL = """#!/bin/sh
printf '%s\\n' "$*" >> "$STUB_LOG"
out=
while [ $# -gt 0 ]; do
  if [ "$1" = "--output" ]; then out="$2"; fi
  shift
done
cp "$STUB_PAYLOAD" "$out"
"""


def install_script():
    lines = ACTION.read_text().splitlines()
    starts = [index for index, line in enumerate(lines) if line.strip() == "run: |"]
    if len(starts) != 1:
        raise AssertionError(f"expected one run block in {ACTION}, found {len(starts)}")
    body = []
    indent = None
    for line in lines[starts[0] + 1 :]:
        if not line.strip():
            body.append("")
            continue
        current = len(line) - len(line.lstrip())
        if indent is None:
            indent = current
        if current < indent:
            break
        body.append(line[indent:])
    return "\n".join(body) + "\n"


def installed_files(root):
    if not root.exists():
        return []
    return sorted(path for path in root.rglob("*") if path.is_file())


class SetupVelnorInstallTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.stub_bin = self.root / "stub-bin"
        self.runner_temp = self.root / "runner-temp"
        self.tool_cache = self.root / "tool-cache"
        self.github_path = self.root / "github-path"
        self.stub_log = self.root / "curl-calls.log"
        self.payload = self.root / "payload"
        for directory in (self.stub_bin, self.runner_temp, self.tool_cache):
            directory.mkdir()
        self.github_path.touch()
        self.payload.write_bytes(PAYLOAD)
        curl = self.stub_bin / "curl"
        curl.write_text(STUB_CURL)
        curl.chmod(0o755)

    def path_with(self, digest_tool):
        if shutil.which(digest_tool) is None:
            self.skipTest(f"{digest_tool} is not installed")
        bin_dir = self.root / f"path-{digest_tool}"
        bin_dir.mkdir()
        for name in (*SCRIPT_TOOLS, digest_tool):
            (bin_dir / name).symlink_to(shutil.which(name))
        (bin_dir / "curl").symlink_to(self.stub_bin / "curl")
        return str(bin_dir)

    def reset(self):
        shutil.rmtree(self.tool_cache)
        self.tool_cache.mkdir()
        self.stub_log.unlink(missing_ok=True)
        self.github_path.write_text("")

    def install(self, expected_digest, path=None):
        env = {
            "PATH": path or f"{self.stub_bin}{os.pathsep}{os.environ['PATH']}",
            "VERSION": "0.1.79",
            "SHA256_LINUX_X64": expected_digest,
            "SHA256_LINUX_ARM64": "",
            "SHA256_MACOS_ARM64": "",
            "RELEASES": "https://github.com/tailrocks/velnor-new/releases/download",
            "RUNNER_OS": "Linux",
            "RUNNER_ARCH": "X64",
            "RUNNER_TEMP": str(self.runner_temp),
            "RUNNER_TOOL_CACHE": str(self.tool_cache),
            "GITHUB_PATH": str(self.github_path),
            "STUB_PAYLOAD": str(self.payload),
            "STUB_LOG": str(self.stub_log),
        }
        result = subprocess.run(
            ["bash", "-c", install_script()],
            env=env,
            capture_output=True,
            text=True,
            timeout=120,
        )
        result.output = result.stdout + result.stderr
        return result

    def curl_calls(self):
        if not self.stub_log.exists():
            return []
        return self.stub_log.read_text().splitlines()

    def test_a_binary_whose_sha256_differs_from_the_pinned_one_is_refused(self):
        wrong_digest = hashlib.sha256(b"some other binary").hexdigest()
        for tool in DIGEST_TOOLS:
            with self.subTest(tool=tool):
                self.reset()
                result = self.install(wrong_digest, self.path_with(tool))
                self.assertEqual(result.returncode, 1, result.output)
                self.assertIn("SHA-256 mismatch for velnor-ci 0.1.79 linux-x64", result.output)
                self.assertIn("velnor-ci was not installed", result.output)
                self.assertEqual(len(self.curl_calls()), 1)
                self.assertTrue(self.curl_calls()[0].endswith(RELEASE_URL), self.curl_calls())
                self.assertEqual(installed_files(self.tool_cache), [], result.output)
                self.assertEqual(self.github_path.read_text(), "")

    def test_a_cached_binary_whose_sha256_differs_is_not_used(self):
        cached = self.tool_cache / "velnor-ci" / "0.1.79" / "x64" / "velnor-ci"
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"tampered binary\n")
        cached.chmod(0o755)
        result = self.install(hashlib.sha256(PAYLOAD).hexdigest())
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(len(self.curl_calls()), 1)
        self.assertEqual(cached.read_bytes(), PAYLOAD)

    def test_a_binary_with_the_pinned_sha256_is_installed_and_put_on_path(self):
        for tool in DIGEST_TOOLS:
            with self.subTest(tool=tool):
                self.reset()
                result = self.install(hashlib.sha256(PAYLOAD).hexdigest(), self.path_with(tool))
                self.assertEqual(result.returncode, 0, result.output)
                installed = self.tool_cache / "velnor-ci" / "0.1.79" / "x64" / "velnor-ci"
                self.assertEqual(installed.read_bytes(), PAYLOAD)
                bin_dir = Path(self.github_path.read_text().strip())
                self.assertEqual((bin_dir / "velnor-ci").read_bytes(), PAYLOAD)


if __name__ == "__main__":
    unittest.main()
