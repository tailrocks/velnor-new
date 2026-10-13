"""Initial trusted SDK qualification probe; never consumes cache/repository code."""
import gzip
import fcntl
import hashlib
import io
import json
import multiprocessing
import os
import selectors
import ssl
import subprocess
import sys
import tarfile
import tempfile
import urllib.request


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, target):
        return None


def qualify(canonical_source):
    if (sys.flags.isolated, sys.flags.no_site,
            sys.flags.ignore_environment, sys.flags.dont_write_bytecode) != (1, 1, 1, 1):
        raise ValueError('foundation_flags')
    if dict(os.environ) != {'LANG': 'C', 'LC_ALL': 'C'} or os.getcwd() != '/':
        raise ValueError('foundation_environment')
    namespace = {'__name__': 'canonical_metadata_container'}
    exec(compile(canonical_source, '<owned-canonical-metadata>', 'exec'), namespace)
    predicate = namespace['is_appledouble']
    header = bytes.fromhex('0005160700020000') + bytes(16) + bytes.fromhex('0001')
    valid = header + bytes.fromhex('000000020000002600000004') + b'data'
    cases = [
        (valid, len(valid), True),
        (b'ordinary source', 15, False),
        (valid[:25], len(valid), False),
        (valid[:38], 41, False),
        (bytes(26), 26, False),
    ]
    for prefix, size, expected in cases:
        if predicate(prefix, size) is not expected:
            raise ValueError('canonical_metadata_positive')
    compressed = gzip.compress(b'fixed public qualification bytes', mtime=0)
    if gzip.decompress(compressed) != b'fixed public qualification bytes':
        raise ValueError('foundation_gzip_positive')
    archive = io.BytesIO()
    with tarfile.open(fileobj=archive, mode='w') as handle:
        member = tarfile.TarInfo('owned.txt')
        member.size = 4
        handle.addfile(member, io.BytesIO(b'data'))
    with tarfile.open(fileobj=io.BytesIO(archive.getvalue()), mode='r:') as handle:
        member = handle.next()
        if member.name != 'owned.txt' or handle.extractfile(member).read() != b'data':
            raise ValueError('foundation_tar_positive')
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.load_verify_locations(cafile='/etc/ssl/certs/ca-certificates.crt')
    certificates = context.cert_store_stats()
    if certificates['x509_ca'] < 1:
        raise ValueError('foundation_ca_positive')
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), _NoRedirect(),
                                        urllib.request.HTTPSHandler(context=context))
    request = urllib.request.Request('https://api.github.com', headers={
        'User-Agent': 'velnor-foundation-qualification'})
    with opener.open(request, timeout=30) as response:
        if response.status != 200 or len(response.read(1024 * 1024 + 1)) > 1024 * 1024:
            raise ValueError('foundation_https_positive')
    return {'canonical_predicate_cases': len(cases), 'gzip_roundtrip': True,
            'tar_roundtrip': True, 'ca_loaded': True, 'anonymous_https': True,
            'stdlib_loaded': sorted((name, getattr(module, '__file__', None))
                                    for name, module in sys.modules.items())}


if __name__ == '__main__':
    # Only the owning Node first-step launcher supplies these canonical bytes.
    source = sys.stdin.buffer.read(1024 * 1024 + 1)
    if len(source) > 1024 * 1024:
        raise ValueError('foundation_source_bound')
    print(json.dumps(qualify(source)))
