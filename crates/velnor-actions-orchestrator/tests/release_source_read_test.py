"""Closed source-object transport; all GitHub observations stay mocked."""
import io
import json
import os
from pathlib import Path
import unittest
from unittest.mock import patch
import urllib.error

ROOT = Path(__file__).resolve().parents[1] / "src"
NS = {"__name__": "source_read_test"}
for filename in ("release_reconcile_common.py", "release_forge_publish_read.py"):
    source = ROOT / filename
    exec(compile(source.read_text(), str(source), "exec"), NS)
SHA = "a" * 40


class Response(io.BytesIO):
    status = 200

    def read(self, limit):
        self.requested = limit
        return super().read(limit)


class SourceReadTests(unittest.TestCase):
    def invoke(self, function, body, suffix):
        response = Response(body)
        requests = []

        class Opener:
            def open(self, request, timeout):
                requests.append((request, timeout))
                return response

        with patch.dict(os.environ, {"GH_TOKEN": "fixture-token"}), patch.object(
                NS["urllib"].request, "build_opener", return_value=Opener()):
            result = NS[function]("owner/repo", SHA)
        request, timeout = requests[0]
        self.assertEqual(request.full_url, "https://api.github.com/repos/owner/repo/git/" + suffix)
        self.assertEqual(request.method, "GET")
        self.assertEqual(request.headers["Authorization"], "Bearer fixture-token")
        self.assertEqual(timeout, 40)
        return result, response.requested

    def test_fixed_source_routes_and_operation_bounds(self):
        cases = (("read_source_commit", "commits/" + SHA, 1),
                 ("read_source_tree", "trees/" + SHA + "?recursive=1", 8),
                 ("read_source_blob", "blobs/" + SHA, 24))
        for function, suffix, megabytes in cases:
            with self.subTest(function=function):
                value = {"sha": SHA}
                result, requested = self.invoke(function, json.dumps(value).encode(), suffix)
                self.assertEqual(result, value)
                self.assertEqual(requested, megabytes * 1024 * 1024 + 1)

    def test_source_response_identity_and_type(self):
        for value in ([], "object", 1, {"sha": "b" * 40}, {}):
            with self.subTest(value=value), self.assertRaisesRegex(
                    NS["ReconcileError"], "forge_read_source_identity"):
                self.invoke("read_source_commit", json.dumps(value).encode(), "commits/" + SHA)

    def test_duplicate_json_authority_is_rejected(self):
        with self.assertRaisesRegex(NS["ReconcileError"], "forge_read_response_json"):
            self.invoke("read_source_blob", ('{"sha":"' + SHA + '","sha":"' + SHA + '"}').encode(),
                        "blobs/" + SHA)

    def test_source_sha_and_repository_are_closed_before_network(self):
        with patch.object(NS["urllib"].request, "build_opener") as transport:
            for value in (True, None, "../HEAD", "A" * 40, SHA + "?recursive=1"):
                with self.subTest(value=value), self.assertRaises(NS["ReconcileError"]):
                    NS["read_source_tree"]("owner/repo", value)
            for repository in ("../repo", "owner/..", "owner/repo/extra", "owner/repo?token=1"):
                with self.subTest(repository=repository), self.assertRaises(NS["ReconcileError"]):
                    NS["read_source_blob"](repository, SHA)
            transport.assert_not_called()

    def test_unapproved_endpoint_and_query_are_rejected_before_network(self):
        endpoints = ("repos/owner/repo/git/trees/" + SHA,
                     "repos/owner/repo/git/trees/" + SHA + "?recursive=0",
                     "repos/owner/repo/git/commits/" + SHA + "?page=1",
                     "repos/other/repo/git/blobs/" + SHA,
                     "repos/owner/repo/contents/Cargo.toml",
                     "https://api.github.com/repos/owner/repo/git/blobs/" + SHA)
        with patch.object(NS["urllib"].request, "build_opener") as transport:
            for endpoint in endpoints:
                with self.subTest(endpoint=endpoint), self.assertRaises(NS["ReconcileError"]):
                    NS["forge_read_request"]("owner/repo", endpoint)
            transport.assert_not_called()

    def test_oversized_source_body_is_rejected(self):
        response = Response(b"x" * (1024 * 1024 + 1))
        with self.assertRaisesRegex(NS["ReconcileError"], "forge_read_response_size"):
            NS["_read_response_body"](response, "source-commit")
        self.assertEqual(response.requested, 1024 * 1024 + 1)

    def test_missing_source_object_returns_no_unproved_payload(self):
        class Opener:
            def open(self, request, timeout):
                raise urllib.error.HTTPError(request.full_url, 404, "missing", {}, None)
        with patch.dict(os.environ, {"GH_TOKEN": "fixture-token"}), patch.object(
                NS["urllib"].request, "build_opener", return_value=Opener()):
            self.assertIsNone(NS["read_source_blob"]("owner/repo", SHA))

    def test_every_redirect_is_denied(self):
        with self.assertRaisesRegex(NS["ReconcileError"], "forge_read_redirect"):
            NS["ForgeRedirect"]().redirect_request(None, None, 302, "redirect", {},
                                                  "https://attacker.invalid/")


if __name__ == "__main__":
    unittest.main()
