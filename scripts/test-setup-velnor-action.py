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
SCRIPT_TOOLS = ("bash", "cut", "tr", "mktemp", "rm", "cp", "mkdir", "chmod", "mv", "wc")

STUB_CURL = """#!/bin/sh
printf '%s\\n' "$*" >> "$STUB_LOG"
out=
max=
while [ $# -gt 0 ]; do
  if [ "$1" = "--output" ]; then out="$2"; fi
  if [ "$1" = "--max-filesize" ] && [ -z "$STUB_IGNORE_MAX" ]; then max="$2"; fi
  shift
done
if [ -n "$max" ] && [ "$(wc -c < "$STUB_PAYLOAD")" -gt "$max" ]; then
  echo "curl: (63) Maximum file size exceeded" >&2
  exit 63
fi
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

    def install(self, expected_digest, path=None, extra_env=None, script=None):
        env = {
            **(extra_env or {}),
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
            ["bash", "-c", script or install_script()],
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

    def test_a_download_above_the_size_limit_is_refused_before_it_is_stored(self):
        with self.payload.open("r+b") as handle:
            handle.truncate(268435457)
        result = self.install(hashlib.sha256(PAYLOAD).hexdigest())
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("larger than 268435456 bytes", result.output)
        self.assertIn("--max-filesize 268435456", self.curl_calls()[0])
        self.assertEqual(installed_files(self.tool_cache), [], result.output)
        self.assertEqual(self.github_path.read_text(), "")
        self.assertEqual(installed_files(self.runner_temp), [], result.output)

    def test_a_download_above_the_size_limit_is_refused_when_curl_ignores_the_cap(self):
        with self.payload.open("r+b") as handle:
            handle.truncate(268435457)
        result = self.install(
            hashlib.sha256(PAYLOAD).hexdigest(), extra_env={"STUB_IGNORE_MAX": "1"}
        )
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("download is larger than 268435456 bytes", result.output)
        self.assertEqual(installed_files(self.tool_cache), [], result.output)
        self.assertEqual(self.github_path.read_text(), "")

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

    def test_a_read_only_tool_cache_does_not_stop_the_install(self):
        self.tool_cache.chmod(0o500)
        self.addCleanup(self.tool_cache.chmod, 0o700)
        if os.access(self.tool_cache, os.W_OK):
            self.skipTest("the tool cache is writable despite mode 0500")
        result = self.install(hashlib.sha256(PAYLOAD).hexdigest())
        self.assertEqual(result.returncode, 0, result.output)
        self.assertIn("could not publish velnor-ci 0.1.79 to the tool cache", result.output)
        bin_dir = Path(self.github_path.read_text().strip())
        self.assertEqual((bin_dir / "velnor-ci").read_bytes(), PAYLOAD)

    def test_a_failed_copy_into_the_tool_cache_publishes_nothing_and_warns(self):
        bin_dir = Path(self.path_with("sha256sum"))
        real_cp = shutil.which("cp")
        (bin_dir / "cp").unlink()
        (bin_dir / "cp").write_text(
            f'#!/bin/sh\nfor a in "$@"; do case "$a" in */.velnor-ci.*) exit 1 ;; esac; done\nexec {real_cp} "$@"\n'
        )
        (bin_dir / "cp").chmod(0o755)
        result = self.install(hashlib.sha256(PAYLOAD).hexdigest(), str(bin_dir))
        self.assertEqual(result.returncode, 0, result.output)
        self.assertIn("could not publish velnor-ci 0.1.79 to the tool cache", result.output)
        self.assertEqual(
            [path for path in installed_files(self.tool_cache) if path.name == "velnor-ci"], []
        )
        bin_path = Path(self.github_path.read_text().strip())
        self.assertEqual((bin_path / "velnor-ci").read_bytes(), PAYLOAD)

    LOCAL_PAYLOAD = b"a velnor-ci built from the working tree\n"

    def local_setup(self, binary=LOCAL_PAYLOAD, mounts=("velnor-ci", ".local-mode"), options="ro,relatime"):
        local_dir = self.root / "opt-velnor-local"
        shutil.rmtree(local_dir, ignore_errors=True)
        local_dir.mkdir()
        if binary is not None:
            (local_dir / "velnor-ci").write_bytes(binary)
            (local_dir / "velnor-ci").chmod(0o755)
        (local_dir / ".local-mode").write_text("velnor-local\n")
        mountinfo = self.root / "mountinfo"
        mountinfo.write_text(
            "".join(f"36 35 98:0 / {local_dir}/{name} {options} - virtiofs host rw\n" for name in mounts)
        )
        script = install_script()
        for literal in ("local_dir=/opt/velnor/local", "local_mounts=/proc/self/mountinfo"):
            self.assertEqual(script.count(literal), 1, literal)
        script = script.replace("/opt/velnor/local", str(local_dir)).replace(
            "/proc/self/mountinfo", str(mountinfo)
        )
        return local_dir, mountinfo, script

    def local_install(self, expected_digest, script, extra_env=None):
        env = {"VELNOR_LOCAL_MODE": "1", "GITHUB_STEP_SUMMARY": str(self.root / "summary"), **(extra_env or {})}
        return self.install(expected_digest, script=script, extra_env=env)

    def assert_release_installed(self, result):
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(len(self.curl_calls()), 1, result.output)
        bin_dir = Path(self.github_path.read_text().strip())
        self.assertEqual((bin_dir / "velnor-ci").read_bytes(), PAYLOAD)

    def test_local_mode_installs_the_mounted_binary_loudly_without_download_or_tool_cache(self):
        _, _, script = self.local_setup()
        pinned = hashlib.sha256(PAYLOAD).hexdigest()
        result = self.local_install(pinned, script)
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(self.curl_calls(), [])
        bin_dir = Path(self.github_path.read_text().strip())
        self.assertEqual((bin_dir / "velnor-ci").read_bytes(), self.LOCAL_PAYLOAD)
        local_digest = hashlib.sha256(self.LOCAL_PAYLOAD).hexdigest()
        self.assertIn("::warning title=setup-velnor::using the local velnor-ci " + local_digest, result.output)
        self.assertIn(local_digest, (self.root / "summary").read_text())
        self.assertEqual(installed_files(self.tool_cache), [])

    def test_local_mode_without_both_read_only_mounts_verifies_the_release_as_before(self):
        pinned = hashlib.sha256(PAYLOAD).hexdigest()
        cases = {
            "no mounts": dict(mounts=()),
            "only the binary": dict(mounts=("velnor-ci",)),
            "only the marker": dict(mounts=(".local-mode",)),
            "writable mounts": dict(options="rw,relatime"),
        }
        for name, kwargs in cases.items():
            with self.subTest(name):
                self.reset()
                _, _, script = self.local_setup(**kwargs)
                forged = {"VELNOR_LOCAL_CI_BINARY": str(self.root / "opt-velnor-local" / "velnor-ci")}
                self.assert_release_installed(self.local_install(pinned, script, forged))
                self.reset()
                wrong = self.local_install(hashlib.sha256(b"x").hexdigest(), script)
                self.assertEqual(wrong.returncode, 1, wrong.output)
                self.assertIn("SHA-256 mismatch", wrong.output)
                self.assertEqual(self.github_path.read_text(), "")

    def test_the_last_mount_at_a_path_decides_whether_it_is_read_only(self):
        _, mountinfo, script = self.local_setup()
        local_dir = self.root / "opt-velnor-local"
        with mountinfo.open("a") as handle:
            handle.write(f"37 35 98:0 / {local_dir}/velnor-ci rw,relatime - virtiofs host rw\n")
        self.assert_release_installed(self.local_install(hashlib.sha256(PAYLOAD).hexdigest(), script))

    def test_a_mounted_file_that_is_not_a_plain_executable_within_the_limit_is_not_installed(self):
        pinned = hashlib.sha256(PAYLOAD).hexdigest()
        for name in ("symlink", "directory", "not executable", "oversize"):
            with self.subTest(name):
                self.reset()
                local_dir, _, script = self.local_setup(binary=None)
                target = local_dir / "velnor-ci"
                if name == "symlink":
                    real = self.root / "real"
                    real.write_bytes(self.LOCAL_PAYLOAD)
                    real.chmod(0o755)
                    target.symlink_to(real)
                elif name == "directory":
                    target.mkdir()
                else:
                    target.write_bytes(self.LOCAL_PAYLOAD)
                    target.chmod(0o644 if name == "not executable" else 0o755)
                    if name == "oversize":
                        with target.open("r+b") as handle:
                            handle.truncate(268435457)
                self.assert_release_installed(self.local_install(pinned, script))

    def test_local_mode_still_requires_the_pinned_sha256_input(self):
        _, _, script = self.local_setup()
        result = self.local_install("", script)
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("no SHA-256 was given for linux-x64", result.output)
        self.assertEqual(self.github_path.read_text(), "")
        self.assertEqual(self.curl_calls(), [])


if __name__ == "__main__":
    unittest.main()
