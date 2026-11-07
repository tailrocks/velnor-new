# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import os
import ssl
import sys
import urllib.error
import urllib.parse
import urllib.request

REGISTRY = "https://crates.io/api/v1/trusted_publishing/tokens"
MAX_RESPONSE = 65536
TIMEOUT = 15


class RejectRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def request(opener, url, method, headers, body=None):
    req = urllib.request.Request(url, data=body, headers=headers, method=method)
    with opener.open(req, timeout=TIMEOUT) as response:
        data = response.read(MAX_RESPONSE + 1)
        if len(data) > MAX_RESPONSE:
            raise ValueError("response too large")
        return data


def valid_request_url(value):
    parsed = urllib.parse.urlsplit(value)
    host = parsed.hostname or ""
    return (
        parsed.scheme == "https"
        and host.endswith(".actions.githubusercontent.com")
        and parsed.username is None
        and parsed.password is None
        and parsed.port in (None, 443)
        and "#" not in value
        and not any(key.casefold() == "audience" for key, _ in urllib.parse.parse_qsl(parsed.query))
    )


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate key")
        result[key] = value
    return result


def run_exchange(opener, oidc_url, oidc_bearer, registry_url):
    separator = "&" if urllib.parse.urlsplit(oidc_url).query else "?"
    response = request(
        opener,
        oidc_url + separator + "audience=crates.io",
        "GET",
        {"Authorization": "Bearer " + oidc_bearer},
    )
    jwt = json.loads(response, object_pairs_hook=unique_object)["value"]
    if not isinstance(jwt, str) or not jwt or len(jwt) > MAX_RESPONSE:
        raise ValueError("invalid identity")
    response = request(
        opener,
        registry_url,
        "POST",
        {"Content-Type": "application/json", "User-Agent": "velnor-oidc-qualification"},
        json.dumps({"jwt": jwt}, separators=(",", ":")).encode(),
    )
    token = json.loads(response, object_pairs_hook=unique_object)["token"]
    if not isinstance(token, str) or not token or len(token) > MAX_RESPONSE:
        raise ValueError("invalid credential")
    request(
        opener,
        registry_url,
        "DELETE",
        {"Authorization": "Bearer " + token, "User-Agent": "velnor-oidc-qualification"},
    )


def main():
    try:
        if "SSLKEYLOGFILE" in os.environ:
            raise ValueError("unsafe TLS environment")
        oidc_url = os.environ["ACTIONS_ID_TOKEN_REQUEST_URL"]
        oidc_bearer = os.environ["ACTIONS_ID_TOKEN_REQUEST_TOKEN"]
        if not valid_request_url(oidc_url) or not oidc_bearer or len(oidc_bearer) > MAX_RESPONSE:
            raise ValueError("invalid identity endpoint")
        context = ssl.create_default_context(cafile="/etc/ssl/certs/ca-certificates.crt")
        opener = urllib.request.build_opener(
            urllib.request.ProxyHandler({}), RejectRedirect(), urllib.request.HTTPSHandler(context=context)
        )
        run_exchange(opener, oidc_url, oidc_bearer, REGISTRY)
    except Exception:
        print("OIDC qualification failed", file=sys.stderr)
        return 1
    print("OIDC qualification succeeded")
    return 0


if __name__ == "__main__":
    sys.exit(main())
