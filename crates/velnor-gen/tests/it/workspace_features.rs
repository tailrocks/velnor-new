// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_rust::{CONFIG_PATH, DetectError, Reason, detect};

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};
use crate::yaml_tree::document;

const FEATURES: &str = "workspace_features = ['local-mode']\n";

fn steps_of(config: &str) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    let jobs = document(&workflow(&repo)?)?["jobs"].clone();
    Ok(jobs
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(_, job)| job["steps"].as_array().cloned().unwrap_or_default())
        .collect())
}

fn running(steps: &[serde_json::Value], tool: &str) -> Vec<String> {
    steps
        .iter()
        .filter_map(|step| step["run"].as_str())
        .filter(|run| run.contains(tool))
        .map(str::to_owned)
        .collect()
}

#[test]
fn workspace_features_apply_to_the_main_lint_doc_and_test_and_no_second_lane_exists() -> TestResult
{
    let steps = steps_of(FEATURES)?;
    for tool in [
        "cargo-clippy clippy",
        "cargo doc",
        "cargo-nextest nextest run",
    ] {
        let found = running(&steps, tool);
        assert_eq!(found.len(), 1, "{tool}: {found:?}");
        assert!(found[0].contains(" --features 'local-mode'"), "{found:?}");
    }
    let named: Vec<_> = steps
        .iter()
        .filter_map(|step| step["name"].as_str())
        .filter(|name| name.contains("(features:"))
        .collect();
    assert!(named.is_empty(), "{named:?}");
    let plain = steps_of("")?;
    for tool in [
        "cargo-clippy clippy",
        "cargo doc",
        "cargo-nextest nextest run",
    ] {
        let found = running(&plain, tool);
        assert!(
            found.iter().all(|run| !run.contains("--features")),
            "{found:?}"
        );
    }
    Ok(())
}

#[test]
fn an_unknown_or_invalid_workspace_feature_name_is_refused() -> TestResult {
    for names in ["['a b']", "['']", "['a', 'a']", "['a,b']", "['$x']"] {
        let config = format!("workspace_features = {names}\n");
        let repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
        match detect(&repo) {
            Err(DetectError::Invalid {
                path,
                reason: Reason::InvalidValue { key, .. },
            }) => {
                assert_eq!((path, key), (CONFIG_PATH, "workspace_features"), "{names}");
            }
            other => return Err(format!("{names}: {other:?}").into()),
        }
    }
    Ok(())
}
