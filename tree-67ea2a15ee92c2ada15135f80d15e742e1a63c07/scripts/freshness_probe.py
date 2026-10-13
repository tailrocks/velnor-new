"""Bounded HTTP text fetch for the read-only freshness probe."""

import gzip
import io
import urllib.request
import zlib

FETCH_TIMEOUT = 10
FETCH_CAP = 1024 * 1024


def fetch_text(url):
    request = urllib.request.Request(
        url,
        headers={"User-Agent": "velnor-freshness-probe",
                 "Accept": "application/json", "Accept-Encoding": "gzip"},
    )
    with urllib.request.urlopen(request, timeout=FETCH_TIMEOUT) as response:
        encoded = response.read(FETCH_CAP + 1)
        encoding = response.headers.get("Content-Encoding", "identity")

    if len(encoded) > FETCH_CAP:
        raise ValueError(f"encoded freshness response exceeds {FETCH_CAP} bytes")

    encoding = encoding.strip().lower()
    if encoding in ("", "identity"):
        decoded = encoded
    elif encoding == "gzip":
        try:
            with gzip.GzipFile(fileobj=io.BytesIO(encoded)) as stream:
                decoded = stream.read(FETCH_CAP + 1)
        except (EOFError, OSError, zlib.error) as err:
            raise ValueError(f"invalid gzip freshness response ({err})") from err
    else:
        raise ValueError(f"unsupported Content-Encoding: {encoding}")

    if len(decoded) > FETCH_CAP:
        raise ValueError(f"decoded freshness response exceeds {FETCH_CAP} bytes")
    return decoded.decode("utf-8", errors="replace")
