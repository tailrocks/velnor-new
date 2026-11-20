# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import io
import os
import re
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest


ACTION = Path(__file__).resolve().parent.parent / "actions" / "setup-velnor" / "action.yml"
RELEASES = "https://github.com/tailrocks/velnor-new/releases/download"
RELEASE_URL = f"{RELEASES}/tools-v0.1.79/velnor-ci-x86_64-unknown-linux-musl.tar.gz"
LEGACY_RELEASE_URL = f"{RELEASES}/rust-ci-v0.1.79/velnor-ci-x86_64-unknown-linux-musl.tar.gz"
PAYLOAD = b"the released velnor-ci binary\n"
DIGEST_TOOLS = ("sha256sum", "shasum")
SCRIPT_TOOLS = ("bash", "cut", "tr", "mktemp", "rm", "cp", "mkdir", "chmod", "mv", "wc", "gzip", "head", "sleep", "seq", "env", "sync")
ARCHIVE = "velnor-ci-x86_64-unknown-linux-musl.tar.gz"
PLATFORMS = (
    ("Linux", "X64", "x86_64-unknown-linux-musl", "SHA256_LINUX_X64"),
    ("Linux", "ARM64", "aarch64-unknown-linux-musl", "SHA256_LINUX_ARM64"),
)

STUB_CURL = """#!/bin/sh
printf '%s\\n' "$*" >> "$STUB_LOG"
out=
max=
for last; do :; done
for missing in $STUB_MISSING; do
  case "$last" in */"$missing"/*) echo "curl: (22) The requested URL returned error: 404" >&2; exit 22 ;; esac
done
while [ $# -gt 0 ]; do
  if [ "$1" = "--output" ]; then out="$2"; fi
  if [ "$1" = "--max-filesize" ] && [ -z "$STUB_IGNORE_MAX" ]; then max="$2"; fi
  shift
done
if [ -n "$max" ] && [ "$(wc -c < "$STUB_PAYLOAD")" -gt "$max" ]; then
  echo "curl: (63) Maximum file size exceeded" >&2
  exit 63
fi
for missing in $STUB_MISSING; do
  case "$*" in */"$missing"/*) echo "curl: (22) The requested URL returned error: 404" >&2; exit 22 ;; esac
done
if [ -n "$STUB_FIRST_PAYLOAD" ] && [ ! -e "$STUB_LOG.served" ]; then
  : > "$STUB_LOG.served"
  cp "$STUB_FIRST_PAYLOAD" "$out"
  exit 0
fi
cp "$STUB_PAYLOAD" "$out"
"""

STUB_MV = """#!/bin/sh
printf 'mv %s\\n' "$*" >> "$STUB_ORDER_LOG"
exec "$REAL_MV" "$@"
"""

STUB_SYNC = """#!/bin/sh
printf 'sync %s\\n' "$*" >> "$STUB_ORDER_LOG"
"""

STUB_TAR = """#!/bin/sh
if [ -p /dev/stdin ]; then pipe=pipe; else pipe=none; fi
printf '%s\\t%s\\n' "$pipe" "$*" >> "$STUB_TAR_LOG"
exec "$REAL_TAR" "$@"
"""

STUB_SLOW_HEAD = """#!/bin/sh
/bin/sleep 1
exec "$REAL_HEAD" "$@"
"""


def archive_bytes(members=None):
    members = [("velnor-ci", PAYLOAD, 0o755)] if members is None else members
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
        for name, data, mode in members:
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = mode
            archive.addfile(info, io.BytesIO(data))
    return buffer.getvalue()


def sha256(data):
    return hashlib.sha256(data).hexdigest()


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


