"""Fresh anonymous HTTPS acquisition; compiler-owned URL and CA only."""
import hashlib
import multiprocessing
import os
import ssl
import time
import urllib.parse
import urllib.request

from cache_receipt_common import ColdReceipt

_LIMIT = 64 * 1024 * 1024
_DEADLINE = 90
_HOSTS = frozenset(('github.com', 'release-assets.githubusercontent.com'))


def _url(value):
    parsed = urllib.parse.urlsplit(value)
    if (parsed.scheme != 'https' or parsed.hostname not in _HOSTS
            or parsed.username is not None or parsed.password is not None
            or parsed.port not in (None, 443) or parsed.fragment):
        raise ColdReceipt('fresh_gh_url')
    return value


class _Redirect(urllib.request.HTTPRedirectHandler):
    max_redirections = 3
    max_repeats = 1

    def redirect_request(self, request, response, code, message, headers, url):
        return super().redirect_request(request, response, code, message, headers, _url(url))


def _fetch(descriptor, record, ca_file):
    # Explicit SSLContext never consults SSLKEYLOGFILE or ambient CA variables.
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.load_verify_locations(cafile=ca_file)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}),
        urllib.request.HTTPSHandler(context=context), _Redirect())
    request = urllib.request.Request(_url(record['asset_url']), method='GET',
        headers={'Accept-Encoding': 'identity', 'User-Agent': 'velnor-fresh-gh'})
    deadline = time.monotonic() + _DEADLINE
    with opener.open(request, timeout=10) as response:
        if response.status != 200 or response.headers.get('Content-Encoding', 'identity') != 'identity':
            raise ColdReceipt('fresh_gh_response')
        _url(response.geturl())
        total, digest = 0, hashlib.sha256()
        while True:
            if time.monotonic() >= deadline:
                raise ColdReceipt('fresh_gh_timeout')
            data = response.read(min(65536, _LIMIT - total + 1))
            if not data:
                break
            total += len(data)
            if total > _LIMIT:
                raise ColdReceipt('fresh_gh_archive_size')
            digest.update(data)
            remaining = memoryview(data)
            while remaining:
                written = os.write(descriptor, remaining)
                if written <= 0:
                    raise ColdReceipt('fresh_gh_write')
                remaining = remaining[written:]
    if total == 0 or digest.hexdigest() != record['archive_sha256']:
        raise ColdReceipt('fresh_gh_archive_digest')


def _worker(descriptor, record, ca_file):
    os.environ.clear()
    try:
        _fetch(descriptor, record, ca_file)
    except (ColdReceipt, OSError, ValueError):
        os.ftruncate(descriptor, 0)


def download(record, ca_file):
    """Parent deadline bounds DNS, redirects and slow reads; no PATH process."""
    descriptor = os.memfd_create('velnor-fresh-gh-archive', os.MFD_CLOEXEC)
    process = multiprocessing.get_context('fork').Process(
        target=_worker, args=(descriptor, record, ca_file))
    try:
        process.start()
        process.join(_DEADLINE)
        if process.is_alive():
            raise ColdReceipt('fresh_gh_timeout')
        size = os.fstat(descriptor).st_size
        if process.exitcode != 0 or not 0 < size <= _LIMIT:
            raise ColdReceipt('fresh_gh_download')
        os.lseek(descriptor, 0, os.SEEK_SET)
        archive = bytearray()
        while chunk := os.read(descriptor, 65536):
            archive.extend(chunk)
            if len(archive) > _LIMIT:
                raise ColdReceipt('fresh_gh_archive_size')
        if hashlib.sha256(archive).hexdigest() != record['archive_sha256']:
            raise ColdReceipt('fresh_gh_archive_digest')
        return bytes(archive)
    finally:
        if process.pid is not None:
            if process.is_alive():
                process.kill()
            process.join()
            process.close()
        os.close(descriptor)
