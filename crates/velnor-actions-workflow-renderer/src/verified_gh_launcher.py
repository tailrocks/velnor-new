# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import fcntl
import hashlib
import os
import re
import stat
import sys
import tempfile

if sys.platform != "linux" or os.uname().machine != "x86_64":
    raise SystemExit("verified_gh_unsupported_platform")
required_version_variable = "@VERSION_VARIABLE@"
if required_version_variable and os.environ.get(required_version_variable) != "@VERSION@":
    raise SystemExit("verified_gh_unqualified_version")
data = os.environ.get("VELNOR_VERIFIED_GH_DATA_DIR") or os.environ.get("MISE_DATA_DIR")
if not data:
    data = os.environ.get("XDG_DATA_HOME")
    data = os.path.join(data, "mise") if data else os.path.join(os.environ.get("HOME", ""), ".local/share/mise")
if not os.path.isabs(data):
    raise SystemExit("verified_gh_invalid")
source = os.path.join(data, "installs/gh/@VERSION@/gh_@VERSION@_@PLATFORM@/bin/gh")
fd = os.open(source, os.O_RDONLY | os.O_CLOEXEC)
metadata = os.fstat(fd)
limit = 128 * 1024 * 1024
if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= limit:
    raise SystemExit("verified_gh_invalid")
memfd = os.memfd_create("velnor-verified-gh", os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
digest = hashlib.sha256()
copied = 0
while True:
    chunk = os.read(fd, 65536)
    if not chunk:
        break
    copied += len(chunk)
    if copied > limit:
        raise SystemExit("verified_gh_invalid")
    digest.update(chunk)
    view = memoryview(chunk)
    while view:
        count = os.write(memfd, view)
        if count <= 0:
            raise SystemExit("verified_gh_invalid")
        view = view[count:]
os.close(fd)
if copied != metadata.st_size or digest.hexdigest() != "@SHA256@":
    raise SystemExit("verified_gh_invalid")
os.fchmod(memfd, 0o500)
fcntl.fcntl(memfd, fcntl.F_ADD_SEALS, fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL)
os.set_inheritable(memfd, True)
allowed = ("TMPDIR", "GH_TOKEN", "GITHUB_TOKEN", "LANG", "LC_ALL")
environment = {key: os.environ[key] for key in allowed if key in os.environ}
repository = os.environ.get("GH_REPO")
if repository:
    if repository != os.environ.get("GITHUB_REPOSITORY") or re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository) is None or any(part in (".", "..") for part in repository.split("/")):
        raise SystemExit("verified_gh_invalid_repository")
    environment["GH_REPO"] = repository
environment["HOME"] = tempfile.mkdtemp(prefix="velnor-verified-gh-")
environment["PATH"] = "/usr/bin:/bin"
environment["GH_PROMPT_DISABLED"] = "1"
os.execve(f"/proc/self/fd/{memfd}", ["gh", *sys.argv[1:]], environment)
