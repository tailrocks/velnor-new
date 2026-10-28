// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_actions_contract::{Job, PermissionLevel, REQUIRED_JOB_ID, StepKind};
use velnor_actions_orchestrator::{
    GenerationPreparation, finalized_jobs, prepare, render_staged_tree,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::native_artifacts_fixture::{
    build_task_artifact, build_task_config, repo_with_build_inputs, repo_with_build_task,
};
use super::support::rendered_job_block;

const TASK_JOB_ID: &str = "task-native-proof";

#[test]
fn public_build_task_artifacts_bind_outcomes_and_join_required() -> Result<(), Box<dyn Error>> {
    let mut declarations = vec![
        build_task_artifact("success", &["artifacts/success.json"], "success", 7),
        build_task_artifact("failure", &["artifacts/failure.json"], "failure", 7),
        build_task_artifact("hidden", &["artifacts/.native-evidence.json"], "always", 7),
    ];
    declarations.extend((3..500).map(|index| {
        build_task_artifact(
            &format!("native-{index:04}"),
            &[&format!("artifacts/evidence-{index:04}.json")],
            "always",
            7,
        )
    }));
    let artifact_config = declarations.join("\n");
    let (_repo, prepared) = repo_with_build_task(Some(&artifact_config), "")?;
    let task = prepared
        .workflow
        .ir
        .jobs
        .get(TASK_JOB_ID)
        .ok_or("build task job is missing from generated IR")?;
    assert_build_task_authority(task)?;
    assert_build_task_uploads(task)?;

    let jobs = finalized_jobs(&prepared)?;
    let required = jobs.get(REQUIRED_JOB_ID).ok_or("Required job is missing")?;
    assert!(required.needs.iter().any(|job| job == TASK_JOB_ID));

    assert_build_task_yaml(&prepared)?;
    Ok(())
}

fn assert_build_task_authority(task: &Job) -> Result<(), Box<dyn Error>> {
    assert_eq!(task.runs_on, "macos-26");
    assert_eq!(task.needs, Vec::<String>::new());
    let permissions = task
        .permissions
        .as_ref()
        .ok_or("build task permissions are missing")?;
    assert_eq!(permissions.contents, PermissionLevel::Read);
    assert_eq!(permissions.id_token, PermissionLevel::None);
    Ok(())
}

fn assert_build_task_uploads(task: &Job) -> Result<(), Box<dyn Error>> {
    let uploads = task
        .steps
        .iter()
        .filter(|step| step.name == "Upload build-task artifact")
        .collect::<Vec<_>>();
    assert_eq!(uploads.len(), 500);
    let conditions = uploads
        .iter()
        .filter_map(|step| step.condition.as_deref())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        conditions,
        BTreeSet::from([
            "always()",
            "always() && steps.build-task-run.outcome == 'failure'",
            "always() && steps.build-task-run.outcome == 'success'",
        ])
    );
    let mut names = BTreeSet::new();
    let mut hidden_path = false;
    for step in uploads {
        let StepKind::Action { with, .. } = &step.kind else {
            return Err("build task upload is not an action step".into());
        };
        names.insert(
            with.get("name")
                .ok_or("build task upload name is missing")?
                .as_str(),
        );
        assert_eq!(
            with.get("if-no-files-found").map(String::as_str),
            Some("error")
        );
        assert_eq!(with.get("retention-days").map(String::as_str), Some("7"));
        if with.get("path").map(String::as_str) == Some("artifacts/.native-evidence.json") {
            hidden_path = true;
            assert_eq!(
                with.get("include-hidden-files").map(String::as_str),
                Some("true")
            );
        }
    }
    assert_eq!(names.len(), 500);
    assert!(hidden_path);
    Ok(())
}

fn assert_build_task_yaml(prepared: &GenerationPreparation) -> Result<(), Box<dyn Error>> {
    let tree = render_staged_tree(prepared)?;
    let workflow = tree.get(WORKFLOW_PATH).ok_or("generated CI is missing")?;
    let block = rendered_job_block(workflow, TASK_JOB_ID)?;
    assert!(block.contains("id: build-task-run"));
    assert!(block.contains("path: artifacts/.native-evidence.json"));
    assert!(block.contains("include-hidden-files: \"true\""));
    Ok(())
}

#[test]
fn omitted_and_empty_build_task_artifacts_emit_no_uploads() -> Result<(), Box<dyn Error>> {
    for artifacts in [None, Some("")] {
        let (_repo, prepared) = repo_with_build_task(artifacts, "")?;
        let task = prepared
            .workflow
            .ir
            .jobs
            .get(TASK_JOB_ID)
            .ok_or("build task job is missing from generated IR")?;
        assert!(
            task.steps
                .iter()
                .all(|step| step.name != "Upload build-task artifact")
        );
        let tree = render_staged_tree(&prepared)?;
        let workflow = tree.get(WORKFLOW_PATH).ok_or("generated CI is missing")?;
        let block = rendered_job_block(workflow, TASK_JOB_ID)?;
        assert!(!block.contains("Upload build-task artifact"));
    }
    Ok(())
}

