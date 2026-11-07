#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import argparse
import hashlib
import os
import pathlib
import shutil
import subprocess
import tempfile
import zipfile

ARCHIVE = "621cb823d7e0d7edaf4e307da48fa7650b62b7d4a0796584489301110286f037"
FILES = {
    "artifact.zip": "982214f915c90ab5872a531b24d93cf32b9cae10646e8c4c32e10ea2c1d3853a",
    "source.tar.gz": "2d46b68b2f43b5f72006c585912fff59b6d7dccc9733b67ee636d34b885388b8",
    "job-image.oci.tar": ARCHIVE,
    "job-image-candidate.json": "41b0c6b72052e4a7f6532180112a7aa9129de552ae29b560931445fcca7844b2",
    "job-image-native-source-closure.json": "4c4a70ff8d97094ac9d79a46391fa1c908709768ab5335fd43e53f5b5e2e6830",
}


def verify(path, digest):
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"fixture is not a regular file: {path.name}")
    with path.open("rb") as stream:
        actual = hashlib.file_digest(stream, "sha256").hexdigest()
    if actual != digest:
        raise ValueError(f"fixture checksum mismatch: {path.name}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cache-root", type=pathlib.Path, required=True)
    parser.add_argument("--import-directory", type=pathlib.Path)
    args = parser.parse_args()
    root = args.cache_root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    destination = root / ARCHIVE
    if destination.exists():
        for name, digest in FILES.items():
            verify(destination / name, digest)
        return
    with tempfile.TemporaryDirectory(prefix=".image-fixture-", dir=root) as staging:
        stage = pathlib.Path(staging)
        if args.import_directory:
            for name in FILES:
                shutil.copyfile(args.import_directory / name, stage / name)
        else:
            for name, route in [
                ("artifact.zip", "repos/tailrocks/velnor-new/actions/artifacts/13472390779/zip"),
                ("source.tar.gz", "repos/tailrocks/velnor-new/tarball/a3417b150ac2f477498cadc329cdf4e23aa5b54b"),
            ]:
                with (stage / name).open("wb") as output:
                    subprocess.run(["gh", "api", route], stdout=output, check=True, timeout=600)
                verify(stage / name, FILES[name])
            with zipfile.ZipFile(stage / "artifact.zip") as archive:
                for name in FILES.keys() - {"artifact.zip", "source.tar.gz"}:
                    with archive.open(name) as source, (stage / name).open("wb") as output:
                        shutil.copyfileobj(source, output)
        for name, digest in FILES.items():
            verify(stage / name, digest)
            os.chmod(stage / name, 0o444)
        os.rename(stage, destination)
        pathlib.Path(staging).mkdir()


if __name__ == "__main__":
    main()
