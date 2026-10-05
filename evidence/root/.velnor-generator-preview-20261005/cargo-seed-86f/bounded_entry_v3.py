#!/usr/bin/env python3
"""Run one v2 seed operation with bounded resources, time, and output."""

from __future__ import annotations

import argparse
import ctypes
import hashlib
import os
import resource
import selectors
import signal
import stat
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence


PRIVATE_ROOT = Path("/root/.velnor-generator-preview-20261005/cargo-seed-86f")
SEED_SCRIPT = PRIVATE_ROOT / "seed_candidate_v3.py"
SOURCE_ROOT = Path("/root/.velnor-generator-preview-20261005/main-source-86f")
SOURCE_MANIFEST = Path("/root/.velnor-generator-preview-20261005/main-source-86f-manifest.json")
SOURCE_MANIFEST_SHA256 = "f12e0de7c5c22c5dc69e4f141d38d704a94f50494448dee3e896ea499a3e61b3"
PYTHON_PATH = Path("/usr/bin/python3.13")
PYTHON_SHA256 = "889c603f0d17cb54060951bcf4c4f9b8c9ebd9e52b392c70209bbb9755d797d9"
GIT_PATH = Path("/usr/bin/git")
GIT_SHA256 = "356db14e102d68a1a37d8a1ac577dfd678d45d46e92f468bef8b7154e7bfdc60"
SEED_SCRIPT_SHA256 = "PENDING_SEED_V3_HASH"
TASK_UID = 0
MAX_RUNTIME_SECONDS = 300.0
DEFAULT_RUNTIME_SECONDS = 120.0
MAX_CAPTURE_BYTES = 1024 * 1024
TERM_GRACE_SECONDS = 0.25
ADDRESS_SPACE_BYTES = 2 * 1024 * 1024 * 1024
FILE_SIZE_BYTES = 1024 * 1024 * 1024
CPU_SECONDS = 300
OPEN_FILE_LIMIT = 64
_ACTIVE_PROCESS: subprocess.Popen[bytes] | None = None
_TERMINATE_REQUESTED = False
_TERMINATING = False


@dataclass(frozen=True)
class BoundedResult:
    returncode: int
    stdout: bytes
    stderr: bytes
    timed_out: bool
    output_limited: bool


def _check_dir_fd(fd: int, label: str) -> None:
    info = os.fstat(fd)
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != TASK_UID or info.st_mode & 0o022:
        raise ValueError(f"untrusted directory ancestry: {label}")


def _open_dir_path(path: Path) -> int:
    if not path.is_absolute() or any(part in (".", "..") for part in path.parts[1:]):
        raise ValueError(f"unsafe absolute path: {path}")
    current = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        _check_dir_fd(current, "/")
        for part in path.parts[1:]:
            child = os.open(
                part,
                os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                dir_fd=current,
            )
            try:
                _check_dir_fd(child, str(path))
            except BaseException:
                os.close(child)
                raise
            os.close(current)
            current = child
        result = current
        current = -1
        return result
    finally:
        if current >= 0:
            os.close(current)


def _pinned_file_hash(path: Path, max_bytes: int = 256 * 1024 * 1024) -> str:
    parent = _open_dir_path(path.parent)
    try:
        fd = os.open(
            path.name,
            os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC,
            dir_fd=parent,
        )
        try:
            before = os.fstat(fd)
            if (
                not stat.S_ISREG(before.st_mode)
                or before.st_uid != TASK_UID
                or before.st_mode & 0o022
                or before.st_nlink != 1
                or before.st_size > max_bytes
            ):
                raise ValueError(f"untrusted pinned file: {path}")
            digest = hashlib.sha256()
            total = 0
            while True:
                chunk = os.read(fd, min(1024 * 1024, max_bytes - total + 1))
                if not chunk:
                    break
                total += len(chunk)
                if total > max_bytes:
                    raise ValueError(f"pinned file exceeded size limit: {path}")
                digest.update(chunk)
            after = os.fstat(fd)
            if (
                (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns)
                != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns)
                or total != before.st_size
            ):
                raise ValueError(f"pinned file changed while hashing: {path}")
            return digest.hexdigest()
        finally:
            os.close(fd)
    finally:
        os.close(parent)


