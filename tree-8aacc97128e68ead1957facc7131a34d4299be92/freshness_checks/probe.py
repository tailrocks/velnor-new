import gzip
import io
import json
import re
import urllib.request

from report import fail_row, info_row, norm_version, pass_row


FETCH_TIMEOUT = 10
FETCH_ENCODED_CAP = 512 * 1024
FETCH_DECOMPRESSED_CAP = 512 * 1024
FETCH_CHUNK_SIZE = 64 * 1024


def response_encoding(response):
    values = response.headers.get_all("Content-Encoding", [])
    encodings = [part.strip().lower() for value in values
                 for part in value.split(",")]
    if not encodings:
        return "identity"
    if len(encodings) != 1 or encodings[0] not in ("identity", "gzip"):
        raise ValueError(f"unsupported Content-Encoding {encodings!r}")
    return encodings[0]


def read_bounded(response, cap, label):
    body = bytearray()
    while True:
        remaining = cap + 1 - len(body)
        chunk = response.read(min(FETCH_CHUNK_SIZE, remaining))
        if not chunk:
            return bytes(body)
        body.extend(chunk)
        if len(body) > cap:
            raise ValueError(f"{label} response exceeds {cap} bytes")


def decode_gzip(encoded):
    output = bytearray()
    with gzip.GzipFile(fileobj=io.BytesIO(encoded), mode="rb") as stream:
        while True:
            remaining = FETCH_DECOMPRESSED_CAP + 1 - len(output)
            chunk = stream.read(min(FETCH_CHUNK_SIZE, remaining))
            if not chunk:
                return bytes(output)
            output.extend(chunk)
            if len(output) > FETCH_DECOMPRESSED_CAP:
                raise ValueError(
                    f"decompressed response exceeds "
                    f"{FETCH_DECOMPRESSED_CAP} bytes")


def fetch_text(url):
    request = urllib.request.Request(
        url, headers={"User-Agent": "velnor-freshness-probe",
                      "Accept": "application/json",
                      "Accept-Encoding": "gzip, identity"})
    with urllib.request.urlopen(request, timeout=FETCH_TIMEOUT) as response:
        encoding = response_encoding(response)
        encoded = read_bounded(response, FETCH_ENCODED_CAP, "encoded")
    if encoding == "gzip":
        body = decode_gzip(encoded)
    else:
        body = encoded
        if len(body) > FETCH_DECOMPRESSED_CAP:
            raise ValueError(
                f"decompressed response exceeds "
                f"{FETCH_DECOMPRESSED_CAP} bytes")
    return body.decode("utf-8", errors="replace")


def github_tag(payload):
    if isinstance(payload, dict) and payload.get("tag_name"):
        return payload["tag_name"]
    if isinstance(payload, list):
        for release in payload:
            if not isinstance(release, dict) or release.get("draft") \
                    or release.get("prerelease"):
                continue
            tag = release.get("tag_name") or release.get("name")
            if tag:
                return tag
    return None


def sniff_latest(source, body):
    try:
        payload = json.loads(body)
    except ValueError:
        payload = None
    if payload is not None:
        if "crates.io/api/v1/crates/" in source \
                and isinstance(payload, dict):
            crate = payload.get("crate") or {}
            return crate.get("max_version")
        tag = github_tag(payload)
        if tag:
            return tag
        crate = (payload.get("crate") or {}) if isinstance(payload, dict) \
            else {}
        if crate.get("max_version"):
            return crate["max_version"]
    match = re.search(r"\[pkg\.rust\]\s*\nversion\s*=\s*\""
                      r"(\d+\.\d+\.\d+)", body)
    if not match:
        match = re.search(r'version\s*=\s*"(\d+\.\d+\.\d+)', body)
    return match.group(1) if match else None


def run_upstream_probe(check_upstream, tools, action_pinned, now):
    if not check_upstream:
        return
    stamp = now.strftime("%Y-%m-%dT%H:%M:%SZ")
    for tool in tools:
        probe_tool(tool, stamp)
    for key, action in sorted(action_pinned.items()):
        probe_action(key, action, stamp)
    info_row("upstream-probe", "runner",
             "latest image family is platform-qualification evidence, "
             "not an API probe")


def probe_tool(tool, stamp):
    name = tool.get("name")
    source = tool.get("source", "")
    try:
        latest = sniff_latest(source, fetch_text(source))
    except Exception as err:  # noqa: BLE001 - probe maps all to failed
        fail_row("upstream-probe", name,
                 f"lookup_failed ({err}); source {source}, checked {stamp}")
        return
    record_probe_result(name, source, tool.get("pinned"), latest, stamp)


def probe_action(key, action, stamp):
    source = action.get("source", "")
    try:
        latest = sniff_latest(source, fetch_text(source))
    except Exception as err:  # noqa: BLE001 - probe maps all to failed
        fail_row("upstream-probe", key,
                 f"lookup_failed ({err}); source {source}, checked {stamp}")
        return
    record_probe_result(key, source, action.get("pinned_version"), latest, stamp)


def record_probe_result(subject, source, pinned, latest, stamp):
    if latest is None:
        fail_row("upstream-probe", subject,
                 f"lookup_failed: no stable release parsed; source "
                 f"{source}, checked {stamp}")
    elif norm_version(latest) != norm_version(pinned):
        fail_row("upstream-probe", subject,
                 f"stale pin: pinned={pinned!r} latest={latest!r}; "
                 f"source {source}, checked {stamp}")
    else:
        pass_row("upstream-probe", subject,
                 f"pinned==latest {latest}; source {source}, checked {stamp}")
