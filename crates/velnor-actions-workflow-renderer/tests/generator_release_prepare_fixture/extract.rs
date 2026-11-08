// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::{Engine, engine::general_purpose::STANDARD};
use std::error::Error;

type PrepareScript = (String, String, Vec<String>, Vec<String>);

pub(super) fn extract_prepare_script(workflow: &str) -> Result<PrepareScript, Box<dyn Error>> {
    let step = workflow
        .split("name: Reconcile immutable family release")
        .nth(1)
        .ok_or("generator reconciliation step is missing")?;
    let encoded = step
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("run: "))
        .ok_or("generator reconciliation run command is missing")?;
    let executor: String = serde_json::from_str(encoded)?;
    let prefix = workflow
        .split("name: Reconcile immutable family release")
        .next()
        .ok_or("missing materializers")?;
    let group = prefix
        .rsplit("name: Materialize fixed script")
        .next()
        .ok_or("missing materializer")?;
    let mut bytes = Vec::new();
    let mut materializers = Vec::new();
    for line in group
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("run: "))
    {
        let run: String = serde_json::from_str(line)?;
        if let Some((_, tail)) = run.split_once("validate=True))' ") {
            let encoded = tail
                .strip_prefix('\'')
                .and_then(|rest| rest.split_once('\''))
                .map(|(chunk, _)| chunk)
                .ok_or("invalid chunk")?;
            bytes.extend(STANDARD.decode(encoded)?);
        }
        materializers.push(run);
    }
    let script = String::from_utf8(bytes)?;
    let expected_line = script
        .lines()
        .find(|line| line.starts_with("readonly prepare_expected_assets='"))
        .ok_or("generator release asset list is missing")?;
    let encoded = expected_line
        .strip_prefix("readonly prepare_expected_assets='")
        .and_then(|value| value.strip_suffix('\''))
        .ok_or("generator release asset list is malformed")?;
    let asset_names = serde_json::from_str(encoded)?;
    Ok((script, executor, materializers, asset_names))
}

pub(super) fn check_project_tool_binding(
    workflow: &str,
    script: &str,
) -> Result<(), Box<dyn Error>> {
    let prepare = workflow
        .split("\n  prepare-generator:\n")
        .nth(1)
        .ok_or("prepare job missing")?
        .split("\n  prepare-")
        .next()
        .ok_or("prepare job missing")?;
    let resolver = prepare
        .find("name: Resolve project tool versions")
        .ok_or("tool resolver missing")?;
    let materializer = prepare
        .find("name: Materialize fixed script")
        .ok_or("materializer missing")?;
    let execution = prepare
        .find("name: Reconcile immutable family release")
        .ok_or("executor missing")?;
    assert!(resolver < materializer && materializer < execution);
    assert!(script.contains("\"${VELNOR_TOOL_RUST}\""));
    assert!(script.contains("\"${VELNOR_TOOL_MR_BOXINGTON}\""));
    Ok(())
}
