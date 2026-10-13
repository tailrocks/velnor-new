"""Bounded anonymous public GitHub attempt evidence; no ambient authority."""
import json
import multiprocessing
import os
import re
import ssl
import time
import urllib.error
import urllib.request

from cache_receipt_common import ColdReceipt, strict_json

_LIMIT = 8 * 1024 * 1024
_SOCKET_TIMEOUT = 10
_DEADLINE = 45
_MAX_PAGES = 32
_MAX_JOBS = 4096
_CA_FILE = '/etc/ssl/certs/ca-certificates.crt'


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, _request, _response, _code, _message, _headers, _url):
        raise ColdReceipt('public_api_redirect')


def _opener():
    # Explicit client context avoids ambient CA and SSLKEYLOGFILE authority.
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.load_verify_locations(cafile=_CA_FILE)
    return urllib.request.build_opener(
        urllib.request.ProxyHandler({}),
        urllib.request.HTTPSHandler(context=context), _NoRedirect())


def _endpoint(repository, endpoint, paginate):
    if (not isinstance(repository, str)
            or re.fullmatch(r'[A-Za-z0-9_-][A-Za-z0-9_.-]*/[A-Za-z0-9_-][A-Za-z0-9_.-]*',
                            repository) is None
            or len(repository) > 256):
        raise ColdReceipt('public_api_repository')
    prefix = 'repos/' + re.escape(repository) + '/actions/runs/'
    pattern = prefix + r'[1-9][0-9]{0,19}/attempts/[1-9][0-9]{0,9}(/jobs\?per_page=100)?'
    if not isinstance(endpoint, str) or re.fullmatch(pattern, endpoint) is None:
        raise ColdReceipt('public_api_endpoint')
    if type(paginate) is not bool or endpoint.endswith('/jobs?per_page=100') != paginate:
        raise ColdReceipt('public_api_pagination')


def _page(opener, endpoint, budget, deadline):
    request = urllib.request.Request('https://api.github.com/' + endpoint, method='GET',
        headers={'Accept': 'application/vnd.github+json', 'Accept-Encoding': 'identity',
                 'User-Agent': 'velnor-public-receipt', 'X-GitHub-Api-Version': '2022-11-28'})
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise ColdReceipt('public_api_timeout')
    with opener.open(request, timeout=min(_SOCKET_TIMEOUT, remaining)) as response:
        if response.status != 200 or response.geturl() != request.full_url:
            raise ColdReceipt('public_api_response')
        if response.headers.get('Content-Encoding', 'identity') != 'identity':
            raise ColdReceipt('public_api_encoding')
        chunks = bytearray()
        while True:
            if time.monotonic() >= deadline:
                raise ColdReceipt('public_api_timeout')
            chunk = response.read(min(65536, budget - len(chunks) + 1))
            if not chunk:
                break
            chunks.extend(chunk)
            if len(chunks) > budget:
                raise ColdReceipt('public_api_size')
    value = strict_json(chunks)
    if not isinstance(value, dict):
        raise ColdReceipt('public_api_shape')
    return value, len(chunks)


def _fetch(repository, endpoint, paginate, opener=None):
    """Internal transport seam; endpoint authorization also checked in worker."""
    _endpoint(repository, endpoint, paginate)
    opener = _opener() if opener is None else opener
    deadline, budget = time.monotonic() + _DEADLINE, _LIMIT
    first, consumed = _page(opener, endpoint, budget, deadline)
    if not paginate:
        return first
    total = first.get('total_count')
    if type(total) is not int or not 0 <= total <= _MAX_JOBS:
        raise ColdReceipt('public_api_jobs_count')
    count = max(1, (total + 99) // 100)
    if count > _MAX_PAGES:
        raise ColdReceipt('public_api_pages')
    pages, budget = [first], budget - consumed
    for page_number in range(2, count + 1):
        page, consumed = _page(opener, endpoint + '&page=' + str(page_number), budget, deadline)
        budget -= consumed
        pages.append(page)
    ids = set()
    for index, page in enumerate(pages):
        jobs = page.get('jobs')
        expected = min(100, total - index * 100)
        if (type(page.get('total_count')) is not int or page['total_count'] != total
                or not isinstance(jobs, list) or len(jobs) != expected):
            raise ColdReceipt('public_api_jobs_incomplete')
        for job in jobs:
            if (not isinstance(job, dict) or type(job.get('id')) is not int
                    or job['id'] <= 0 or job['id'] in ids):
                raise ColdReceipt('public_api_job_identity')
            ids.add(job['id'])
    return pages


def _worker(descriptor, repository, endpoint, paginate):
    try:
        result = _fetch(repository, endpoint, paginate)
        data = json.dumps(result, separators=(',', ':'), allow_nan=False).encode('utf-8')
        if len(data) > _LIMIT:
            return
        remaining = memoryview(data)
        while remaining:
            written = os.write(descriptor, remaining)
            if written <= 0:
                return
            remaining = remaining[written:]
    except (ColdReceipt, OSError, ValueError, RecursionError):
        # No response bodies, URLs, or credentials appear in diagnostics.
        return


class PublicReceiptApi:
    """Repository literal issued by qualified public source factory only."""
    def __init__(self, compiled_repository):
        if not isinstance(compiled_repository, str):
            raise ColdReceipt('public_api_repository')
        _endpoint(compiled_repository,
                  'repos/' + compiled_repository + '/actions/runs/1/attempts/1', False)
        self._repository = compiled_repository

    def api(self, endpoint, paginate=False):
        _endpoint(self._repository, endpoint, paginate)
        # A parent deadline also bounds DNS and repeated short socket reads.
        if not hasattr(os, 'memfd_create') or not hasattr(os, 'MFD_CLOEXEC'):
            raise ColdReceipt('public_api_host_unsupported')
        descriptor = os.memfd_create('velnor-public-api', os.MFD_CLOEXEC)
        process = multiprocessing.get_context('fork').Process(
            target=_worker, args=(descriptor, self._repository, endpoint, paginate))
        try:
            deadline = time.monotonic() + _DEADLINE
            process.start()
            process.join(max(0, deadline - time.monotonic()))
            if process.is_alive():
                process.kill()
                process.join()
                raise ColdReceipt('public_api_timeout')
            if process.exitcode != 0 or not 0 < os.fstat(descriptor).st_size <= _LIMIT:
                raise ColdReceipt('public_api_unavailable')
            os.lseek(descriptor, 0, os.SEEK_SET)
            return strict_json(os.read(descriptor, _LIMIT + 1))
        except OSError as error:
            raise ColdReceipt('public_api_unavailable') from error
        finally:
            if process.pid is not None:
                if process.is_alive():
                    process.kill()
                    process.join()
                process.close()
            os.close(descriptor)
