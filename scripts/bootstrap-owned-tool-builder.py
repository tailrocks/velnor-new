#!/usr/bin/env python3
"""Install both exact catalog-pinned official source-builder executables."""

import io
import os
import sys
import stat
from pathlib import Path
import tarfile

sys.path.insert(0, str(Path(__file__).resolve().parent))

from owned_tool_source import (MAX_BOOTSTRAP_DOWNLOAD, MAX_BOOTSTRAP_EXECUTABLE_BYTES,
                               MAX_FILES, MAX_SOURCE, build_bootstrap_descriptor,
                               canonical, digest, fetch, source_path)


def binary_bytes(data, archive_format, expected_member="mise/bin/mise", allowed_members=None):
    if archive_format == "standalone":
        return data
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        binaries, names, total = [], set(), 0
        for member in archive:
            name = source_path(member.name.rstrip("/") if member.isdir() else member.name)
            if (canonical(name) in names or len(names) >= MAX_FILES
                    or not (member.isfile() or member.isdir()) or member.mode & 0o6000):
                raise ValueError("unsafe official bootstrap archive")
            names.add(canonical(name))
            total += member.size
            if member.size < 0 or total > MAX_SOURCE:
                raise ValueError("official bootstrap archive expansion bound")
            if name == expected_member:
                if not member.isfile() or member.size > MAX_BOOTSTRAP_EXECUTABLE_BYTES:
                    raise ValueError("invalid official bootstrap executable")
                with archive.extractfile(member) as file:
                    binaries.append(file.read(MAX_BOOTSTRAP_EXECUTABLE_BYTES + 1))
        if len(binaries) != 1:
            raise ValueError("official bootstrap member missing")
        if allowed_members is not None and names != {canonical(name) for name in allowed_members}:
            raise ValueError("unexpected official bootstrap archive layout")
        return binaries[0]


def materialize_assets(directory, descriptor, target, native=False):
    assets = build_bootstrap_descriptor(descriptor, target)
    return _materialize_assets(directory, assets, native)


def _materialize_assets(directory, assets, native=False):
    directory.mkdir(mode=0o700)
    paths = {}
    for tool, asset in assets.items():
        data = binary_bytes(fetch(asset["url"], asset["archive_sha256"],
                                  MAX_BOOTSTRAP_DOWNLOAD),
                            asset["format"], asset["binary_member"],
                            ("mbx", "LICENSE", "NOTICE") if native and tool == "mbx" else None)
        if digest(data) != asset["binary_sha256"]:
            raise ValueError("official bootstrap executable SHA256 mismatch")
        binary = directory / tool
        binary.write_bytes(data)
        binary.chmod(0o755)
        paths[tool] = binary
    return paths


def bootstrap():
    assets = build_bootstrap_descriptor(os.environ["OWNED_TOOL_BUILD_BOOTSTRAP_JSON"],
                                        os.environ["OWNED_TOOL_TARGET"])
    root = Path(os.environ["RUNNER_TEMP"])
    if not root.is_absolute() or any(char in str(root) for char in ("\n", "\r")):
        raise ValueError("absolute runner temporary directory required")
    paths = _materialize_assets(root / "velnor-owned-tool-bootstrap", assets)
    with Path(os.environ["GITHUB_ENV"]).open("a", encoding="utf-8") as environment_file:
        for tool, binary in paths.items():
            environment_file.write("VELNOR_BOOTSTRAP_" + tool.upper() + "=" + str(binary) + "\n")
    return paths


def native_bootstrap(purpose):
    root = Path(os.environ["RUNNER_TEMP"])
    if not root.is_absolute() or any(char in str(root) for char in ("\n", "\r")):
        raise ValueError("absolute runner temporary directory required")
    for part in (root, *root.parents):
        if stat.S_ISLNK(part.lstat().st_mode):
            raise ValueError("runner temporary path contains a symlink")
    info = root.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid():
        raise ValueError("runner temporary directory is not owned")
    if purpose == "qualification":
        native = root / "velnor-qualification-runtime"
        native.mkdir(mode=0o700)
    elif purpose == "candidate":
        native = root / "velnor"
        info = native.lstat()
        if (not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid()
                or stat.S_IMODE(info.st_mode) & 0o077):
            raise ValueError("native temporary directory is not private and owned")
    else:
        raise ValueError("unknown native bootstrap purpose")
    return materialize_assets(native / "native-tools",
                              os.environ["OWNED_TOOL_BUILD_BOOTSTRAP_JSON"],
                              "x86_64-unknown-linux-gnu", native=True)


def main():
    try:
        if len(sys.argv) == 1:
            binary = bootstrap()
        elif len(sys.argv) == 2 and sys.argv[1] in ("--native-candidate", "--native-qualification"):
            binary = native_bootstrap(sys.argv[1][len("--native-"):])
        else:
            raise ValueError("invalid bootstrap invocation")
    except (KeyError, ValueError, OSError, tarfile.TarError) as error:
        raise SystemExit("official bootstrap failed: " + str(error)) from error
    print(binary)


if __name__ == "__main__":
    main()
