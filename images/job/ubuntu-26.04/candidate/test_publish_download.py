#!/usr/bin/env python3
"""Tests for OCI blob GET requests."""

import io
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from test_publish import digest, publish


class Response:
    def __init__(self, status, headers, body=b""):
        self.status = status
        self.headers = headers
        self.body = io.BytesIO(body)

    def getheader(self, name, default=None):
        return self.headers.get(name, default)

    def getheaders(self):
        return list(self.headers.items())

    def read(self, size):
        return self.body.read(size)


class Connection:
    def __init__(self, response):
        self.response = response
        self.requests = []
        self.closed = False

    def request(self, method, path, headers):
        self.requests.append((method, path, headers.copy()))

    def getresponse(self):
        return self.response

    def close(self):
        self.closed = True


class DownloadTests(unittest.TestCase):
    def setUp(self):
        self.content = b"verified blob bytes"
        self.digest = digest(self.content)
        self.url = f"https://ghcr.io/v2/{publish.PACKAGE}/blobs/{self.digest}"
        self.location = (
            f"https://{publish.DOWNLOAD_HOST}/ghcrblobs11/blobs/{self.digest}"
            "?sig=opaque%2Bvalue%3D&se=unchanged"
        )

    def verify(self, responses, expected=None, url=None, size=None):
        connections = [Connection(response) for response in responses]
        with mock.patch.object(publish.http.client, "HTTPSConnection", side_effect=connections) as factory:
            result = publish._HttpsTransport().verify_get(
                url or self.url,
                {"Authorization": "Bearer test-secret", "Cookie": "test-cookie"},
                self.content if expected is None else expected,
                len(self.content) if size is None else size,
                self.digest,
            )
        return result, connections, factory

    def test_blob_redirect_preserves_query_and_removes_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            expected = Path(directory) / "blob"
            expected.write_bytes(self.content)
            _, connections, factory = self.verify([
                Response(307, {"Location": self.location}),
                Response(200, {"Content-Length": str(len(self.content))}, self.content),
            ], expected=expected)
        self.assertEqual([call.args[0] for call in factory.call_args_list],
                         [publish.REGISTRY, publish.DOWNLOAD_HOST])
        self.assertEqual(connections[1].requests, [(
            "GET", self.location.removeprefix(f"https://{publish.DOWNLOAD_HOST}"),
            {"Accept": "application/octet-stream"},
        )])
        self.assertTrue(all(connection.closed for connection in connections))

    def test_manifest_redirect_remains_rejected(self):
        with self.assertRaisesRegex(publish.PublishError, "manifest redirects"):
            self.verify([Response(307, {"Location": self.location})],
                        url=self.url.replace("/blobs/", "/manifests/"))

    def test_download_redirect_has_one_hop(self):
        with self.assertRaisesRegex(publish.PublishError, "status is incorrect: 307"):
            self.verify([Response(307, {"Location": self.location}),
                         Response(307, {"Location": self.location})])

    def test_download_url_rejects_foreign_and_incorrect_locations(self):
        for location in (
            self.location.replace("https:", "http:"),
            self.location.replace(publish.DOWNLOAD_HOST, "foreign.example"),
            self.location.replace(publish.DOWNLOAD_HOST, "user@" + publish.DOWNLOAD_HOST),
            self.location.replace(publish.DOWNLOAD_HOST, publish.DOWNLOAD_HOST + ":444"),
            self.location + "#fragment",
            self.location + "\n",
            self.location + "%0d",
            self.location.replace("/ghcrblobs11/", "/ghcrblobs11\\/"),
            self.location.replace(self.digest, "sha256:" + "0" * 64),
            "/relative/location",
        ):
            with self.subTest(location=location), self.assertRaises(publish.PublishError):
                publish._blob_download_location(location, self.digest)

    def test_redirect_keeps_byte_digest_and_size_checks(self):
        invalid_responses = (
            Response(200, {"Content-Length": "0"}, self.content),
            Response(200, {"Content-Length": str(len(self.content))}, self.content[:-1]),
            Response(200, {"Content-Length": str(len(self.content))}, b"x" * len(self.content)),
            Response(200, {"Content-Length": str(len(self.content))}, self.content + b"x"),
            Response(200, {"Content-Length": str(len(self.content)),
                           "Docker-Content-Digest": "sha256:" + "0" * 64}, self.content),
        )
        for response in invalid_responses:
            with self.subTest(headers=response.headers), self.assertRaises(publish.PublishError):
                self.verify([Response(307, {"Location": self.location}), response])

    def test_direct_get_still_verifies_bytes(self):
        result, connections, _ = self.verify([
            Response(200, {"Content-Length": str(len(self.content))}, self.content),
        ])
        self.assertEqual(result.status, 200)
        self.assertEqual(len(connections), 1)


if __name__ == "__main__":
    unittest.main()
