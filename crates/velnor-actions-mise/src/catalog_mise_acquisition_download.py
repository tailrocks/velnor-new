"""Bounded HTTPS delivery for a source-qualified GitHub release asset."""

import hashlib
import re
import ssl
import sys
import unicodedata
import urllib.parse
import urllib.request


MAX_REDIRECTS = 3
TIMEOUT_SECONDS = 30
_COMPONENT = r"[A-Za-z0-9_][A-Za-z0-9_.-]*"
_RELEASE_PATH = re.compile(
    rf"/{_COMPONENT}/{_COMPONENT}/releases/download/{_COMPONENT}/{_COMPONENT}"
)


def _parse_url(url):
    if not isinstance(url, str) or not url or len(url) > 16384:
        raise ValueError("invalid release URL")
    if "\\" in url or any(c.isspace() or unicodedata.category(c).startswith("C") for c in url):
        raise ValueError("invalid release URL")
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme != "https" or parsed.fragment or "#" in url:
        raise ValueError("release URL must use canonical HTTPS")
    if parsed.netloc not in ("github.com", "release-assets.githubusercontent.com"):
        raise ValueError("release URL has unsupported authority")
    if "%" in parsed.path or any(p in ("", ".", "..") for p in parsed.path[1:].split("/")):
        raise ValueError("release URL has invalid path")
    if urllib.parse.urlunsplit(parsed) != url:
        raise ValueError("release URL is not canonical")
    return parsed


def _validate_source_url(url):
    parsed = _parse_url(url)
    if parsed.netloc != "github.com" or parsed.query or "?" in url:
        raise ValueError("expected a GitHub release asset URL")
    if not _RELEASE_PATH.fullmatch(parsed.path):
        raise ValueError("expected canonical GitHub release asset path")


def _validate_delivery_url(url):
    parsed = _parse_url(url)
    if parsed.netloc != "release-assets.githubusercontent.com" or not parsed.path.startswith("/"):
        raise ValueError("unsupported release asset redirect")
    if not parsed.path.isascii() or not parsed.query.isascii():
        raise ValueError("release asset redirect must use ASCII")


class _ReleaseRedirect(urllib.request.HTTPRedirectHandler):
    def __init__(self):
        super().__init__()
        self.redirects = 0

    def http_error_302(self, req, fp, code, msg, headers):
        location = headers.get("location", headers.get("uri"))
        _validate_delivery_url(location)
        return super().http_error_302(req, fp, code, msg, headers)

    http_error_301 = http_error_302
    http_error_303 = http_error_302
    http_error_307 = http_error_302
    http_error_308 = http_error_302

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        _validate_delivery_url(newurl)
        self.redirects += 1
        if self.redirects > MAX_REDIRECTS:
            raise ValueError("release asset redirect limit exceeded")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def _tls_context():
    certificates = {
        "linux": "/etc/ssl/certs/ca-certificates.crt",
        "darwin": "/etc/ssl/cert.pem",
    }
    cafile = certificates.get(sys.platform)
    if cafile is None:
        raise ValueError("unsupported release download platform")
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.load_verify_locations(cafile=cafile)
    return context


def _read_bounded(response):
    chunks = []
    total = 0
    while True:
        chunk = response.read(min(1024 * 1024, executable_limit() + 1 - total))
        if not chunk:
            return b"".join(chunks)
        total += len(chunk)
        if total > executable_limit():
            raise ValueError("release asset exceeds download limit")
        chunks.append(chunk)


def download_asset(url: str, expected_sha256: str) -> bytes:
    """Fetch a literal qualified URL without proxy or certificate environment overrides."""
    _validate_source_url(url)
    if not isinstance(expected_sha256, str) or not re.fullmatch(r"[0-9a-f]{64}", expected_sha256):
        raise ValueError("invalid expected archive SHA-256")
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({}),
        urllib.request.HTTPSHandler(context=_tls_context()),
        _ReleaseRedirect(),
    )
    request = urllib.request.Request(url, headers={"Accept-Encoding": "identity"})
    with opener.open(request, timeout=TIMEOUT_SECONDS) as response:
        final_url = response.geturl()
        if final_url != url:
            _validate_delivery_url(final_url)
        if response.status != 200:
            raise ValueError("release asset response must be HTTP 200")
        archive = _read_bounded(response)
    if hashlib.sha256(archive).hexdigest() != expected_sha256:
        raise ValueError("archive SHA-256 mismatch")
    return archive
