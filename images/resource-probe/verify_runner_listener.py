"""Verify the pinned Actions Runner listener in an image."""

import os
import pathlib
import re
import struct
import subprocess
import sys


def verify_elf64_x86_64(path):
    binary = pathlib.Path(path)
    if binary.is_symlink() or not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError("Runner.Listener must be an executable regular file")
    with binary.open("rb") as stream:
        header = stream.read(20)
    if (
        len(header) != 20
        or header[:4] != b"\x7fELF"
        or header[4] != 2
        or header[5] != 1
        or header[6] != 1
        or struct.unpack_from("<H", header, 18)[0] != 62
    ):
        raise ValueError("Runner.Listener must be an ELF64 little-endian x86-64 executable")


def verify_runner_listener(path, expected_version, run=subprocess.run):
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", expected_version):
        raise ValueError("expected runner version must contain three decimal numbers separated by periods")
    verify_elf64_x86_64(path)
    result = run(
        [os.fspath(path), "--version"],
        check=False,
        cwd=os.fspath(pathlib.Path(path).parent.parent),
        env={"PATH": "/usr/bin:/bin", "HOME": "/tmp"},
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=10,
    )
    if result.returncode != 0 or result.stdout.strip() != expected_version:
        raise ValueError("Runner.Listener --version does not match the pinned version")
    print(f"Runner.Listener is ELF64 x86-64 and reports version {expected_version}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: verify_runner_listener.py EXPECTED_VERSION")
    try:
        verify_runner_listener("/opt/runner/bin/Runner.Listener", sys.argv[1])
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        raise SystemExit(f"runner listener qualification failed: {error}") from error
