# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import http.server
import json
import pathlib
import ssl
import sys
import urllib.parse

state, tls, fixture = map(pathlib.Path, sys.argv[1:])
manifest = (fixture / "release-manifest.json").read_bytes()
bundle = (fixture / "release-manifest.json.intoto.jsonl").read_bytes()
source = "f1041f322c54cd7edfdef06afd307799eeaa801a"
base = "/repos/tailrocks/velnor-new"
last_mode = None
tag_reads = 0


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def respond(self, value, binary=False):
        data = value if binary else json.dumps(value).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream" if binary else "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        if self.path != "/graphql":
            self.send_error(404)
            return
        self.rfile.read(int(self.headers.get("Content-Length", "0")))
        self.respond({"data": {"repository": {"release": {"databaseId": 7, "isDraft": False}}}})

    def do_GET(self):
        global last_mode, tag_reads
        mode = (state / "mode").read_text() if (state / "mode").exists() else "ok"
        if mode != last_mode:
            tag_reads = 0
            last_mode = mode
        path = urllib.parse.urlsplit(self.path).path
        if path == base:
            return self.respond({"id": 1390620900, "full_name": "tailrocks/velnor-new", "default_branch": "main"})
        if path == base + "/releases/7":
            return self.respond({"id": 7, "tag_name": "v0.1.5", "draft": False, "prerelease": False, "immutable": True, "assets": []})
        if path == base + "/git/matching-refs/tags/v0.1.5":
            tag_reads += 1
            sha = "d" * 40 if mode == "moved-tag" and tag_reads > 1 else source
            return self.respond([{"ref": "refs/tags/v0.1.5", "object": {"type": "commit", "sha": sha}}])
        if path == base + "/releases/7/assets":
            def asset(identity, name, data):
                size = 2 * 1024 * 1024 if mode == "size" and identity == 11 else len(data)
                digest = "0" * 64 if mode == "digest" and identity == 11 else hashlib.sha256(data).hexdigest()
                return {"id": identity, "name": name, "size": size, "state": "uploaded", "digest": "sha256:" + digest}
            assets = [asset(11, "release-manifest.json", manifest), asset(12, "release-manifest.json.intoto.jsonl", bundle)]
            if mode == "missing": assets.pop()
            if mode == "duplicate": assets.append(asset(13, "release-manifest.json", manifest))
            return self.respond(assets)
        if path == base + "/releases/assets/11": return self.respond(manifest, binary=True)
        if path == base + "/releases/assets/12": return self.respond(bundle, binary=True)
        self.send_error(404)


server = http.server.ThreadingHTTPServer(("127.0.0.1", 443), Handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(tls / "cert.pem", tls / "key.pem")
server.socket = context.wrap_socket(server.socket, server_side=True)
(state / "ready").write_text("ready")
server.serve_forever()
