#!/usr/bin/env python3
"""Run one v2 seed operation with bounded resources, time, and output."""

from __future__ import annotations

import argparse
import os
import resource
import selectors
import signal
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence


PRIVATE_ROOT = Path(__file__).resolve().parent
SEED_SCRIPT = PRIVATE_ROOT / "seed_candidate_v2.py"
MAX_RUNTIME_SECONDS = 300.0
DEFAULT_RUNTIME_SECONDS = 120.0
MAX_CAPTURE_BYTES = 1024 * 1024
TERM_GRACE_SECONDS = 0.25
ADDRESS_SPACE_BYTES = 2 * 1024 * 1024 * 1024
FILE_SIZE_BYTES = 1024 * 1024 * 1024
CPU_SECONDS = 300
OPEN_FILE_LIMIT = 64


@dataclass(frozen=True)
class BoundedResult:
    returncode: int
    stdout: bytes
    stderr: bytes
    timed_out: bool
    output_limited: bool


def _minimal_environment() -> dict[str, str]:
    return {
        "PATH": os.defpath,
        "LANG": "C",
        "LC_ALL": "C",
        "PYTHONNOUSERSITE": "1",
        "PYTHONDONTWRITEBYTECODE": "1",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_OPTIONAL_LOCKS": "0",
        "GIT_TERMINAL_PROMPT": "0",
    }


def _set_child_limits() -> None:
    resource.setrlimit(resource.RLIMIT_CPU, (CPU_SECONDS, CPU_SECONDS))
    resource.setrlimit(resource.RLIMIT_AS, (ADDRESS_SPACE_BYTES, ADDRESS_SPACE_BYTES))
    resource.setrlimit(resource.RLIMIT_FSIZE, (FILE_SIZE_BYTES, FILE_SIZE_BYTES))
    resource.setrlimit(resource.RLIMIT_NOFILE, (OPEN_FILE_LIMIT, OPEN_FILE_LIMIT))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


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

    process = subprocess.Popen(
        list(command),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=dict(_minimal_environment() if env is None else env),
        close_fds=True,
        start_new_session=True,
        preexec_fn=_set_child_limits,
        bufsize=0,
    )
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
        for stream in (process.stdout, process.stderr):
            os.set_blocking(stream.fileno(), False)
            selector.register(stream, selectors.EVENT_READ)
        while selector.get_map():
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