def verify_entry_admission() -> None:
    if os.geteuid() != TASK_UID:
        raise ValueError("bounded v3 entry requires the pinned root task identity")
    if Path(os.path.abspath(sys.executable)) != PYTHON_PATH or sys.version_info[:3] != (3, 13, 5):
        raise ValueError("bounded v3 entry is not running under the pinned Python runtime")
    for directory in (PRIVATE_ROOT, SOURCE_ROOT, SOURCE_MANIFEST.parent, PYTHON_PATH.parent, GIT_PATH.parent):
        fd = _open_dir_path(directory)
        os.close(fd)
    pins = (
        (PYTHON_PATH, PYTHON_SHA256),
        (GIT_PATH, GIT_SHA256),
        (SEED_SCRIPT, SEED_SCRIPT_SHA256),
        (SOURCE_MANIFEST, SOURCE_MANIFEST_SHA256),
    )
    for path, expected in pins:
        if _pinned_file_hash(path) != expected:
            raise ValueError(f"pinned input hash mismatch: {path}")


def _minimal_environment() -> dict[str, str]:
    return {
        "PATH": "/usr/bin",
        "LANG": "C",
        "LC_ALL": "C",
        "PYTHONNOUSERSITE": "1",
        "PYTHONDONTWRITEBYTECODE": "1",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_OPTIONAL_LOCKS": "0",
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_NO_REPLACE_OBJECTS": "1",
    }


def _set_child_limits(parent_pid: int) -> None:
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(1, signal.SIGTERM, 0, 0, 0) != 0:
        errno = ctypes.get_errno()
        raise OSError(errno, os.strerror(errno))
    if os.getppid() != parent_pid:
        os.kill(os.getpid(), signal.SIGTERM)
    resource.setrlimit(resource.RLIMIT_CPU, (CPU_SECONDS, CPU_SECONDS))
    resource.setrlimit(resource.RLIMIT_AS, (ADDRESS_SPACE_BYTES, ADDRESS_SPACE_BYTES))
    resource.setrlimit(resource.RLIMIT_FSIZE, (FILE_SIZE_BYTES, FILE_SIZE_BYTES))
    resource.setrlimit(resource.RLIMIT_NOFILE, (OPEN_FILE_LIMIT, OPEN_FILE_LIMIT))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


def install_cancellation_handlers() -> None:
    signal.signal(signal.SIGTERM, _on_sigterm)


def _on_sigterm(signum: int, _frame) -> None:
    global _TERMINATE_REQUESTED, _TERMINATING
    if _TERMINATING:
        return
    _TERMINATE_REQUESTED = True
    _TERMINATING = True
    process = _ACTIVE_PROCESS
    if process is not None:
        _stop_process_group(process)
    raise SystemExit(128 + signum)


def _signal_process_group(process_group: int, sig: int) -> None:
    try:
        os.killpg(process_group, sig)
    except ProcessLookupError:
        pass


