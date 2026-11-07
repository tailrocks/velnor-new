# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import http.server
import json
import os
import pathlib
import ssl
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request

base = pathlib.Path(sys.argv[1]).resolve()
source_sha = sys.argv[2]
base.mkdir(parents=True, exist_ok=True)


def openssl(*args):
    subprocess.run(
        ["openssl", *args],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


openssl(
    "req",
    "-x509",
    "-newkey",
    "rsa:2048",
    "-nodes",
    "-keyout",
    str(base / "ca.key"),
    "-out",
    str(base / "ca.crt"),
    "-days",
    "1",
    "-subj",
    "/CN=Fixture Root",
    "-addext",
    "basicConstraints=critical,CA:TRUE",
    "-addext",
    "keyUsage=critical,keyCertSign,cRLSign",
)
openssl(
    "req",
    "-newkey",
    "rsa:2048",
    "-nodes",
    "-keyout",
    str(base / "leaf.key"),
    "-out",
    str(base / "leaf.csr"),
    "-subj",
    "/CN=fixture.actions.githubusercontent.com",
)
(base / "ext").write_text(
    "basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:api.github.com,DNS:github.com,DNS:fixture.actions.githubusercontent.com,DNS:crates.io,DNS:index.crates.io\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n"
)
openssl(
    "x509",
    "-req",
    "-in",
    str(base / "leaf.csr"),
    "-CA",
    str(base / "ca.crt"),
    "-CAkey",
    str(base / "ca.key"),
    "-CAcreateserial",
    "-out",
    str(base / "leaf.crt"),
    "-days",
    "1",
    "-extfile",
    str(base / "ext"),
)
subprocess.run(
    [
        "cp",
        str(base / "ca.crt"),
        "/usr/local/share/ca-certificates/coordinator-fixture.crt",
    ],
    check=True,
)
subprocess.run(
    ["update-ca-certificates"],
    check=True,
    stdout=subprocess.DEVNULL,
    stderr=subprocess.DEVNULL,
)
counts = {"oidc": 0, "exchange": 0, "revoke": 0}


class Handler(http.server.BaseHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Connection", "close")
        self.close_connection = True
        super().end_headers()

    def log_message(self, fmt, *args):
        sys.stderr.write("https-fixture: " + fmt % args + "\n")

    def reply(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.headers.get("host") == "index.crates.io" and os.environ.get("VELNOR_FIXTURE_REAL_CARGO"):
            origin = os.environ["VELNOR_FIXTURE_CARGO_API_ORIGIN"]
            parsed = urllib.parse.urlsplit(origin)
            if (parsed.scheme != "http" or parsed.hostname != "127.0.0.1"
                    or parsed.port is None or parsed.path or parsed.query or parsed.fragment):
                raise ValueError("fixture registry origin must be loopback")
            if self.path == "/config.json":
                return self.reply(200, {"api": origin, "dl": origin + "/crates"})
            try:
                with urllib.request.urlopen(origin + self.path, timeout=5) as response:
                    data = response.read(1024 * 1024 + 1)
                if len(data) > 1024 * 1024:
                    return self.reply(413, {"error": "index too large"})
                self.send_response(200)
                self.send_header("Content-Length", str(len(data) + 1))
                self.end_headers()
                self.wfile.write(data + b"\n")
                return
            except urllib.error.HTTPError as error:
                return self.reply(error.code, {"error": "not found"})
        if self.path == "/__fixture/state":
            return self.reply(200, counts)
        if self.path.startswith("/oidc"):
            counts["oidc"] += 1
            if os.environ.get("VELNOR_FIXTURE_FAILURE") == "oidc":
                return self.reply(403, {"error": "denied"})
            assert self.headers.get("Authorization") == "Bearer fixture-oidc"
            return self.reply(200, {"value": "fixture-jwt"})
        pr = {
            "user": {"id": 1, "login": "fixture"},
            "number": 1,
            "html_url": "https://github.com/owner/project/pull/1",
            "head": {"ref": "release-plz/fixture", "sha": source_sha},
            "title": "fixture",
            "body": None,
            "labels": [],
        }
        return self.reply(
            200,
            [{"author": None, "sha": source_sha}]
            if self.path.endswith("/pulls/1/commits")
            else [pr],
        )

    def do_POST(self):
        if (
            self.headers.get("host") == "crates.io"
            and self.path == "/api/v1/trusted_publishing/tokens"
        ):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            assert body == {"jwt": "fixture-jwt"}
            counts["exchange"] += 1
            return self.reply(200, {"token": "fixture-scoped"})
        self.reply(403, {"error": "denied"})

    def do_DELETE(self):
        assert (
            self.headers.get("host") == "crates.io"
            and self.path == "/api/v1/trusted_publishing/tokens"
            and self.headers.get("Authorization") == "Bearer fixture-scoped"
        )
        counts["revoke"] += 1
        self.send_response(204)
        self.send_header("Content-Length", "0")
        self.end_headers()


server = http.server.ThreadingHTTPServer(("127.0.0.1", 443), Handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(base / "leaf.crt", base / "leaf.key")
server.socket = context.wrap_socket(server.socket, server_side=True)
print("https fixture ready", flush=True)
server.serve_forever()
