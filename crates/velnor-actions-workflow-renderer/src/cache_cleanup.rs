//! Generated `.github/workflows/cache-cleanup.yml` (cache decision 5).
//!
//! PR cache writes from the same repository use only `refs/pull/<n>/merge`.
//! When the PR closes, cleanup lists and deletes only that exact ref.
//! The job checks out no repository code.

use velnor_actions_contract::{RunsOn, config::ValidatedImageReference};

use crate::RenderError;
use crate::cache_rest_client;
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

const CLIENT_MAIN: &str = r#"def main():
    ref = os.environ.get("PR_REF", "")
    endpoint, token, opener, deadline = client_context(ref, True)
    entries = list_caches(endpoint, ref, token, opener, deadline, False)
    deleted = 0
    failed = 0
    for entry in entries:
        status = delete_cache(endpoint, entry["id"], token, opener, deadline)
        if status == 204:
            deleted += 1
        else:
            failed += 1
            print("::warning::cache_unavailable: The cleanup client could not delete cache entry " + str(entry["id"]) + ".")
    print("cache_cleanup: listed=" + str(len(entries)) + " deleted=" + str(deleted)
          + " delete_failed=" + str(failed))
    if failed:
        fail("cache_unavailable")


if __name__ == "__main__":
    main()
"#;

pub(crate) fn client_script() -> String {
    cache_rest_client::script(CLIENT_MAIN)
}

/// Render cleanup for the route selected by the generator.
///
/// A `ScaleSet` route may select an immutable job container image.
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
        (RunsOn::ScaleSet(selector), image) => (
            format!("[{}]", selector.labels().join(", ")),
            image.map_or_else(String::new, |image| {
                format!("    container:\n      image: {}\n", image.as_str())
            }),
        ),
        (RunsOn::Hosted(_), Some(_)) => {
            return Err(RenderError::InvalidWorkflow(
                "cache_cleanup_hosted_container_unexpected".to_owned(),
            ));
        }
    };
    let mut script = String::new();
    let client = client_script();
    for line in client.lines() {
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

    use super::render_cache_cleanup_yml;

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
        assert_eq!(
            run,
            format!("python3 - <<'PY'\n{}PY\n", super::client_script())
        );
    }
}
