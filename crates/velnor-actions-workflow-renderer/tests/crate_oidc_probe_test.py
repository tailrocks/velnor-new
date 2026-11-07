# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import importlib.util
import contextlib
import io
import os
from unittest.mock import patch
import json
from pathlib import Path
import threading
import subprocess
import tempfile
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import ProxyHandler, build_opener

path = Path(__file__).resolve().parents[1] / "src" / "crate_oidc_probe.py"
spec = importlib.util.spec_from_file_location("crate_oidc_probe", path)
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class Exchange(BaseHTTPRequestHandler):
    records = []
    revoke_status = 200
    oidc_status = 200
    oidc_response = b'{"value":"identity-secret"}'
    response_data = b'{"token":"registry-secret"}'

    def log_message(self, format, *args):
        pass

    def send(self, status, data):
        self.send_response(status)
        if status == 302:
            self.send_header("Location", "/leak")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        self.records.append((self.command, self.path, self.headers.get("Authorization")))
        self.send(self.oidc_status, self.oidc_response)

    def do_POST(self):
        size = int(self.headers["Content-Length"])
        self.records.append((self.command, self.path, json.loads(self.rfile.read(size))))
        self.send(200, self.response_data)

    def do_DELETE(self):
        self.records.append((self.command, self.path, self.headers.get("Authorization")))
        self.send(self.revoke_status, b"{}")


class ProbeTest(unittest.TestCase):
    def setUp(self):
        Exchange.records = []
        Exchange.revoke_status = 200
        Exchange.oidc_status = 200
        Exchange.oidc_response = b'{"value":"identity-secret"}'
        Exchange.response_data = b'{"token":"registry-secret"}'
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Exchange)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.origin = f"http://127.0.0.1:{self.server.server_port}"
        self.opener = build_opener(ProxyHandler({}), probe.RejectRedirect())

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def test_launcher_ignores_injected_interpreter_environment(self):
        launcher = path.with_name("crate_oidc_probe_launcher.sh")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bash_env = root / "bash-env"
            marker = root / "executed"
            bash_env.write_text("touch " + str(marker) + "\n")
            keylog = root / "tls-log"
            code = "import os,ssl; assert 'SSLKEYLOGFILE' not in os.environ; assert getattr(ssl.create_default_context(), 'keylog_filename', None) is None"
            env = dict(os.environ, BASH_ENV=str(bash_env), SSLKEYLOGFILE=str(keylog), PYTHONPATH=str(root))
            subprocess.run(["/bin/bash", "-p", str(launcher), "-c", "raise SystemExit(9)"],
                           input=code, text=True, env=env, check=True)
            self.assertFalse(marker.exists())
            self.assertFalse(keylog.exists())

    def test_probe_rejects_keylog_environment_before_ssl(self):
        with patch.dict(os.environ, {"SSLKEYLOGFILE": "synthetic"}, clear=True):
            with patch.object(probe.ssl, "create_default_context") as create:
                with contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(probe.main(), 1)
                create.assert_not_called()

    def test_identity_exchange_revokes_once(self):
        probe.run_exchange(self.opener, self.origin + "/oidc?x=1", "request-secret", self.origin + "/tokens")
        self.assertEqual(Exchange.records, [
            ("GET", "/oidc?x=1&audience=crates.io", "Bearer request-secret"),
            ("POST", "/tokens", {"jwt": "identity-secret"}),
            ("DELETE", "/tokens", "Bearer registry-secret"),
        ])

    def test_bad_identity_is_not_sent_to_registry(self):
        for status, body in [(302, b"redirect"), (200, b"not json"), (200, b"x" * (probe.MAX_RESPONSE + 1))]:
            Exchange.records = []
            Exchange.oidc_status = status
            Exchange.oidc_response = body
            with self.assertRaises(Exception):
                probe.run_exchange(self.opener, self.origin + "/oidc", "request-secret", self.origin + "/tokens")
            self.assertEqual([r[0] for r in Exchange.records], ["GET"])

    def test_failed_revocation_fails(self):
        Exchange.revoke_status = 500
        with self.assertRaises(Exception):
            probe.run_exchange(self.opener, self.origin + "/oidc", "request-secret", self.origin + "/tokens")
        self.assertEqual([r[0] for r in Exchange.records], ["GET", "POST", "DELETE"])

    def test_malformed_or_oversized_registry_response_fails(self):
        for value in [b"not json", b"x" * (probe.MAX_RESPONSE + 1),
                      b'{"token":"first","token":"second"}']:
            Exchange.response_data = value
            Exchange.records = []
            with self.assertRaises(Exception):
                probe.run_exchange(self.opener, self.origin + "/oidc", "request-secret", self.origin + "/tokens")
            self.assertEqual([r[0] for r in Exchange.records], ["GET", "POST"])

    def test_failure_does_not_disclose_identity_credentials(self):
        output = io.StringIO()
        synthetic = "synthetic-credential-do-not-print"
        with patch.dict(os.environ, {"ACTIONS_ID_TOKEN_REQUEST_URL": "https://invalid.example/" + synthetic,
                                    "ACTIONS_ID_TOKEN_REQUEST_TOKEN": synthetic}, clear=True):
            with contextlib.redirect_stderr(output):
                self.assertEqual(probe.main(), 1)
        self.assertNotIn(synthetic, output.getvalue())
        self.assertEqual(output.getvalue(), "OIDC qualification failed\n")

    def test_identity_endpoint_rejects_untrusted_origins(self):
        self.assertTrue(probe.valid_request_url("https://a.actions.githubusercontent.com/request?x=1"))
        for value in ["http://a.actions.githubusercontent.com/request", "https://actions.githubusercontent.com.evil/request", "https://evil@a.actions.githubusercontent.com/request", "https://a.actions.githubusercontent.com/request?audience=other", "https://a.actions.githubusercontent.com/request?Audience=other"]:
            self.assertFalse(probe.valid_request_url(value))


if __name__ == "__main__":
    unittest.main()
