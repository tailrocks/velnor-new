# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import gzip
import hashlib
import io
import os
import sys
import tarfile
import tempfile
import time

source = open(sys.argv[1], encoding="utf-8").read().split("\ntry:\n    main()")[0]
source = source.replace("@VERSION@", "test").replace("@PLATFORM@", "linux_amd64")
source = source.replace("@SHA256@", hashlib.sha256(b"qualified").hexdigest())
namespace = {}
exec(compile(source, "acquire", "exec"), namespace)
prefix = namespace["PREFIX"]
binary = namespace["BINARY"]

def archive(entries):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as tar:
        for name, data, kind in entries:
            info = tarfile.TarInfo(name)
            if kind == "link":
                info.type = tarfile.SYMTYPE
                info.linkname = "/etc/passwd"
                tar.addfile(info)
            else:
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))
    output.seek(0)
    return output

def check(entries, expected, limit=None):
    with tempfile.TemporaryDirectory() as root, tempfile.TemporaryFile() as unpacked:
        namespace["TAR_LIMIT"] = limit or 128 * 1024 * 1024
        try:
            namespace["unpack"](archive(entries), unpacked, root, time.monotonic() + 10)
        except (ValueError, tarfile.TarError, OSError):
            if expected:
                raise
        else:
            if not expected:
                raise AssertionError("invalid archive was accepted")
            path = os.path.join(root, "installs", "gh", "test", prefix, "bin", "gh")
            with open(path, "rb") as stream:
                assert stream.read() == b"qualified"

valid = (binary, b"qualified", "file")
check([valid], True)
check([valid, valid], False)
check([valid, (prefix + "/readme", b"x", "file"), (prefix + "/readme", b"x", "file")], False)
check([valid, (prefix + "/../escape", b"x", "file")], False)
check([valid, (prefix + "/alias", b"", "link")], False)
check([(binary, b"changed", "file")], False)
check([valid], False, 1024)
