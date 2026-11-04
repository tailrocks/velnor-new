// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::BTreeMap;
use velnor_actions_contract::{Step, StepRole};

use crate::{RenderError, ambient_shell_step, marker};

pub fn acquire_step(
    version: &str,
    runner: &str,
    source_sha: &str,
    manifest_sha256: &str,
) -> Result<Step, RenderError> {
    marker::validate_version(version)?;
    if !velnor_actions_contract::ids::is_lower_hex_len(source_sha, 40)
        || !velnor_actions_contract::ids::is_lower_hex_len(manifest_sha256, 64)
        || runner != crate::release_tree::RELEASE_HOSTED_RUNNER
        || !semver::Version::parse(version).is_ok_and(|parsed| {
            parsed.pre.is_empty() && parsed.build.is_empty() && parsed.to_string() == version
        })
    {
        return Err(RenderError::InvalidWorkflow(
            "unqualified_generator_bootstrap".to_owned(),
        ));
    }
    let encoded = STANDARD.encode(include_bytes!("release_generator_bootstrap.sh"));
    let command =
        format!("set -o pipefail; printf %s {encoded} | /usr/bin/base64 --decode | /bin/bash");
    let mut step = ambient_shell_step(
        "Acquire Velnor",
        vec!["bash".to_owned(), "-c".to_owned(), command],
        BTreeMap::from([
            ("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned()),
            ("VELNOR_GENERATOR_VERSION".to_owned(), version.to_owned()),
            (
                "VELNOR_GENERATOR_SOURCE_SHA".to_owned(),
                source_sha.to_owned(),
            ),
            (
                "VELNOR_GENERATOR_MANIFEST_SHA256".to_owned(),
                manifest_sha256.to_owned(),
            ),
        ]),
    )?;
    step.role = Some(StepRole::AcquireVelnor);
    Ok(step)
}
