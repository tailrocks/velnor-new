"""Regression tests for bounded, content-encoding-aware freshness fetches."""

import gzip
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import re
import threading
import unittest

from freshness_probe import FETCH_CAP, fetch_text


class ProbeResponse:
    def __init__(self, body, encoding=None):
        self.body = body
        self.encoding = encoding
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), self.handler())
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def handler(self):
        body, encoding = self.body, self.encoding

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                if encoding is not None:
                    self.send_header("Content-Encoding", encoding)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, _format, *_args):
                return

        return Handler

    def __enter__(self):
        self.thread.start()
        host, port = self.server.server_address
        return f"http://{host}:{port}/probe"

    def __exit__(self, *_args):
        self.server.shutdown()
        self.thread.join(timeout=2)
        self.server.server_close()


class FreshnessProbeTests(unittest.TestCase):
    def test_gzip_html_is_decoded_before_python_release_regex(self):
        html = b"<a>Download Python 3.14.8</a>"
        with ProbeResponse(gzip.compress(html), "gzip") as url:
            body = fetch_text(url)
        self.assertEqual(re.search(r">Download Python (\d+\.\d+\.\d+)<", body).group(1),
                         "3.14.8")

    def test_identity_response_remains_supported(self):
        with ProbeResponse(b'{"crate":{"max_version":"1.2.3"}}') as url:
            self.assertEqual(fetch_text(url), '{"crate":{"max_version":"1.2.3"}}')

    def test_encoded_body_is_bounded(self):
        with ProbeResponse(b"x" * (FETCH_CAP + 1), "identity") as url:
            with self.assertRaisesRegex(ValueError, "encoded freshness response"):
                fetch_text(url)

    def test_decompressed_body_is_bounded(self):
        with ProbeResponse(gzip.compress(b"x" * (FETCH_CAP + 1)), "gzip") as url:
            with self.assertRaisesRegex(ValueError, "decoded freshness response"):
                fetch_text(url)

    def test_unsupported_content_encoding_fails_closed(self):
        with ProbeResponse(b"body", "br") as url:
            with self.assertRaisesRegex(ValueError, "unsupported Content-Encoding"):
                fetch_text(url)

    def test_truncated_gzip_fails_closed(self):
        with ProbeResponse(b"\x1f\x8b\x08", "gzip") as url:
            with self.assertRaisesRegex(ValueError, "invalid gzip freshness response"):
                fetch_text(url)


    def test_encoded_body_at_limit_is_accepted(self):
        with ProbeResponse(b"x" * FETCH_CAP, "identity") as url:
            self.assertEqual(len(fetch_text(url)), FETCH_CAP)

    def test_decompressed_body_at_limit_is_accepted(self):
        body = gzip.compress(b"x" * FETCH_CAP, mtime=0)
        with ProbeResponse(body, "gzip") as url:
            self.assertEqual(len(fetch_text(url)), FETCH_CAP)

    def test_gzip_checksum_corruption_fails_closed(self):
        body = bytearray(gzip.compress(b"payload", mtime=0))
        body[-8] ^= 1
        with ProbeResponse(bytes(body), "gzip") as url:
            with self.assertRaisesRegex(ValueError, "invalid gzip freshness response"):
                fetch_text(url)

    def test_truncated_gzip_footer_fails_closed(self):
        body = gzip.compress(b"payload", mtime=0)[:-1]
        with ProbeResponse(body, "gzip") as url:
            with self.assertRaisesRegex(ValueError, "invalid gzip freshness response"):
                fetch_text(url)


if __name__ == "__main__":
    unittest.main()
