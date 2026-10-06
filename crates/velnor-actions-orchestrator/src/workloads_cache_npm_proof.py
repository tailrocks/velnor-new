"""Authenticate public source authority through credential-free npm requests."""
import base64
import concurrent.futures
import hashlib
import http.client
import json
import os
import ssl
import threading
import time
import urllib.parse

class UnsupportedRegistry(Exception):
    pass


class PublicSourceDenied(Exception):
    pass


class PublicSourceUnavailable(Exception):
    pass


class PublicSourceIntegrityMismatch(Exception):
    pass


MAX_METADATA = 1024 * 1024
MAX_TARBALL = 64 * 1024 * 1024
MAX_TOTAL = 256 * 1024 * 1024
MAX_PACKAGES = 1024


def public_url(url):
    parsed = urllib.parse.urlsplit(url)
    if (parsed.scheme != 'https' or parsed.hostname != 'registry.npmjs.org'
            or parsed.port not in (None, 443) or parsed.username is not None
            or parsed.password is not None or parsed.query or parsed.fragment):
        raise UnsupportedRegistry('not a fixed public npm origin')
    return parsed


def open_public(url, deadline):
    parsed = public_url(url)
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise TimeoutError('proof deadline')
    connection = http.client.HTTPSConnection(
        'registry.npmjs.org', timeout=min(5, remaining),
        context=ssl.create_default_context())
    try:
        # http.client reads no npmrc, token env, netrc, cookies or proxy env.
        connection.request('GET', parsed.path, headers={
            'Accept': 'application/json, application/octet-stream',
            'Accept-Encoding': 'identity', 'User-Agent': 'velnor-public-proof-v1'})
        response = connection.getresponse()
        if response.status in (401, 403):
            raise PublicSourceDenied('anonymous source denied')
        if response.status != 200:
            raise PublicSourceUnavailable('anonymous source unavailable')
        if response.getheader('Content-Encoding') not in (None, 'identity'):
            raise PublicSourceUnavailable('unsupported source encoding')
        return connection, response
    except Exception:
        connection.close()
        raise


class Budget:
    def __init__(self):
        self.lock = threading.Lock()
        self.used = 0
        self.metadata_bytes = 0
        self.tarball_bytes = 0
        self.deadline = time.monotonic() + 40

    def consume(self, size, tarball):
        with self.lock:
            self.used += size
            if tarball:
                self.tarball_bytes += size
            else:
                self.metadata_bytes += size
            if self.used > MAX_TOTAL or time.monotonic() > self.deadline:
                raise ValueError('proof budget exceeded')


def fetch(url, maximum, budget, transport, digest=None, tarball=False):
    connection, response = transport(url, budget.deadline)
    chunks = []
    size = 0
    try:
        while True:
            chunk = response.read(min(65536, maximum + 1 - size))
            if not chunk:
                break
            size += len(chunk)
            budget.consume(len(chunk), digest is not None or tarball)
            if size > maximum:
                raise ValueError('source exceeds proof bound')
            if digest is None:
                chunks.append(chunk)
            else:
                digest.update(chunk)
        return b''.join(chunks)
    finally:
        connection.close()


def verify_metadata(source, budget, transport=open_public):
    public_url(source['resolved'])
    try:
        expected = base64.b64decode(source['integrity'][7:], validate=True)
        if not source['integrity'].startswith('sha512-') or len(expected) != 64:
            raise ValueError('invalid immutable SHA512')
    except (KeyError, TypeError, ValueError):
        raise PublicSourceIntegrityMismatch('invalid immutable SHA512') from None
    name = urllib.parse.quote(source['name'], safe='@')
    version = urllib.parse.quote(source['version'], safe='')
    metadata_url = 'https://registry.npmjs.org/' + name + '/' + version
    try:
        metadata = json.loads(fetch(metadata_url, MAX_METADATA, budget, transport))
    except (OSError, TimeoutError, ValueError, http.client.HTTPException):
        raise PublicSourceUnavailable('anonymous metadata unavailable') from None
    if not isinstance(metadata, dict) or not isinstance(metadata.get('dist'), dict):
        raise PublicSourceUnavailable('anonymous metadata unproven')
    dist = metadata['dist']
    if (metadata.get('name') != source['name'] or metadata.get('version') != source['version']
            or dist.get('integrity') != source['integrity'] or dist.get('tarball') != source['resolved']):
        raise PublicSourceUnavailable('anonymous metadata unproven')
    return expected


def qualify_verified(source, budget, transport=open_public):
    expected = verify_metadata(source, budget, transport)
    digest = hashlib.sha512()
    try:
        fetch(source['resolved'], MAX_TARBALL, budget, transport, digest)
    except (OSError, TimeoutError, ValueError, http.client.HTTPException):
        raise PublicSourceUnavailable('anonymous tarball unavailable') from None
    if digest.digest() != expected:
        raise PublicSourceIntegrityMismatch('public tarball SHA512 mismatch')
    return source['integrity']
