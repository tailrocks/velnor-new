// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ProductReleasePins, workflow_steps};
use crate::{RenderError, yaml::Yaml};

pub(in super::super) const PATH: &str = "${{ runner.temp }}/stable-source-metadata.json";

pub(in super::super) fn step(
    pins: &ProductReleasePins,
    artifact_id: &str,
) -> Result<Yaml, RenderError> {
    let run = format!(
        "set -euo pipefail\n{}\n{}",
        workflow_steps::gh_function(&pins.gh_argv)?,
        r#"rm -f -- "$SOURCES_METADATA" "$SOURCES_METADATA.partial"
trap 'rm -f -- "$SOURCES_METADATA.partial"' EXIT
[[ "$SOURCES_ID" =~ ^[1-9][0-9]*$ ]]
gh api --hostname github.com --method GET "repos/tailrocks/velnor-new/actions/artifacts/$SOURCES_ID" | head -c 1048577 > "$SOURCES_METADATA.partial"
test "$(wc -c < "$SOURCES_METADATA.partial")" -le 1048576
mv -- "$SOURCES_METADATA.partial" "$SOURCES_METADATA""#
    );
    Ok(workflow_steps::bash_step_with_env(
        "Fetch exact source artifact metadata",
        &run,
        vec![
            ("GH_TOKEN", "${{ github.token }}"),
            ("SOURCES_ID", artifact_id),
            ("SOURCES_METADATA", PATH),
        ],
    ))
}
