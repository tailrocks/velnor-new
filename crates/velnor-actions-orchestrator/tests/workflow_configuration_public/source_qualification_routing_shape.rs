// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::source_qualification::{WORKFLOW_PATH, generator_repo};
use super::source_qualification_jobs::{job_ids, rendered_workflow};
use super::support::rendered_job_block;
use std::error::Error;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

#[test]
fn source_qualification_keeps_triggers_permissions_and_concurrency() -> Result<(), Box<dyn Error>> {
    let workflow = rendered_workflow()?;
    assert!(workflow.contains(
        "\"on\":\n  pull_request:\n    types:\n      - opened\n      - edited\n      - synchronize\n      - reopened\n      - ready_for_review\n  workflow_dispatch: {}\npermissions:\n  contents: read\ncache-mode: read\nconcurrency:\n  group: source-qualification-${{ github.ref }}\n  cancel-in-progress: false\njobs:\n"
    ));
    let ids = job_ids(&workflow);
    assert_eq!(workflow.matches("concurrency:").count(), 1);
    for id in &ids {
        let block = rendered_job_block(&workflow, id)?;
        assert!(
            block.contains("\n    timeout-minutes: "),
            "job {id} has no timeout"
        );
        let parsed: serde_json::Value = serde_saphyr::from_str(&workflow)?;
        let permission_value = &parsed["jobs"][id]["permissions"];
        let permissions = if permission_value.is_null() {
            parsed["permissions"].as_object()
        } else {
            permission_value.as_object()
        }
        .ok_or("missing effective permissions")?;
        assert_eq!(
            permissions.get("contents"),
            Some(&serde_json::json!("read")),
            "job {id} must read checkout"
        );
        assert!(
            permissions
                .values()
                .all(|level| level == "read" || level == "none"),
            "job {id} grants a write permission"
        );
    }
    Ok(())
}

#[test]
fn source_qualification_pr_work_uses_the_canonical_plan_and_dispatch_stays_full()
-> Result<(), Box<dyn Error>> {
    let repo = generator_repo(true)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci: serde_json::Value =
        serde_saphyr::from_str(tree.get(".github/workflows/ci.yml").ok_or("missing CI")?)?;
    let qualification: serde_json::Value =
        serde_saphyr::from_str(tree.get(WORKFLOW_PATH).ok_or("missing qualification")?)?;
    let ci_plan = &ci["jobs"]["plan"];
    let plan = &qualification["jobs"]["plan"];
    let outputs = plan["outputs"].as_object().ok_or("plan outputs")?;
    assert_eq!(outputs.len(), 4);
    for name in [
        "source_plan_sha256",
        "source_matrix_sha256",
        "source_binary_sha256",
        "plan_attempt",
    ] {
        assert!(outputs.contains_key(name), "missing custody output {name}");
    }
    let steps = plan["steps"]
        .as_array()
        .ok_or("missing cloned plan steps")?;
    let hash = steps
        .iter()
        .position(|step| step["name"] == "Bind qualified plan artifact")
        .ok_or("missing plan hash")?;
    assert_eq!(
        steps.get(hash + 1).ok_or("missing artifact upload")?["name"],
        "Publish plan"
    );
    let original: Vec<_> = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| (index != hash).then_some(step.clone()))
        .collect();
    assert_eq!(serde_json::Value::Array(original), ci_plan["steps"]);
    assert!(
        plan["if"]
            .as_str()
            .is_some_and(|condition| condition.contains("pull_request"))
    );
    for id in ["ordinary-harness", "keep-harness", "generation-checks"] {
        let job = &qualification["jobs"][id];
        assert!(
            job["needs"]
                .as_array()
                .is_some_and(|needs| needs.iter().any(|need| need == "selection")),
            "{id}"
        );
        assert!(
            job["if"]
                .as_str()
                .is_some_and(|condition| condition.contains("selection")),
            "{id}"
        );
    }
    let gather = &qualification["jobs"]["source-qualification"];
    assert!(
        gather["if"]
            .as_str()
            .is_some_and(|condition| condition.contains("always()"))
    );
    assert!(qualification["on"]["workflow_dispatch"].is_object());
    Ok(())
}

#[test]
fn source_qualification_stays_bound_to_each_preparations_checkout_and_plan()
-> Result<(), Box<dyn Error>> {
    for (excluded, other) in [("docs", "notes"), ("notes", "docs")] {
        let (repo, _) = super::source_qualification_selection::fixture()?;
        let config = repo.path().join(".velnor/config.toml");
        let original = std::fs::read_to_string(&config)?;
        std::fs::write(
            config,
            original.replace("docs/**", &format!("{excluded}/**")),
        )?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let ci: serde_json::Value =
            serde_saphyr::from_str(tree.get(".github/workflows/ci.yml").ok_or("CI")?)?;
        let qualification: serde_json::Value =
            serde_saphyr::from_str(tree.get(WORKFLOW_PATH).ok_or("qualification")?)?;
        for id in ["ordinary-harness", "keep-harness", "generation-checks"] {
            let steps = qualification["jobs"][id]["steps"]
                .as_array()
                .ok_or("steps")?;
            let checkout = steps
                .iter()
                .find(|step| step["name"] == "Checkout exact event source")
                .ok_or("checkout")?;
            let sparse = checkout["with"]["sparse-checkout"]
                .as_str()
                .ok_or("sparse checkout")?;
            assert!(
                sparse.contains(&format!("!/{excluded}/**")),
                "{id}: {sparse}"
            );
            assert!(!sparse.contains(&format!("!/{other}/**")), "{id}: {sparse}");
        }
        let ci_steps = ci["jobs"]["plan"]["steps"].as_array().ok_or("CI plan")?;
        let source_steps = qualification["jobs"]["plan"]["steps"]
            .as_array()
            .ok_or("source plan")?;
        let original: Vec<_> = source_steps
            .iter()
            .filter(|step| step["name"] != "Bind qualified plan artifact")
            .cloned()
            .collect();
        assert_eq!(&original, ci_steps);
        let selection = qualification["jobs"]["selection"]["steps"]
            .as_array()
            .ok_or("selection")?;
        let route = selection
            .iter()
            .find(|step| step["name"] == "Admit source qualification selection")
            .ok_or("route")?;
        assert_eq!(route["env"]["CLI_ROUTES"], "[\"rust-velnor-actions-cli\"]");
    }
    Ok(())
}
