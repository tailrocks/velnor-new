"""Network-free evidence for the closed release download boundary."""

import hashlib
import io
import unittest
import urllib.request
from unittest.mock import Mock, patch

import catalog_mise_acquisition_download as download
from catalog_executable_bounds import executable_limit, archive_limit, stream_sha256
download.executable_limit = executable_limit
download.archive_limit = archive_limit
download.stream_sha256 = stream_sha256


SOURCE = "https://github.com/owned/mise/releases/download/v2026.10.0/mise-linux-x64.tar.gz"
DELIVERY = "https://release-assets.githubusercontent.com/github-production-release-asset/123/file?sig=a%2Fb"
ARCHIVE = b"qualified archive bytes"
DIGEST = hashlib.sha256(ARCHIVE).hexdigest()


class Response(io.BytesIO):
    def __init__(self, contents=ARCHIVE, url=DELIVERY, status=200):
        super().__init__(contents)
        self.url = url
        self.status = status

    def geturl(self):
        return self.url


class DownloadTests(unittest.TestCase):
    def test_canonical_source(self):
        download._validate_source_url(SOURCE)

    def test_source_rejects_unqualified_urls(self):
        rejected = [
            SOURCE.replace("https:", "http:"),
            SOURCE.replace("github.com", "GITHUB.COM"),
            SOURCE.replace("github.com", "github.com:443"),
            SOURCE.replace("github.com", "user@github.com"),
            SOURCE.replace("github.com", "github.com.evil.example"),
            SOURCE + "?download=1", SOURCE + "?", SOURCE + "#", SOURCE + "#fragment",
            SOURCE.replace("owned", "%6fwned"),
            SOURCE.replace("/owned/", "/../"), SOURCE.replace("/owned/", "/./"),
            SOURCE.replace("/owned/", "//owned/"),
            SOURCE.replace("/owned/", "/owned\\/"),
            SOURCE.replace("/owned/", "/ow\nned/"),
            SOURCE.replace("/releases/download/", "/archive/"),
            DELIVERY,
        ]
        for url in rejected:
            with self.subTest(url=url), self.assertRaises(ValueError):
                download._validate_source_url(url)

    def test_delivery_signed_query_and_closed_authority(self):
        download._validate_delivery_url(DELIVERY)
        rejected = [
            SOURCE, DELIVERY.replace("https:", "http:"),
            DELIVERY.replace(".com/", ".com:443/"),
            DELIVERY.replace("https://", "https://user@"),
            DELIVERY.replace("github-production-release-asset", ".."),
            DELIVERY.replace("/123/", "/%31%32%33/"),
            DELIVERY + "#fragment", DELIVERY + "\n", DELIVERY + " ",
            "https://release-assets.githubusercontent.com/",
        ]
        for url in rejected:
            with self.subTest(url=url), self.assertRaises(ValueError):
                download._validate_delivery_url(url)

    def test_redirect_bound(self):
        handler = download._ReleaseRedirect()
        request = urllib.request.Request(SOURCE)
        for _ in range(download.MAX_REDIRECTS):
            self.assertEqual(handler.redirect_request(request, None, 302, "", {}, DELIVERY).full_url, DELIVERY)
        with self.assertRaisesRegex(ValueError, "redirect limit"):
            handler.redirect_request(request, None, 302, "", {}, DELIVERY)

    def test_bad_redirect_rejected_before_request(self):
        with self.assertRaises(ValueError):
            download._ReleaseRedirect().redirect_request(
                urllib.request.Request(SOURCE), None, 302, "", {}, "http://localhost/secret"
            )

    def test_raw_location_checked_before_urllib_normalization(self):
        for location in [DELIVERY + "\n", "/relative/asset", None]:
            with self.subTest(location=location), self.assertRaises(ValueError):
                download._ReleaseRedirect().http_error_302(
                    urllib.request.Request(SOURCE), None, 302, "", {"location": location}
                )

    def test_download_digest_and_environment_independent_handlers(self):
        context = Mock()
        opener = Mock()
        opener.open.return_value = Response()
        with patch.object(download, "_tls_context", return_value=context), patch.object(
            download.urllib.request, "build_opener", return_value=opener
        ) as build:
            self.assertEqual(download.download_asset(SOURCE, DIGEST), ARCHIVE)
        handlers = build.call_args.args
        self.assertEqual(handlers[0].proxies, {})
        self.assertIs(handlers[1]._context, context)
        self.assertIsInstance(handlers[2], download._ReleaseRedirect)
        request = opener.open.call_args.args[0]
        self.assertEqual(request.full_url, SOURCE)
        self.assertEqual(opener.open.call_args.kwargs, {"timeout": 30})
        self.assertEqual(request.get_header("Accept-encoding"), "identity")

    def test_download_rejects_digest_status_and_unexpected_final_url(self):
        cases = [
            (Response(), "0" * 64, "SHA-256 mismatch"),
            (Response(status=206), DIGEST, "HTTP 200"),
            (Response(url="https://evil.example/asset"), DIGEST, "authority"),
        ]
        for response, digest, message in cases:
            opener = Mock()
            opener.open.return_value = response
            with self.subTest(message=message), patch.object(download, "_tls_context"), patch.object(
                download.urllib.request, "build_opener", return_value=opener
            ), self.assertRaisesRegex(ValueError, message):
                download.download_asset(SOURCE, digest)

    def test_invalid_digest_rejected_before_transport(self):
        for digest in ["A" * 64, "0" * 63, "0" * 65, None]:
            with self.subTest(digest=digest), patch.object(download, "_tls_context") as tls:
                with self.assertRaises(ValueError):
                    download.download_asset(SOURCE, digest)
                tls.assert_not_called()

    def test_response_size_bound(self):
        with patch.object(download, "executable_limit", return_value=3):
            self.assertEqual(download._read_bounded(io.BytesIO(b"abc")), b"abc")
            with self.assertRaisesRegex(ValueError, "download limit"):
                download._read_bounded(io.BytesIO(b"abcd"))

    def test_explicit_system_certificates(self):
        for platform, cafile in [
            ("linux", "/etc/ssl/certs/ca-certificates.crt"), ("darwin", "/etc/ssl/cert.pem")
        ]:
            with self.subTest(platform=platform), patch.object(download.sys, "platform", platform), patch.object(
                download.ssl, "SSLContext"
            ) as context:
                download._tls_context()
                context.assert_called_once_with(download.ssl.PROTOCOL_TLS_CLIENT)
                context.return_value.load_verify_locations.assert_called_once_with(cafile=cafile)
        with patch.object(download.sys, "platform", "unsupported"), self.assertRaises(ValueError):
            download._tls_context()


if __name__ == "__main__":
    unittest.main()
