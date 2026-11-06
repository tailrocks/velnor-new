# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import os
import pathlib
import subprocess
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

FILTER = 'blob:limit=18446744073709551615'
TOKEN = 'fixture-only-dummy'
AUTH = 'basic ' + base64.b64encode(('x-access-token:' + TOKEN).encode()).decode()


def git(root, *args, auth=False, lazy=True):
    env = {'PATH': '/usr/bin:/bin', 'HOME': '/nonexistent', 'GIT_CONFIG_NOSYSTEM': '1',
           'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_LFS_SKIP_SMUDGE': '1',
           'GIT_TERMINAL_PROMPT': '0', 'GIT_NO_LAZY_FETCH': '0' if lazy else '1'}
    if auth:
        env.update({'GIT_CONFIG_COUNT': '1', 'GIT_CONFIG_KEY_0': 'http.extraHeader',
                    'GIT_CONFIG_VALUE_0': 'AUTHORIZATION: ' + AUTH})
    return subprocess.run(['/usr/bin/git', '-C', str(root), *args], env=env, check=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30).stdout.strip()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        self.respond()

    def do_POST(self):
        self.respond()

    def respond(self):
        if self.headers.get('Authorization') != AUTH:
            self.send_error(401)
            return
        self.server.authenticated += 1
        assert TOKEN not in self.path
        length = int(self.headers.get('Content-Length', '0'))
        if length > 16 * 1024 * 1024:
            self.send_error(400)
            return
        path, _, query = self.path.partition('?')
        env = {'PATH': '/usr/bin:/bin', 'GIT_PROJECT_ROOT': str(self.server.root),
               'GIT_HTTP_EXPORT_ALL': '1', 'PATH_INFO': path, 'QUERY_STRING': query,
               'REQUEST_METHOD': self.command, 'CONTENT_TYPE': self.headers.get('Content-Type', ''),
               'CONTENT_LENGTH': str(length), 'GIT_PROTOCOL': self.headers.get('Git-Protocol', '')}
        out = subprocess.run(['/usr/bin/git', 'http-backend'], env=env, input=self.rfile.read(length),
                             stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=15, check=True).stdout
        headers, payload = out.split(b'\r\n\r\n', 1)
        self.send_response(200)
        for line in headers.split(b'\r\n'):
            key, value = line.split(b':', 1)
            if key.lower() not in (b'status', b'transfer-encoding'):
                self.send_header(key.decode('ascii'), value.strip().decode('ascii'))
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def main():
    with tempfile.TemporaryDirectory() as temporary:
        root = pathlib.Path(temporary)
        source, bare = root / 'source', root / 'repo.git'
        source.mkdir()
        git(source, 'init', '-q')
        git(source, 'config', 'user.name', 'fixture')
        git(source, 'config', 'user.email', 'fixture@example.invalid')
        (source / 'excluded').mkdir()
        (source / 'excluded/target').write_text('hidden')
        (source / 'link').symlink_to('excluded/target')
        (source / 'kept').write_text('visible')
        git(source, 'add', '.')
        git(source, 'commit', '-qm', 'fixture')
        oid = git(source, 'rev-parse', ':excluded/target').decode()
        git(root, 'clone', '-q', '--bare', str(source), str(bare))
        git(bare, 'config', 'uploadpack.allowFilter', 'true')
        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        server.root, server.authenticated = root, 0
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            url = f'http://127.0.0.1:{server.server_port}/repo.git'
            for filter_value, present in [('blob:none', False), (FILTER, True)]:
                checkout = root / ('full-blobs' if present else 'missing-blobs')
                git(root, '-c', 'protocol.http.allow=always', 'clone', '-q', '--depth=1',
                    '--filter=' + filter_value, '--no-checkout', url, str(checkout), auth=True)
                git(checkout, 'sparse-checkout', 'init', '--no-cone')
                (checkout / '.git/info/sparse-checkout').write_text('/*\n!/excluded/**\n')
                git(checkout, 'checkout', '-q', auth=True)
                assert not (checkout / 'excluded/target').exists()
                assert (checkout / 'link').is_symlink()
                assert git(checkout, 'ls-files', '-t', 'excluded/target').startswith(b'S ')
                result = subprocess.run(['/usr/bin/git', '-C', str(checkout), 'cat-file', '-e', oid],
                                        env={'PATH': '/usr/bin:/bin', 'HOME': '/nonexistent',
                                             'GIT_CONFIG_NOSYSTEM': '1', 'GIT_CONFIG_GLOBAL': '/dev/null',
                                             'GIT_NO_LAZY_FETCH': '1'}, stdout=subprocess.DEVNULL,
                                        stderr=subprocess.DEVNULL, timeout=10)
                assert (result.returncode == 0) == present
                config = (checkout / '.git/config').read_bytes()
                assert TOKEN.encode() not in config and AUTH.encode() not in config
                if present:
                    git(root, 'clone', '-q', '--no-local', str(checkout), str(root / 'child'), lazy=False)
                    assert (root / 'child/excluded/target').read_text() == 'hidden'
            assert server.authenticated > 0
        finally:
            server.shutdown()
            thread.join()
            server.server_close()


if __name__ == '__main__':
    main()
