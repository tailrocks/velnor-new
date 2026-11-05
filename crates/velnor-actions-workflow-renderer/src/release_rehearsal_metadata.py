# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import os
from pathlib import Path
import resource
import stat
import subprocess
import sys
import tarfile
import tempfile


def digest(stream):
    value = hashlib.sha256()
    while chunk := stream.read(65536):
        value.update(chunk)
    return value.digest()


def snapshot(source, archive):
    expected = {}
    directories = set()
    with tarfile.open(archive, "r:") as tar:
        for entry in tar:
            parts = entry.name.split("/")
            if (not parts or any(part in ("", ".", "..") for part in parts)
                    or entry.name in expected or not (entry.isfile() or entry.issym())):
                raise ValueError("invalid source archive")
            for index in range(1, len(parts)):
                directories.add("/".join(parts[:index]))
            if entry.isfile():
                stream = tar.extractfile(entry)
                if stream is None:
                    raise ValueError("missing source file")
                expected[entry.name] = ("file", bool(entry.mode & 0o111), digest(stream))
            else:
                expected[entry.name] = ("link", entry.linkname)
    actual = {}
    seen_directories = set()
    for parent, dirs, files in os.walk(source, followlinks=False):
        relative = Path(parent).relative_to(source)
        for name in dirs + files:
            path = Path(parent) / name
            key = (relative / name).as_posix()
            info = path.lstat()
            if stat.S_ISLNK(info.st_mode):
                actual[key] = ("link", os.readlink(path))
            elif stat.S_ISDIR(info.st_mode):
                seen_directories.add(key)
            elif stat.S_ISREG(info.st_mode):
                with path.open("rb") as stream:
                    actual[key] = ("file", bool(info.st_mode & 0o111), digest(stream))
            else:
                raise ValueError("unexpected source file type")
    if expected != actual or directories != seen_directories:
        raise ValueError("approved source changed")


def main():
    source, archive, manifest, cargo, rustc, rustup, scratch, output = sys.argv[1:]
    source = Path(source).resolve(strict=True)
    archive = Path(archive).resolve(strict=True)
    parts = Path(manifest).parts
    if not parts or any(part in ("", ".", "..") for part in parts) or Path(manifest).is_absolute():
        raise ValueError("invalid manifest path")
    manifest_path = source.joinpath(*parts)
    if not manifest_path.is_file() or not manifest_path.resolve(strict=True).is_relative_to(source):
        raise ValueError("invalid source manifest")
    cargo = Path(cargo).resolve(strict=True)
    rustc = Path(rustc).resolve(strict=True)
    scratch = Path(scratch).resolve(strict=True)
    snapshot(source, archive)
    with tempfile.TemporaryDirectory(dir=scratch) as temporary:
        base = Path(temporary)
        (base / "home").mkdir()
        (base / "cargo").mkdir()
        (base / "work").mkdir()
        for ancestor in (base / "work").parents:
            if (ancestor / ".cargo/config").exists() or (ancestor / ".cargo/config.toml").exists():
                raise ValueError("non-sterile Cargo working directory")
        env = {"PATH": "/usr/bin:/bin", "HOME": str(base / "home"),
               "CARGO_HOME": str(base / "cargo"), "RUSTUP_HOME": rustup,
               "RUSTC": str(rustc), "RUSTUP_AUTO_INSTALL": "0",
               "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null",
               "GIT_TERMINAL_PROMPT": "0"}
        def limit_output():
            resource.setrlimit(resource.RLIMIT_FSIZE, (16 * 1024 * 1024, 16 * 1024 * 1024))
        with tempfile.TemporaryFile(dir=base) as captured:
            result = subprocess.run(
                [str(cargo), "metadata", "--locked", "--no-deps", "--format-version", "1",
                 "--manifest-path", str(manifest_path)],
                cwd=base / "work", env=env, stdin=subprocess.DEVNULL, stdout=captured,
                stderr=subprocess.DEVNULL, timeout=180, check=False, preexec_fn=limit_output,
            )
            if result.returncode != 0:
                raise ValueError("Cargo metadata failed")
            captured.seek(0)
            metadata = captured.read(16 * 1024 * 1024 + 1)
            if not metadata or len(metadata) > 16 * 1024 * 1024:
                raise ValueError("metadata is empty or too large")
    snapshot(source, archive)
    with open(output, "xb") as result:
        result.write(metadata)


if __name__ == "__main__":
    main()
