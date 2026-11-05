# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import base64
import http.server
import os
import pathlib
import ssl
import subprocess
import tempfile
import threading
import urllib.parse


def run(arguments, **kwargs):
    result = subprocess.run(arguments, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **kwargs)
    if result.returncode:
        raise RuntimeError("git fixture command failed")


with tempfile.TemporaryDirectory() as temporary:
    root = pathlib.Path(temporary)
    repo = root / "repos" / "owner" / "project.git"
    work = root / "work"
    home = root / "home"
    repo.parent.mkdir(parents=True)
    work.mkdir()
    home.mkdir()
    certificate = root / "certificate.pem"
    key = root / "key.pem"
    run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1", "-subj", "/CN=github.com", "-addext", "subjectAltName=DNS:github.com", "-keyout", str(key), "-out", str(certificate)])
    run(["git", "init", "--bare", "-q", str(repo)])
    run(["git", f"--git-dir={repo}", "config", "http.receivepack", "true"])
    (repo / "git-daemon-export-ok").touch()
    fixture_token = os.environ["GH_TOKEN"]
    backend = pathlib.Path(subprocess.check_output(["git", "--exec-path"], text=True).strip()) / "git-http-backend"
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(certificate, key)

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_CONNECT(self):
            if self.path != "github.com:443":
                self.send_error(403)
                return
            self.wfile.write(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            self.wfile.flush()
            wrapped = context.wrap_socket(self.connection, server_side=True)
            Handler(wrapped, self.client_address, self.server)

        def do_GET(self):
            self.serve_git()

        def do_POST(self):
            self.serve_git()

        def serve_git(self):
            expected = "Basic " + base64.b64encode(("x-access-token:" + fixture_token).encode()).decode()
            if self.headers.get("Authorization") != expected:
                self.send_response(401)
                self.send_header("WWW-Authenticate", 'Basic realm="fixture"')
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            parsed = urllib.parse.urlsplit(self.path)
            size = int(self.headers.get("Content-Length", "0"))
            env = {"PATH": "/usr/bin:/bin", "GIT_PROJECT_ROOT": str(repo.parent.parent), "GIT_HTTP_EXPORT_ALL": "1", "PATH_INFO": parsed.path, "QUERY_STRING": parsed.query, "REQUEST_METHOD": self.command, "CONTENT_TYPE": self.headers.get("Content-Type", ""), "CONTENT_LENGTH": str(size), "REMOTE_USER": "fixture"}
            result = subprocess.run([backend], input=self.rfile.read(size), env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, check=True)
            header, body = result.stdout.split(b"\r\n\r\n", 1)
            headers = header.decode().split("\r\n")
            status = next((int(line.split()[1]) for line in headers if line.lower().startswith("status:")), 200)
            self.send_response(status)
            for line in headers:
                name, value = line.split(":", 1)
                if name.lower() not in ("status", "transfer-encoding", "content-length"):
                    self.send_header(name, value.strip())
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    proxy = f"http://127.0.0.1:{server.server_port}"
    publish = os.environ["VELNOR_PUBLISH"]
    marker = "number=$(velnor_gh pr list"
    if publish.count(marker) != 1:
        raise RuntimeError("publish API boundary missing")
    publish = publish.split(marker, 1)[0]
    upgrade = root / "upgrade"
    upgrade.mkdir()
    (upgrade / "summary.md").write_text("fixture\n", encoding="utf-8")
    env = {"PATH": "/usr/bin:/bin", "HOME": str(home), "MISE_DATA_DIR": os.environ["MISE_DATA_DIR"], "GIT_TERMINAL_PROMPT": "0", "GIT_SSL_CAINFO": str(certificate), "HTTPS_PROXY": proxy, "https_proxy": proxy, "NO_PROXY": "", "no_proxy": "", "GH_TOKEN": fixture_token, "VELNOR_PUBLISH": publish, "WORK": str(work), "RUNNER_TEMP": str(root), "VELNOR_UPGRADE_DIR": str(upgrade), "VELNOR_UPGRADE_BRANCH": "upgrade", "VELNOR_DEFAULT_BRANCH": "main", "GITHUB_REPOSITORY": "owner/project", "VELNOR_GH_VERSION": "2.103.3"}
    script = r'''set -eu
cd "$WORK"
git init -q
git config user.name fixture
git config user.email fixture@example.com
git config credential.helper '!touch "$HOME/hostile-local"'
git config --global credential.helper '!touch "$HOME/hostile-global"'
git config core.sshCommand 'touch "$HOME/hostile-ssh"'
git config 'url.ssh://invalid/.insteadOf' 'https://github.com/'
printf '#!/bin/sh\ntouch "$HOME/hostile-hook"\n' > .git/hooks/pre-push
chmod +x .git/hooks/pre-push
printf 'fixture\n' > content
git add content
git commit -qm fixture
git remote add origin ssh://invalid/project.git
printf 'updated\n' > content
git add content
git config core.fsmonitor 'touch "$HOME/hostile-fsmonitor"'
VELNOR_VERIFIED_TREE=$(git -c core.fsmonitor=false write-tree); export VELNOR_VERIFIED_TREE
eval "$VELNOR_PUBLISH"
[ ! -e "$HOME/hostile-local" ] && [ ! -e "$HOME/hostile-global" ] && [ ! -e "$HOME/hostile-ssh" ] && [ ! -e "$HOME/hostile-hook" ] && [ ! -e "$HOME/hostile-fsmonitor" ]
if printf 'protocol=https\nhost=evil.example\n\n' | git credential fill >/dev/null 2>&1; then exit 1; fi
[ ! -e "$HOME/hostile-local" ] && [ ! -e "$HOME/hostile-global" ] && [ ! -e "$HOME/hostile-ssh" ] && [ ! -e "$HOME/hostile-hook" ] && [ ! -e "$HOME/hostile-fsmonitor" ]
'''
    try:
        run(["/bin/bash", "-c", script], env=env)
        def output(arguments):
            return subprocess.check_output(arguments, env=env, stderr=subprocess.DEVNULL).strip()
        ref = "refs/heads/upgrade"
        first = output(["git", f"--git-dir={repo}", "rev-parse", ref])
        base = output(["git", "-C", str(work), "rev-parse", "HEAD"])
        assert output(["git", f"--git-dir={repo}", "rev-parse", f"{ref}^{{tree}}"]) == output(["git", "-C", str(work), "-c", "core.fsmonitor=false", "write-tree"])
        assert output(["git", f"--git-dir={repo}", "rev-parse", f"{ref}^"]) == base
        replay = 'cd "$WORK"; VELNOR_VERIFIED_TREE=$(git -c core.fsmonitor=false write-tree); export VELNOR_VERIFIED_TREE; eval "$VELNOR_PUBLISH"'
        run(["/bin/bash", "-c", replay], env=env)
        assert output(["git", f"--git-dir={repo}", "rev-parse", ref]) == first
        (work / "content").write_text("new update\n", encoding="utf-8")
        run(["git", "-C", str(work), "-c", "core.fsmonitor=false", "add", "content"], env=env)
        run(["/bin/bash", "-c", replay], env=env)
        assert output(["git", f"--git-dir={repo}", "rev-parse", f"{ref}^{{tree}}"]) == output(["git", "-C", str(work), "-c", "core.fsmonitor=false", "write-tree"])
        assert output(["git", f"--git-dir={repo}", "rev-parse", f"{ref}^1"]) == first
        assert output(["git", f"--git-dir={repo}", "rev-parse", f"{ref}^2"]) == base
        assert all(not (home / name).exists() for name in ("hostile-local", "hostile-global", "hostile-ssh", "hostile-hook", "hostile-fsmonitor"))
        basic = base64.b64encode(("x-access-token:" + fixture_token).encode())
        for path in root.rglob("*"):
            if path.is_file() and not path.is_symlink():
                data = path.read_bytes()
                assert fixture_token.encode() not in data and basic not in data
    finally:
        server.shutdown()
        thread.join()
