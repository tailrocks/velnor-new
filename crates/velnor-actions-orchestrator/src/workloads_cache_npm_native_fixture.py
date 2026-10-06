"""Pinned native producer integration; code and absolute Node path are arguments."""
import base64, hashlib, http.client, http.server, io, json, os, subprocess, tempfile, threading, time, tarfile, unittest.mock, sys
from pathlib import Path
engine = {'__name__': 'fixture_engine'}
exec(sys.argv[1], engine)
BODY_SHA256 = hashlib.sha256(sys.argv[1].encode()).hexdigest()
NODE = Path(sys.argv[2])
EXPECTED_OWNER = json.loads(sys.argv[3])
NPM = NODE.parent.parent / 'lib/node_modules/npm'
requests = []
mode = 'public'

def package(version):
    data = json.dumps({'name': '@scope/public', 'version': version, 'scripts': {'install': 'touch SHOULD_NOT_EXIST'}}).encode()
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode='w:gz') as archive:
        item = tarfile.TarInfo('package/package.json')
        item.size = len(data)
        archive.addfile(item, io.BytesIO(data))
    data = output.getvalue()
    return (data, 'sha512-' + base64.b64encode(hashlib.sha512(data).digest()).decode())
packages = {version: package(version) for version in ('1.2.3', '2.0.0')}
sources = {version: {'name': '@scope/public', 'version': version, 'resolved': 'https://registry.npmjs.org/@scope/public/-/public-' + version + '.tgz', 'integrity': integrity} for version, (_, integrity) in packages.items()}

class Handler(http.server.BaseHTTPRequestHandler):

    def log_message(self, *args):
        pass

    def do_GET(self):
        requests.append((self.path, dict(self.headers)))
        status = 200
        if self.path.endswith('.tgz'):
            version = self.path.rsplit('public-', 1)[1][:-4]
            body = packages[version][0]
        else:
            version = self.path.rsplit('/', 1)[1]
            source = sources[version]
            metadata = {'name': source['name'], 'version': source['version'], 'dist': {'tarball': source['resolved'], 'integrity': source['integrity']}}
            if mode in ('401', '403', '404'):
                status = int(mode)
            if mode == 'partial-private' and version == '2.0.0':
                status = 403
            if mode == 'identity-mismatch':
                metadata['name'] = '@scope/private'
            body = json.dumps(metadata).encode()
        self.send_response(status)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)
server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
thread = threading.Thread(target=server.serve_forever, daemon=True)
thread.start()

def local_https(host, timeout, context):
    assert host == 'registry.npmjs.org'
    return http.client.HTTPConnection('127.0.0.1', server.server_port, timeout=timeout)

def counts():
    tar = sum((path.endswith('.tgz') for path, _ in requests))
    for _, headers in requests:
        assert not any((name.lower() in ('authorization', 'cookie', 'proxy-authorization') for name in headers))
        assert 'PRIVATE_SENTINEL' not in str(headers)
    return {'metadata_requests': len(requests) - tar, 'tarball_requests': tar}

def produce(selected, cache):
    requests.clear()
    start = time.monotonic()
    integrities = engine['produce'](selected, str(cache), str(NODE), EXPECTED_OWNER, compatibility=BODY_SHA256)
    return (integrities, dict(counts(), elapsed_ms=round(1000 * (time.monotonic() - start), 2)))

def denied(selected, cache, reason):
    marker = cache / 'public-proof-v1.json'
    before = marker.read_bytes() if marker.exists() else None
    start = time.monotonic()
    try:
        produce(selected, cache)
    except engine['SourceEligibilityDenied'] as error:
        assert error.reason == reason, (error.reason, reason)
    else:
        raise AssertionError('denied candidates authorized export')
    assert (marker.read_bytes() if marker.exists() else None) == before
    return dict(counts(), elapsed_ms=round(1000 * (time.monotonic() - start), 2), export=False, error=reason)

