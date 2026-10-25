//! Generated push-only cache pruning workflow (cache decision 4).

use velnor_actions_contract::config::ValidatedImageReference;
use velnor_actions_contract::{RunsOn, workflow::cache_family::CacheFamily};

use crate::RenderError;
use crate::cache_rest_client;
use crate::marker::with_marker;
use crate::render::RenderedFile;

/// Generated workflow path.
pub const CACHE_PRUNE_PATH: &str = ".github/workflows/cache-prune.yml";

/// Trusted family prefixes and their retained-entry limits.
#[must_use]
pub fn prune_targets() -> Vec<(String, u64)> {
    CacheFamily::ALL
        .into_iter()
        .map(|family| {
            (
                format!("{}trusted-", family.key_prefix()),
                family.spec().retained_entries,
            )
        })
        .collect()
}

/// Generate the bounded REST client that prunes each family independently.
#[must_use]
pub fn prune_script() -> String {
    let targets = prune_targets()
        .into_iter()
        .map(|(prefix, retained)| format!(r#"{{"prefix":"{prefix}","retained":{retained}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    cache_rest_client::script(&PRUNE_MAIN.replace("@TARGETS@", &targets))
}

const PRUNE_MAIN: &str = r#"import datetime

TARGETS = @TARGETS@


def main():
    ref = os.environ.get("GITHUB_REF", "")
    expected_ref = os.environ.get("EXPECTED_REF", "")
    if ref != expected_ref:
        fail("unexpected_ref")
    endpoint, token, opener, deadline = client_context(ref, False)
    try:
        entries = list_caches(endpoint, ref, token, opener, deadline, True)
    except SystemExit:
        print("::warning::cache_unavailable: The cache client could not list all cache entries for the ref.")
        return

    groups = {}
    for entry in entries:
        key = entry["key"]
        target = next((item for item in TARGETS if key.startswith(item["prefix"])), None)
        if target is None:
            continue
        match = re.search(r"-s256-([0-9a-f]{64})$", key)
        if match is None:
            fail("invalid_trusted_cache_key")
        try:
            created = datetime.datetime.fromisoformat(entry["created_at"].replace("Z", "+00:00"))
        except ValueError:
            fail("invalid_cache_timestamp")
        if created.tzinfo is None:
            fail("invalid_cache_timestamp")
        unit = key[:match.start()]
        groups.setdefault((target["prefix"], unit), []).append((created, entry["id"]))

    doomed = []
    retained_by_prefix = {item["prefix"]: item["retained"] for item in TARGETS}
    for (prefix, _unit), candidates in groups.items():
        candidates.sort(reverse=True)
        doomed.extend(cache_id for _created, cache_id in candidates[retained_by_prefix[prefix]:])

    deleted = 0
    failed = 0
    for cache_id in sorted(doomed):
        if delete_cache(endpoint, cache_id, token, opener, deadline) == 204:
            deleted += 1
        else:
            failed += 1
            print("::warning::cache_unavailable: The prune client could not delete cache entry " + str(cache_id) + ".")
    print("cache_prune: listed=" + str(len(entries)) + " deleted=" + str(deleted)
          + " delete_failed=" + str(failed))


if __name__ == "__main__":
    main()
"#;

const BODY: &str = r#"name: Cache prune
"on":
  push:
    branches:
      - @DEFAULT_BRANCH@
permissions: {}
concurrency:
  group: velnor-cache-prune
  cancel-in-progress: false
jobs:
  prune:
    name: Prune superseded cache entries
    if: github.ref_protected == true
    runs-on: @RUNS_ON@
@CONTAINER@    timeout-minutes: 5
    permissions:
      actions: write
    steps:
      - name: Prune superseded cache entries
        env:
          GH_TOKEN: ${{ github.token }}
          GH_REPO: ${{ github.repository }}
          GH_API_URL: ${{ github.api_url }}
          EXPECTED_REF: refs/heads/@DEFAULT_BRANCH@
        run: |
          python3 - <<'PY'
@SCRIPT@          PY
"#;

/// Render pruning for the configured runner route and protected default branch.
///
/// # Errors
/// Returns an error when the runner/container pairing, branch, or marker is invalid.
pub fn render_cache_prune_yml(
    runs_on: &str,
    image: Option<&ValidatedImageReference>,
    default_branch: &str,
    version: &str,
) -> Result<RenderedFile, RenderError> {
    let runner = RunsOn::parse(runs_on).map_err(RenderError::Contract)?;
    let (label, container) = match (runner, image) {
        (RunsOn::Hosted(label), None) => (label, String::new()),
        (RunsOn::ScaleSet(selector), image) => (
            format!("[{}]", selector.labels().join(", ")),
            image.map_or_else(String::new, |image| {
                format!("    container:\n      image: {}\n", image.as_str())
            }),
        ),
        (RunsOn::Hosted(_), Some(_)) => {
            return Err(RenderError::InvalidWorkflow(
                "cache_prune_hosted_container_unexpected".to_owned(),
            ));
        }
    };
    let safe = !default_branch.is_empty()
        && default_branch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'));
    if !safe {
        return Err(RenderError::InvalidWorkflow(
            "cache_prune_default_branch".to_owned(),
        ));
    }
    let mut script = String::new();
    for line in prune_script().lines() {
        if !line.is_empty() {
            script.push_str("          ");
        }
        script.push_str(line);
        script.push('\n');
    }
    let content = BODY
        .replace("@DEFAULT_BRANCH@", default_branch)
        .replace("@RUNS_ON@", &label)
        .replace("@CONTAINER@", &container)
        .replace("@SCRIPT@", &script);
    Ok(RenderedFile {
        path: CACHE_PRUNE_PATH.to_owned(),
        bytes: with_marker(version, &content)?,
    })
}

#[cfg(test)]
#[path = "cache_prune_tests.rs"]
mod tests;
