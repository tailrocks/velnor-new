"""Fixed crates.io upload framing; no executable package inputs."""
import http.client
import json
import struct
import urllib.error
import urllib.request


PUBLISH_URL = "https://crates.io/api/v1/crates/new"
MAX_UPLOAD_RESPONSE = 2 * 1024 * 1024


class PublishRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, newurl):
        raise ReconcileError("publish_redirect")


def publish_body(metadata, archive):
    require(isinstance(metadata, dict) and type(archive) is bytes, "publish_input_types")
    serialized = json.dumps(metadata, ensure_ascii=False, separators=(",", ":"),
                            allow_nan=False).encode("utf-8")
    require(len(serialized) <= 0xffffffff and len(archive) <= 0xffffffff,
            "publish_frame_size")
    return struct.pack("<I", len(serialized)) + serialized + \
        struct.pack("<I", len(archive)) + archive


def upload_package(metadata, archive, token):
    require(isinstance(token, str) and token and len(token) <= 16384 and
            all(32 < ord(character) < 127 for character in token), "publish_token")
    request = urllib.request.Request(
        PUBLISH_URL, data=publish_body(metadata, archive), method="PUT",
        headers={"Authorization": token, "Content-Type": "application/octet-stream",
                 "Accept": "application/json", "User-Agent": "velnor-fixed-publisher/1"})
    opener = urllib.request.build_opener(PublishRedirect())
    try:
        with opener.open(request, timeout=90) as response:
            require(response.status in (200, 201), "publish_response_status")
            content = response.read(MAX_UPLOAD_RESPONSE + 1)
    except urllib.error.HTTPError as error:
        # Never retain server text: it can echo credential-bearing request data.
        raise ReconcileError(f"publish_http:{error.code}") from None
    except (urllib.error.URLError, TimeoutError, OSError, http.client.HTTPException):
        raise ReconcileError("publish_transport_uncertain") from None
    require(len(content) <= MAX_UPLOAD_RESPONSE, "publish_response_size")
    try:
        result = decode_json(content) if content else {}
    except (ValueError, UnicodeError):
        raise ReconcileError("publish_response_json") from None
    require(isinstance(result, dict) and not result.get("errors"), "publish_response_errors")
    warnings = result.get("warnings", {})
    require(isinstance(warnings, dict), "publish_warnings")
    return {"status": "submitted", "warnings": {
        key: warnings.get(key, []) for key in ("invalid_categories", "invalid_badges", "other")}}
