# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import gzip
import hashlib
import os
import pathlib
import sys
import shutil
import tarfile
import tempfile
import time
import urllib.parse
import urllib.request

VERSION = "@VERSION@"
PLATFORM = "@PLATFORM@"
ARCHIVE_SHA256 = "@ARCHIVE_SHA256@"
BINARY_SHA256 = "@SHA256@"
ARCHIVE_LIMIT = 64 * 1024 * 1024
TAR_LIMIT = 128 * 1024 * 1024
BINARY_LIMIT = 128 * 1024 * 1024
PREFIX = f"gh_{VERSION}_{PLATFORM}"
BINARY = f"{PREFIX}/bin/gh"


class Redirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        parsed = urllib.parse.urlsplit(newurl)
        if parsed.scheme != "https" or parsed.hostname not in ("github.com", "release-assets.githubusercontent.com") or parsed.username or parsed.password:
            raise ValueError("redirect")
        return super().redirect_request(request, fp, code, msg, headers, newurl)


def download(archive, deadline):
    url = f"https://github.com/cli/cli/releases/download/v{VERSION}/{PREFIX}.tar.gz"
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), Redirect())
    request = urllib.request.Request(url, headers={"User-Agent": "velnor-qualified-gh"})
    digest = hashlib.sha256()
    total = 0
    with opener.open(request, timeout=30) as response:
        if response.status != 200:
            raise ValueError("status")
        while True:
            if time.monotonic() >= deadline:
                raise ValueError("deadline")
            chunk = response.read(65536)
            if not chunk:
                break
            total += len(chunk)
            if total > ARCHIVE_LIMIT:
                raise ValueError("size")
            digest.update(chunk)
            archive.write(chunk)
    if not total or digest.hexdigest() != ARCHIVE_SHA256:
        raise ValueError("digest")
    archive.seek(0)


def unpack(archive, uncompressed, root, deadline):
    size = 0
    with gzip.GzipFile(fileobj=archive) as data:
        while True:
            if time.monotonic() >= deadline:
                raise ValueError("deadline")
            chunk = data.read(65536)
            if not chunk:
                break
            size += len(chunk)
            if size > TAR_LIMIT:
                raise ValueError("tar size")
            uncompressed.write(chunk)
    uncompressed.seek(0)
    with tarfile.open(fileobj=uncompressed, mode="r:") as tar:
        seen = set()
        binary = None
        total = 0
        for member in tar:
            if time.monotonic() >= deadline:
                raise ValueError("deadline")
            path = pathlib.PurePosixPath(member.name)
            raw_parts = member.name.split("/")
            if len(seen) >= 1000 or member.name in seen or path.is_absolute() or not path.parts or "\\" in member.name or any(part in ("", ".", "..") for part in raw_parts) or path.parts[0] != PREFIX or member.size < 0 or not (member.isfile() or member.isdir()) or member.issparse():
                raise ValueError("member")
            seen.add(member.name)
            total += member.size
            if total > TAR_LIMIT:
                raise ValueError("members size")
            if member.name == BINARY:
                if not member.isfile() or not 0 < member.size <= BINARY_LIMIT or binary is not None:
                    raise ValueError("binary")
                binary = member
        if binary is None:
            raise ValueError("binary missing")
        parent = root
        for component in ("installs", "gh", VERSION, PREFIX, "bin"):
            parent = os.path.join(parent, component)
            os.mkdir(parent, mode=0o700)
        path = os.path.join(parent, "gh")
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o500)
        digest = hashlib.sha256()
        copied = 0
        with os.fdopen(fd, "wb") as output, tar.extractfile(binary) as source:
            while True:
                if time.monotonic() >= deadline:
                    raise ValueError("deadline")
                chunk = source.read(65536)
                if not chunk:
                    break
                copied += len(chunk)
                if copied > BINARY_LIMIT:
                    raise ValueError("binary size")
                digest.update(chunk)
                output.write(chunk)
        if copied != binary.size or digest.hexdigest() != BINARY_SHA256:
            raise ValueError("binary digest")


def main():
    if sys.platform != "linux" or os.uname().machine != "x86_64":
        raise ValueError("platform")
    base = os.environ.get("RUNNER_TEMP", "")
    if not os.path.isabs(base) or any(ord(c) < 32 or ord(c) == 127 for c in base) or os.path.islink(base) or not os.path.isdir(base):
        raise ValueError("temp")
    root = tempfile.mkdtemp(prefix="velnor-gh-", dir=base)
    deadline = time.monotonic() + 90
    try:
        with tempfile.TemporaryFile(dir=root) as archive, tempfile.TemporaryFile(dir=root) as uncompressed:
            download(archive, deadline)
            unpack(archive, uncompressed, root, deadline)
    except Exception:
        shutil.rmtree(root)
        raise
    print(root)


try:
    main()
except Exception:
    raise SystemExit("verified_gh_bootstrap_failed")
