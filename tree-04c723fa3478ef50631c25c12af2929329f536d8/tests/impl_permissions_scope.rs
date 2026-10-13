//! Generated permissions stay scoped to the artifact-consuming job.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::workflow::StepKind;
use velnor_actions_contract_workflow::workflow::ir::Job;
use velnor_actions_contract_workflow::workflow::permissions::PermissionLevel;
use velnor_actions_orchestrator::{
    GenerationPreparation, finalized_jobs, prepare, render_staged_tree,
};

use crate::impl_common::{
    TestResult, config_with_branch, git, make_repo, without_ambient_identity,
};

fn indentation(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn child_mapping(
    lines: &[&str],
    header_at: usize,
    header_indent: usize,
) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for line in &lines[header_at + 1..] {
        let indent = indentation(line);
        if !line.trim().is_empty() && indent <= header_indent {
            break;
        }
        if indent != header_indent + 2 {
            continue;
        }
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        values.insert(key.to_owned(), value.trim().to_owned());
    }
    values
}

fn permissions_for_job(
    lines: &[&str],
) -> Result<BTreeMap<String, BTreeMap<String, String>>, Box<dyn std::error::Error>> {
    let jobs_at = lines
        .iter()
        .position(|line| line.trim() == "jobs:")
        .ok_or("rendered workflow has no jobs")?;
    let mut jobs = BTreeMap::new();
    let mut current_job: Option<String> = None;
    for (index, line) in lines.iter().enumerate().skip(jobs_at + 1) {
        let indent = indentation(line);
        let trimmed = line.trim();
        if indent == 0 {
            break;
        }
        if indent == 2 && trimmed.ends_with(':') {
            current_job = Some(trimmed.trim_end_matches(':').to_owned());
        } else if indent == 4 && trimmed == "permissions:" {
            let job = current_job
                .as_ref()
                .ok_or("rendered permissions do not belong to a job")?;
            jobs.insert(job.clone(), child_mapping(lines, index, indent));
        }
    }
    Ok(jobs)
}

fn token_binding_steps(lines: &[&str], key: &str, value: &str) -> Vec<(String, String)> {
    let mut bindings = Vec::new();
    let mut job = String::new();
    let mut step = String::new();
    for line in lines {
        let indent = indentation(line);
        let trimmed = line.trim();
        if indent == 2 && trimmed.ends_with(':') {
            trimmed.trim_end_matches(':').clone_into(&mut job);
            step.clear();
        } else if indent == 6 && trimmed.starts_with("- name: ") {
            trimmed.trim_start_matches("- name: ").clone_into(&mut step);
        } else if indent == 10 && trimmed == format!("{key}: {value}") {
            bindings.push((job.clone(), step.clone()));
        }
    }
    bindings
}

fn has_checkout(job: &Job) -> bool {
    job.steps.iter().any(|step| {
        matches!(
            &step.kind,
            StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@")
        )
    })
}

#[test]
fn consumer_workflow_scopes_actions_read_to_required() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let jobs = finalized_jobs(&prep)?;
    assert_ir_permissions(&prep, &jobs, false)?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(".github/workflows/ci.yml")
        .ok_or("missing generated CI workflow")?;
    assert_rendered_permissions(yaml, false)
}

#[test]
fn repository_workflow_scopes_actions_read_to_plan_and_required() -> TestResult {
    without_ambient_identity(
        "repository_workflow_scopes_actions_read_to_plan_and_required",
        || {
            let config = "schema = 1\n[workflow]\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n";
            let repo = make_repo(config)?;
            git(
                &[
                    "remote",
                    "add",
                    "origin",
                    "https://github.com/tailrocks/velnor-new.git",
                ],
                repo.path(),
            )?;
            let prep = prepare(repo.path())?;
            let jobs = finalized_jobs(&prep)?;
            assert_ir_permissions(&prep, &jobs, true)?;
            let tree = render_staged_tree(&prep)?;
            let yaml = tree
                .get(".github/workflows/ci.yml")
                .ok_or("missing generated CI workflow")?;
            assert_rendered_permissions(yaml, true)
        },
    )
}