def attempt_logs(output):
    pattern = r"(\w+) attempt (\d+): expected SHA-256 ([0-9a-f]{64}), received ([0-9a-f]{64}) \((\d+) bytes\)"
    return [
        (source, int(number), expected, received, int(size))
        for source, number, expected, received, size in re.findall(pattern, output)
    ]


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
        self.tar_log = self.root / "tar-calls.log"
        for directory in (self.stub_bin, self.runner_temp, self.tool_cache):
            directory.mkdir()
        self.github_path.touch()
        self.archive = archive_bytes()
        self.payload.write_bytes(self.archive)
        for name, body in (("curl", STUB_CURL), ("tar", STUB_TAR)):
            stub = self.stub_bin / name
            stub.write_text(body)
            stub.chmod(0o755)

    def path_with(self, digest_tool):
        if shutil.which(digest_tool) is None:
            self.skipTest(f"{digest_tool} is not installed")
        bin_dir = self.root / f"path-{digest_tool}"
        bin_dir.mkdir()
        for name in (*SCRIPT_TOOLS, digest_tool):
            (bin_dir / name).symlink_to(shutil.which(name))
        (bin_dir / "curl").symlink_to(self.stub_bin / "curl")
        (bin_dir / "tar").symlink_to(self.stub_bin / "tar")
        return str(bin_dir)

    def reset(self):
        shutil.rmtree(self.tool_cache)
        self.tool_cache.mkdir()
        self.stub_log.unlink(missing_ok=True)
        Path(f"{self.stub_log}.served").unlink(missing_ok=True)
        self.tar_log.unlink(missing_ok=True)
        self.github_path.write_text("")

    def install(self, expected_digest, path=None, extra_env=None, runner=("Linux", "X64")):
        digest_variable = next(
            (variable for os_name, arch, _, variable in PLATFORMS if (os_name, arch) == runner),
            "SHA256_LINUX_X64",
        )
        env = {
            **(extra_env or {}),
            "PATH": path or f"{self.stub_bin}{os.pathsep}{os.environ['PATH']}",
            "VERSION": "0.1.79",
            "SHA256_LINUX_X64": "",
            "SHA256_LINUX_ARM64": "",
            digest_variable: expected_digest,
            "RELEASES": "https://github.com/tailrocks/velnor-new/releases/download",
            "RUNNER_OS": runner[0],
            "RUNNER_ARCH": runner[1],
            "RUNNER_TEMP": str(self.runner_temp),
            "RUNNER_TOOL_CACHE": str(self.tool_cache),
            "GITHUB_PATH": str(self.github_path),
            "STUB_PAYLOAD": str(self.payload),
            "STUB_LOG": str(self.stub_log),
            "STUB_TAR_LOG": str(self.tar_log),
            "REAL_TAR": shutil.which("tar"),
            "REAL_HEAD": shutil.which("head"),
        }
        result = subprocess.run(
            ["bash", "-c", install_script()],
            env=env,
            stdin=subprocess.DEVNULL,
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

    def tar_calls(self):
        if not self.tar_log.exists():
            return []
        return self.tar_log.read_text().splitlines()

    def extract_calls(self):
        return [call for call in self.tar_calls() if "--extract" in call.split("\t", 1)[1].split()]

    def cached_archive(self):
        return self.tool_cache / "velnor-ci" / "0.1.79" / "x64" / ARCHIVE

    def installed_binary(self):
        bin_dir = Path(self.github_path.read_text().strip())
        return bin_dir / "velnor-ci"

    def test_each_runner_downloads_the_archive_named_after_its_rust_target_triple(self):
        for os_name, arch, triple, _ in PLATFORMS:
            with self.subTest(runner=f"{os_name}-{arch}"):
                self.reset()
                result = self.install(sha256(self.archive), runner=(os_name, arch))
                self.assertEqual(result.returncode, 0, result.output)
                calls = self.curl_calls()
                self.assertEqual(len(calls), 1, calls)
                self.assertTrue(
                    calls[0].endswith(
                        f"/tools-v0.1.79/velnor-ci-{triple}.tar.gz"
                    ),
                    calls,
                )
                self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_runner_without_a_build_downloads_nothing(self):
        result = self.install(sha256(self.archive), runner=("Windows", "X64"))
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("velnor-ci has no build for Windows X64", result.output)
        self.assertEqual(self.curl_calls(), [])

    def test_an_archive_whose_sha256_differs_from_the_pinned_one_is_refused_and_not_unpacked(self):
        wrong_digest = sha256(b"some other archive")
        for tool in DIGEST_TOOLS:
            with self.subTest(tool=tool):
                self.reset()
                result = self.install(wrong_digest, self.path_with(tool))
                self.assertEqual(result.returncode, 1, result.output)
                self.assertIn(
                    "SHA-256 mismatch for velnor-ci 0.1.79 linux-x64", result.output
                )
                self.assertIn("velnor-ci was not installed", result.output)
                self.assertEqual(len(self.curl_calls()), 2, self.curl_calls())
                self.assertTrue(all(call.endswith(RELEASE_URL) for call in self.curl_calls()))
                self.assertEqual(self.tar_calls(), [])
                self.assertEqual(installed_files(self.tool_cache), [], result.output)
                self.assertEqual(self.github_path.read_text(), "")

    def test_the_digest_of_the_extracted_binary_is_not_accepted_as_the_archive_digest(self):
        result = self.install(sha256(PAYLOAD))
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("SHA-256 mismatch", result.output)
        self.assertEqual(self.tar_calls(), [])
        self.assertEqual(self.github_path.read_text(), "")

    def test_a_cached_archive_whose_sha256_differs_is_not_used(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"tampered archive\n")
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(len(self.curl_calls()), 1)
        self.assertEqual(cached.read_bytes(), self.archive)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_mismatched_cached_archive_in_a_read_only_tool_cache_still_downloads(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"tampered archive\n")
        cached.parent.chmod(0o500)
        self.addCleanup(cached.parent.chmod, 0o700)
        if os.access(cached.parent, os.W_OK):
            self.skipTest("the tool cache is writable despite mode 0500")
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(len(self.curl_calls()), 1)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_cached_archive_with_the_pinned_sha256_is_unpacked_without_a_download(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(self.archive)
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(self.curl_calls(), [])
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_the_archive_is_extracted_by_one_tar_command_that_reads_a_file_not_a_pipe(self):
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 0, result.output)
        calls = self.extract_calls()
        self.assertEqual(len(calls), 1, calls)
        pipe, arguments = calls[0].split("\t", 1)
        self.assertEqual(pipe, "none", "tar read its archive from a pipe")
        words = arguments.split()
        self.assertIn("--extract", words)
        self.assertIn("--file", words)
        self.assertEqual(words[-1], "velnor-ci")
        self.assertIn(ARCHIVE, words[words.index("--file") + 1])

    def test_the_archive_is_extracted_with_no_recursion(self):
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 0, result.output)
        self.assertIn("--no-recursion", self.extract_calls()[0].split("\t", 1)[1].split())

    def test_an_archive_with_two_velnor_ci_members_is_refused(self):
        self.archive = archive_bytes(
            [("velnor-ci", PAYLOAD, 0o755), ("velnor-ci", b"second copy\n", 0o755)]
        )
        self.payload.write_bytes(self.archive)
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("velnor-ci was not installed", result.output)
        self.assertEqual(installed_files(self.tool_cache), [], result.output)
        self.assertEqual(self.github_path.read_text(), "")

    def test_an_exported_tar_options_cannot_change_what_is_extracted(self):
        result = self.install(
            sha256(self.archive), extra_env={"TAR_OPTIONS": "--exclude=velnor-ci"}
        )
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_only_the_velnor_ci_member_of_the_archive_is_extracted(self):
        self.archive = archive_bytes(
            [("velnor-ci", PAYLOAD, 0o755), ("planted", b"planted\n", 0o755)]
        )
        self.payload.write_bytes(self.archive)
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)
        self.assertEqual(
            sorted(path.name for path in installed_files(self.runner_temp)), ["velnor-ci"]
        )

    def test_an_archive_without_a_velnor_ci_member_is_refused(self):
        self.archive = archive_bytes([("velnor-gen", PAYLOAD, 0o755)])
        self.payload.write_bytes(self.archive)
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("velnor-ci was not installed", result.output)
        self.assertEqual(installed_files(self.tool_cache), [], result.output)
        self.assertEqual(self.github_path.read_text(), "")

    def test_an_archive_whose_velnor_ci_member_is_a_symlink_is_refused(self):
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
            link = tarfile.TarInfo("velnor-ci")
            link.type = tarfile.SYMTYPE
            link.linkname = "/etc/hostname"
            archive.addfile(link)
        self.archive = buffer.getvalue()
        self.payload.write_bytes(self.archive)
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("velnor-ci was not installed", result.output)
        self.assertEqual(self.github_path.read_text(), "")

    def test_a_binary_that_unpacks_above_the_size_limit_is_refused(self):
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w:gz", compresslevel=1) as archive:
            info = tarfile.TarInfo("velnor-ci")
            info.size = 268435457
            info.mode = 0o755

            class Zeros:
                remaining = info.size

                def read(self, count=-1):
                    count = self.remaining if count < 0 else min(count, self.remaining)
                    self.remaining -= count
                    return bytes(count)

            archive.addfile(info, Zeros())
        self.archive = buffer.getvalue()
        self.assertLess(len(self.archive), 268435456)
        self.payload.write_bytes(self.archive)
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("velnor-ci was not installed", result.output)
        self.assertEqual(self.github_path.read_text(), "")
        self.assertEqual(installed_files(self.tool_cache), [], result.output)

    def test_a_binary_on_a_slow_disk_is_installed_not_refused_as_too_large(self):
        slow = self.stub_bin / "head"
        slow.write_text(STUB_SLOW_HEAD)
        slow.chmod(0o755)
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_download_above_the_size_limit_is_refused_before_it_is_stored(self):
        with self.payload.open("r+b") as handle:
            handle.truncate(268435457)
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("larger than 268435456 bytes", result.output)
        self.assertIn("--max-filesize 268435456", self.curl_calls()[0])
        self.assertEqual(installed_files(self.tool_cache), [], result.output)
        self.assertEqual(self.github_path.read_text(), "")
        self.assertEqual(installed_files(self.runner_temp), [], result.output)

    def test_a_download_above_the_size_limit_is_refused_when_curl_ignores_the_cap(self):
        with self.payload.open("r+b") as handle:
            handle.truncate(268435457)
        result = self.install(sha256(self.archive), extra_env={"STUB_IGNORE_MAX": "1"})
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("is larger than 268435456 bytes", result.output)
        self.assertEqual(installed_files(self.tool_cache), [], result.output)
        self.assertEqual(self.github_path.read_text(), "")

    def test_an_archive_with_the_pinned_sha256_puts_an_executable_velnor_ci_on_path(self):
        for tool in DIGEST_TOOLS:
            with self.subTest(tool=tool):
                self.reset()
                result = self.install(sha256(self.archive), self.path_with(tool))
                self.assertEqual(result.returncode, 0, result.output)
                self.assertEqual(self.cached_archive().read_bytes(), self.archive)
                binary = self.installed_binary()
                self.assertEqual(binary.read_bytes(), PAYLOAD)
                self.assertTrue(os.access(binary, os.X_OK))
                self.assertFalse(binary.is_symlink())

    def test_the_tools_v_tag_is_tried_first_and_the_legacy_tag_is_not_touched_when_it_exists(self):
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 0, result.output)
        calls = self.curl_calls()
        self.assertEqual(len(calls), 1, calls)
        self.assertTrue(calls[0].endswith(RELEASE_URL), calls)

    def test_a_release_with_only_the_legacy_rust_ci_v_tag_is_installed_after_the_tools_v_tag_is_missing(self):
        result = self.install(sha256(self.archive), extra_env={"STUB_MISSING": "tools-v0.1.79"})
        self.assertEqual(result.returncode, 0, result.output)
        calls = self.curl_calls()
        self.assertEqual(len(calls), 2, calls)
        self.assertTrue(calls[0].endswith(RELEASE_URL), calls)
        self.assertTrue(calls[1].endswith(LEGACY_RELEASE_URL), calls)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_an_archive_from_the_legacy_tag_is_still_checked_against_the_pinned_sha256(self):
        result = self.install(
            sha256(b"some other archive"), extra_env={"STUB_MISSING": "tools-v0.1.79"}
        )
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("SHA-256 mismatch for velnor-ci 0.1.79 linux-x64", result.output)
        self.assertEqual(installed_files(self.tool_cache), [], result.output)
        self.assertEqual(self.github_path.read_text(), "")

    def test_a_version_with_neither_tag_fails_naming_both(self):
        result = self.install(
            sha256(self.archive),
            extra_env={"STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79"},
        )
        self.assertEqual(result.returncode, 1, result.output)
        requested = [call.rsplit(" ", 1)[-1] for call in self.curl_calls()]
        self.assertEqual(requested, [RELEASE_URL, LEGACY_RELEASE_URL])
        self.assertIn("downloading velnor-ci", result.output)
        self.assertIn("tools-v0.1.79", result.output)
        self.assertIn("rust-ci-v0.1.79", result.output)
        self.assertEqual(installed_files(self.tool_cache), [], result.output)
        self.assertEqual(self.github_path.read_text(), "")

    def test_a_read_only_tool_cache_does_not_stop_the_install(self):
        self.tool_cache.chmod(0o500)
        self.addCleanup(self.tool_cache.chmod, 0o700)
        if os.access(self.tool_cache, os.W_OK):
            self.skipTest("the tool cache is writable despite mode 0500")
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 0, result.output)
        self.assertIn("could not publish velnor-ci 0.1.79 to the tool cache", result.output)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_failed_copy_into_the_tool_cache_publishes_nothing_and_warns(self):
        bin_dir = Path(self.path_with("sha256sum"))
        real_cp = shutil.which("cp")
        (bin_dir / "cp").unlink()
        (bin_dir / "cp").write_text(
            f'#!/bin/sh\nfor a in "$@"; do case "$a" in */.velnor-ci.*) exit 1 ;; esac; done\nexec {real_cp} "$@"\n'
        )
        (bin_dir / "cp").chmod(0o755)
        result = self.install(sha256(self.archive), str(bin_dir))
        self.assertEqual(result.returncode, 0, result.output)
        self.assertIn("could not publish velnor-ci 0.1.79 to the tool cache", result.output)
        self.assertEqual(
            [path for path in installed_files(self.tool_cache) if path.name == ARCHIVE], []
        )
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_download_that_mismatches_once_is_retried_into_a_fresh_file_and_installed(self):
        bad = archive_bytes([("velnor-ci", b"corrupted in transit\n", 0o755)])
        first = self.root / "first-payload"
        first.write_bytes(bad)
        result = self.install(sha256(self.archive), extra_env={"STUB_FIRST_PAYLOAD": str(first)})
        self.assertEqual(result.returncode, 0, result.output)
        calls = self.curl_calls()
        self.assertEqual(len(calls), 2, calls)
        self.assertTrue(all(call.endswith(RELEASE_URL) for call in calls), calls)
        outputs = [call.split()[call.split().index("--output") + 1] for call in calls]
        self.assertNotEqual(outputs[0], outputs[1], "the retry reused the first download's file")
        self.assertEqual(
            attempt_logs(result.output),
            [
                ("download", 1, sha256(self.archive), sha256(bad), len(bad)),
                ("download", 2, sha256(self.archive), sha256(self.archive), len(self.archive)),
            ],
            result.output,
        )
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)
        self.assertEqual(self.cached_archive().read_bytes(), self.archive)

    def test_a_download_that_mismatches_twice_is_fatal_and_logs_both_attempts(self):
        expected = sha256(b"some other archive")
        result = self.install(expected)
        self.assertEqual(result.returncode, 1, result.output)
        self.assertEqual(len(self.curl_calls()), 2, self.curl_calls())
        self.assertEqual(
            attempt_logs(result.output),
            [
                ("download", 1, expected, sha256(self.archive), len(self.archive)),
                ("download", 2, expected, sha256(self.archive), len(self.archive)),
            ],
            result.output,
        )
        self.assertEqual(self.tar_calls(), [])
        self.assertEqual(self.github_path.read_text(), "")

    def test_a_truncated_stored_archive_is_ignored_and_replaced_by_a_fresh_download(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(self.archive[: len(self.archive) // 2])
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(len(self.curl_calls()), 1, self.curl_calls())
        self.assertEqual(cached.read_bytes(), self.archive)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)
        leftovers = [path for path in installed_files(cached.parent) if path.name.startswith(".velnor-ci.")]
        self.assertEqual(leftovers, [], result.output)

    def test_a_stored_archive_that_mismatches_is_removed_even_when_the_download_is_also_bad(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"tampered archive\n")
        result = self.install(sha256(b"some other archive"))
        self.assertEqual(result.returncode, 1, result.output)
        self.assertFalse(cached.exists(), "a mismatched store entry was left for the next job")

    def test_a_store_write_is_flushed_to_disk_before_it_is_renamed_into_the_tool_cache(self):
        order = self.root / "order.log"
        for name, body in (("mv", STUB_MV), ("sync", STUB_SYNC)):
            stub = self.stub_bin / name
            stub.write_text(body)
            stub.chmod(0o755)
        result = self.install(
            sha256(self.archive),
            extra_env={"STUB_ORDER_LOG": str(order), "REAL_MV": shutil.which("mv")},
        )
        self.assertEqual(result.returncode, 0, result.output)
        lines = order.read_text().splitlines()
        cache_dir = str(self.cached_archive().parent)
        renames = [i for i, line in enumerate(lines) if line.startswith("mv ") and cache_dir in line]
        self.assertEqual(len(renames), 1, lines)
        temp = lines[renames[0]].split()[-2]
        self.assertEqual(Path(temp).parent, Path(cache_dir), "the temp file is not beside the store entry")
        flushes = [i for i, line in enumerate(lines) if line == f"sync {temp}"]
        self.assertTrue(flushes and flushes[0] < renames[0], lines)


if __name__ == "__main__":
    unittest.main()
