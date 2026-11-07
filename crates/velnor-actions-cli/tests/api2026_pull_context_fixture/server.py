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
base, head, merge, root_tree, velnor_tree = sys.argv[3:]
repository = "/repos/test-owner/test-tap"

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        parsed = urllib.parse.urlsplit(self.path)
        query = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
        query = [(k, v) for k, v in query if k != "_velnor_read_nonce"]
        route = parsed.path.lstrip("/")
        if query:
            route += "?" + urllib.parse.urlencode(query)
        with (state / "requests.log").open("a") as log:
            log.write("route=" + route + "\n")
        if self.headers.get("Authorization") != "token fixture_token":
            self.send_error(401)
            return
        if (self.headers.get("X-GitHub-Api-Version") != "2026-03-10"
                or self.headers.get("Cache-Control") != "no-cache, no-store"
                or self.headers.get("Pragma") != "no-cache"):
            self.send_error(400)
            return
        accept = self.headers.get("Accept", "")
        if parsed.path in [repository + "/commits/" + v for v in (base, head, merge)]:
            if accept != "application/vnd.github.sha":
                self.send_error(400)
                return
            self.respond(200, parsed.path.rsplit("/", 1)[-1].encode(), "text/plain")
            return
        if accept != "application/vnd.github+json":
            self.send_error(400)
            return
        failure = (state / "failure").read_text()
        mode = (state / "mergeability").read_text()
        if parsed.path == repository:
            if failure == "http":
                self.respond(403, json.dumps({"message":"fixture-secret-canary https://redirect.invalid/private?credential=fixture-signed-canary\u001b[31m"}).encode())
                return
            if failure == "stdout":
                self.respond(200, b"0" * 1048577)
                return
            if failure == "stderr":
                self.send_response(302)
                self.send_header("Location", "https://" + "a" * 1100000 + ".invalid/path")
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            if failure == "deadline":
                time.sleep(15)
        if route == "repos/test-owner/test-tap/pulls/17":
            count = int((state / "pull-count").read_text()) + 1
            (state / "pull-count").write_text(str(count))
            if failure == "merge-api-deadline" and count >= 2:
                time.sleep(15)
            filename = "pull-ready.json" if mode not in ("none", "pending") and count >= 3 else "pull.json"
        elif route == "repos/test-owner/test-tap/git/matching-refs/pull/17/merge":
            if mode == "false" or (mode != "none" and int((state / "pull-count").read_text()) < 3):
                self.respond(200, b"[]")
                return
            count = int((state / "ref-count").read_text()) + 1
            (state / "ref-count").write_text(str(count))
            filename = "merge-ref-first.json" if count == 1 else "merge-ref-second.json"
        else:
            filename = {
                "repos/test-owner/test-tap": "repository.json",
                "repos/test-owner/test-tap/git/commits/" + merge: "merge-commit.json",
                "repos/test-owner/test-tap/git/commits/" + base: "base-commit.json",
                "repos/test-owner/test-tap/git/commits/" + head: "head-commit.json",
                "repos/test-owner/test-tap/git/matching-refs/heads/main": "main-ref.json",
                "repos/test-owner/test-tap/branches/main": "branch.json",
                "repos/test-owner/test-tap/compare/" + base + "..." + head + "?per_page=1&page=2": "compare.json",
                "repos/test-owner/test-tap/git/trees/" + root_tree: "root-tree.json",
                "repos/test-owner/test-tap/git/trees/" + velnor_tree: "velnor-tree.json",
            }.get(route)
        if filename is None:
            self.send_error(404)
            return
        self.respond(200, (state / filename).read_bytes())

    def respond(self, status, body, content_type="application/json"):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass

server = http.server.ThreadingHTTPServer(("127.0.0.1", 443), Handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(tls / "cert.pem", tls / "key.pem")
server.socket = context.wrap_socket(server.socket, server_side=True)
(state / "ready").write_text("ready")
server.serve_forever()
