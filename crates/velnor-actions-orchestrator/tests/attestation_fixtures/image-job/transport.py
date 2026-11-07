# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import http.server
import json
import pathlib
import ssl
import sys
import time
import urllib.parse

state = pathlib.Path(sys.argv[1])
tls = pathlib.Path(sys.argv[2])
bundle = pathlib.Path(sys.argv[3]).read_bytes()
if len(bundle) > 1024 * 1024:
    raise RuntimeError("image attestation bundle exceeds limit")
payload = json.dumps({"attestations": [{"bundle": json.loads(bundle)}]}).encode()
route = "/repos/tailrocks/velnor-new/attestations/sha256:41b0c6b72052e4a7f6532180112a7aa9129de552ae29b560931445fcca7844b2"


class Handler(http.server.BaseHTTPRequestHandler):
    def setup(self):
        super().setup()
        self.connection.settimeout(5)

    def log_message(self, *_args):
        pass

    def do_GET(self):
        parsed = urllib.parse.urlsplit(self.path)
        query = urllib.parse.parse_qs(parsed.query, strict_parsing=True)
        if (parsed.path != route
                or query != {"per_page": ["100"], "predicate_type": ["https://slsa.dev/provenance/v1"]}
                or self.headers.get("Authorization") != "token fixture_token"):
            self.send_error(400)
            return
        if (state / "missing").exists():
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


server = http.server.ThreadingHTTPServer(("127.0.0.1", 443), Handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(tls / "cert.pem", tls / "key.pem")
server.socket = context.wrap_socket(server.socket, server_side=True)
(state / "ready").write_text("ready")
server.timeout = 5
deadline = time.monotonic() + 90
for _ in range(8):
    if time.monotonic() >= deadline:
        break
    server.handle_request()
server.server_close()
