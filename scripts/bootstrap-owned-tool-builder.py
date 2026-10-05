#!/usr/bin/env python3
"""Install both exact catalog-pinned official source-builder executables."""

import io
import os
from pathlib import Path
import tarfile

from owned_tool_source import (MAX_FILES, MAX_SOURCE, build_bootstrap_descriptor,
                               canonical, digest, fetch, source_path)


def binary_bytes(data, archive_format, expected_member="mise/bin/mise"):
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
                if not member.isfile() or member.size > 128 * 1024 * 1024:
                    raise ValueError("invalid official bootstrap executable")
                with archive.extractfile(member) as file:
                    binaries.append(file.read(128 * 1024 * 1024 + 1))
        if len(binaries) != 1:
            raise ValueError("official bootstrap member missing")
        return binaries[0]


def bootstrap():
    assets = build_bootstrap_descriptor(os.environ["OWNED_TOOL_BUILD_BOOTSTRAP_JSON"],
                                        os.environ["OWNED_TOOL_TARGET"])
    root = Path(os.environ["RUNNER_TEMP"])
    if not root.is_absolute() or any(char in str(root) for char in ("\n", "\r")):
        raise ValueError("absolute runner temporary directory required")
    directory = root / "velnor-owned-tool-bootstrap"
    directory.mkdir(mode=0o700)
    paths = {}
    for tool, asset in assets.items():
        data = binary_bytes(fetch(asset["url"], asset["archive_sha256"]),
                            asset["format"], asset["binary_member"])
        if digest(data) != asset["binary_sha256"]:
            raise ValueError("official bootstrap executable SHA256 mismatch")
        binary = directory / tool
        binary.write_bytes(data)
        binary.chmod(0o755)
        paths[tool] = binary
    with Path(os.environ["GITHUB_ENV"]).open("a", encoding="utf-8") as environment_file:
        for tool, binary in paths.items():
            environment_file.write("VELNOR_BOOTSTRAP_" + tool.upper() + "=" + str(binary) + "\n")
    return paths


def main():
    try:
        binary = bootstrap()
    except (KeyError, ValueError, OSError, tarfile.TarError) as error:
        raise SystemExit("official bootstrap failed: " + str(error)) from error
    print(binary)


if __name__ == "__main__":
    main()
