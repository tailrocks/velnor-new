# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import http.server
import io
import json
import pathlib
import ssl
import sys
import urllib.parse
import zipfile

state = pathlib.Path(sys.argv[1])
payload = pathlib.Path(sys.argv[2])
tls = pathlib.Path(sys.argv[3])
artifact = sys.argv[4]
base = "/repos/tailrocks/velnor-new/actions"
listing = base + "/runs/731947/artifacts"
archive = base + "/artifacts/42/zip"


def bundle():
    output = io.BytesIO()
    paths = []
    for path in payload.rglob("*"):
        paths.append(path)
        if len(paths) > 128:
            raise ValueError("too many fixture members")
    paths.sort()
    total = 0
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_STORED) as result:
        for path in paths:
            if path.is_symlink():
                raise ValueError("fixture symlink")
            if path.is_dir():
                continue
            if not path.is_file():
                raise ValueError("invalid fixture member")
            with path.open("rb") as stream:
                data = stream.read(1048577)
            total += len(data)
            if len(data) > 1048576 or total > 8388608:
                raise ValueError("fixture exceeds bounds")
            result.writestr(path.relative_to(payload).as_posix(), data)
    return output.getvalue()


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        route = urllib.parse.urlsplit(self.path)
        if self.headers.get("Authorization") != "token fixture_token":
            self.send_error(401)
            return
        mode = (state / "mode").read_text()
        if route.path == listing and urllib.parse.parse_qs(route.query) == {"per_page": ["100"]}:
            name = "wrong-artifact" if mode == "wrong-id" else artifact
            artifacts = [] if mode == "missing" else [{"name": name, "size_in_bytes": 1, "expired": False, "archive_download_url": "https://api.github.com" + archive}]
            self.respond(json.dumps({"artifacts": artifacts}).encode(), "application/json")
        elif route.path == archive:
            if mode == "missing-zip":
                self.send_error(404)
                return
            with (state / "gh-requests.tsv").open("a") as log:
                log.write("731947\t" + artifact + "\ttailrocks/velnor-new\n")
            self.respond(b"corrupt zip" if mode == "corrupt" else bundle(), "application/zip")
        else:
            self.send_error(404)

    def respond(self, data, content_type):
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


server = http.server.ThreadingHTTPServer(("127.0.0.1", 443), Handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(tls / "cert.pem", tls / "key.pem")
server.socket = context.wrap_socket(server.socket, server_side=True)
(state / "ready").write_text("ready")
server.serve_forever()
