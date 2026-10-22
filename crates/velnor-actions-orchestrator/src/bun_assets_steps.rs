//! Steps of a member's assets job and of the hand-off every suite job runs
//! instead of the App-token block (HD-7).
//!
//! The assets job indexes the tree the hardened `private-fetch-v1` verified
//! and extracted (`.velnor/deps`, with `verified.json`) file by file and uploads
//! it. A suite job downloads it and checks, before any install or script:
//! every file against the index, no file outside it, and every archive digest
//! `.velnor/deps.lock` pins (rendered literally here) in `verified.json`. The
//! artifact lives only in this run, so a fork cannot plant one.

use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_workflow_renderer::shell_step;
use velnor_actions_workflow_renderer::verification_jobs::{
    BUN_ASSETS_DIR, BUN_ASSETS_INDEX, bun_assets_download_step, bun_assets_upload_step,
};

use super::bun_steps::bash;
use crate::OrchestratorError;

/// Env of the hand-off check: the lock's `id:sha256` pins, space-separated.
const PINS_ENV: &str = "VELNOR_ASSET_PINS";

/// After the fetch and revoke: index the verified tree, then upload it.
pub(crate) fn assets_tail(member_job_id: &str) -> Result<Vec<Step>, OrchestratorError> {
    let body = format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE/{BUN_ASSETS_DIR}\"; test -f verified.json; \
find . -type f ! -name {BUN_ASSETS_INDEX} -print0 | LC_ALL=C sort -z | xargs -0 sha256sum > {BUN_ASSETS_INDEX}; \
test -s {BUN_ASSETS_INDEX}"
    );
    Ok(vec![
        shell_step("Index private assets", bash(&body), BTreeMap::new())?,
        bun_assets_upload_step(member_job_id)?,
    ])
}

/// What a suite job runs where a member job runs the App-token block.
pub(crate) fn suite_handoff(
    member_job_id: &str,
    pins: &[String],
) -> Result<Vec<Step>, OrchestratorError> {
    let body = format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE/{BUN_ASSETS_DIR}\"; test -f verified.json; \
sha256sum --check --quiet --strict {BUN_ASSETS_INDEX} || {{ echo 'velnor: private_asset_digest_mismatch' >&2; exit 1; }}; \
t=\"$RUNNER_TEMP/velnor\"; mkdir -p \"$t\"; cut -c67- {BUN_ASSETS_INDEX} | LC_ALL=C sort > \"$t/assets-listed\"; \
find . -type f ! -name {BUN_ASSETS_INDEX} | LC_ALL=C sort > \"$t/assets-actual\"; \
cmp -s \"$t/assets-listed\" \"$t/assets-actual\" || {{ echo 'velnor: private_asset_unlisted_file' >&2; exit 1; }}; \
for pin in ${PINS_ENV}; do grep -qF \"\\\"sha256\\\": \\\"${{pin#*:}}\\\"\" verified.json || {{ echo \"velnor: private_asset_pin_missing:${{pin%%:*}}\" >&2; exit 1; }}; done"
    );
    let env = BTreeMap::from([(PINS_ENV.to_owned(), pins.join(" "))]);
    Ok(vec![
        bun_assets_download_step(member_job_id)?,
        shell_step("Verify private assets", bash(&body), env)?,
    ])
}
