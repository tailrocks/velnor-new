"""Fixed crates.io bootstrap and native Trusted Publishing authentication."""
from contextlib import contextmanager
import http.client
import json
import os
from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit
import urllib.error
import urllib.request


TRUSTED_TOKEN_URL = "https://crates.io/api/v1/trusted_publishing/tokens"


class AuthRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, newurl):
        raise ReconcileError("publish_auth_redirect")


def _secret_request(request, statuses):
    opener = urllib.request.build_opener(AuthRedirect())
    try:
        with opener.open(request, timeout=30) as response:
            require(response.status in statuses, "publish_auth_status")
            data = response.read(1024 * 1024 + 1)
    except urllib.error.HTTPError as error:
        raise ReconcileError(f"publish_auth_http:{error.code}") from None
    except (urllib.error.URLError, TimeoutError, OSError, http.client.HTTPException):
        raise ReconcileError("publish_auth_transport") from None
    require(len(data) <= 1024 * 1024, "publish_auth_size")
    return decode_json(data) if data else {}


def _token_value(value):
    require(isinstance(value, str) and value and len(value) <= 16384 and
            all(32 < ord(character) < 127 for character in value), "publish_auth_token")
    return value


def _trusted_token():
    parsed = urlsplit(os.environ["ACTIONS_ID_TOKEN_REQUEST_URL"])
    require(parsed.scheme == "https" and parsed.hostname is not None and
            parsed.hostname.endswith(".actions.githubusercontent.com") and
            parsed.port in (None, 443) and parsed.username is None and
            parsed.password is None and not parsed.fragment and parsed.path,
            "publish_oidc_origin")
    query = parse_qsl(parsed.query, keep_blank_values=True)
    require(not any(key == "audience" for key, _ in query), "publish_oidc_audience_override")
    url = urlunsplit((parsed.scheme, parsed.netloc, parsed.path,
                     urlencode([*query, ("audience", "crates.io")]), ""))
    request_token = _token_value(os.environ["ACTIONS_ID_TOKEN_REQUEST_TOKEN"])
    oidc = _secret_request(urllib.request.Request(
        url, headers={"Authorization": "Bearer " + request_token,
                      "Accept": "application/json"}), (200,))
    require(isinstance(oidc, dict), "publish_oidc_response")
    jwt = _token_value(oidc.get("value"))
    exchange = _secret_request(urllib.request.Request(
        TRUSTED_TOKEN_URL, data=json.dumps({"jwt": jwt}).encode("utf-8"), method="POST",
        headers={"Content-Type": "application/json", "Accept": "application/json"}), (200,))
    require(isinstance(exchange, dict) and set(exchange) == {"token"},
            "publish_exchange_response")
    return _token_value(exchange["token"])


@contextmanager
def registry_token(approved, name):
    require(name in approved["packages"], "publish_token_scope")
    bootstrap = os.environ.get("CARGO_REGISTRY_TOKEN")
    oidc = [os.environ.get(key) for key in
            ("ACTIONS_ID_TOKEN_REQUEST_URL", "ACTIONS_ID_TOKEN_REQUEST_TOKEN")]
    if approved["authentication"] == "bootstrap-token":
        require(bootstrap and not any(oidc), "publish_auth_exclusive")
        yield _token_value(bootstrap)
        return
    require(approved["authentication"] == "trusted-publishing" and
            not bootstrap and all(oidc), "publish_auth_exclusive")
    token = _trusted_token()
    try:
        yield token
    finally:
        _secret_request(urllib.request.Request(
            TRUSTED_TOKEN_URL, method="DELETE",
            headers={"Authorization": "Bearer " + token}), (200, 204))
