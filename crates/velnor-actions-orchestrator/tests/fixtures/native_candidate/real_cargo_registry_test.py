# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
"""Exercise real Cargo's upload protocol against a loopback-only disposable registry."""
import hashlib
import io
import json
import os
import pathlib
import struct
import subprocess
import tarfile
import tempfile
import threading
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Registry(BaseHTTPRequestHandler):
    upload = None
    upload_attempts = 0
    index_checksum = None

    def log_message(self, _format, *args):
        pass

    def do_GET(self):
        if self.path == '/download/velnor-fixture-demo/0.1.1/download' and Registry.upload is not None:
            self.reply(200, Registry.upload[1])
        elif self.path.startswith('/api/v3/') and self.path.endswith('/pulls'):
            self.reply(200, b'[]')
        elif self.path.startswith('/index/') and self.path != '/index/config.json' and Registry.upload is not None:
            metadata, archive = Registry.upload
            record = {'name': metadata['name'], 'vers': metadata['vers'], 'deps': metadata.get('deps', []), 'cksum': hashlib.sha256(archive).hexdigest(), 'features': metadata.get('features', {}), 'yanked': False}
            Registry.index_checksum = record['cksum']
            self.reply(200, (json.dumps(record) + '\n').encode())
        elif self.path == '/index/config.json':
            port = self.server.server_address[1]
            self.reply(200, json.dumps({'api': f'http://127.0.0.1:{port}',
                                        'dl': f'http://127.0.0.1:{port}/download'}).encode())
        else:
            self.reply(404, b'')

    def do_PUT(self):
        Registry.upload_attempts += 1
        if self.path != '/api/v1/crates/new' or Registry.upload is not None:
            self.reply(409, b'')
            return
        length = int(self.headers.get('Content-Length', '0'))
        if not 0 < length <= 16 * 1024 * 1024:
            self.reply(413, b'')
            return
        data = self.rfile.read(length)
        try:
            metadata_len = struct.unpack_from('<I', data)[0]
            metadata_end = 4 + metadata_len
            metadata = json.loads(data[4:metadata_end])
            archive_len = struct.unpack_from('<I', data, metadata_end)[0]
            archive = data[metadata_end + 4:]
            if len(archive) != archive_len or metadata['name'] != 'velnor-fixture-demo' or metadata['vers'] != '0.1.1':
                raise ValueError('invalid upload')
        except (ValueError, KeyError, struct.error, json.JSONDecodeError):
            self.reply(400, b'')
            return
        Registry.upload = (metadata, archive)
        self.reply(200, b'{"warnings":{"invalid_categories":[],"invalid_badges":[],"other":[]}}')

    def reply(self, status, payload):
        self.send_response(status)
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def main():
    if not pathlib.Path('/proc/net/route').is_file() or len(pathlib.Path('/proc/net/route').read_text().splitlines()) != 1:
        raise RuntimeError('fixture requires a network-isolated Linux namespace')
    if hashlib.sha256(pathlib.Path('/release-plz').read_bytes()).hexdigest() != '678330a1f03330fb375d15be8d6e094d0f65d2b70e2af417c69fcec78fa74466':
        raise RuntimeError('release-plz digest mismatch')
    cargo = pathlib.Path(os.environ['VELNOR_FIXTURE_CARGO'])
    if hashlib.sha256(cargo.read_bytes()).hexdigest() != 'a670e02cfa39ac2e53f77a2d911ca1fc9e2c479b0603ec956625be1daf24e3af':
        raise RuntimeError('Cargo executable digest mismatch')
    version = subprocess.run([str(cargo), '--version'], capture_output=True, text=True, check=True, timeout=10).stdout.strip()
    if version != 'cargo 1.99.1 (5f94df478 2026-08-27)':
        raise RuntimeError('Cargo version mismatch')
    Registry.upload = None
    Registry.upload_attempts = 0
    Registry.index_checksum = None
    server = ThreadingHTTPServer(('127.0.0.1', 0), Registry)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        with tempfile.TemporaryDirectory(prefix='velnor-cargo-registry-') as temp:
            root = pathlib.Path(temp) / "source"
            root.mkdir()
            (root / 'src').mkdir()
            (root / 'src/lib.rs').write_text('pub fn fixture() {}\n')
            (root / 'Cargo.toml').write_text('[package]\nname="velnor-fixture-demo"\nversion="0.1.1"\nedition="2024"\nlicense="Apache-2.0"\ndescription="local registry fixture"\n')
            cargo_home = root.parent / 'cargo-home'
            cargo_home.mkdir()
            (cargo_home / 'config.toml').write_text('[registries.fixture]\nindex="sparse+http://127.0.0.1:%d/index/"\n[registry]\nglobal-credential-providers=["cargo:token"]\n' % server.server_address[1])
            (root / 'release-plz.toml').write_text('[workspace]\ngit_release_enable=false\ngit_tag_enable=false\n')
            env = {'PATH': str(cargo.parent) + ':/usr/bin:/bin', 'HOME': str(root), 'CARGO_HOME': str(cargo_home), 'CARGO_REGISTRIES_FIXTURE_TOKEN': 'fixture-only', 'CARGO_NET_RETRY': '0', 'CARGO_TARGET_DIR': str(root.parent / 'target'), 'GIT_TOKEN': 'fixture-only'}
            subprocess.run(['/usr/bin/git', 'init', '-q'], cwd=root, env=env, check=True)
            subprocess.run(['/usr/bin/git', 'config', 'user.name', 'Fixture'], cwd=root, env=env, check=True)
            subprocess.run(['/usr/bin/git', 'config', 'user.email', 'fixture@example.invalid'], cwd=root, env=env, check=True)
            subprocess.run(['/usr/bin/git', 'add', 'Cargo.toml', 'src', 'release-plz.toml'], cwd=root, env=env, check=True)
            subprocess.run(['/usr/bin/git', 'commit', '-qm', 'fixture'], cwd=root, env=env, check=True)
            result = subprocess.run(['/release-plz', 'release', '--registry', 'fixture', '--repo-url', f'http://127.0.0.1:{server.server_address[1]}/example/fixture', '--forge', 'github', '--no-verify', '--allow-dirty'], cwd=root, env=env, capture_output=True, timeout=120, check=False)
            if result.returncode:
                raise RuntimeError(result.stderr.decode(errors='replace'))
            if Registry.upload is None or not Registry.upload[1].startswith(b'\x1f\x8b'):
                raise RuntimeError('Cargo did not upload an archive')
            archive = Registry.upload[1]
            subprocess.run([os.environ['VELNOR_FIXTURE_CARGO'], 'package', '--no-verify', '--allow-dirty'], cwd=root, env=env, capture_output=True, timeout=120, check=True)
            packaged = (root.parent / 'target/package/velnor-fixture-demo-0.1.1.crate').read_bytes()
            if Registry.upload_attempts != 1 or Registry.index_checksum != hashlib.sha256(archive).hexdigest():
                raise RuntimeError('upload count or index checksum mismatch')
            downloaded = urllib.request.urlopen(f'http://127.0.0.1:{server.server_address[1]}/download/velnor-fixture-demo/0.1.1/download', timeout=10).read()
            if hashlib.sha256(archive).digest() != hashlib.sha256(downloaded).digest():
                raise RuntimeError('registry download checksum mismatch')
            if hashlib.sha256(archive).digest() != hashlib.sha256(packaged).digest():
                raise RuntimeError('uploaded archive checksum mismatch')
            with tarfile.open(fileobj=io.BytesIO(archive), mode='r:gz') as package:
                source = package.extractfile('velnor-fixture-demo-0.1.1/src/lib.rs')
                if source is None or source.read() != (root / 'src/lib.rs').read_bytes():
                    raise RuntimeError('archive source mismatch')
    finally:
        server.shutdown()
        server.server_close()
        worker.join()


if __name__ == '__main__':
    main()
