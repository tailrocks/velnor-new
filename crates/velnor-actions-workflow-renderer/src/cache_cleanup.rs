// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::RunsOn;

use crate::RenderError;
use crate::marker::with_marker;
use crate::render::RenderedFile;
use crate::yaml::quote_scalar;

pub const CACHE_CLEANUP_PATH: &str = ".github/workflows/cache-cleanup.yml";
pub const CACHE_CLEANUP_FALLBACK_RUNNER: &str = "ubuntu-24.04";

const BODY: &str = r#"name: Cache cleanup
"on":
  pull_request:
    types:
      - closed
permissions: {}
cache-mode: read
concurrency:
  group: velnor-cache-cleanup-${{ github.event.pull_request.number }}
  cancel-in-progress: false
jobs:
  cleanup:
    name: Delete closed pull request cache entries
    if: github.event.pull_request.head.repo.fork == false
    runs-on: @RUNS_ON@
    timeout-minutes: 5
    permissions:
      actions: write
    steps:
      - name: Delete the pull request's cache entries
        env:
          GH_TOKEN: ${{ github.token }}
          GH_REPO: ${{ github.repository }}
          PR_REF: refs/pull/${{ github.event.pull_request.number }}/merge
        run: @SCRIPT@
"#;

pub fn render_cache_cleanup_yml(runs_on: &str, version: &str) -> Result<RenderedFile, RenderError> {
    RunsOn::parse(runs_on).map_err(RenderError::Contract)?;
    let label = CACHE_CLEANUP_FALLBACK_RUNNER;
    let script = format!(
        "set -eu\n{}\ngh() {{ {} \"$@\"; }}\ngh cache list --ref \"$PR_REF\" --limit 1000 --json id --jq '.[].id' > \"$RUNNER_TEMP/cache-ids\"; while read -r id; do if gh cache delete \"$id\"; then printf 'velnor: deleted cache entry %s of %s\\n' \"$id\" \"$PR_REF\"; else printf '::warning::cache_unavailable: could not delete cache entry %s\\n' \"$id\"; fi; done < \"$RUNNER_TEMP/cache-ids\"",
        crate::verified_gh_shell::bootstrap(),
        crate::verified_gh_shell::command()
    );
    Ok(RenderedFile {
        path: CACHE_CLEANUP_PATH.to_owned(),
        bytes: with_marker(
            version,
            &BODY
                .replace("@RUNS_ON@", label)
                .replace("@SCRIPT@", &quote_scalar(&script)),
        )?,
    })
}
