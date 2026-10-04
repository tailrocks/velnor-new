"""Anonymous transport and bounded pagination rejection evidence."""
import io
import json
import os
import ssl
import tempfile
import unittest
from unittest.mock import Mock, patch

import cache_receipt_api as api
from cache_receipt_common import ColdReceipt

_REPOSITORY = 'owner/project'
_ENDPOINT = 'repos/owner/project/actions/runs/7/attempts/2'


class Response(io.BytesIO):
    def __init__(self, data, url, status=200, encoding='identity'):
        super().__init__(data)
        self.url, self.status = url, status
        self.headers = {'Content-Encoding': encoding}

    def geturl(self):
        return self.url


class Opener:
    def __init__(self, values):
        self.values, self.requests = list(values), []

    def open(self, request, timeout):
        self.requests.append((request, timeout))
        value = self.values.pop(0)
        if isinstance(value, bytes):
            return Response(value, request.full_url)
        return Response(json.dumps(value).encode(), request.full_url)


def jobs(start, count, total):
    return {'total_count': total, 'jobs': [{'id': value} for value in range(start, start + count)]}


class PublicReceiptApiTests(unittest.TestCase):
    def test_fixed_anonymous_request(self):
        opener = Opener([{'id': 7}])
        with patch.dict(os.environ, {'GH_TOKEN': 'secret', 'GITHUB_TOKEN': 'secret',
                                     'HTTPS_PROXY': 'https://attacker.invalid'}):
            self.assertEqual(api._fetch(_REPOSITORY, _ENDPOINT, False, opener), {'id': 7})
        request, timeout = opener.requests[0]
        self.assertEqual(request.full_url, 'https://api.github.com/' + _ENDPOINT)
        self.assertEqual(request.get_method(), 'GET')
        self.assertFalse(request.has_header('Authorization'))
        self.assertFalse(request.has_header('Proxy-authorization'))
        self.assertLessEqual(timeout, 10)

    def test_closed_endpoint(self):
        for endpoint in ('https://attacker.invalid/', '//attacker.invalid/',
                         _ENDPOINT + '?token=x', _ENDPOINT.replace('/7/', '/0/'),
                         _ENDPOINT.replace('owner/project', 'owner/other'),
                         _ENDPOINT + '/../../secrets', _ENDPOINT + '/jobs?per_page=100&page=2'):
            with self.subTest(endpoint=endpoint), self.assertRaises(ColdReceipt):
                api._fetch(_REPOSITORY, endpoint, False, Opener([]))
        for repository in ('../project', 'owner/..', 'owner/project/extra', None, ''):
            with self.subTest(repository=repository), self.assertRaises(ColdReceipt):
                api.PublicReceiptApi(repository)
        with self.assertRaises(ColdReceipt):
            api._fetch(_REPOSITORY, _ENDPOINT, True, Opener([]))

    def test_opener_disables_proxy_and_ambient_ca(self):
        with patch.object(api.ssl, 'SSLContext') as context, \
                patch.object(api.urllib.request, 'build_opener') as build:
            api._opener()
        context.assert_called_once_with(ssl.PROTOCOL_TLS_CLIENT)
        context.return_value.load_verify_locations.assert_called_once_with(
            cafile='/etc/ssl/certs/ca-certificates.crt')
        handlers = build.call_args.args
        self.assertEqual(handlers[0].proxies, {})
        self.assertIsInstance(handlers[1], api.urllib.request.HTTPSHandler)
        self.assertIsInstance(handlers[2], api._NoRedirect)

    def test_actual_context_ignores_keylog_proxy_and_ca_environment(self):
        trusted_ca = ssl.get_default_verify_paths().cafile
        self.assertIsNotNone(trusted_ca)
        with tempfile.TemporaryDirectory() as root:
            keylog = root + '/attacker-keylog'
            environment = {'SSLKEYLOGFILE': keylog, 'SSL_CERT_FILE': root + '/missing-ca',
                           'SSL_CERT_DIR': root + '/missing-directory',
                           'HTTPS_PROXY': 'https://attacker.invalid',
                           'GH_TOKEN': 'secret', 'GITHUB_TOKEN': 'secret'}
            with patch.dict(os.environ, environment), patch.object(api, '_CA_FILE', trusted_ca):
                opener = api._opener()
            handler = next(item for item in opener.handlers
                           if isinstance(item, api.urllib.request.HTTPSHandler))
            self.assertIsNone(handler._context.keylog_filename)
            self.assertTrue(handler._context.check_hostname)
            self.assertEqual(handler._context.verify_mode, ssl.CERT_REQUIRED)
            self.assertFalse(os.path.exists(keylog))

    def test_every_redirect_rejected(self):
        handler = api._NoRedirect()
        for status in (301, 302, 303, 307, 308):
            with self.subTest(status=status), self.assertRaises(ColdReceipt):
                handler.redirect_request(None, None, status, '', {}, 'https://api.github.com/other')

    def test_response_rejection(self):
        for response in (Response(b'{}', 'https://attacker.invalid'),
                         Response(b'{}', 'https://api.github.com/' + _ENDPOINT, 302),
                         Response(b'{}', 'https://api.github.com/' + _ENDPOINT, encoding='gzip')):
            with self.subTest(response=response), self.assertRaises(ColdReceipt):
                api._fetch(_REPOSITORY, _ENDPOINT, False, Mock(open=Mock(return_value=response)))

    def test_strict_json_and_response_limit(self):
        for data in (b'{"id":7,"id":8}', b'{"x":NaN}', b'{"x":1e999}', b'[]', b'\xff'):
            with self.subTest(data=data), self.assertRaises(ColdReceipt):
                api._fetch(_REPOSITORY, _ENDPOINT, False, Opener([data]))
        with patch.object(api, '_LIMIT', 8), self.assertRaises(ColdReceipt):
            api._fetch(_REPOSITORY, _ENDPOINT, False, Opener([b'{"id":777}']))

    def test_complete_pagination_ignores_links(self):
        opener = Opener([jobs(1, 100, 101), jobs(101, 1, 101)])
        endpoint = _ENDPOINT + '/jobs?per_page=100'
        self.assertEqual(len(api._fetch(_REPOSITORY, endpoint, True, opener)), 2)
        self.assertEqual(opener.requests[1][0].full_url,
                         'https://api.github.com/' + endpoint + '&page=2')
        self.assertEqual(api._fetch(_REPOSITORY, endpoint, True, Opener([jobs(1, 0, 0)])),
                         [{'total_count': 0, 'jobs': []}])

    def test_job_page_limits_and_incomplete_evidence(self):
        endpoint = _ENDPOINT + '/jobs?per_page=100'
        bad = ([{'total_count': True, 'jobs': []}], [jobs(1, 0, 4097)],
               [jobs(1, 0, 3201)], [jobs(1, 1, 2)],
               [jobs(1, 100, 101), jobs(101, 1, 102)],
               [jobs(1, 100, 101), jobs(1, 1, 101)],
               [{'total_count': 1, 'jobs': [{'id': True}]}])
        for values in bad:
            with self.subTest(values=values), self.assertRaises(ColdReceipt):
                api._fetch(_REPOSITORY, endpoint, True, Opener(values))
        first, second = jobs(1, 100, 101), jobs(101, 1, 101)
        combined_limit = len(json.dumps(first).encode()) + len(json.dumps(second).encode()) - 1
        with patch.object(api, '_LIMIT', combined_limit), self.assertRaises(ColdReceipt):
            api._fetch(_REPOSITORY, endpoint, True,
                       Opener([first, second]))

    def test_deadline_and_parent_worker_termination(self):
        with patch.object(api, '_DEADLINE', 0), self.assertRaises(ColdReceipt):
            api._fetch(_REPOSITORY, _ENDPOINT, False, Opener([]))
        process = Mock(pid=1, exitcode=None)
        process.is_alive.side_effect = [True, False]
        context = Mock(Process=Mock(return_value=process))
        with tempfile.TemporaryFile() as temporary, \
                patch.object(api.os, 'memfd_create', return_value=os.dup(temporary.fileno()), create=True), \
                patch.object(api.os, 'MFD_CLOEXEC', 1, create=True), \
                patch.object(api.multiprocessing, 'get_context', return_value=context), \
                self.assertRaises(ColdReceipt):
            api.PublicReceiptApi(_REPOSITORY).api(_ENDPOINT)
        process.kill.assert_called_once()
        process.close.assert_called_once()


if __name__ == '__main__':
    unittest.main()
