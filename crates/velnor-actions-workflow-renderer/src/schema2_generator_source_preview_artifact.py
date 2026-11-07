# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import http.client
import io
import os
from pathlib import Path
import re
import ssl
import stat
import time
from urllib.parse import urlsplit
import zipfile


def download_verified_payload(client, artifact, output_path):
    name = "velnor-actions-x86_64-unknown-linux-gnu"
    limit = 256 * 1024 * 1024
    archive_limit = 257 * 1024 * 1024
    identity = artifact.get("id")
    size = artifact.get("size_in_bytes")
    expected = artifact.get("digest")
    if (type(identity) is not int or identity <= 0 or type(size) is not int
            or not 0 < size <= archive_limit or not isinstance(expected, str)
            or not re.fullmatch(r"sha256:[0-9a-f]{64}", expected)):
        raise ValueError("preview artifact metadata")
    host = "api.github.com"
    path = "/repos/tailrocks/velnor-new/actions/artifacts/" + str(identity) + "/zip"
    authorized = True
    for _ in range(4):
        remaining = client.deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("preview artifact deadline")
        connection = http.client.HTTPSConnection(host, timeout=min(30, remaining), context=ssl.create_default_context())
        try:
            headers = {"Accept": "application/vnd.github+json", "User-Agent": "velnor-source-preview"}
            if authorized:
                headers["Authorization"] = "Bearer " + client.token
            connection.request("GET", path, headers=headers)
            response = connection.getresponse()
            if response.status in (301, 302, 303, 307, 308):
                redirect = urlsplit(response.getheader("Location", ""))
                if (redirect.scheme != "https" or not redirect.hostname
                        or not re.fullmatch(r"productionresultssa[0-9]+\.blob\.core\.windows\.net", redirect.hostname)
                        or redirect.username is not None or redirect.password is not None
                        or redirect.port not in (None, 443) or redirect.fragment):
                    raise ValueError("preview artifact redirect")
                host = redirect.hostname
                path = redirect.path + ("?" + redirect.query if redirect.query else "")
                authorized = False
                continue
            if response.status != 200 or authorized:
                raise ValueError("preview artifact response")
            data = bytearray()
            while len(data) <= archive_limit:
                remaining = client.deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("preview artifact deadline")
                if connection.sock is not None:
                    connection.sock.settimeout(min(30, remaining))
                chunk = response.read(min(65536, archive_limit + 1 - len(data)))
                if not chunk:
                    break
                data.extend(chunk)
            if len(data) != size or len(data) > archive_limit or "sha256:" + hashlib.sha256(data).hexdigest() != expected:
                raise ValueError("preview artifact archive identity")
            break
        finally:
            connection.close()
    else:
        raise ValueError("preview artifact redirect limit")
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        entries = archive.infolist()
        if (len(entries) != 1 or entries[0].filename != name or entries[0].flag_bits & 1
                or not stat.S_ISREG(entries[0].external_attr >> 16)
                or entries[0].compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED)
                or not 20 <= entries[0].file_size <= limit):
            raise ValueError("preview artifact ZIP entry")
        payload = archive.read(entries[0])
    if (len(payload) != entries[0].file_size or payload[:7] != b"\x7fELF\x02\x01\x01"
            or payload[18:20] != b"\x3e\x00"):
        raise ValueError("preview artifact ELF")
    path = Path(output_path)
    descriptor = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o500)
    try:
        with os.fdopen(descriptor, "wb") as output:
            os.fchmod(output.fileno(), 0o500)
            output.write(payload)
    except BaseException:
        path.unlink(missing_ok=True)
        raise
    return {"target": "x86_64-unknown-linux-gnu", "name": name, "size": len(payload), "sha256": hashlib.sha256(payload).hexdigest()}
