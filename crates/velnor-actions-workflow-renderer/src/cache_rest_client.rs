//! Bounded GitHub cache REST client embedded in generated housekeeping jobs.

pub(crate) const SUPPORT: &str = r#"import json
import os
import re
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlencode, urlsplit, urlunsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

PAGE_SIZE = 100
MAX_PAGES = 64
MAX_CACHES = PAGE_SIZE * MAX_PAGES
MAX_PAGE_BYTES = 1024 * 1024
MAX_TOTAL_BYTES = 16 * 1024 * 1024
REQUEST_TIMEOUT = 15
DEADLINE_SECONDS = 210


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        return None


def fail(code):
    print("cache_housekeeping: " + code, file=sys.stderr)
    raise SystemExit(1)


def request(opener, method, url, token, deadline):
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        fail("deadline_exceeded")
    req = Request(url, method=method, headers={
        "Accept": "application/vnd.github+json",
        "Authorization": "Bearer " + token,
        "User-Agent": "velnor-cache-housekeeping",
        "X-GitHub-Api-Version": "2026-03-10",
    })
    try:
        with opener.open(req, timeout=min(REQUEST_TIMEOUT, remaining)) as response:
            status = response.status
            if method == "DELETE":
                return status, b""
            data = response.read(MAX_PAGE_BYTES + 1)
    except HTTPError as error:
        return error.code, b""
    except (URLError, TimeoutError, OSError):
        return 0, b""
    if len(data) > MAX_PAGE_BYTES:
        return 0, b""
    return status, data


def client_context(ref, pull_request):
    repo = os.environ.get("GH_REPO", "")
    api = os.environ.get("GH_API_URL", "")
    token = os.environ.get("GH_TOKEN", "")
    if not re.fullmatch(r"[A-Za-z0-9.-]+/[A-Za-z0-9_.-]+", repo):
        fail("invalid_repository")
    valid_ref = (re.fullmatch(r"refs/pull/[1-9][0-9]*/merge", ref) if pull_request
                 else re.fullmatch(r"refs/heads/[A-Za-z0-9._/-]+", ref))
    if not valid_ref:
        fail("invalid_ref")
    if not token:
        fail("missing_token")
    parts = urlsplit(api)
    local_test_origin = parts.hostname in ("127.0.0.1", "::1")
    if ((parts.scheme != "https" and not (parts.scheme == "http" and local_test_origin))
            or not parts.hostname or parts.username or parts.password
            or parts.query or parts.fragment):
        fail("invalid_api_origin")
    base = urlunsplit((parts.scheme, parts.netloc, parts.path.rstrip("/"), "", ""))
    endpoint = base + "/repos/" + quote(repo, safe="/") + "/actions/caches"
    return endpoint, token, build_opener(NoRedirect()), time.monotonic() + DEADLINE_SECONDS


def list_caches(endpoint, ref, token, opener, deadline, require_keys):
    entries = []
    seen = set()
    total_bytes = 0
    expected = None
    page = 1
    while page <= MAX_PAGES:
        query = urlencode({"ref": ref, "per_page": PAGE_SIZE, "page": page})
        status, body = request(opener, "GET", endpoint + "?" + query, token, deadline)
        if status != 200:
            fail("cache_list_failed")
        total_bytes += len(body)
        if total_bytes > MAX_TOTAL_BYTES:
            fail("response_budget_exceeded")
        try:
            payload = json.loads(body)
        except (UnicodeDecodeError, json.JSONDecodeError):
            fail("invalid_cache_list")
        if not isinstance(payload, dict):
            fail("invalid_cache_list")
        count = payload.get("total_count")
        records = payload.get("actions_caches")
        if type(count) is not int or not 0 <= count <= MAX_CACHES or not isinstance(records, list):
            fail("invalid_cache_list")
        if expected is None:
            expected = count
        elif count != expected:
            fail("cache_list_changed")
        if len(records) > PAGE_SIZE:
            fail("invalid_cache_page")
        for entry in records:
            if not isinstance(entry, dict) or entry.get("ref") != ref:
                fail("cache_ref_mismatch")
            cache_id = entry.get("id")
            if type(cache_id) is not int or cache_id <= 0 or cache_id in seen:
                fail("invalid_cache_id")
            seen.add(cache_id)
            if require_keys:
                key = entry.get("key")
                created_at = entry.get("created_at")
                if not isinstance(key, str) or not key or len(key.encode("utf-8")) > 512:
                    fail("invalid_cache_key")
                if not isinstance(created_at, str):
                    fail("invalid_cache_timestamp")
                entries.append({"id": cache_id, "key": key, "created_at": created_at})
            else:
                entries.append({"id": cache_id})
        if len(entries) > expected:
            fail("cache_count_mismatch")
        if len(entries) == expected:
            break
        if not records:
            fail("cache_list_incomplete")
        page += 1
    else:
        fail("page_limit_exceeded")
    if expected is None or len(entries) != expected:
        fail("cache_list_incomplete")
    return entries


def delete_cache(endpoint, cache_id, token, opener, deadline):
    return request(opener, "DELETE", endpoint + "/" + str(cache_id), token, deadline)[0]
"#;

pub(crate) fn script(main: &str) -> String {
    let mut script = String::with_capacity(SUPPORT.len() + main.len() + 1);
    script.push_str(SUPPORT);
    script.push('\n');
    script.push_str(main);
    script
}
