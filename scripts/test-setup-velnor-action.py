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
import time
import unittest


ACTION = Path(__file__).resolve().parent.parent / "actions" / "setup-velnor" / "action.yml"
RELEASES = "https://github.com/tailrocks/velnor-new/releases/download"
RELEASE_URL = f"{RELEASES}/tools-v0.1.79/velnor-ci-x86_64-unknown-linux-musl.tar.gz"
LEGACY_RELEASE_URL = f"{RELEASES}/rust-ci-v0.1.79/velnor-ci-x86_64-unknown-linux-musl.tar.gz"
PAYLOAD = b"the released velnor-ci binary\n"
DIGEST_TOOLS = ("sha256sum", "shasum")
SCRIPT_TOOLS = ("bash", "cut", "tr", "mktemp", "rm", "cp", "mkdir", "chmod", "mv", "wc", "gzip", "head", "sleep", "seq", "env", "sync", "find", "uname", "sort", "stat", "touch")
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
if [ $# -gt 0 ]; then cat "$1" > "$STUB_SYNCED"; fi
"""

STUB_RACING_MV = """#!/bin/sh
if [ ! -e "$STUB_RACE_MARK" ]; then
  for arg; do
    case "$arg" in
      */velnor-ci-x86_64-unknown-linux-musl.tar.gz)
        : > "$STUB_RACE_MARK"; cp "$STUB_RACE" "$arg"; break ;;
    esac
  done
fi
exec "$REAL_MV" "$@"
"""

STUB_PLANTING_MV = """#!/bin/sh
if [ ! -e "$STUB_RACE_MARK" ]; then
  for arg; do
    case "$arg" in
      */velnor-ci-x86_64-unknown-linux-musl.tar.gz)
        : > "$STUB_RACE_MARK"; mkdir -p "$STUB_TARGET"; ln -s "$STUB_TARGET" "$arg"; break ;;
    esac
  done
  for arg; do src="$dst"; dst="$arg"; done
  exec "$REAL_MV" -f "$src" "$dst"
fi
exec "$REAL_MV" "$@"
"""

STUB_REPLANTING_RM = """#!/bin/sh
for arg; do last="$arg"; done
"$REAL_RM" "$@" || exit $?
case "$last" in */velnor-ci-x86_64-unknown-linux-musl.tar.gz) mkdir -p "$last" ;; esac
"""

STUB_REPLANTING_EVICT_MV = """#!/bin/sh
for arg; do last="$arg"; done
"$REAL_MV" "$@" || exit $?
case "$last" in
  *.velnor-ci.evict.*)
    if [ ! -e "$STUB_RACE_MARK" ]; then
      : > "$STUB_RACE_MARK"
      mkdir -p "$STUB_ENTRY"
    fi ;;
esac
"""

STUB_GARBAGE_FIRST_HEAD = """#!/bin/sh
if [ ! -e "$STUB_HEAD_MARK" ]; then
  : > "$STUB_HEAD_MARK"
  printf 'garbage\\n'
  exit 0
fi
exec "$REAL_HEAD" "$@"
"""

STUB_PLANT_PID_QUARANTINE_MV = """#!/bin/sh
for arg; do last="$arg"; done
case "$last" in
  */.velnor-ci.evict.*)
    if [ ! -e "$STUB_RACE_MARK" ]; then
      : > "$STUB_RACE_MARK"
      cp "$STUB_RACE" "${last%/*}/.velnor-ci.evict.$PPID"
    fi ;;
esac
exec "$REAL_MV" "$@"
"""

STUB_PLANT_QUARANTINE_MV = """#!/bin/sh
for arg; do src="$dst"; dst="$arg"; done
"$REAL_MV" "$@" || exit $?
case "$dst" in
  */.velnor-ci.evict.*)
    if [ ! -e "$STUB_RACE_MARK" ]; then
      : > "$STUB_RACE_MARK"
      cp -p "$STUB_RACE" "$dst"
      cp -p "$STUB_RACE" "$src"
    fi ;;
