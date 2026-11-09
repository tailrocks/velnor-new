// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use std::error::Error;

use super::{find_file, rendered};

#[test]
fn preview_generator_target_environment_mappings_are_valid() -> Result<(), Box<dyn Error>> {
    let files = rendered()?;
    let workflow = find_file(&files, ".github/workflows/product-release-generator.yml")?;
    let yaml: Value = serde_saphyr::from_str(workflow)?;
    let jobs = yaml["jobs"]
        .as_object()
        .ok_or("workflow jobs are missing")?;
    let darwin = jobs
        .get("source-preview-qualify-darwin")
        .and_then(Value::as_object)
        .ok_or("Darwin preview qualifier is missing")?;
    let darwin_steps = darwin
        .get("steps")
        .and_then(Value::as_array)
        .ok_or("Darwin preview qualifier steps are missing")?;
    for step in darwin_steps {
        if let Some(env) = step.get("env") {
            let env = env.as_object().ok_or("Darwin step env is not a mapping")?;
            assert!(!env.is_empty(), "Darwin step env mapping is empty");
        }
    }

    let linux = jobs
        .get("source-preview-qualify-linux")
        .and_then(Value::as_object)
        .ok_or("Linux preview qualifier is missing")?;
    let linux_steps = linux
        .get("steps")
        .and_then(Value::as_array)
        .ok_or("Linux preview qualifier steps are missing")?;
    let qualify = linux_steps
        .iter()
        .find(|step| step["name"] == "Qualify exact source preview bytes")
        .ok_or("Linux qualification step is missing")?;
    let env = qualify
        .get("env")
        .and_then(Value::as_object)
        .ok_or("Linux qualification step env is missing")?;
    assert_eq!(
        env.get("SOURCES_CACHE_HIT").and_then(Value::as_str),
        Some("${{ steps.sources-restore.outputs.cache-hit }}")
    );
    assert_eq!(
        env.get("SOURCES_CACHE_MATCHED_KEY").and_then(Value::as_str),
        Some("${{ steps.sources-restore.outputs.cache-matched-key }}")
    );
    Ok(())
}