#[test]
fn public_build_task_artifacts_reject_invalid_declarations() -> Result<(), Box<dyn Error>> {
    let duplicate = [
        build_task_artifact("duplicate", &["artifacts/one.json"], "always", 7),
        build_task_artifact("duplicate", &["artifacts/two.json"], "always", 7),
    ]
    .join("\n");
    let cases = [
        (duplicate, "duplicate_build_task_artifact:duplicate"),
        (
            build_task_artifact("native-{runner}", &["artifacts/evidence.json"], "always", 7),
            "build_task_artifact_name_must_be_literal",
        ),
        (
            build_task_artifact(
                "multiple-paths",
                &["artifacts/one.json", "artifacts/two.json"],
                "always",
                7,
            ),
            "build_task_artifact_requires_one_path_pattern",
        ),
        (
            build_task_artifact("dollar", &["artifacts/cost$report.json"], "always", 7),
            "build_task_artifact_path_contains_dollar",
        ),
        (
            build_task_artifact("retention", &["artifacts/evidence.json"], "always", 91),
            "bad_artifact_retention_days:91",
        ),
    ];
    let mut unexpected = Vec::new();
    for (artifacts, expected) in cases {
        let config = build_task_config(Some(&artifacts), "");
        let repo = repo_with_build_inputs(&config)?;
        match prepare(repo.path()) {
            Err(error) if error.to_string().contains(expected) => {}
            Err(error) => unexpected.push(format!("expected {expected}; found {error}")),
            Ok(_) => unexpected.push(format!("invalid artifact config accepted: {expected}")),
        }
    }

    let artifacts = (0..501)
        .map(|index| {
            build_task_artifact(
                &format!("native-{index:04}"),
                &[&format!("artifacts/evidence-{index:04}.json")],
                "always",
                7,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let config = build_task_config(Some(&artifacts), "");
    let repo = repo_with_build_inputs(&config)?;
    match prepare(repo.path()) {
        Err(error)
            if error
                .to_string()
                .contains("build_task_artifact_count_exceeds_job_limit") => {}
        Err(error) => unexpected.push(format!(
            "expected build_task_artifact_count_exceeds_job_limit; found {error}"
        )),
        Ok(_) => unexpected.push("501 build task artifacts were accepted".to_owned()),
    }
    if !unexpected.is_empty() {
        return Err(unexpected.join("\n").into());
    }
    Ok(())
}

#[test]
fn strict_public_render_rejects_tampered_build_task_artifacts_and_missing_policy()
-> Result<(), Box<dyn Error>> {
    for tamper in ["path", "predicate", "hidden"] {
        let artifact = build_task_artifact(
            "native-evidence",
            &["artifacts/.native-evidence.json"],
            "success",
            7,
        );
        let (_repo, mut prepared) = repo_with_build_task(Some(&artifact), "")?;
        let step = build_task_upload_mut(&mut prepared)?;
        match &mut step.kind {
            StepKind::Action { with, .. } => match tamper {
                "path" => {
                    with.insert("path".to_owned(), "artifacts/other.json".to_owned());
                }
                "predicate" => step.condition = Some("success()".to_owned()),
                "hidden" => {
                    with.insert("include-hidden-files".to_owned(), "false".to_owned());
                }
                _ => return Err("unknown artifact tamper".into()),
            },
            _ => return Err("build task upload is not an action step".into()),
        }
        assert_strict_render_rejects(&prepared, "build_task_job_contract:task-native-proof")?;
    }

    let artifact = build_task_artifact(
        "native-evidence",
        &["artifacts/.native-evidence.json"],
        "success",
        7,
    );
    let (_repo, mut prepared) = repo_with_build_task(Some(&artifact), "")?;
    prepared.workflow.context.workflow_tasks.clear();
    assert_strict_render_rejects(&prepared, "native_authority_graph")?;
    Ok(())
}

#[test]
fn mixed_native_build_and_mbx_suite_preserve_suite_artifact_behavior() -> Result<(), Box<dyn Error>>
{
    let native_artifact = build_task_artifact(
        "native-evidence",
        &["artifacts/.native-evidence.json"],
        "always",
        7,
    );
    let suites = r#"[[stacks.rust.suites]]
id = "proof-suite"
profile = "default"
triggers = ["pull_request", "push"]
required = true
package = "demo"
manifest_path = "Cargo.toml"
artifacts = [{ name = "suite-evidence", paths = ["artifacts/suite-evidence.json"], when = "failure", retention_days = 7 }]
"#;
    let (_repo, prepared) = repo_with_build_task(Some(&native_artifact), suites)?;
    let finalized = finalized_jobs(&prepared)?;
    let required = finalized
        .get(REQUIRED_JOB_ID)
        .ok_or("Required job is missing")?;
    assert!(required.needs.iter().any(|job| job == TASK_JOB_ID));
    assert!(required.needs.iter().any(|job| job == "suite-proof-suite"));
    let tree = render_staged_tree(&prepared)?;
    let workflow = tree.get(WORKFLOW_PATH).ok_or("generated CI is missing")?;
    let native_block = rendered_job_block(workflow, TASK_JOB_ID)?;
    assert!(native_block.contains("include-hidden-files: \"true\""));
    let suite_block = rendered_job_block(workflow, "suite-proof-suite")?;
    assert!(suite_block.contains("if: failure()"));
    assert!(suite_block.contains("if-no-files-found: warn"));
    assert!(!suite_block.contains("include-hidden-files"));
    Ok(())
}

fn build_task_upload_mut(
    prepared: &mut GenerationPreparation,
) -> Result<&mut velnor_actions_contract::Step, Box<dyn Error>> {
    prepared
        .workflow
        .ir
        .jobs
        .get_mut(TASK_JOB_ID)
        .ok_or("build task job is missing from generated IR")?
        .steps
        .iter_mut()
        .find(|step| step.name == "Upload build-task artifact")
        .ok_or_else(|| "build task upload is missing from generated IR".into())
}

fn assert_strict_render_rejects(
    prepared: &GenerationPreparation,
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    let error = match render_staged_tree(prepared) {
        Err(error) => error.to_string(),
        Ok(_) => return Err(format!("strict render accepted tamper: {expected}").into()),
    };
    assert!(
        error.contains(expected),
        "expected {expected}; found {error}"
    );
    Ok(())
}