esac
"""

STUB_TAR = """#!/bin/sh
if [ -p /dev/stdin ]; then pipe=pipe; else pipe=none; fi
printf '%s\\t%s\\n' "$pipe" "$*" >> "$STUB_TAR_LOG"
exec "$REAL_TAR" "$@"
"""

STUB_DIGEST = """#!/bin/sh
printf '%s\\n' "$*" >> "$STUB_DIGEST_LOG"
if [ -n "$REAL_SHA256SUM" ]; then exec "$REAL_SHA256SUM" "$@"; fi
exec "$REAL_SHASUM" -a 256 "$@"
"""

STUB_SLOW_HEAD = """#!/bin/sh
/bin/sleep 1
exec "$REAL_HEAD" "$@"
"""

STUB_COUNTING_HEAD = """#!/bin/sh
for arg; do
  case "$arg" in */velnor-ci-x86_64-unknown-linux-musl.tar.gz) printf '%s\\n' "$2" >> "$STUB_READ_LOG" ;; esac
done
exec "$REAL_HEAD" "$@"
"""

STUB_FAILING_FIRST_HEAD = """#!/bin/sh
if [ ! -e "$STUB_HEAD_MARK" ]; then
  : > "$STUB_HEAD_MARK"
  echo "head: write error: No space left on device" >&2
  exit 1
fi
exec "$REAL_HEAD" "$@"
"""

STUB_OVERSIZE_FIRST_WC = """#!/bin/sh
if [ ! -e "$STUB_WC_MARK" ]; then
  : > "$STUB_WC_MARK"
  echo 268435457
  exit 0
fi
exec "$REAL_WC" "$@"
"""

STUB_NO_INODE_MKTEMP = """#!/bin/sh
for arg; do
  case "$arg" in *.velnor-ci.evict.*) echo "mktemp: $arg: No space left on device" >&2; exit 1 ;; esac
done
exec "$REAL_MKTEMP" "$@"
"""

STUB_FAILING_RM = """#!/bin/sh
for arg; do
  case "$arg" in *.velnor-ci.evict.*) echo "rm: $arg: Permission denied" >&2; exit 1 ;; esac
done
exec "$REAL_RM" "$@"
"""

STUB_RESTORE_RACE_MV = """#!/bin/sh
for arg; do last="$arg"; done
case "$last" in
  *.velnor-ci.evict.*)
    if [ ! -e "$STUB_RACE_MARK" ]; then
      : > "$STUB_RACE_MARK"
      for arg; do
        case "$arg" in */velnor-ci-x86_64-unknown-linux-musl.tar.gz) cp "$STUB_RACE" "$arg" ;; esac
      done
    fi ;;
  *)
    case "$*" in *.velnor-ci.evict.*) printf 'planted\\n' > "$last" ;; esac ;;
esac
exec "$REAL_MV" "$@"
"""

MV_PASS_THROUGH = """#!/bin/sh
exec "{real}" "$@"
"""

MV_COMPAT = """#!/bin/sh
case "$1 $2" in
  "-f -T" | "-n -T")
    mode=$1
    shift 2
    if [ -L "$2" ]; then
      [ "$mode" = -n ] && exit 1
      "{rm}" -f -- "$2" || exit 1
    elif [ -d "$2" ]; then
      echo "mv: cannot overwrite directory $2 with a non-directory" >&2
      exit 1
    elif [ "$mode" = -n ] && [ -e "$2" ]; then
      exit 1
    fi
    exec "{real}" "$mode" "$1" "$2" ;;
esac
exec "{real}" "$@"
"""


def host_mv_supports_no_target_directory():
    with tempfile.TemporaryDirectory() as probe:
        source, target = Path(probe) / "source", Path(probe) / "target"
        source.touch()
        target.touch()
        return subprocess.run(["mv", "-f", "-T", str(source), str(target)], capture_output=True).returncode == 0


HOST_MV_HAS_T = host_mv_supports_no_target_directory()

STUB_PLANTING_PUBLISH_MV = """#!/bin/sh
for arg; do last="$arg"; done
if [ "$last" = "$STUB_ENTRY" ] && [ ! -e "$STUB_RACE_MARK" ]; then
  : > "$STUB_RACE_MARK"
  mkdir -p "$STUB_ENTRY"
fi
exec "$REAL_MV" "$@"
"""

STUB_PLANTING_RESTORE_MV = """#!/bin/sh
for arg; do last="$arg"; done
if [ "$last" = "$STUB_ENTRY" ] && [ "$1" = -n ] && [ ! -e "$STUB_RACE_MARK" ]; then
  : > "$STUB_RACE_MARK"
  mkdir -p "$STUB_ENTRY"
