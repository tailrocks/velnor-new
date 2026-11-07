# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import http.server
import json
import pathlib
import sys
import urllib.parse

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
archives = {}
for entry in manifest["archives"]:
    path = pathlib.Path(entry["path"]).resolve(strict=True)
    data = path.read_bytes()
    if len(data) != entry["size"] or not 0 < len(data) < 10_000_000:
        raise ValueError("invalid fixture archive")
    archives[entry["name"]] = (path, hashlib.sha256(data).hexdigest())

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        path = urllib.parse.urlsplit(self.path).path
        parts = path.strip("/").split("/")
        body = None
        if len(parts) >= 4 and parts[:3] == ["api", "v1", "crates"]:
            name = parts[3]
            if name in archives:
                if len(parts) == 4:
                    body = {"crate": {"id": name, "name": name, "versions": [1]},
                            "versions": [{"id": 1, "crate": name, "num": "0.1.0", "yanked": False}]}
                elif parts[4:] == ["owners"]:
                    body = {"users": [{"id": 370532, "login": "owner", "kind": "user"}]}
                elif parts[4:] == ["0.1.0"]:
                    body = {"version": {"id": 1, "crate": name, "num": "0.1.0",
                                        "checksum": archives[name][1], "yanked": False, "features": {}}}
        if len(parts) == 3 and parts[0] == "crates" and parts[1] in archives:
            name = parts[1]
            if parts[2] == f"{name}-0.1.0.crate":
                return self.respond(archives[name][0].read_bytes(), "application/octet-stream")
        if len(parts) >= 2 and parts[0] == "index" and parts[-1] in archives:
            name = parts[-1]
            body = {"name": name, "vers": "0.1.0", "cksum": archives[name][1],
                    "yanked": False, "deps": [], "features": {}}
        if body is None:
            self.send_error(404)
        else:
            self.respond(json.dumps(body).encode(), "application/json")

    def respond(self, data, content_type):
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *_args):
        pass

server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
print(json.dumps({"origin": f"http://127.0.0.1:{server.server_port}"}), flush=True)
server.serve_forever()
