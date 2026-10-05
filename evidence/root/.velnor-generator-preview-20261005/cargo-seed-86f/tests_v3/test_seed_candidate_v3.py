from __future__ import annotations

import hashlib
import io
import json
import os
import subprocess
import sys
import tarfile
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

import seed_candidate_v2 as seed_v2
from bounded_entry_v2 import run_bounded
from seed_candidate_v2 import (
    REGISTRY_SOURCE,
    PRIVATE_ROOT,
    SeedError,
    SourcePin,
    _copy_verified_archive,
    _git,
    _open_archive_members,
    _read_regular_file,
    index_relative_path,
    prepare_candidate,
    verify_candidate,
)


def run_git(root: Path, *args: str) -> str:
    env = {
        "PATH": os.defpath,
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_AUTHOR_NAME": "Offline Fixture",
        "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
        "GIT_COMMITTER_NAME": "Offline Fixture",
        "GIT_COMMITTER_EMAIL": "fixture@example.invalid",
    }
    result = subprocess.run(
        ["git", "-c", "core.hooksPath=/dev/null", "-C", str(root), *args],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
    )
    return result.stdout.strip()


def create_archive(path: Path, name: str, version: str, *, traversal: bool = False) -> str:
    content = {
        "Cargo.toml": f'[package]\nname = "{name}"\nversion = "{version}"\nedition = "2024"\n',
        "src/lib.rs": "#![forbid(unsafe_code)]\npub fn fixture() -> u8 { 7 }\n",
    }
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as tar:
        root = tarfile.TarInfo(f"{name}-{version}/")
        root.type = tarfile.DIRTYPE
        root.mode = 0o755
        tar.addfile(root)
        for relative, text in sorted(content.items()):
            data = text.encode()
            member_name = f"{name}-{version}/{relative}"
            if traversal and relative == "src/lib.rs":
                member_name = f"{name}-{version}/../../escape.txt"
            member = tarfile.TarInfo(member_name)
            member.mode = 0o644
            member.size = len(data)
            tar.addfile(member, io.BytesIO(data))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(buffer.getvalue())
    return hashlib.sha256(buffer.getvalue()).hexdigest()