fi
exec "$REAL_MV" "$@"
"""

STUB_PLANTING_EVICTION_MV = """#!/bin/sh
for arg; do last="$arg"; done
case "$last" in
  *.velnor-ci.evict.*)
    if [ ! -e "$STUB_RACE_MARK" ]; then
      : > "$STUB_RACE_MARK"
      mkdir -p -- "$last"
    fi ;;
esac
exec "$REAL_MV" "$@"
"""

STUB_PLANTING_SYMLINK_PUBLISH_MV = """#!/bin/sh
for arg; do last="$arg"; done
if [ "$last" = "$STUB_ENTRY" ] && [ ! -e "$STUB_RACE_MARK" ]; then
  : > "$STUB_RACE_MARK"
  mkdir -p "$STUB_TARGET"
  ln -s "$STUB_TARGET" "$STUB_ENTRY"
fi
exec "$REAL_MV" "$@"
"""

WATCHED_TOOLS = (
    "cat", "head", "tail", "dd", "cp", "od", "xxd", "hexdump", "sha256sum", "shasum", "tar", "gzip", "zcat",
    "wc", "awk", "sed", "grep", "base64", "strings", "openssl", "find", "mv", "rm",
)
NON_READING_TOOLS = ("find", "mv", "rm")


def watch_shim(name, real):
    return f"""#!/bin/sh
for arg; do
  case "$arg" in *"$STUB_WATCHED"*) printf '%s %s\\n' "{name}" "$*" >> "$STUB_READ_LOG" ;; esac
