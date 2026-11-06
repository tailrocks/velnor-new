# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import http.client
import json
import re
import socket
import ssl
import threading
import time


MAX_HEADER = 16 * 1024
MAX_RESPONSE = 8 * 1024 * 1024
MAX_REQUESTS = 64
MAX_TOTAL_RESPONSE = 16 * 1024 * 1024


def remaining(deadline):
    value = (deadline - time.clock_gettime_ns(time.CLOCK_BOOTTIME)) / 1_000_000_000
    if value <= 0:
        raise TimeoutError('broker deadline')
    return min(value, 10)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate JSON key')
        result[key] = value
    return result


def invalid_constant(_value):
    raise ValueError('invalid JSON constant')


def validate_body(body):
    if not isinstance(body, bytes) or len(body) > MAX_RESPONSE:
        raise ValueError('invalid upstream body')
    value = json.loads(body.decode('utf-8'), object_pairs_hook=unique_object, parse_constant=invalid_constant)
    if not isinstance(value, list):
        raise ValueError('invalid upstream body')
    return body


class GitHubTransport:
    def __init__(self, token):
        if not isinstance(token, str) or not token or any(ord(c) < 33 or ord(c) > 126 for c in token):
            raise ValueError('invalid credential')
        self._token = token
        self._lock = threading.Lock()
        self._cancelled = threading.Event()
        self._stream = None

    def cancel(self):
        self._cancelled.set()
        with self._lock:
            if self._stream is not None:
                try:
                    self._stream.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass

    def __call__(self, route, deadline):
        if self._cancelled.is_set():
            raise ValueError('upstream cancelled')
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        context.load_verify_locations(cafile='/etc/ssl/certs/ca-certificates.crt')
        connection = http.client.HTTPSConnection('api.github.com', timeout=remaining(deadline), context=context)
        watcher = None
        finished = threading.Event()
        try:
            connection.connect()
            remaining(deadline)
            stream = connection.sock
            if not isinstance(stream, ssl.SSLSocket):
                raise ValueError('upstream TLS stream unavailable')
            with self._lock:
                if self._cancelled.is_set():
                    raise ValueError('upstream cancelled')
                self._stream = stream
            watcher = threading.Thread(target=interrupt_at_deadline, args=(stream, deadline, finished), daemon=True)
            watcher.start()
            connection.request('GET', route, headers={
                'Accept': 'application/vnd.github+json',
                'Accept-Encoding': 'identity',
                'Authorization': 'Bearer ' + self._token,
                'User-Agent': 'velnor-release-broker',
                'X-GitHub-Api-Version': '2022-11-28',
            })
            response = connection.getresponse()
            if response.status != 200 or response.headers.get_all('Link'):
                raise ValueError('invalid upstream response')
            if response.headers.get_all('Content-Encoding'):
                raise ValueError('invalid upstream encoding')
            lengths = response.headers.get_all('Content-Length', [])
            encodings = response.headers.get_all('Transfer-Encoding', [])
            if (len(lengths) > 1 or len(encodings) > 1 or (lengths and encodings)
                    or (encodings and encodings[0].lower() != 'chunked')
                    or (lengths and not re.fullmatch(r'[0-9]+', lengths[0]))):
                raise ValueError('invalid upstream framing')
            parts = []
            size = 0
            while not response.isclosed():
                stream.settimeout(remaining(deadline))
                part = response.read1(min(65536, MAX_RESPONSE + 1 - size))
                if not part:
                    break
                size += len(part)
                if size > MAX_RESPONSE:
                    raise ValueError('upstream response too large')
                parts.append(part)
            if response.length not in (None, 0):
                raise ValueError('truncated upstream response')
            remaining(deadline)
            body = validate_body(b''.join(parts))
            if self._token.encode() in body:
                raise ValueError('credential in upstream response')
            return body
        finally:
            with self._lock:
                self._stream = None
            finished.set()
            if watcher is not None:
                watcher.join()
            connection.close()


def interrupt_at_deadline(stream, deadline, finished):
    while not finished.wait(0.01):
        if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
            try:
                stream.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            return


class ForgeBroker:
    def __init__(self, owner, repository, sha, deadline, fetch):
        if (not re.fullmatch(r'[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?', owner)
                or not re.fullmatch(r'[A-Za-z0-9_.-]{1,100}', repository)
                or repository in ('.', '..') or not re.fullmatch(r'[0-9a-f]{40}', sha)
                or not callable(fetch)):
            raise ValueError('invalid broker identity')
        remaining(deadline)
        self.deadline = deadline
        self._path = f'/repos/{owner}/{repository}/commits/{sha}/pulls'
        self._fetch = fetch
        self._socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._socket.bind(('127.0.0.1', 0))
        self._socket.listen(8)
        self._socket.settimeout(0.1)
        self.port = self._socket.getsockname()[1]
        self._closed = threading.Event()
        self._lock = threading.Lock()
        self._thread = None
        self._client = None
        self._cached = None
        self._total = 0

    def close(self):
        self._closed.set()
        self._socket.close()
        with self._lock:
            if self._client is not None:
                try:
                    self._client.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
            thread = self._thread
        cancel = getattr(self._fetch, 'cancel', None)
        if cancel is not None:
            cancel()
        if thread is not None and thread is not threading.current_thread():
            thread.join(1)
            if thread.is_alive():
                raise TimeoutError('broker cleanup uncertain')

    def serve(self):
        with self._lock:
            if self._closed.is_set():
                return
            if self._thread is not None:
                raise ValueError('broker already served')
            self._thread = threading.current_thread()
        count = 0
        while not self._closed.is_set():
            try:
                remaining(self.deadline)
                client, _ = self._socket.accept()
            except socket.timeout:
                continue
            except (OSError, TimeoutError):
                return
            count += 1
            if count > MAX_REQUESTS:
                client.close()
                return
            with client:
                with self._lock:
                    if self._closed.is_set():
                        return
                    self._client = client
                try:
                    self._respond(client)
                except (OSError, TimeoutError, ValueError):
                    pass
                finally:
                    with self._lock:
                        self._client = None

    def _respond(self, client):
        data = bytearray()
        while b'\r\n\r\n' not in data:
            client.settimeout(remaining(self.deadline))
            part = client.recv(min(4096, MAX_HEADER + 1 - len(data)))
            if not part:
                return
            data.extend(part)
            if len(data) > MAX_HEADER:
                return
        head, extra = bytes(data).split(b'\r\n\r\n', 1)
        if extra:
            return
        lines = head.split(b'\r\n')
        if lines[0] != ('GET /api/v3' + self._path + ' HTTP/1.1').encode('ascii'):
            return
        headers = {}
        for line in lines[1:]:
            name, sep, value = line.partition(b':')
            name = name.lower()
            if (not sep or not re.fullmatch(rb'[a-z0-9-]+', name) or name in headers
                    or any(byte < 32 or byte > 126 for byte in value)):
                return
            headers[name] = value.strip()
        if (headers.get(b'host') != f'127.0.0.1:{self.port}'.encode()
                or b'transfer-encoding' in headers or b'content-length' in headers):
            return
        body = validate_body(self._fetch(self._path, self.deadline))
        if self._cached is not None and body != self._cached:
            raise ValueError('upstream response changed')
        self._total += len(body)
        if self._total > MAX_TOTAL_RESPONSE:
            raise ValueError('broker response budget')
        self._cached = body
        remaining(self.deadline)
        if self._closed.is_set():
            return
        response = (b'HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n'
                    + b'Content-Length: ' + str(len(body)).encode() + b'\r\n\r\n' + body)
        client.settimeout(remaining(self.deadline))
        client.sendall(response)