def consumer(temp, cache, offline=True):
    project = temp / 'consumer'
    project.mkdir(exist_ok=True)
    (project / 'package.json').write_text(json.dumps({'name': 'consumer', 'version': '1.0.0', 'dependencies': {'@scope/public': '1.2.3'}}))
    resolved = sources['1.2.3']['resolved'] if offline else 'http://127.0.0.1:%d/@scope/public/-/public-1.2.3.tgz' % server.server_port
    (project / 'package-lock.json').write_text(json.dumps({'name': 'consumer', 'version': '1.0.0', 'lockfileVersion': 3, 'packages': {'': {'name': 'consumer', 'version': '1.0.0', 'dependencies': {'@scope/public': '1.2.3'}}, 'node_modules/@scope/public': {'name': '@scope/public', 'version': '1.2.3', 'resolved': resolved, 'integrity': sources['1.2.3']['integrity']}}}))
    env = {'PATH': str(NODE.parent) + ':/usr/bin:/bin', 'HOME': str(temp)}
    command = [str(NODE), str(NPM / 'bin/npm-cli.js'), 'ci', '--cache', str(cache), '--ignore-scripts', '--no-audit', '--no-fund', '--userconfig=/dev/null', '--globalconfig=' + str(cache / 'empty-global.npmrc')]
    if offline:
        command.append('--offline')
    result = subprocess.run(command, cwd=project, env=env, capture_output=True)
    assert result.returncode == 0, result.stderr.decode()
    assert (project / 'node_modules/@scope/public/package.json').is_file()

def main():
    global mode
    results = {}
    for name in ('NPM_TOKEN', 'NODE_AUTH_TOKEN', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY'):
        os.environ[name] = 'PRIVATE_SENTINEL'
    try:
        with tempfile.TemporaryDirectory(prefix='producer-fixture-', dir=Path('/tmp').resolve()) as location:
            temp = Path(location)
            cache = temp / 'cache'
            source = sources['1.2.3']
            integrity = source['integrity']
            with unittest.mock.patch.object(http.client, 'HTTPSConnection', side_effect=local_https):
                for case in ('cold', 'warm', 'corrupt'):
                    if case == 'corrupt':
                        digest = hashlib.sha512(packages['1.2.3'][0]).hexdigest()
                        path = cache / '_cacache/content-v2/sha512' / digest[:2] / digest[2:4] / digest[4:]
                        path.write_bytes(b'CORRUPT_PRIVATE')
                    actual, observed = produce([source], cache)
                    assert actual == [integrity], (case, actual)
                    assert observed['tarball_requests'] == (0 if case == 'warm' else 1), (case, observed)
                    results[case] = observed
                requests.clear()
                consumer(temp, cache)
                assert counts()['tarball_requests'] == 0
                results['actual_npm_ci_offline'] = counts()
                actual, results['post_consumer_warm'] = produce([source], cache)
                assert actual == [integrity] and results['post_consumer_warm']['tarball_requests'] == 0
                actual, results['new_version'] = produce([source, sources['2.0.0']], cache)
                assert set(actual) == {value['integrity'] for value in sources.values()}
                assert results['new_version']['tarball_requests'] == 1
                actual, results['original_still_warm'] = produce([source], cache)
                assert actual == [integrity] and results['original_still_warm']['tarball_requests'] == 0
                marker_path = cache / 'public-proof-v1.json'
                marker = json.loads(marker_path.read_text())
                marker['owner']['npm'] = 'unsupported-owner'
                marker_path.write_text(json.dumps(marker))
                actual, results['changed_owner_provenance'] = produce([source], cache)
                assert actual == [integrity] and results['changed_owner_provenance']['tarball_requests'] == 1
                malformed = dict(source, private_marker='TOKEN_PRIVATE')
                try:
                    produce([malformed], cache)
                except ValueError:
                    results['extra_source_fields_rejected'] = counts()
                else:
                    raise AssertionError('malformed tuple accepted')
                assert counts() == {'metadata_requests': 0, 'tarball_requests': 0}
                for case in ('401', '403', '404', 'identity-mismatch', 'partial-private'):
                    mode = case
                    reason = 'PRIVATE_OR_AUTH_REQUIRED' if case in ('401', '403', 'partial-private') else 'PUBLIC_AUTHORITY_UNAVAILABLE'
                    selected = [source, sources['2.0.0']] if case == 'partial-private' else [source]
                    results[case] = denied(selected, cache, reason)
                    assert results[case]['tarball_requests'] == 0
                mode = 'public'
                requests.clear()
                consumer(temp, temp / 'missing-producer-cache', offline=False)
                assert counts()['tarball_requests'] == 1
                results['missing_producer_consumer_ci'] = counts()
                mode = '401'
                failed_cache = temp / 'failed-producer-cache'
                observed = denied([source], failed_cache, 'PRIVATE_OR_AUTH_REQUIRED')
                assert observed['tarball_requests'] == 0
                mode = 'public'
                requests.clear()
                consumer(temp, failed_cache, offline=False)
                assert counts()['tarball_requests'] == 1
                results['failed_producer_consumer_ci'] = counts()
                assert not (temp / 'SHOULD_NOT_EXIST').exists()
        print(json.dumps({'owner': EXPECTED_OWNER, 'tarball_bytes': len(packages['1.2.3'][0]), 'production_helper_cases': results}, indent=2))
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
main()
