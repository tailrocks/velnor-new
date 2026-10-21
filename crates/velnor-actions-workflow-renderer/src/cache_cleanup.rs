//! Generated `.github/workflows/cache-cleanup.yml` (cache decision 5).
//!
//! PR cache writes from the same repository use only `refs/pull/<n>/merge`.
//! When the PR closes, cleanup lists and deletes only that exact ref.
//! The job checks out no repository code.

use velnor_actions_contract::{RunsOn, config::ValidatedImageReference};

use crate::RenderError;
use crate::marker::with_marker;
use crate::render::RenderedFile;

/// Path of the generated workflow.
pub const CACHE_CLEANUP_PATH: &str = ".github/workflows/cache-cleanup.yml";

const BODY: &str = r#"name: Cache cleanup
"on":
  pull_request:
    types:
      - closed
permissions: {}
concurrency:
  group: velnor-cache-cleanup-${{ github.event.pull_request.number }}
  cancel-in-progress: false
jobs:
  cleanup:
    name: Delete closed pull request cache entries
    if: github.event.pull_request.head.repo.fork == false
    runs-on: @RUNS_ON@
@CONTAINER@    timeout-minutes: 5
    permissions:
      actions: write
    steps:
      - name: Delete the pull request's cache entries
        env:
          GH_TOKEN: ${{ github.token }}
          GH_REPO: ${{ github.repository }}
          GH_API_URL: ${{ github.api_url }}
          PR_REF: refs/pull/${{ github.event.pull_request.number }}/merge
        run: |
          python3 - <<'PY'
@SCRIPT@          PY
"#;

const CLIENT: &str = r#"import json
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
    print("cache_cleanup: " + code, file=sys.stderr)
    raise SystemExit(1)


def request(opener, method, url, token, deadline):
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        fail("deadline_exceeded")
    req = Request(url, method=method, headers={
        "Accept": "application/vnd.github+json",
        "Authorization": "Bearer " + token,
        "User-Agent": "velnor-cache-cleanup",
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


def main(deadline_seconds=DEADLINE_SECONDS):
    repo = os.environ.get("GH_REPO", "")
    ref = os.environ.get("PR_REF", "")
    api = os.environ.get("GH_API_URL", "")
    token = os.environ.get("GH_TOKEN", "")
    if not re.fullmatch(r"[A-Za-z0-9.-]+/[A-Za-z0-9_.-]+", repo):
        fail("invalid_repository")
    if not re.fullmatch(r"refs/pull/[1-9][0-9]*/merge", ref):
        fail("invalid_pull_request_ref")
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
    opener = build_opener(NoRedirect())
    deadline = time.monotonic() + deadline_seconds
    ids = []
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
        entries = payload.get("actions_caches")
        if type(count) is not int or not 0 <= count <= MAX_CACHES or not isinstance(entries, list):
            fail("invalid_cache_list")
        if expected is None:
            expected = count
        elif count != expected:
            fail("cache_list_changed")
        if len(entries) > PAGE_SIZE:
            fail("invalid_cache_page")
        for entry in entries:
            if not isinstance(entry, dict) or entry.get("ref") != ref:
                fail("cache_ref_mismatch")
            cache_id = entry.get("id")
            if type(cache_id) is not int or cache_id <= 0 or cache_id in seen:
                fail("invalid_cache_id")
            seen.add(cache_id)
            ids.append(cache_id)
        if len(ids) > expected:
            fail("cache_count_mismatch")
        if len(ids) == expected:
            break
        if not entries:
            fail("cache_list_incomplete")
        page += 1
    else:
        fail("page_limit_exceeded")
    if expected is None or len(ids) != expected:
        fail("cache_list_incomplete")

    deleted = 0
    failed = 0
    for cache_id in ids:
        status, _ = request(opener, "DELETE", endpoint + "/" + str(cache_id), token, deadline)
        if status == 204:
            deleted += 1
        else:
            failed += 1
            print("::warning::cache_unavailable: The cleanup client could not delete cache entry " + str(cache_id) + ".")
    print("cache_cleanup: listed=" + str(expected) + " deleted=" + str(deleted)
          + " delete_failed=" + str(failed))
    if failed:
        fail("cache_unavailable")


if __name__ == "__main__":
    main()
"#;

/// Render cleanup for the route selected by the generator.
///
/// A `ScaleSet` route requires the selected immutable job container image.
/// A hosted route uses the selected hosted runner label and rejects a container.
///
/// # Errors
///
/// This function returns [`RenderError`] if the runner, container pairing, or version is invalid.
pub fn render_cache_cleanup_yml(
    runs_on: &str,
    image: Option<&ValidatedImageReference>,
    version: &str,
) -> Result<RenderedFile, RenderError> {
    let runner = RunsOn::parse(runs_on).map_err(RenderError::Contract)?;
    let (label, container) = match (runner, image) {
        (RunsOn::Hosted(label), None) => (label, String::new()),
        (RunsOn::ScaleSet(selector), Some(image)) => (
            format!("[{}]", selector.labels().join(", ")),
            format!("    container:\n      image: {}\n", image.as_str()),
        ),
        (RunsOn::ScaleSet(_), None) => {
            return Err(RenderError::InvalidWorkflow(
                "cache_cleanup_scale_set_container_missing".to_owned(),
            ));
        }
        (RunsOn::Hosted(_), Some(_)) => {
            return Err(RenderError::InvalidWorkflow(
                "cache_cleanup_hosted_container_unexpected".to_owned(),
            ));
        }
    };
    let mut script = String::new();
    for line in CLIENT.lines() {
        if !line.is_empty() {
            script.push_str("          ");
        }
        script.push_str(line);
        script.push('\n');
    }
    let content = BODY
        .replace("@RUNS_ON@", &label)
        .replace("@CONTAINER@", &container)
        .replace("@SCRIPT@", &script);
    Ok(RenderedFile {
        path: CACHE_CLEANUP_PATH.to_owned(),
        bytes: with_marker(version, &content)?,
    })
}

#[cfg(test)]
#[path = "cache_cleanup_tests.rs"]
mod tests;

#[cfg(test)]
mod rendering_tests {
    use saphyr::LoadableYamlNode;

    use super::{CLIENT, render_cache_cleanup_yml};

    #[test]
    fn blank_script_lines_have_no_indentation_and_content_is_preserved() {
        let rendered = render_cache_cleanup_yml("ubuntu-26.04", None, "0.1.37")
            .expect("The cleanup workflow must render.");
        assert!(
            rendered
                .bytes
                .lines()
                .all(|line| { line.is_empty() || !line.trim().is_empty() })
        );

        let document = saphyr::Yaml::load_from_str(&rendered.bytes)
            .expect("The rendered cleanup workflow must parse.");
        let run = document[0]["jobs"]["cleanup"]["steps"][0]["run"]
            .as_str()
            .expect("The cleanup step must contain a script.");
        assert_eq!(run, format!("python3 - <<'PY'\n{CLIENT}PY\n"));
    }
}