fn assert_ir_permissions(
    prep: &GenerationPreparation,
    jobs: &BTreeMap<String, Job>,
    plan_auth: bool,
) -> TestResult {
    let plan = jobs.get("plan").ok_or("missing Plan job")?;
    let plan_permissions = plan
        .permissions
        .as_ref()
        .unwrap_or(&prep.workflow.ir.permissions);
    assert_eq!(plan_permissions.contents, PermissionLevel::Read);
    assert_eq!(
        plan_permissions.actions,
        if plan_auth {
            PermissionLevel::Read
        } else {
            PermissionLevel::None
        }
    );
    let required = jobs.get("required").ok_or("missing Required job")?;
    let required_permissions = required
        .permissions
        .as_ref()
        .ok_or("Required permissions are implicit")?;
    assert_eq!(required_permissions.contents, PermissionLevel::Read);
    assert_eq!(required_permissions.actions, PermissionLevel::Read);
    assert!(
        required
            .steps
            .iter()
            .any(|step| step.name == "Download every expected matrix artifact"),
        "Required receives Actions read for its artifact download"
    );

    for (id, job) in jobs {
        let effective = job
            .permissions
            .as_ref()
            .unwrap_or(&prep.workflow.ir.permissions);
        let expected = if id == "required" || (id == "plan" && plan_auth) {
            PermissionLevel::Read
        } else {
            PermissionLevel::None
        };
        assert_eq!(
            effective.actions, expected,
            "effective actions scope for {id}"
        );
    }
    assert_eq!(prep.workflow.ir.permissions.contents, PermissionLevel::Read);
    assert_eq!(prep.workflow.ir.permissions.actions, PermissionLevel::None);
    for id in ["plan", "actionlint"] {
        let job = jobs.get(id).ok_or("missing checkout job")?;
        assert!(has_checkout(job), "{id} keeps the checkout action");
        assert_eq!(
            job.permissions
                .as_ref()
                .unwrap_or(&prep.workflow.ir.permissions)
                .contents,
            PermissionLevel::Read,
            "{id} inherits contents read for checkout"
        );
    }

    Ok(())
}

fn assert_rendered_permissions(yaml: &str, plan_auth: bool) -> TestResult {
    let lines = yaml.lines().collect::<Vec<_>>();
    let workflow_permissions_at = lines
        .iter()
        .position(|line| indentation(line) == 0 && line.trim() == "permissions:")
        .ok_or("missing workflow permissions")?;
    let workflow_permissions = child_mapping(&lines, workflow_permissions_at, 0);
    assert_eq!(
        workflow_permissions.get("contents").map(String::as_str),
        Some("read")
    );
    assert!(!workflow_permissions.contains_key("actions"));

    let rendered_job_permissions = permissions_for_job(&lines)?;
    let required = rendered_job_permissions
        .get("required")
        .ok_or("missing rendered Required permissions")?;
    assert_eq!(required.get("contents").map(String::as_str), Some("read"));
    if plan_auth {
        let plan = rendered_job_permissions
            .get("plan")
            .ok_or("missing rendered Plan permissions")?;
        assert_eq!(plan.get("contents").map(String::as_str), Some("read"));
        assert_eq!(plan.get("actions").map(String::as_str), Some("read"));
    } else {
        assert!(!rendered_job_permissions.contains_key("plan"));
    }
    assert_eq!(required.get("actions").map(String::as_str), Some("read"));
    for (id, permissions) in &rendered_job_permissions {
        if id != "plan" && id != "required" {
            assert!(
                !permissions.contains_key("actions"),
                "actions scope on {id}"
            );
        }
    }
    let expected_required = (
        "required".to_owned(),
        "Download every expected matrix artifact".to_owned(),
    );
    assert_eq!(
        token_binding_steps(&lines, "GH_TOKEN", "${{ github.token }}"),
        if plan_auth {
            vec![
                ("plan".to_owned(), "Plan".to_owned()),
                expected_required.clone(),
            ]
        } else {
            vec![expected_required.clone()]
        }
    );
    assert_eq!(
        token_binding_steps(&lines, "GH_REPO", "${{ github.repository }}"),
        if plan_auth {
            vec![("plan".to_owned(), "Plan".to_owned()), expected_required]
        } else {
            vec![expected_required]
        }
    );
    Ok(())
}
