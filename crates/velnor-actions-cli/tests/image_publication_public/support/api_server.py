# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import datetime
import hashlib
import http.server
import json
import os
import pathlib
import stat
import re
import ssl
import subprocess
import sys
import tempfile
import time
import urllib.parse

home = pathlib.Path(sys.argv[1])
tls = pathlib.Path(sys.argv[2])
backend = pathlib.Path(sys.argv[3])


def bounded(path, maximum):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= maximum:
            raise RuntimeError("TUF snapshot file is not bounded regular data")
        data = stream.read(maximum + 1)
    if len(data) != metadata.st_size:
        raise RuntimeError("TUF snapshot file changed")
    return data


class Handler(http.server.BaseHTTPRequestHandler):
    def setup(self):
        super().setup()
        self.connection.settimeout(5)

    def log_message(self, *_args):
        pass

    def tuf_GET(self, host, parsed):
        if self.headers.get("Authorization") or parsed.query:
            self.send_error(400)
            return
        root = pathlib.Path("/tools/image-attestation-tuf")
        identity = bounded(root / "selected", 64).decode()
        if not re.fullmatch(r"[0-9a-f]{64}", identity):
            raise RuntimeError("invalid TUF snapshot identity")
        snapshot = root / identity
        payload = bounded(snapshot / "manifest.json", 65536)
        if len(payload) > 65536 or hashlib.sha256(payload).hexdigest() != identity:
            raise RuntimeError("TUF snapshot manifest mismatch")
        manifest = json.loads(payload)
        entry = manifest["hosts"][host]
        expiry = datetime.datetime.fromisoformat(entry["expires"].replace("Z", "+00:00"))
        if expiry <= datetime.datetime.now(datetime.timezone.utc):
            raise RuntimeError("TUF snapshot expired; provision before suite")
        if parsed.path == f"/{entry['next_root']}.root.json":
            self.send_error(404)
            return
        expected = entry["routes"].get(parsed.path)
        if expected is None or not re.fullmatch(r"[0-9a-f]{64}", expected["sha256"]):
            self.send_error(404)
            return
        body = bounded(snapshot / "objects" / expected["sha256"], 1024 * 1024)
        if len(body) != expected["size"] or len(body) > 1024 * 1024 or hashlib.sha256(body).hexdigest() != expected["sha256"]:
            raise RuntimeError("TUF snapshot response mismatch")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        parsed = urllib.parse.urlsplit(self.path)
        host = self.headers.get("Host", "")
        if host in ("tuf-repo.github.com", "tuf-repo-cdn.sigstore.dev"):
            self.tuf_GET(host, parsed)
            return
        if host != "api.github.com":
            self.send_error(404)
            return
        query = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
        nonces = [value for key, value in query if key == "_velnor_read_nonce"]
        if (not parsed.path.startswith("/repos/tailrocks/velnor-new/")
                and parsed.path != "/repos/tailrocks/velnor-new"):
            print("fixture route rejected", file=sys.stderr)
            self.send_error(404)
            return
        if (self.headers.get("Authorization") != "token fixture-source-token"
                or self.headers.get("Cache-Control") != "no-cache, no-store"
                or self.headers.get("Pragma") != "no-cache"
                or len(nonces) != 1
                or not re.fullmatch(r"[0-9a-f]+-[0-9a-f]+-[0-9a-f]+", nonces[0])):
            print("fixture headers rejected", file=sys.stderr)
            self.send_error(400)
            return
        route = parsed.path.lstrip("/")
        if query:
            route += "?" + urllib.parse.urlencode(query)
        try:
            with tempfile.TemporaryFile() as output:
                result = subprocess.run(
                    ["/bin/sh", "-c", 'ulimit -f 16384; exec "$@"', "fixture", str(backend), "--", "github-api", "api", route],
                    env={"HOME": str(home), "PATH": "/usr/bin:/bin"},
                    stdin=subprocess.DEVNULL,
                    stdout=output,
                    stderr=subprocess.DEVNULL,
                    timeout=10,
                    check=False,
                )
                output.seek(0)
                body = output.read(16 * 1024 * 1024 + 1)
        except (OSError, subprocess.TimeoutExpired):
            print("fixture backend execution failed", file=sys.stderr)
            self.send_error(500)
            return
        if result.returncode != 0 or len(body) > 16 * 1024 * 1024:
            print("fixture backend rejected request", file=sys.stderr)
            self.send_error(500)
            return
        self.send_response(200)
        content_type = "application/octet-stream" if "/tarball/" in parsed.path else "application/json"
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


server = http.server.ThreadingHTTPServer(("127.0.0.1", 443), Handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(tls / "cert.pem", tls / "key.pem")
server.socket = context.wrap_socket(server.socket, server_side=True)
(home / "api-ready").write_text("ready")
server.timeout = 5
deadline = time.monotonic() + 90
for _ in range(128):
    if time.monotonic() >= deadline:
        break
    server.handle_request()
server.server_close()