class SeedCandidateFixture(unittest.TestCase):
    def setUp(self) -> None:
        self.private = PRIVATE_ROOT / "test-work-v2"
        self.private.mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=self.private)
        self.work = Path(self.temp.name)
        self.source = self.work / "source"
        self.registry = self.work / "registry"
        self.output_parent = self.work / "candidates"
        self.output_parent.mkdir()

    def tearDown(self) -> None:
        self.temp.cleanup()
        try:
            self.private.rmdir()
        except OSError:
            pass

    def _build_fixture(self, *, bad_index: bool = False, traversal: bool = False) -> None:
        self.source.mkdir()
        (self.source / "crates" / "fixture-app").mkdir(parents=True)
        (self.source / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["crates/fixture-app"]\nresolver = "3"\n'
            '[workspace.package]\nversion = "0.1.0"\n',
            encoding="utf-8",
        )
        (self.source / "crates" / "fixture-app" / "Cargo.toml").write_text(
            '[package]\nname = "fixture-app"\nversion.workspace = true\nedition = "2024"\n',
            encoding="utf-8",
        )

        crate_root = self.registry / "cache" / "index.crates.io-fixture"
        index_root = self.registry / "index" / "index.crates.io-fixture"
        (crate_root).mkdir(parents=True)
        (index_root / ".cache").mkdir(parents=True)
        (self.registry / "src" / "index.crates.io-fixture").mkdir(parents=True)

        packages: list[tuple[str, str, str]] = []
        for version in ("1.0.0", "2.0.0"):
            archive_path = crate_root / f"fixture-lib-{version}.crate"
            checksum = create_archive(archive_path, "fixture-lib", version, traversal=traversal and version == "1.0.0")
            packages.append(("fixture-lib", version, checksum))

        lock_lines = [
            "version = 4",
            "",
            "[[package]]",
            'name = "fixture-app"',
            'version = "0.1.0"',
            "dependencies = [",
            ' "fixture-lib 1.0.0",',
            ' "fixture-lib 2.0.0",',
            "]",
            "",
        ]
        for name, version, checksum in packages:
            lock_lines.extend(
                [
                    "[[package]]",
                    f'name = "{name}"',
                    f'version = "{version}"',
                    f'source = "{REGISTRY_SOURCE}"',
                    f'checksum = "{checksum}"',
                    "",
                ]
            )
        lock_bytes = ("\n".join(lock_lines)).encode()
        (self.source / "Cargo.lock").write_bytes(lock_bytes)

        index_file = index_root / ".cache" / "fi" / "xt" / "fixture-lib"
        index_file.parent.mkdir(parents=True)
        cache_parts = [b"\x03\x02\x00\x00\x00fixture-etag"]
        for name, version, checksum in packages:
            record_checksum = "0" * 64 if bad_index and version == "1.0.0" else checksum
            record = json.dumps(
                {"name": name, "vers": version, "cksum": record_checksum, "deps": [], "features": {}},
                sort_keys=True,
                separators=(",", ":"),
            ).encode()
            cache_parts.extend([version.encode(), record])
        index_file.write_bytes(b"\0".join(cache_parts) + b"\0")

        run_git(self.source, "init", "--quiet", "--template=/dev/null")
        run_git(self.source, "add", "Cargo.toml", "Cargo.lock", "crates/fixture-app/Cargo.toml")
        run_git(self.source, "-c", "user.name=Offline Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--quiet", "-m", "fixture")
        commit = run_git(self.source, "rev-parse", "HEAD")
        tree = run_git(self.source, "rev-parse", "HEAD^{tree}")
        self.pin = SourcePin(
            commit=commit,
            tree=tree,
            lock_sha256=hashlib.sha256(lock_bytes).hexdigest(),
            lock_version=4,
            package_count=3,
            registry_package_count=2,
            path_package_count=1,
            registry_name_count=1,
        )

    def candidate(self, name: str = "candidate") -> Path:
        return self.output_parent / name

    def test_builds_and_verifies_exact_two_version_capsule(self) -> None:
        self._build_fixture()
        output = prepare_candidate(
            self.source,
            self.registry,
            self.candidate(),
            pin=self.pin,
            private_root=self.private,
        )
        result = verify_candidate(
            output,
            source_root=self.source,
            pin=self.pin,
            private_root=self.private,
        )
        self.assertEqual(result["counts"]["registry_packages"], 2)
        self.assertEqual(result["counts"]["registry_names"], 1)
        self.assertTrue((output / "index-records/fi/xt/fixture-lib/1.0.0.json").is_file())
        self.assertTrue((output / "index-records/fi/xt/fixture-lib/2.0.0.json").is_file())
        self.assertTrue((output / "sources/fixture-lib-1.0.0/Cargo.toml").is_file())

    def test_rejects_index_checksum_mismatch_and_cleans_partial_candidate(self) -> None:
        self._build_fixture(bad_index=True)
        output = self.candidate()
        with self.assertRaises(SeedError):
            prepare_candidate(self.source, self.registry, output, pin=self.pin, private_root=self.private)
        self.assertFalse(output.exists())

    def test_rejects_archive_checksum_mismatch_and_cleans_partial_candidate(self) -> None:
        self._build_fixture()
        archive = self.registry / "cache" / "index.crates.io-fixture" / "fixture-lib-1.0.0.crate"
        content = bytearray(archive.read_bytes())
        content[-1] ^= 1
        archive.write_bytes(content)
        output = self.candidate()
        with self.assertRaises(SeedError):
            prepare_candidate(self.source, self.registry, output, pin=self.pin, private_root=self.private)
        self.assertFalse(output.exists())

    def test_rejects_archive_path_traversal_without_writing_outside_capsule(self) -> None:
        self._build_fixture(traversal=True)
        output = self.candidate()
        with self.assertRaises(SeedError):
            prepare_candidate(self.source, self.registry, output, pin=self.pin, private_root=self.private)
        self.assertFalse((self.work / "escape.txt").exists())
        self.assertFalse(output.exists())

    def test_verifier_rejects_unexpected_config_or_credential_file(self) -> None:
        self._build_fixture()
        output = prepare_candidate(
            self.source,
            self.registry,
            self.candidate(),
            pin=self.pin,
            private_root=self.private,
        )
        (output / "config.toml").write_text("[registry]\n", encoding="utf-8")
        with self.assertRaises(SeedError):
            verify_candidate(output, source_root=self.source, pin=self.pin, private_root=self.private)

    def test_refuses_destination_at_private_directory_root(self) -> None:
        self._build_fixture()
        with self.assertRaises(SeedError):
            prepare_candidate(
                self.source,
                self.registry,
                self.private,
                pin=self.pin,
                private_root=self.private,
            )

    def test_refuses_source_commit_that_does_not_match_pin(self) -> None:
        self._build_fixture()
        wrong_pin = SourcePin(
            commit="0" * 40,
            tree=self.pin.tree,
            lock_sha256=self.pin.lock_sha256,
            lock_version=self.pin.lock_version,
            package_count=self.pin.package_count,
            registry_package_count=self.pin.registry_package_count,
            path_package_count=self.pin.path_package_count,
            registry_name_count=self.pin.registry_name_count,
        )
        output = self.candidate()
        with self.assertRaises(SeedError):
            prepare_candidate(self.source, self.registry, output, pin=wrong_pin, private_root=self.private)
        self.assertFalse(output.exists())

    def test_crates_io_index_path_shapes(self) -> None:
        self.assertEqual(index_relative_path("x").as_posix(), "1/x")
        self.assertEqual(index_relative_path("cc").as_posix(), "2/cc")
        self.assertEqual(index_relative_path("fnv").as_posix(), "3/f/fnv")
        self.assertEqual(index_relative_path("serde").as_posix(), "se/rd/serde")

    def test_fifo_is_rejected_without_waiting_for_a_writer(self) -> None:
        fifo = self.work / "record.fifo"
        os.mkfifo(fifo)
        started = time.monotonic()
        with self.assertRaises(SeedError):
            _read_regular_file(fifo, max_bytes=1024)
        self.assertLess(time.monotonic() - started, 1.0)

    def test_read_rejects_initial_file_over_limit(self) -> None:
        oversized = self.work / "oversized.bin"
        oversized.write_bytes(b"x" * 17)
        with self.assertRaises(SeedError):
            _read_regular_file(oversized, max_bytes=16)

    def test_read_detects_file_growth_during_read(self) -> None:
        target = self.work / "changing.bin"
        target.write_bytes(b"before")

        class AppendOnRead:
            def __init__(self) -> None:
                self.stream = target.open("rb", buffering=0)
                self.changed = False

            def fileno(self) -> int:
                return self.stream.fileno()

            def read(self, size: int = -1) -> bytes:
                if not self.changed:
                    self.changed = True
                    with target.open("ab", buffering=0) as writer:
                        writer.write(b"-after")
                return self.stream.read(size)

            def __enter__(self):
                return self

            def __exit__(self, *_exc) -> None:
                self.stream.close()

        with patch.object(seed_v2, "_open_regular_binary", return_value=AppendOnRead()):
            with self.assertRaisesRegex(SeedError, "changed while reading"):
                _read_regular_file(target, max_bytes=64)

    def test_archive_copy_rejects_oversize_and_removes_partial_output(self) -> None:
        archive = self.work / "too-large.crate"
        archive.write_bytes(b"x" * 33)
        destination = self.work / "out" / "too-large.crate"
        with patch.object(seed_v2, "MAX_ARCHIVE_BYTES", 32):
            with self.assertRaisesRegex(SeedError, "size limit"):
                _copy_verified_archive(
                    archive,
                    destination,
                    {"name": "fixture", "version": "1.0.0", "checksum": "0" * 64},
                )
        self.assertFalse(destination.exists())

    def test_archive_copy_enforces_streaming_limit_when_file_grows(self) -> None:
        archive = self.work / "growing.crate"
        archive.write_bytes(b"start")
        destination = self.work / "out" / "growing.crate"

        class AppendOnRead:
            def __init__(self) -> None:
                self.stream = archive.open("rb", buffering=0)
                self.changed = False

            def fileno(self) -> int:
                return self.stream.fileno()

            def read(self, size: int = -1) -> bytes:
                if not self.changed:
                    self.changed = True
                    with archive.open("ab", buffering=0) as writer:
                        writer.write(b"x" * 16)
                return self.stream.read(size)

            def __enter__(self):
                return self

            def __exit__(self, *_exc) -> None:
                self.stream.close()

        with (
            patch.object(seed_v2, "MAX_ARCHIVE_BYTES", 8),
            patch.object(seed_v2, "_open_regular_binary", return_value=AppendOnRead()),
        ):
            with self.assertRaisesRegex(SeedError, "size limit"):
                _copy_verified_archive(
                    archive,
                    destination,
                    {"name": "fixture", "version": "1.0.0", "checksum": "0" * 64},
                )
        self.assertFalse(destination.exists())

    def test_archive_decompression_has_a_total_stream_limit(self) -> None:
        archive = self.work / "fixture.crate"
        create_archive(archive, "fixture-lib", "1.0.0")
        with patch.object(seed_v2, "MAX_PACKAGE_TAR_BYTES", 128):
            with self.assertRaisesRegex(SeedError, "decompressed archive exceeds size limit"):
                with _open_archive_members(archive) as tar:
                    list(tar)

    def test_bounded_entry_times_out_and_reaps_process_tree(self) -> None:
        fixture = Path(__file__).with_name("process_tree_fixture.py")
        pid_file = self.work / "child.pid"
        started = time.monotonic()
        result = run_bounded(
            [sys.executable, "-I", "-B", str(fixture), str(pid_file)],
            timeout_seconds=0.6,
        )
        self.assertTrue(result.timed_out)
        self.assertLess(time.monotonic() - started, 5.0)
        child_pid = int(pid_file.read_text(encoding="ascii"))
        for _ in range(50):
            try:
                os.kill(child_pid, 0)
            except ProcessLookupError:
                break
            proc_stat = Path(f"/proc/{child_pid}/stat")
            try:
                if proc_stat.read_text(encoding="ascii").split()[2] == "Z":
                    break
            except FileNotFoundError:
                break
            time.sleep(0.02)
        else:
            self.fail(f"bounded entry left fixture child {child_pid} running")

    def test_bounded_entry_caps_combined_output(self) -> None:
        result = run_bounded(
            [sys.executable, "-I", "-B", "-c", "print('x' * 1000)"],
            timeout_seconds=5,
            output_limit=64,
        )
        self.assertTrue(result.output_limited)
        self.assertLessEqual(len(result.stdout) + len(result.stderr), 64)

    def test_git_capture_is_capped(self) -> None:
        fake_git = self.work / "fake-git"
        fake_git.write_text(
            f"#!{sys.executable}\nimport sys\nsys.stdout.write('x' * 4096)\n",
            encoding="utf-8",
        )
        fake_git.chmod(0o755)
        with (
            patch.object(seed_v2.shutil, "which", return_value=str(fake_git)),
            patch.object(seed_v2, "MAX_GIT_OUTPUT_BYTES", 64),
        ):
            with self.assertRaisesRegex(SeedError, "output exceeded size limit"):
                _git(self.work, "rev-parse")

    def test_git_command_has_a_runtime_bound(self) -> None:
        fake_git = self.work / "slow-git"
        fake_git.write_text(
            f"#!{sys.executable}\nimport time\ntime.sleep(5)\n",
            encoding="utf-8",
        )
        fake_git.chmod(0o755)
        started = time.monotonic()
        with (
            patch.object(seed_v2.shutil, "which", return_value=str(fake_git)),
            patch.object(seed_v2, "GIT_TIMEOUT_SECONDS", 0.05),
        ):
            with self.assertRaisesRegex(SeedError, "timed out"):
                _git(self.work, "rev-parse")
        self.assertLess(time.monotonic() - started, 1.0)


if __name__ == "__main__":
    unittest.main()
