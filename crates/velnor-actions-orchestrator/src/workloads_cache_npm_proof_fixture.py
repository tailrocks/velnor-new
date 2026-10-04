"""Local transport seam; never reachable from generated workflow inputs."""
import base64
import hashlib
import http.client
import http.server
import io
import json
import os
import sys
import threading
import unittest.mock

engine = {'__name__': 'fixture_engine'}
exec(sys.argv[1], engine)
TARBALL = b'public exact npm tarball bytes'
INTEGRITY = 'sha512-' + base64.b64encode(hashlib.sha512(TARBALL).digest()).decode()
source = {'name': '@scope/pkg', 'version': '1.2.3',
          'resolved': 'https://registry.npmjs.org/@scope/pkg/-/pkg-1.2.3.tgz',
          'integrity': INTEGRITY}
requests = []
mode = 'valid'

def qualify_sources(sources):
    try:
        return sorted({engine['qualify_verified'](source, engine['Budget']()) for source in sources})
    except (engine['PublicSourceDenied'], engine['PublicSourceUnavailable'],
            engine['PublicSourceIntegrityMismatch'], engine['UnsupportedRegistry']):
        return []


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        requests.append((self.path, dict(self.headers)))
        status = 200
        if self.path.endswith('.tgz'):
            body = TARBALL if mode != 'wrongbytes' else b'private unrelated bytes'
        else:
            assert self.path == '/@scope%2Fpkg/1.2.3', self.path
            metadata = {'name': source['name'], 'version': source['version'],
                        'dist': {'integrity': INTEGRITY, 'tarball': source['resolved']}}
            if mode in ('401', '403', '404'):
                status = int(mode)
            elif mode == 'redirect':
                status = 302
            elif mode == 'missing':
                del metadata['dist']
            elif mode == 'wrongintegrity':
                metadata['dist']['integrity'] = 'sha512-' + base64.b64encode(b'x' * 64).decode()
            elif mode == 'wrongtarball':
                metadata['dist']['tarball'] = 'https://registry.npmjs.org/private/-/private.tgz'
            elif mode == 'wrongversion':
                metadata['version'] = '9.9.9'
            body = json.dumps(metadata).encode()
        self.send_response(status)
        if mode == 'redirect':
            self.send_header('Location', 'https://private.example/secret')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
thread = threading.Thread(target=server.serve_forever, daemon=True)
thread.start()


def local_https(host, timeout, context):
    assert host == 'registry.npmjs.org'
    return http.client.HTTPConnection('127.0.0.1', server.server_port, timeout=timeout)


for key in ('NPM_TOKEN', 'NODE_AUTH_TOKEN', 'npm_config__authToken',
            'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY'):
    os.environ[key] = 'ambient-private-token-sentinel'
try:
    # Patches the internal HTTPS constructor solely inside the local test.
    with unittest.mock.patch.object(http.client, 'HTTPSConnection', side_effect=local_https):
        for case in ('valid', '401', '403', '404', 'redirect', 'missing',
                     'wrongintegrity', 'wrongtarball', 'wrongversion', 'wrongbytes'):
            mode = case
            requests.clear()
            actual = qualify_sources([source])
            assert actual == ([INTEGRITY] if case == 'valid' else []), (case, actual)
            assert len(requests) == (2 if case in ('valid', 'wrongbytes') else 1)
            for path, headers in requests:
                lowered = {key.lower(): value for key, value in headers.items()}
                assert 'authorization' not in lowered and 'cookie' not in lowered
                assert 'proxy-authorization' not in lowered
                assert 'ambient-private-token-sentinel' not in str(headers)
        requests.clear()
        for url in ('http://registry.npmjs.org/pkg', 'https://private.example/pkg',
                    'https://token@registry.npmjs.org/pkg',
                    'https://registry.npmjs.org/pkg?token=secret'):
            candidate = dict(source, resolved=url)
            assert qualify_sources([candidate]) == []
        assert requests == []
        def interrupted(url, deadline):
            if url.endswith('.tgz'):
                raise http.client.IncompleteRead(b'partial', 20)
            metadata = dict(name=source['name'], version=source['version'],
                            dist=dict(integrity=INTEGRITY, tarball=source['resolved']))
            return io.BytesIO(), io.BytesIO(json.dumps(metadata).encode())
        try:
            engine['qualify_verified'](source, engine['Budget'](), interrupted)
        except engine['PublicSourceUnavailable']:
            pass
        else:
            raise AssertionError('interrupted HTTP source must be unavailable')
finally:
    server.shutdown()
    server.server_close()
    thread.join()
