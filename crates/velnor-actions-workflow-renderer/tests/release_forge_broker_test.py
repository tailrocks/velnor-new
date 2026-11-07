# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import email.message
import json
from pathlib import Path
import time
import sys
import unittest
from unittest.mock import patch

root = Path(__file__).resolve().parents[1] / 'src'
entry_path = root / 'release_publish_entry.py'
entry = {'__name__': 'broker_entry_fixture'}
source = entry_path.read_text().replace('__VELNOR_JOB_TIMEOUT_SECONDS__', '1800')
exec(compile(source, str(entry_path), 'exec'), entry)
assert 'velnor_publisher_broker' not in sys.modules
payload = base64.b64encode((root / 'release_forge_broker.py').read_bytes()).decode()
broker = entry['module']('velnor_publisher_broker', payload)


class Stream:
    def settimeout(self, timeout):
        pass

    def shutdown(self, how):
        pass


class Response:
    def __init__(self, body, headers=(), status=200, length=None):
        self.body = body
        self.status = status
        self.headers = email.message.Message()
        for key, value in headers:
            self.headers.add_header(key, value)
        self.length = len(body) if length is None else length

    def isclosed(self):
        return not self.body

    def read1(self, amount):
        result, self.body = self.body[:amount], self.body[amount:]
        self.length -= len(result)
        return result


class Connection:
    responses = []
    routes = []

    def __init__(self, host, timeout, context):
        assert host == 'api.github.com'
        self.sock = Stream()

    def connect(self):
        pass

    def request(self, method, route, headers):
        self.routes.append(route)

    def getresponse(self):
        return self.responses.pop(0)

    def close(self):
        pass


def response(value, **kwargs):
    return Response(json.dumps(value).encode(), **kwargs)


class TransportTest(unittest.TestCase):
    def setUp(self):
        Connection.routes = []
        self.connection = patch.object(broker.http.client, 'HTTPSConnection', Connection)
        self.socket = patch.object(broker.ssl, 'SSLSocket', Stream)
        self.context = patch.object(broker.ssl, 'SSLContext')
        self.connection.start()
        self.socket.start()
        self.context.start()
        self.addCleanup(self.connection.stop)
        self.addCleanup(self.socket.stop)
        self.addCleanup(self.context.stop)
        self.transport = broker.GitHubTransport('secret-token')
        self.deadline = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 60_000_000_000

    def fetch(self, pages):
        Connection.responses = list(pages)
        return json.loads(self.transport('/repos/a/b/commits/' + 'a' * 40 + '/pulls', self.deadline))

    def test_pagination(self):
        for pages, expected in [([response([1] * 100, headers=[('Link', '<https://evil.invalid>; rel="next"')]), response([2])], [1] * 100 + [2]),
                                ([response([3])], [3]),
                                ([response([4] * 100), response([])], [4] * 100)]:
            with self.subTest(expected=len(expected)):
                Connection.routes = []
                self.assertEqual(self.fetch(pages), expected)
                self.assertEqual([r.partition('?')[2] for r in Connection.routes],
                                 [f'per_page=100&page={i}' for i in range(1, len(pages) + 1)])

    def test_invalid_pages(self):
        for pages in ([response([1] * 100), response([2] * 101)],
                      [response([1] * 100), Response(b'{')],
                      [response([1] * 100) for _ in range(100)],
                      [response(['x' * 83100] * 100), response(['x' * 83100])],
                      [response([], headers=[('Content-Encoding', 'gzip')])],
                      [response([], headers=[('Content-Length', '2'), ('Content-Length', '2')])],
                      [response([], headers=[('Content-Length', '2'), ('Transfer-Encoding', 'chunked')])],
                      [response([], headers=[('Transfer-Encoding', 'gzip')])],
                      [response([1] * 100), response([], headers=[('Content-Encoding', 'gzip')])],
                      [response([1] * 100), Response(b'[\"secret-token\"]')],
                      [Response(b'[]', length=3)],
                      [Response(b'["secret-token"]')]):
            with self.subTest(pages=len(pages)), self.assertRaises(ValueError):
                self.fetch(pages)

    def test_cancel_and_deadline(self):
        self.transport.cancel()
        with self.assertRaises(ValueError):
            self.fetch([response([])])
        self.transport = broker.GitHubTransport('secret-token')
        self.deadline = 0
        with self.assertRaises(TimeoutError):
            self.fetch([response([])])


if __name__ == '__main__':
    unittest.main()
