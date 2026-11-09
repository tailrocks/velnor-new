# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import importlib.util
import io
from pathlib import Path
import stat
import struct
import sys
import tempfile
import time
import zipfile

spec = importlib.util.spec_from_file_location("preview_artifact", sys.argv[1])
p = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p)
linux_target = "x86_64-unknown-linux-gnu"
darwin_target = "aarch64-apple-darwin"
name = "velnor-actions-" + linux_target
darwin_name = "velnor-actions-" + darwin_target
elf = b"\x7fELF\x02\x01\x01" + b"\0" * 11 + b"\x3e\x00" + b"fixture"

macho = struct.pack("<IIIIIIII", 0xFEEDFACF, 0x0100000C, 0, 2, 0, 0, 0, 0) + b"fixture"

def archive(payload=elf, entry=name, extra=False, mode=stat.S_IFREG | 0o644, compression=zipfile.ZIP_STORED):
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as output:
        item = zipfile.ZipInfo(entry)
        item.create_system = 3
        item.compress_type = compression
        item.external_attr = mode << 16
        output.writestr(item, payload)
        if extra:
            output.writestr(zipfile.ZipInfo(extra if isinstance(extra, str) else "other"), b"foreign")
    return buffer.getvalue()

class Client:
    token = "fixture-token"
    deadline = 0

class Response:
    def __init__(self, status, data=b"", location=""):
        self.status, self.data, self.location = status, data, location
    def getheader(self, name, default=None):
        return self.location if name == "Location" else default
    def read(self, limit):
        result, self.data = self.data[:limit], self.data[limit:]
        return result

class Network:
    data = b""
    location = "https://productionresultssa32.blob.core.windows.net/fixture?sig=secret"
    redirects = 0
    requests = []
    def __init__(self, host, **_):
        self.host = host
        self.sock = None
    def request(self, method, path, headers):
        assert method == "GET"
        self.requests.append((self.host, path, headers))
        if self.host == "api.github.com":
            assert headers.get("Authorization") == "Bearer fixture-token"
            self.response = Response(302, location=self.location)
        else:
            assert "Authorization" not in headers and "Cookie" not in headers
            if self.redirects:
                type(self).redirects -= 1
                self.response = Response(302, location=self.location)
            else:
                self.response = Response(200, self.data)
    def getresponse(self):
        return self.response
    def close(self):
        pass

p.http.client.HTTPSConnection = Network

def run(label, data, *, digest=None, size=None, location=None, redirects=0, expired=False, reject=False, target=linux_target, expected_payload=elf):
    with tempfile.TemporaryDirectory() as temporary:
        output = Path(temporary) / name
        sentinel = Path(temporary) / "foreign-sentinel"
        sentinel.write_bytes(b"foreign")
        Network.data, Network.requests, Network.redirects = data, [], redirects
        Network.location = location or "https://productionresultssa32.blob.core.windows.net/fixture?sig=secret"
        Client.deadline = time.monotonic() - 1 if expired else time.monotonic() + 20
        artifact = {"id": 7, "size_in_bytes": len(data) if size is None else size, "digest": "sha256:" + (hashlib.sha256(data).hexdigest() if digest is None else digest)}
        try:
            result = p.download_verified_payload(Client(), artifact, output, target)
        except (ValueError, OSError, TimeoutError, zipfile.BadZipFile):
            assert reject, label + " unexpectedly rejected"
            assert not output.exists(), label + " wrote an output"
            assert sentinel.read_bytes() == b"foreign"
            return
        assert not reject, label + " was accepted"
        assert output.read_bytes() == expected_payload and stat.S_IMODE(output.stat().st_mode) == 0o500
        assert result == {"target": target, "name": "velnor-actions-" + target, "size": len(expected_payload), "sha256": hashlib.sha256(expected_payload).hexdigest()}
        assert sentinel.read_bytes() == b"foreign"
        assert len(Network.requests) == 2
        assert Network.requests[0][1] == "/repos/tailrocks/velnor-new/actions/artifacts/7/zip", label + " requested wrong artifact"

run("valid producer artifact", archive())
run("wrong archive digest", archive(), digest="0" * 64, reject=True)
run("wrong archive size", archive(), size=1, reject=True)
run("foreign ZIP member", archive(entry="../" + name), reject=True)
run("multiple ZIP members", archive(extra=True), reject=True)
run("duplicate ZIP members", archive(extra=name), reject=True)
run("symlink ZIP member", archive(mode=stat.S_IFLNK | 0o777), reject=True)
run("non-ELF binary", archive(payload=b"x" * 30), reject=True)
run("wrong ELF endianness", archive(payload=elf[:5] + b"\x02" + elf[6:]), reject=True)
run("wrong ELF architecture", archive(payload=elf[:18] + b"\xb7\x00" + elf[20:]), reject=True)
run("foreign redirect", archive(), location="https://evil.example/fixture", reject=True)
run("excess redirects", archive(), redirects=4, reject=True)
run("expired deadline", archive(), expired=True, reject=True)
large = bytearray(archive())
index = large.index(b"PK\x01\x02")
struct.pack_into("<I", large, index + 24, 256 * 1024 * 1024 + 1)
run("declared oversized payload", bytes(large), reject=True)
encrypted = bytearray(archive())
index = encrypted.index(b"PK\x01\x02")
struct.pack_into("<H", encrypted, index + 8, 1)
run("encrypted ZIP member", bytes(encrypted), reject=True)
run("unsupported ZIP compression", archive(compression=zipfile.ZIP_BZIP2), reject=True)

run("valid Darwin producer artifact", archive(payload=macho, entry=darwin_name), target=darwin_target, expected_payload=macho)
run("Darwin payload in Linux slot", archive(payload=macho), reject=True)
run("Linux payload in Darwin slot", archive(payload=elf, entry=darwin_name), target=darwin_target, reject=True)
run("Darwin payload with Linux name", archive(payload=macho), target=darwin_target, reject=True)
run("unsupported target", archive(), target="x86_64-apple-darwin", reject=True)
run("truncated Mach-O header", archive(payload=macho[:28], entry=darwin_name), target=darwin_target, reject=True)
run("wrong Mach-O endianness", archive(payload=b"\xfe\xed\xfa\xcf" + macho[4:], entry=darwin_name), target=darwin_target, reject=True)
run("wrong Mach-O architecture", archive(payload=macho[:4] + struct.pack("<I", 0x01000007) + macho[8:], entry=darwin_name), target=darwin_target, reject=True)
run("Mach-O library instead of executable", archive(payload=macho[:12] + struct.pack("<I", 6) + macho[16:], entry=darwin_name), target=darwin_target, reject=True)
run("fat Mach-O instead of exact target", archive(payload=b"\xca\xfe\xba\xbe" + macho[4:], entry=darwin_name), target=darwin_target, reject=True)
