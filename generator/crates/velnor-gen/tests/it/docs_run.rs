// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const POST_MERGE: &str = "(github.event_name == 'push' || github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')";

fn documentation_conditions(config: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    repo.files
        .insert(path(".velnor/rust-ci.toml")?, config.as_bytes().to_vec());
    let yaml: serde_json::Value = crate::yaml_tree::document(&workflow(&repo)?)?;
    let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
    let conditions: Vec<String> = jobs
        .values()
        .flat_map(|job| job["steps"].as_array().into_iter().flatten())
        .filter(|step| step["name"] == "Documentation")
        .map(|step| step["if"].as_str().unwrap_or_default().to_owned())
        .collect();
    Ok(conditions)
}

#[test]
fn documentation_runs_on_every_event_unless_configured() -> TestResult {
    for config in ["", "docs = \"always\"\n"] {
        let conditions = documentation_conditions(config)?;
        assert!(!conditions.is_empty());
        assert!(conditions.iter().all(|c| !c.contains("github.event_name")));
    }
    Ok(())
}

#[test]
fn post_merge_documentation_skips_pull_requests_and_merge_groups() -> TestResult {
    let conditions = documentation_conditions("docs = \"post_merge\"\n")?;
    assert!(!conditions.is_empty());
    for condition in &conditions {
        assert!(condition.contains(POST_MERGE), "{condition}");
        assert!(condition.contains("!cancelled()"), "{condition}");
    }
    Ok(())
}

#[test]
fn an_unknown_docs_mode_is_refused() -> TestResult {
    assert!(documentation_conditions("docs = \"never\"\n").is_err());
    Ok(())
}