done
exec "{real}" "$@"
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
        self.mv_compat = self.root / "mv-compat"
        shim = MV_PASS_THROUGH if HOST_MV_HAS_T else MV_COMPAT
        self.mv_compat.write_text(shim.replace("{real}", shutil.which("mv")).replace("{rm}", shutil.which("rm")))
        self.mv_compat.chmod(0o755)
        (self.stub_bin / "mv").write_text(self.mv_compat.read_text())
        (self.stub_bin / "mv").chmod(0o755)

    def path_with(self, digest_tool):
        if shutil.which(digest_tool) is None:
            self.skipTest(f"{digest_tool} is not installed")
        bin_dir = self.root / f"path-{digest_tool}"
        bin_dir.mkdir()
        for name in (*SCRIPT_TOOLS, digest_tool):
            (bin_dir / name).symlink_to(self.mv_compat if name == "mv" else shutil.which(name))
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
            "REAL_WC": shutil.which("wc"),
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

    def test_an_oversized_store_entry_is_evicted_even_when_the_download_fails(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(self.archive)
        with cached.open("r+b") as handle:
            handle.truncate(268435457)
        result = self.install(sha256(self.archive), extra_env={"STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79"})
        self.assertEqual(result.returncode, 1, result.output)
        self.assertFalse(cached.exists(), "an oversized store entry survived the job and will be read by the next one")

    def test_an_oversized_store_entry_is_removed_without_being_read(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(self.archive)
        with cached.open("r+b") as handle:
            handle.truncate(268435457)
        stub = self.stub_bin / "sha256sum"
        stub.write_text(STUB_DIGEST)
        stub.chmod(0o755)
        head = self.stub_bin / "head"
        head.write_text(STUB_COUNTING_HEAD)
        head.chmod(0o755)
        digest_log = self.root / "digest-calls.log"
        read_log = self.root / "bytes-read.log"
        result = self.install(
            sha256(self.archive),
            extra_env={
                "STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79",
                "STUB_DIGEST_LOG": str(digest_log),
                "STUB_READ_LOG": str(read_log),
                "REAL_SHA256SUM": shutil.which("sha256sum") or "",
                "REAL_SHASUM": shutil.which("shasum") or "",
            },
        )
        self.assertEqual(result.returncode, 1, result.output)
        self.assertFalse(cached.exists(), "an oversized store entry survived the job")
        digested = digest_log.read_text().splitlines() if digest_log.exists() else []
        self.assertEqual(
            [call for call in digested if ".velnor-ci.evict." in call],
            [],
            "an oversized store entry was read in full to hash it",
        )
        requested = [int(count) for count in read_log.read_text().split()] if read_log.exists() else []
        self.assertEqual(requested, [], f"head was asked for {sum(requested)} bytes of an oversized store entry")

    def test_an_oversized_store_entry_is_not_read_by_any_command(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(self.archive)
        with cached.open("r+b") as handle:
            handle.truncate(268435457)
        read_log = self.root / "store-entry-calls.log"
        for name in WATCHED_TOOLS:
            real = str(self.mv_compat) if name == "mv" else shutil.which(name)
            if real is None:
                continue
            stub = self.stub_bin / name
            stub.write_text(watch_shim(name, real))
            stub.chmod(0o755)
        result = self.install(
            sha256(self.archive),
            extra_env={
                "STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79",
                "STUB_WATCHED": str(cached),
                "STUB_READ_LOG": str(read_log),
            },
        )
        self.assertEqual(result.returncode, 1, result.output)
        self.assertFalse(cached.exists(), "an oversized store entry survived the job")
        calls = read_log.read_text().splitlines() if read_log.exists() else []
        self.assertTrue(calls, "the eviction did not name the store entry at all, so the check saw nothing")
        reads = [call for call in calls if call.split()[0] not in NON_READING_TOOLS]
        self.assertEqual(reads, [], "a command read the bytes of an oversized store entry")

    def test_a_verified_copy_published_during_an_eviction_is_kept_for_the_next_job(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"tampered archive\n")
        stub = self.stub_bin / "mv"
        stub.write_text(STUB_RACING_MV)
        stub.chmod(0o755)
        extra_env = {
            "STUB_RACE": str(self.payload),
            "STUB_RACE_MARK": str(self.root / "race-mark"),
            "REAL_MV": str(self.mv_compat),
            "STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79",
        }
        first = self.install(sha256(self.archive), extra_env=extra_env)
        self.assertEqual(first.returncode, 1, first.output)
        self.assertTrue(cached.exists(), "a verified copy another job published was deleted")
        self.assertEqual(cached.read_bytes(), self.archive)
        self.stub_log.unlink(missing_ok=True)
        self.github_path.write_text("")
        second = self.install(sha256(self.archive), extra_env=extra_env)
        self.assertEqual(second.returncode, 0, second.output)
        self.assertEqual(self.curl_calls(), [])
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_verified_archive_retained_by_an_earlier_eviction_is_not_overwritten_by_a_later_one(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"tampered archive\n")
        stub = self.stub_bin / "mv"
        stub.write_text(STUB_PLANT_PID_QUARANTINE_MV)
        stub.chmod(0o755)
        result = self.install(
            sha256(self.archive),
            extra_env={
                "STUB_RACE": str(self.payload),
                "STUB_RACE_MARK": str(self.root / "race-mark"),
                "REAL_MV": str(self.mv_compat),
                "STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79",
            },
        )
        self.assertEqual(result.returncode, 1, result.output)
        retained = [path for path in installed_files(self.tool_cache) if path.read_bytes() == self.archive]
        self.assertTrue(retained, "a verified archive kept by an earlier eviction was overwritten and deleted")

    def test_kept_quarantines_are_pruned_to_the_newest_three_and_a_symlink_among_them_is_not_followed(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"tampered archive\n")
        stub = self.stub_bin / "mv"
        stub.write_text(STUB_PLANT_QUARANTINE_MV)
        stub.chmod(0o755)
        now = time.time()
        stale = []
        for index, age in enumerate((5000, 4000, 3000, 2000, 1000)):
            entry = cached.parent / f".velnor-ci.evict.{100 + index}.{index}"
            entry.write_bytes(b"old quarantine\n")
            os.utime(entry, (now - age, now - age))
            stale.append((entry.name, age))
        outside = self.root / "outside-target"
        outside.write_bytes(b"outside the quarantine\n")
        link = cached.parent / ".velnor-ci.evict.1.1"
        link.symlink_to(outside)
        os.utime(link, (now - 10000, now - 10000), follow_symlinks=False)
        result = self.install(
            sha256(self.archive),
            extra_env={
                "STUB_RACE": str(self.payload),
                "STUB_RACE_MARK": str(self.root / "race-mark"),
                "REAL_MV": str(self.mv_compat),
                "STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79",
            },
        )
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("is kept at", result.output)
        remaining = sorted(path.name for path in cached.parent.glob(".velnor-ci.evict.*"))
        self.assertEqual(len(remaining), 3, f"expected the newest three quarantines, found {remaining}")
        self.assertIn(".velnor-ci.evict.104.4", remaining)
        self.assertIn(".velnor-ci.evict.103.3", remaining)
        kept = [path for path in cached.parent.glob(".velnor-ci.evict.*") if path.read_bytes() == self.archive]
        self.assertEqual(len(kept), 1, "the verified quarantine written by this eviction was not kept")
        self.assertFalse(link.is_symlink(), "the stale symlink in the quarantine was not removed")
        self.assertEqual(outside.read_bytes(), b"outside the quarantine\n", "a quarantine symlink was followed")

    def test_the_archive_kept_by_an_eviction_is_not_pruned_as_its_own_oldest_quarantine(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"tampered archive\n")
        now = time.time()
        os.utime(self.payload, (now - 10000, now - 10000))
        for index, age in enumerate((100, 200, 300)):
            entry = cached.parent / f".velnor-ci.evict.{100 + index}.{index}"
            entry.write_bytes(b"recent quarantine\n")
            os.utime(entry, (now - age, now - age))
        stub = self.stub_bin / "mv"
        stub.write_text(STUB_PLANT_QUARANTINE_MV)
        stub.chmod(0o755)
        result = self.install(
            sha256(self.archive),
            extra_env={
                "STUB_RACE": str(self.payload),
                "STUB_RACE_MARK": str(self.root / "race-mark"),
                "REAL_MV": str(self.mv_compat),
                "STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79",
            },
        )
        self.assertEqual(result.returncode, 1, result.output)
        self.assertIn("is kept at", result.output)
        kept = [path for path in cached.parent.glob(".velnor-ci.evict.*") if path.read_bytes() == self.archive]
        self.assertEqual(len(kept), 1, "the verified archive this eviction kept was pruned as the oldest quarantine")

    def test_a_directory_at_the_store_entry_path_is_removed_when_the_download_fails(self):
        cached = self.cached_archive()
        cached.mkdir(parents=True)
        (cached / "stale").write_bytes(b"left behind\n")
        result = self.install(
            sha256(self.archive), extra_env={"STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79"}
        )
        self.assertEqual(result.returncode, 1, result.output)
        self.assertFalse(
            cached.exists() or cached.is_symlink(),
            "a directory at the store entry path survived a job that could not download",
        )

    def test_a_fifo_at_the_store_entry_path_is_removed_when_the_download_fails(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        os.mkfifo(cached)
        result = self.install(
            sha256(self.archive), extra_env={"STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79"}
        )
        self.assertEqual(result.returncode, 1, result.output)
        self.assertFalse(
            cached.exists() or cached.is_symlink(),
            "a fifo at the store entry path survived a job that could not download",
        )

    def test_a_store_entry_found_oversized_while_copying_is_evicted_without_being_hashed(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(self.archive)
        wc = self.stub_bin / "wc"
        wc.write_text(STUB_OVERSIZE_FIRST_WC)
        wc.chmod(0o755)
        digest = self.stub_bin / "sha256sum"
        digest.write_text(STUB_DIGEST)
        digest.chmod(0o755)
        digest_log = self.root / "digest-calls.log"
        result = self.install(
            sha256(self.archive),
            extra_env={
                "STUB_WC_MARK": str(self.root / "wc-mark"),
                "STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79",
                "STUB_DIGEST_LOG": str(digest_log),
                "REAL_SHA256SUM": shutil.which("sha256sum") or "",
                "REAL_SHASUM": shutil.which("shasum") or "",
            },
        )
        self.assertEqual(result.returncode, 1, result.output)
        self.assertFalse(cached.exists(), "an oversized store entry survived the job")
        digested = digest_log.read_text().splitlines() if digest_log.exists() else []
        self.assertEqual([call for call in digested if str(cached) in call], [])

    def test_a_dangling_symlink_at_the_store_entry_path_is_removed_when_the_download_fails(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.symlink_to(self.root / "no-such-target")
        result = self.install(
            sha256(self.archive), extra_env={"STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79"}
        )
        self.assertEqual(result.returncode, 1, result.output)
        self.assertFalse(
            cached.exists() or cached.is_symlink(),
            "a dangling symlink at the store entry path survived a job that could not download",
        )

    def test_a_mismatched_store_entry_is_evicted_even_when_no_new_file_can_be_created_for_it(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"tampered archive\n")
        stub = self.stub_bin / "mktemp"
        stub.write_text(STUB_NO_INODE_MKTEMP)
        stub.chmod(0o755)
        result = self.install(
            sha256(self.archive),
            extra_env={
                "STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79",
                "REAL_MKTEMP": shutil.which("mktemp"),
            },
        )
        self.assertEqual(result.returncode, 1, result.output)
        self.assertFalse(cached.exists(), "a mismatched store entry survived because eviction needed a new file")

    def test_a_directory_planted_at_the_eviction_quarantine_path_does_not_receive_the_evicted_archive(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"tampered archive\n")
        stub_mv = self.stub_bin / "mv"
        stub_mv.write_text(STUB_PLANTING_EVICTION_MV)
        stub_mv.chmod(0o755)
        result = self.install(
            sha256(self.archive),
            extra_env={
                "REAL_MV": str(self.mv_compat),
                "STUB_ENTRY": str(cached),
                "STUB_RACE_MARK": str(self.root / "race-mark"),
            },
        )
        self.assertTrue((self.root / "race-mark").exists(), result.output)
        self.assertEqual(result.returncode, 0, result.output)
        self.assertIn(
            f"::warning title=setup-velnor::could not remove the tool-cache copy of {ARCHIVE} for velnor-ci 0.1.79",
            result.output,
        )
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)
        self.assertEqual(cached.read_bytes(), self.archive, "the store entry was not refreshed with the verified archive")
        nested = [path for path in cached.parent.rglob("*") if path.is_file() and path.parent != cached.parent]
        self.assertEqual(nested, [], "the evicted archive was moved inside the directory planted at the quarantine path")

    def test_a_healthy_store_entry_that_cannot_be_copied_to_the_runner_temp_directory_is_kept(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(self.archive)
        head = self.stub_bin / "head"
        head.write_text(STUB_FAILING_FIRST_HEAD)
        head.chmod(0o755)
        head_mark = str(self.root / "head-mark")
        first = self.install(
            sha256(self.archive),
            extra_env={"STUB_HEAD_MARK": head_mark, "STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79"},
        )
        self.assertEqual(first.returncode, 1, first.output)
        self.assertNotIn("removed the tool-cache copy", first.output)
        self.assertIn("is kept", first.output)
        self.assertTrue(
            cached.is_file() and cached.read_bytes() == self.archive, "the healthy store entry was not kept"
        )
        self.stub_log.unlink(missing_ok=True)
        self.github_path.write_text("")
        second = self.install(sha256(self.archive), extra_env={"STUB_HEAD_MARK": head_mark})
        self.assertEqual(second.returncode, 0, second.output)
        self.assertIn("is in the tool cache", second.output)
        self.assertEqual(self.curl_calls(), [])
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_failed_removal_of_the_eviction_quarantine_does_not_abort_the_install(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"tampered archive\n")
        stub = self.stub_bin / "rm"
        stub.write_text(STUB_FAILING_RM)
        stub.chmod(0o755)
        result = self.install(sha256(self.archive), extra_env={"REAL_RM": shutil.which("rm")})
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_verified_archive_is_kept_when_it_cannot_be_put_back_at_the_store_entry_path(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"tampered archive\n")
        stub = self.stub_bin / "mv"
        stub.write_text(STUB_RESTORE_RACE_MV)
        stub.chmod(0o755)
        result = self.install(
            sha256(self.archive),
            extra_env={
                "STUB_RACE": str(self.payload),
                "STUB_RACE_MARK": str(self.root / "race-mark"),
                "REAL_MV": str(self.mv_compat),
                "STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79",
            },
        )
        self.assertEqual(result.returncode, 1, result.output)
        kept = [path for path in installed_files(self.tool_cache) if path.read_bytes() == self.archive]
        self.assertTrue(kept, "the verified archive was deleted when it could not be put back at the store entry path")

    def test_every_download_attempt_is_capped_at_ten_minutes_and_a_fifteen_minute_retry_window(self):
        result = self.install(sha256(self.archive), extra_env={"STUB_MISSING": "tools-v0.1.79"})
        self.assertEqual(result.returncode, 0, result.output)
        calls = self.curl_calls()
        self.assertEqual(len(calls), 2, calls)
        limits = {"--max-time": 600, "--retry-max-time": 900}
        minimum = 60
        for call in calls:
            words = call.split()
            for flag, limit in limits.items():
                self.assertIn(flag, words, call)
                value = words[words.index(flag) + 1]
                self.assertRegex(value, r"^[1-9][0-9]*$", call)
                self.assertGreaterEqual(int(value), minimum, f"{flag} {value} is too short for a 256 MiB archive: {call}")
                self.assertLessEqual(int(value), limit, call)

    def test_a_stored_archive_that_mismatches_is_removed_even_when_the_download_is_also_bad(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"tampered archive\n")
        result = self.install(sha256(b"some other archive"))
        self.assertEqual(result.returncode, 1, result.output)
        self.assertFalse(cached.exists(), "a mismatched store entry was left for the next job")

    def test_a_valid_store_entry_that_reads_slowly_is_kept_and_not_reported_as_oversized(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(self.archive)
        slow = self.stub_bin / "head"
        slow.write_text(STUB_SLOW_HEAD)
        slow.chmod(0o755)
        result = self.install(sha256(self.archive), extra_env={"STUB_MISSING": "tools-v0.1.79 rust-ci-v0.1.79"})
        self.assertEqual(result.returncode, 1, result.output)
        self.assertNotIn("removed the tool-cache copy", result.output)
        self.assertIn("could not be read in time", result.output)
        self.assertTrue(cached.exists(), "a valid store entry that was only slow to read was deleted")
        self.assertEqual(cached.read_bytes(), self.archive)

    def test_a_directory_at_the_store_entry_path_is_replaced_by_the_verified_file(self):
        cached = self.cached_archive()
        cached.mkdir(parents=True)
        (cached / "stale").write_bytes(b"left behind\n")
        result = self.install(sha256(self.archive))
        self.assertEqual(result.returncode, 0, result.output)
        self.assertTrue(cached.is_file(), "the verified copy was moved inside the directory at the store entry path")
        self.assertEqual(cached.read_bytes(), self.archive)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_store_write_is_flushed_to_disk_before_it_is_renamed_into_the_tool_cache(self):
        order = self.root / "order.log"
        synced = self.root / "synced"
        for name, body in (("mv", STUB_MV), ("sync", STUB_SYNC)):
            stub = self.stub_bin / name
            stub.write_text(body)
            stub.chmod(0o755)
        result = self.install(
            sha256(self.archive),
            extra_env={
                "STUB_ORDER_LOG": str(order),
                "STUB_SYNCED": str(synced),
                "REAL_MV": str(self.mv_compat),
            },
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
        self.assertTrue(synced.exists(), "the temp file was never flushed")
        self.assertEqual(synced.read_bytes(), self.archive, "the flushed bytes are not the complete archive")
        self.assertEqual(self.cached_archive().read_bytes(), self.archive)

    def plant_symlink_before_publish(self):
        target = self.root / "attacker-target"
        stub = self.stub_bin / "mv"
        stub.write_text(STUB_PLANTING_MV)
        stub.chmod(0o755)
        result = self.install(
            sha256(self.archive),
            extra_env={
                "STUB_TARGET": str(target),
                "STUB_RACE_MARK": str(self.root / "race-mark"),
                "REAL_MV": str(self.mv_compat),
            },
        )
        return result, target

    def test_a_rename_that_follows_a_planted_symlink_is_not_reported_as_installed_in_the_store(self):
        result, target = self.plant_symlink_before_publish()
        self.assertEqual(result.returncode, 0, result.output)
        self.assertIn("could not publish velnor-ci 0.1.79 to the tool cache", result.output)
        self.assertNotIn(f"installed velnor-ci 0.1.79 in {self.cached_archive().parent}", result.output)
        self.assertIn(f"installed velnor-ci 0.1.79 in {self.runner_temp}/velnor-ci-bin", result.output)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_directory_planted_at_the_store_entry_path_makes_publish_fail_and_is_not_moved_into(self):
        stub = self.stub_bin / "rm"
        stub.write_text(STUB_REPLANTING_RM)
        stub.chmod(0o755)
        cached = self.cached_archive()
        cached.mkdir(parents=True)
        result = self.install(sha256(self.archive), extra_env={"REAL_RM": shutil.which("rm")})
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(installed_files(cached), [], "the verified copy was moved inside the directory at the store entry path")
        self.assertIn("could not publish velnor-ci 0.1.79 to the tool cache", result.output)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_verified_copy_evicted_while_a_directory_is_planted_at_the_store_entry_path_is_kept(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(self.archive)
        stub_head = self.stub_bin / "head"
        stub_head.write_text(STUB_GARBAGE_FIRST_HEAD)
        stub_head.chmod(0o755)
        stub_mv = self.stub_bin / "mv"
        stub_mv.write_text(STUB_REPLANTING_EVICT_MV)
        stub_mv.chmod(0o755)
        result = self.install(
            sha256(self.archive),
            extra_env={
                "REAL_MV": str(self.mv_compat),
                "STUB_ENTRY": str(cached),
                "STUB_RACE_MARK": str(self.root / "race-mark"),
                "STUB_HEAD_MARK": str(self.root / "head-mark"),
            },
        )
        self.assertEqual(result.returncode, 0, result.output)
        self.assertIn("because the store entry path is taken", result.output)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_directory_planted_between_the_check_and_the_publish_rename_is_not_moved_into(self):
        stub = self.stub_bin / "mv"
        stub.write_text(STUB_PLANTING_PUBLISH_MV)
        stub.chmod(0o755)
        cached = self.cached_archive()
        result = self.install(
            sha256(self.archive),
            extra_env={
                "REAL_MV": str(self.mv_compat),
                "STUB_ENTRY": str(cached),
                "STUB_RACE_MARK": str(self.root / "race-mark"),
            },
        )
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(installed_files(cached), [], "the verified copy was moved inside the directory planted at the store entry path")
        self.assertIn("could not publish velnor-ci 0.1.79 to the tool cache", result.output)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_directory_planted_between_the_eviction_and_the_restore_does_not_receive_the_verified_copy(self):
        cached = self.cached_archive()
        cached.parent.mkdir(parents=True)
        cached.write_bytes(self.archive)
        stub_head = self.stub_bin / "head"
        stub_head.write_text(STUB_GARBAGE_FIRST_HEAD)
        stub_head.chmod(0o755)
        stub_mv = self.stub_bin / "mv"
        stub_mv.write_text(STUB_PLANTING_RESTORE_MV)
        stub_mv.chmod(0o755)
        result = self.install(
            sha256(self.archive),
            extra_env={
                "REAL_MV": str(self.mv_compat),
                "STUB_ENTRY": str(cached),
                "STUB_RACE_MARK": str(self.root / "race-mark"),
                "STUB_HEAD_MARK": str(self.root / "head-mark"),
            },
        )
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(installed_files(cached), [], "the verified copy was restored inside the directory planted at the store entry path")
        self.assertIn("because the store entry path is taken", result.output)
        kept = [path for path in cached.parent.glob(".velnor-ci.evict.*") if path.is_file()]
        self.assertEqual([path.read_bytes() for path in kept], [self.archive])
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)

    def test_a_symlink_planted_at_the_store_entry_path_before_the_publish_rename_is_replaced_not_followed(self):
        target = self.root / "attacker-target"
        stub = self.stub_bin / "mv"
        stub.write_text(STUB_PLANTING_SYMLINK_PUBLISH_MV)
        stub.chmod(0o755)
        cached = self.cached_archive()
        result = self.install(
            sha256(self.archive),
            extra_env={
                "REAL_MV": str(self.mv_compat),
                "STUB_ENTRY": str(cached),
                "STUB_TARGET": str(target),
                "STUB_RACE_MARK": str(self.root / "race-mark"),
            },
        )
        self.assertEqual(result.returncode, 0, result.output)
        self.assertEqual(installed_files(target), [], "the verified copy was moved through the symlink planted at the store entry path")
        self.assertTrue(cached.is_file() and not cached.is_symlink(), "the symlink at the store entry path was not replaced")
        self.assertEqual(cached.read_bytes(), self.archive)
        self.assertEqual(self.installed_binary().read_bytes(), PAYLOAD)


if __name__ == "__main__":
    unittest.main()
