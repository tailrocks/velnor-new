# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import http.client
from pathlib import Path
from urllib.parse import urlsplit, urlunsplit

import job_image_authority_publish as publish_module

RESPONSE_BODY_LIMIT = 65536
class _RegistryResponse:
    def __init__(self, status, headers, body):
        self.status = status
        self.headers = {key.lower(): value for key, value in headers.items()}
        self.body = body
class _RegistryStream:
    def __init__(self, connection, response):
        self.status = response.status
        self.headers = {key.lower(): value for key, value in response.getheaders()}
        self._connection = connection
        self._response = response
    def read(self, size=-1):
        try:
            return self._response.read(size)
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            raise publish_module.AuthorityError("Registry request failed or timed out.") from error
    def close(self):
        try:
            self._response.close()
        finally:
            self._connection.close()
def _check_origin(parts, registry):
    allowed = (registry.REGISTRY, registry.DOWNLOAD_HOST)
    if (
        parts.scheme != "https"
        or parts.hostname not in allowed
        or parts.port not in (None, 443)
        or parts.username is not None
        or parts.password is not None
        or parts.fragment
    ):
        raise publish_module.AuthorityError("Registry URL is outside the fixed HTTPS origin.")
class RegistryTransport:
    def __init__(self, timeout, registry):
        self.timeout = timeout
        self.registry = registry
    def _connect(self, method, url, headers):
        parts = urlsplit(url)
        _check_origin(parts, self.registry)
        for name, value in headers.items():
            if "\r" in name + value or "\n" in name + value:
                raise publish_module.AuthorityError("Registry request header has an incorrect format.")
        connection = http.client.HTTPSConnection(parts.hostname, timeout=self.timeout)
        try:
            target = urlunsplit(("", "", parts.path, parts.query, ""))
            connection.putrequest(method, target)
            for name, value in headers.items():
                connection.putheader(name, value)
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            connection.close()
            raise publish_module.AuthorityError("Registry request failed or timed out.") from error
        return connection
    def request(self, method, url, headers, body=None, length=0):
        connection = self._connect(method, url, headers)
        try:
            if body is not None:
                if isinstance(body, Path):
                    try:
                        actual = body.stat().st_size
                    except OSError as error:
                        raise publish_module.AuthorityError("Registry request body is missing.") from error
                    if actual != length:
                        raise publish_module.AuthorityError("Registry request body length changed.")
                    connection.putheader("Content-Length", str(length))
                    connection.endheaders()
                    with body.open("rb") as stream:
                        while chunk := stream.read(65536):
                            connection.send(chunk)
                else:
                    if not isinstance(body, bytes) or len(body) != length:
                        raise publish_module.AuthorityError("Registry request body length changed.")
                    connection.putheader("Content-Length", str(length))
                    connection.endheaders()
                    connection.send(body)
            else:
                connection.endheaders()
            response = connection.getresponse()
            chunks = bytearray()
            while len(chunks) <= RESPONSE_BODY_LIMIT:
                chunk = response.read(min(65536, RESPONSE_BODY_LIMIT + 1 - len(chunks)))
                if not chunk:
                    break
                chunks.extend(chunk)
            if len(chunks) > RESPONSE_BODY_LIMIT:
                raise publish_module.AuthorityError("Registry response exceeds its size limit.")
            return _RegistryResponse(response.status, dict(response.getheaders()), bytes(chunks))
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            raise publish_module.AuthorityError("Registry request failed or timed out.") from error
        finally:
            connection.close()
    def stream(self, method, url, headers):
        connection = self._connect(method, url, headers)
        try:
            connection.endheaders()
            response = connection.getresponse()
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            connection.close()
            raise publish_module.AuthorityError("Registry request failed or timed out.") from error
        return _RegistryStream(connection, response)
