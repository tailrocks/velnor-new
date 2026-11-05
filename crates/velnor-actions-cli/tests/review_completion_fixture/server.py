# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import http.server
import json
import pathlib
import ssl
import subprocess
import sys

home = pathlib.Path(sys.argv[1])
tls = pathlib.Path(sys.argv[2])
script = pathlib.Path(sys.argv[3])


class Handler(http.server.BaseHTTPRequestHandler):
    requests = 0

    def log_message(self, *_args):
        pass

    def do_GET(self):
        self.respond()

    def do_POST(self):
        self.respond()

    def do_PATCH(self):
        self.respond()

    def respond(self):
        type(self).requests += 1
        if type(self).requests > 2048:
            self.send_error(429)
            return
        if (self.headers.get("Authorization") != "token fixture-nonsecret-token"
                or self.headers.get("Accept") != "application/vnd.github+json"
                or self.headers.get("X-GitHub-Api-Version") != "2026-03-10"
                or len(self.path) > 8192):
            self.send_error(400)
            return
        length = self.headers.get("Content-Length", "0")
        if not length.isascii() or not length.isdecimal() or len(length) > 8:
            self.send_error(400)
            return
        size = int(length)
        if size > 2 * 1024 * 1024:
            self.send_error(413)
            return
        try:
            data = json.loads(self.rfile.read(size)) if size else {}
        except (ValueError, UnicodeError):
            self.send_error(400)
            return
        if not isinstance(data, dict):
            self.send_error(400)
            return
        route = self.path.lstrip("/")
        args = ["api", "--include", "-H", "Accept: application/vnd.github+json", "-H", "X-GitHub-Api-Version: 2026-03-10", route]
        if route == "graphql":
            for key, value in data.items():
                if key == "variables":
                    if not isinstance(value, dict) or any(not isinstance(item, (str, int)) for item in value.values()):
                        self.send_error(400)
                        return
                    for variable, item in value.items():
                        args.extend(["--field", f"{variable}={item}"])
                else:
                    args.extend(["--raw-field" if key == "query" else "--field", f"{key}={value}"])
        elif self.command == "GET":
            if self.headers.get("Cache-Control") != "no-cache, no-store" or self.headers.get("Pragma") != "no-cache":
                self.send_error(400)
                return
            args.extend(["-H", "Cache-Control: no-cache, no-store", "-H", "Pragma: no-cache"])
        else:
            args.extend(["--method", self.command])
            for key, value in data.items():
                if key == "output":
                    if not isinstance(value, dict) or any(not isinstance(item, str) for item in value.values()):
                        self.send_error(400)
                        return
                    for nested, item in value.items():
                        args.extend(["--raw-field", f"output[{nested}]={item}"])
                else:
                    args.extend(["--raw-field", f"{key}={value}"])
        result = subprocess.run(["/bin/sh", str(script), *args], env={"HOME": str(home), "PATH": "/usr/bin:/bin"}, capture_output=True, timeout=10, check=False)
        if result.returncode or len(result.stdout) > 4 * 1024 * 1024:
            self.send_error(500)
            return
        header, body = result.stdout.split(b"\r\n\r\n", 1)
        status = int(header.splitlines()[0].split()[1])
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


server = http.server.HTTPServer(("127.0.0.1", 443), Handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(tls / "cert.pem", tls / "key.pem")
server.socket = context.wrap_socket(server.socket, server_side=True)
(home / "ready").write_text("ready")
server.serve_forever()
