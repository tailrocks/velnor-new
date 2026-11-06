// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::ffi::OsStr;
use std::path::Path;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::maintenance_features::owner_feature_repo;

const SELF_REPOSITORY_IGNORE: &str = "# zizmor: ignore[self-repository]";

fn structured_uses(workflow: &serde_json::Value) -> Vec<String> {
    let mut uses = Vec::new();
    if let Some(jobs) = workflow.get("jobs").and_then(serde_json::Value::as_object) {
        for job in jobs.values() {
            if let Some(value) = job.get("uses").and_then(serde_json::Value::as_str) {
                uses.push(value.to_owned());
            }
            if let Some(steps) = job.get("steps").and_then(serde_json::Value::as_array) {
                uses.extend(steps.iter().filter_map(|step| {
                    step.get("uses")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                }));
            }
        }
    }
    if let Some(steps) = workflow
        .get("runs")
        .and_then(|runs| runs.get("steps"))
        .and_then(serde_json::Value::as_array)
    {
        uses.extend(steps.iter().filter_map(|step| {
            step.get("uses")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        }));
    }
    uses
}

fn raw_uses_lines(bytes: &str) -> Vec<(String, &str)> {
    bytes
        .lines()
        .filter_map(|line| {
            let line = line
                .trim_start()
                .strip_prefix("- ")
                .unwrap_or(line.trim_start());
            let value = line.strip_prefix("uses:")?.trim_start();
            let value = value.split('#').next().unwrap_or(value).trim();
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .or_else(|| {
                    value
                        .strip_prefix('\'')
                        .and_then(|value| value.strip_suffix('\''))
                })
                .unwrap_or(value);
            Some((value.to_owned(), line))
        })
        .collect()
}

#[test]
fn generated_actions_annotate_local_actions_only_per_policy() -> Result<(), Box<dyn Error>> {
    for policy in [
        WorkflowPolicy::ConsumerV1,
        WorkflowPolicy::VelnorRepositoryV1,
    ] {
        let repo = owner_feature_repo(policy, false, "default", "default")?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let mut expected_uses = Vec::new();
        let mut actual_uses = Vec::new();
        for file in &tree.files {
            if Path::new(&file.path).extension() != Some(OsStr::new("yml")) {
                continue;
            }
            let workflow: serde_json::Value = serde_saphyr::from_str(&file.bytes)?;
            expected_uses.extend(structured_uses(&workflow));
            actual_uses.extend(raw_uses_lines(&file.bytes));
        }
        expected_uses.sort();
        let mut raw_values: Vec<_> = actual_uses.iter().map(|(value, _)| value.clone()).collect();
        raw_values.sort();
        assert_eq!(
            raw_values, expected_uses,
            "raw action references must match parsed generated workflow structure for {policy:?}"
        );

        let mut local_count = 0;
        let mut external_pinned_count = 0;
        let mut violations = Vec::new();
        for (uses, line) in actual_uses {
            if uses.starts_with("./") {
                local_count += 1;
                if !line.contains(SELF_REPOSITORY_IGNORE) {
                    violations.push(format!("unannotated local use for {policy:?}: {line}"));
                }
            } else if uses.starts_with("docker://") {
                if line.contains(SELF_REPOSITORY_IGNORE) {
                    violations.push(format!(
                        "container use annotated as local for {policy:?}: {line}"
                    ));
                }
            } else if uses.contains('/') {
                if uses.contains('@') {
                    external_pinned_count += 1;
                }
                if line.contains(SELF_REPOSITORY_IGNORE) {
                    violations.push(format!(
                        "external use annotated as local for {policy:?}: {line}"
                    ));
                }
            } else {
                violations.push(format!("unclassified action use for {policy:?}: {line}"));
            }
        }
        assert!(
            local_count > 0,
            "no local action uses generated for {policy:?}"
        );
        assert!(
            external_pinned_count > 0,
            "no external pinned action generated for {policy:?}"
        );
        assert!(
            violations.is_empty(),
            "generated action annotation violations:\n{}",
            violations.join("\n")
        );
    }
    Ok(())
}
