# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import fcntl
import hashlib
import os
from pathlib import Path
import re
import stat
import sys


def require(value):
    if not value:
        raise ValueError("source preview executable rejected")


def main():
    require(sys.platform == "linux" and len(sys.argv) == 1)
    root = Path(os.environ["RUNNER_TEMP"])
    expected = os.environ["VERIFIED_LINUX_SHA256"]
    require(root.is_absolute() and re.fullmatch(r"[0-9a-f]{64}", expected) is not None)
    source = os.open(root / "source-preview-runtime" / "linux", os.O_RDONLY | os.O_NOFOLLOW)
    sealed = -1
    try:
        metadata = os.fstat(source)
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_uid == os.geteuid() and 0 < metadata.st_size <= 268435456)
        sealed = os.memfd_create("velnor-approved-preview", os.MFD_ALLOW_SEALING)
        digest = hashlib.sha256()
        size = 0
        while data := os.read(source, 65536):
            size += len(data)
            require(size <= metadata.st_size)
            digest.update(data)
            view = memoryview(data)
            while view:
                written = os.write(sealed, view)
                require(written > 0)
                view = view[written:]
        require(size == metadata.st_size and digest.hexdigest() == expected)
        seals = fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL
        fcntl.fcntl(sealed, fcntl.F_ADD_SEALS, seals)
        require(fcntl.fcntl(sealed, fcntl.F_GET_SEALS) == seals)
        os.set_inheritable(sealed, True)
        executable = f"/proc/self/fd/{sealed}"
        args = [executable, "image-maintenance", "verify-mac-attestation",
                "--producer-run-id", os.environ["PRODUCER_RUN_ID"],
                "--producer-run-attempt", os.environ["PRODUCER_RUN_ATTEMPT"],
                "--producer-artifact-id", os.environ["PRODUCER_ARTIFACT_ID"],
                "--publisher-artifact-id", os.environ["PUBLISHER_ARTIFACT_ID"],
                "--assets-dir", str(root / "runner-family-release" / "assets"),
                "--runner-temp", str(root)]
        allowed = {"PATH", "HOME", "RUNNER_TEMP", "GH_TOKEN", "GITHUB_SHA", "GITHUB_RUN_ID",
                   "GITHUB_RUN_ATTEMPT", "GITHUB_JOB", "GITHUB_OUTPUT", "GITHUB_EVENT_NAME",
                   "GITHUB_REF", "GITHUB_REF_PROTECTED", "GITHUB_REPOSITORY", "GITHUB_WORKFLOW_REF",
                   "GITHUB_WORKFLOW_SHA", "VELNOR_Q_REPOSITORY", "VELNOR_Q_PATH", "VELNOR_Q_REF",
                   "VELNOR_Q_SHA", "VELNOR_Q_CHECK_RUN_ID", "VELNOR_Q_GH"}
        environment = {key: value for key, value in os.environ.items() if key in allowed}
        os.execve(executable, args, environment)
    finally:
        os.close(source)
        if sealed >= 0:
            os.close(sealed)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError):
        raise SystemExit("source preview executable rejected") from None
