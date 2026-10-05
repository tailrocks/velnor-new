//! Generated permissions stay scoped to the artifact-consuming job.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::ir::{Job, StepKind};
use velnor_actions_contract::workflow::permissions::PermissionLevel;
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};

use crate::impl_common::{TestResult, config_with_branch, make_repo};

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

fn permissions_for_job(lines: &[&str]) -> BTreeMap<String, BTreeMap<String, String>> {
    let jobs_at = lines
        .iter()
        .position(|line| line.trim() == "jobs:")
        .expect("rendered workflow has jobs");
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
            let job = current_job.as_ref().expect("permissions belong to a job");
            jobs.insert(job.clone(), child_mapping(lines, index, indent));
        }
    }
    jobs
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
fn generated_workflow_scopes_actions_read_to_required() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let jobs = finalized_jobs(&prep)?;
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

    for (id, job) in &jobs {
        let effective = job.permissions.as_ref().unwrap_or(&prep.workflow.ir.permissions);
        let expected = if id == "required" {
            PermissionLevel::Read
        } else {
            PermissionLevel::None
        };
        assert_eq!(effective.actions, expected, "effective actions scope for {id}");
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

    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(".github/workflows/ci.yml")
        .ok_or("missing generated CI workflow")?;
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

    let rendered_job_permissions = permissions_for_job(&lines);
    let required = rendered_job_permissions
        .get("required")
        .ok_or("missing rendered Required permissions")?;
    assert_eq!(required.get("contents").map(String::as_str), Some("read"));
    assert_eq!(required.get("actions").map(String::as_str), Some("read"));
    for (id, permissions) in &rendered_job_permissions {
        if id != "required" {
            assert!(!permissions.contains_key("actions"), "actions scope on {id}");
        }
    }
    Ok(())
}