def _group_exists(process_group: int) -> bool:
    try:
        os.killpg(process_group, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def _stop_process_group(process: subprocess.Popen[bytes], *, grace: float = TERM_GRACE_SECONDS) -> None:
    group = process.pid
    _signal_process_group(group, signal.SIGTERM)
    until = time.monotonic() + grace
    while time.monotonic() < until and _group_exists(group):
        time.sleep(0.01)
    # Always issue SIGKILL after the grace interval. The parent may have exited
    # while a descendant kept a pipe open or ignored SIGTERM.
    _signal_process_group(group, signal.SIGKILL)
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def run_bounded(
    command: Sequence[str],
    *,
    timeout_seconds: float,
    output_limit: int = MAX_CAPTURE_BYTES,
    env: dict[str, str] | None = None,
) -> BoundedResult:
    """Run a command without a shell and cap wall time, output, and resources."""
    if not command or any(not isinstance(part, str) or not part for part in command):
        raise ValueError("command must be a non-empty sequence of non-empty strings")
    if not 0 < timeout_seconds <= MAX_RUNTIME_SECONDS:
        raise ValueError(f"timeout must be between 0 and {MAX_RUNTIME_SECONDS:g} seconds")
    if output_limit <= 0:
        raise ValueError("output limit must be positive")

    global _ACTIVE_PROCESS
    parent_pid = os.getpid()

    def child_setup() -> None:
        _set_child_limits(parent_pid)

    process = subprocess.Popen(
        list(command), stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        env=dict(_minimal_environment() if env is None else env), close_fds=True,
        start_new_session=True, preexec_fn=child_setup, bufsize=0,
    )
    _ACTIVE_PROCESS = process
    assert process.stdout is not None and process.stderr is not None
    selector = selectors.DefaultSelector()
    stdout_fd = process.stdout.fileno()
    stderr_fd = process.stderr.fileno()
    streams = {
        stdout_fd: bytearray(),
        stderr_fd: bytearray(),
    }
    total = 0
    timed_out = False
    output_limited = False
    deadline = time.monotonic() + timeout_seconds
    try:
        if _TERMINATE_REQUESTED:
            timed_out = True
        for stream in (process.stdout, process.stderr):
            os.set_blocking(stream.fileno(), False)
            selector.register(stream, selectors.EVENT_READ)
        while selector.get_map():
            if _TERMINATE_REQUESTED:
                timed_out = True
                break
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                timed_out = True
                break
            for key, _ in selector.select(min(remaining, 0.1)):
                fd = key.fd
                allowance = output_limit - total
                try:
                    chunk = os.read(fd, min(64 * 1024, allowance + 1))
                except BlockingIOError:
                    continue
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                if len(chunk) > allowance:
                    streams[fd].extend(chunk[:allowance])
                    total = output_limit
                    output_limited = True
                    break
                streams[fd].extend(chunk)
                total += len(chunk)
            if output_limited:
                break
        if not output_limited:
            try:
                process.wait(timeout=max(0, deadline - time.monotonic()))
            except subprocess.TimeoutExpired:
                timed_out = True
    finally:
        selector.close()
        # This also cleans up descendants that inherited pipes and outlived the
        # main command. It is harmless when the process group already exited.
        _stop_process_group(process)
        process.stdout.close()
        process.stderr.close()
        if _ACTIVE_PROCESS is process:
            _ACTIVE_PROCESS = None

    return BoundedResult(
        returncode=process.returncode if process.returncode is not None else 125,
        stdout=bytes(streams[stdout_fd]),
        stderr=bytes(streams[stderr_fd]),
        timed_out=timed_out,
        output_limited=output_limited,
    )


def _seed_command(args: argparse.Namespace) -> list[str]:
    command = [sys.executable, "-I", "-B", str(SEED_SCRIPT), args.operation]
    if args.operation == "plan":
        command.extend(["--source-root", str(args.source_root), "--output", str(args.output)])
    elif args.operation == "prepare":
        command.extend(
            [
                "--source-root",
                str(args.source_root),
                "--registry-root",
                str(args.registry_root),
                "--output",
                str(args.output),
            ]
        )
    else:
        command.extend([str(args.candidate), "--source-root", str(args.source_root)])
    return command


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--timeout-seconds",
        type=float,
        default=DEFAULT_RUNTIME_SECONDS,
        help=f"wall limit, at most {MAX_RUNTIME_SECONDS:g} seconds",
    )
    parser.add_argument(
        "--output-limit-bytes",
        type=int,
        default=MAX_CAPTURE_BYTES,
        help="combined stdout/stderr capture limit",
    )
    subparsers = parser.add_subparsers(dest="operation", required=True)
    plan = subparsers.add_parser("plan")
    plan.add_argument("--source-root", required=True, type=Path)
    plan.add_argument("--output", required=True, type=Path)
    prepare = subparsers.add_parser("prepare")
    prepare.add_argument("--source-root", required=True, type=Path)
    prepare.add_argument("--registry-root", required=True, type=Path)
    prepare.add_argument("--output", required=True, type=Path)
    verify = subparsers.add_parser("verify")
    verify.add_argument("candidate", type=Path)
    verify.add_argument("--source-root", required=True, type=Path)
    args = parser.parse_args(argv)
    if args.output_limit_bytes <= 0 or args.output_limit_bytes > MAX_CAPTURE_BYTES:
        parser.error(f"output limit must be between 1 and {MAX_CAPTURE_BYTES} bytes")
    try:
        result = run_bounded(
            _seed_command(args),
            timeout_seconds=args.timeout_seconds,
            output_limit=args.output_limit_bytes,
            env=_minimal_environment(),
        )
    except (OSError, ValueError) as exc:
        parser.error(str(exc))
    sys.stdout.buffer.write(result.stdout)
    sys.stdout.buffer.write(result.stderr)
    if result.timed_out:
        print("bounded entry: operation timed out", file=sys.stderr)
        return 124
    if result.output_limited:
        print("bounded entry: output limit exceeded", file=sys.stderr)
        return 125
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
