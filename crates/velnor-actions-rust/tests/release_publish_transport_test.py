"""Protocol framing and credential exchange tested through local mocks."""
from contextlib import contextmanager
import json
from pathlib import Path
import struct
from types import SimpleNamespace
import unittest
from unittest.mock import patch

FIXTURE = {"__file__": str(Path(__file__).with_name("release_publish_test_fixture.py"))}
exec(compile(Path(__file__).with_name("release_publish_test_fixture.py").read_text(),
             "release_publish_test_fixture.py", "exec"), FIXTURE)
NS = FIXTURE["NS"]
ReconcileError = FIXTURE["ReconcileError"]


class TransportTests(unittest.TestCase):
    def test_exact_cargo_frame_and_raw_authorization(self):
        metadata, data = {"name": "demo", "authors": ["Δ"]}, b"exact gzip bytes"
        response = SimpleNamespace(status=200, read=lambda _: b'{}')
        @contextmanager
        def opened(*args, **kwargs):
            yield response
        opener = SimpleNamespace(open=opened)
        with patch.object(NS["urllib"].request, "build_opener", return_value=opener):
            with patch.object(opener, "open", wraps=opened) as call:
                NS["upload_package"](metadata, data, "opaque-token")
        request = call.call_args.args[0]
        self.assertEqual(request.full_url, "https://crates.io/api/v1/crates/new")
        self.assertEqual(request.method, "PUT")
        self.assertEqual(request.headers["Authorization"], "opaque-token")
        length = struct.unpack("<I", request.data[:4])[0]
        self.assertEqual(json.loads(request.data[4:4 + length]), metadata)
        self.assertEqual(struct.unpack("<I", request.data[4 + length:8 + length])[0], len(data))
        self.assertEqual(request.data[8 + length:], data)

    def test_redirect_and_header_injection_fail(self):
        with self.assertRaisesRegex(ReconcileError, "publish_redirect"):
            NS["PublishRedirect"]().redirect_request(None, None, 302, "", {}, "https://evil.test")
        with self.assertRaisesRegex(ReconcileError, "publish_token"):
            NS["upload_package"]({}, b"archive", "token\nInjected: true")

    def test_trusted_exchange_fresh_jwt_exact_headers_and_revoke(self):
        approved = {"packages": {"demo": "1.0.0"}, "authentication": "trusted-publishing"}
        environment = {"ACTIONS_ID_TOKEN_REQUEST_URL":
                       "https://pipelines.actions.githubusercontent.com/token?api-version=2.0",
                       "ACTIONS_ID_TOKEN_REQUEST_TOKEN": "runner-token"}
        calls = []
        def secret(request, statuses):
            calls.append(request)
            if request.get_method() == "POST":
                return {"token": "registry-token"}
            return {} if request.get_method() == "DELETE" else {"value": "oidc-jwt"}
        with patch.dict(NS["os"].environ, environment, clear=True), \
             patch.dict(NS, {"_secret_request": secret}):
            with NS["registry_token"](approved, "demo") as token:
                self.assertEqual(token, "registry-token")
        self.assertIn("audience=crates.io", calls[0].full_url)
        self.assertEqual(calls[0].headers["Authorization"], "Bearer runner-token")
        self.assertEqual(json.loads(calls[1].data), {"jwt": "oidc-jwt"})
        self.assertNotIn("Authorization", calls[1].headers)
        self.assertEqual(calls[2].method, "DELETE")
        self.assertEqual(calls[2].headers["Authorization"], "Bearer registry-token")

    def test_authentication_exclusive_and_oidc_origin_fixed(self):
        approved = {"packages": {"demo": "1.0.0"}, "authentication": "bootstrap-token"}
        with patch.dict(NS["os"].environ, {"CARGO_REGISTRY_TOKEN": "token",
                "ACTIONS_ID_TOKEN_REQUEST_TOKEN": "also-token"}, clear=True):
            with self.assertRaisesRegex(ReconcileError, "publish_auth_exclusive"):
                with NS["registry_token"](approved, "demo"):
                    self.fail("conflicting credentials admitted")
        with patch.dict(NS["os"].environ, {"ACTIONS_ID_TOKEN_REQUEST_URL": "https://evil.test/token"}, clear=True):
            with self.assertRaisesRegex(ReconcileError, "publish_oidc_origin"):
                NS["_trusted_token"]()


if __name__ == "__main__":
    unittest.main()
