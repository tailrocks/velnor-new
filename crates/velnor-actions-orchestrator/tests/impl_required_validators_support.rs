use std::fs;

use velnor_actions_contract::{
    FinalReport, FinalStatus, JobConclusion, Plan, needs_inventory_digest,
};
use velnor_actions_orchestrator::{GenerationPreparation, render_staged_tree};

use crate::impl_common::{make_repo, passing_reports};
use crate::impl_orch_core::{merge, merge_request};

/// Velnor-policy config: repository validators join the workflow.
pub(crate) fn velnor_config() -> String {
    "schema = 1\n[workflow]\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n"
        .to_owned()
}

/// Fixture accepted by the Velnor-repository identity check.
pub(crate) fn make_velnor_repo() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(&velnor_config())?;
    let git_config = repo.path().join(".git/config");
    let mut text = fs::read_to_string(&git_config)?;
    text.push_str("[remote \"origin\"]\n\turl = https://github.com/tailrocks/velnor-new.git\n");
    fs::write(git_config, text)?;
    Ok(repo)
}

/// Rendered `ci.yml` text for one preparation.
pub(crate) fn rendered_workflow(
    prep: &GenerationPreparation,
) -> Result<String, Box<dyn std::error::Error>> {
    let tree = render_staged_tree(prep)?;
    tree.get(".github/workflows/ci.yml")
        .map(str::to_owned)
        .ok_or_else(|| {
            Box::new(std::io::Error::other("missing workflow")) as Box<dyn std::error::Error>
        })
}

/// Complete Required inventory parsed from rendered YAML.
pub(crate) fn rendered_needs_inventory(
    yaml: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let line = yaml
        .lines()
        .find(|line| line.contains("VELNOR_NEEDS_EXPECTED:"))
        .ok_or("missing needs-expected line")?;
    let scalar = line
        .split_once("VELNOR_NEEDS_EXPECTED:")
        .ok_or("needs-expected shape")?
        .1
        .trim();
    let expected = scalar
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(scalar);
    let mut inventory = Vec::new();
    let mut required = false;
    let mut in_needs = false;
    for line in yaml.lines() {
        if line == "  required:" {
            required = true;
            continue;
        }
        if required && line.starts_with("  ") && !line.starts_with("   ") {
            required = false;
            in_needs = false;
        }
        if required && line == "    needs:" {
            in_needs = true;
            continue;
        }
        if in_needs {
            if let Some(id) = line.strip_prefix("      - ") {
                inventory.push(id.to_owned());
            } else {
                in_needs = false;
            }
        }
    }
    inventory.sort();
    if expected != needs_inventory_digest(&inventory) {
        return Err("needs digest does not match Required.needs".into());
    }
    inventory.retain(|id| id != velnor_actions_workflow_renderer::render::TASK_JOB_ID);
    inventory.sort();
    Ok(inventory)
}

/// Merge verdict over passing crates with one validator at `conclusion`.
pub(crate) fn merge_with_validator(
    plan: &Plan,
    inventory: &[String],
    failing: &str,
    conclusion: &str,
) -> Result<FinalReport, Box<dyn std::error::Error>> {
    let reports = passing_reports(plan)?;
    let jobs: Vec<serde_json::Value> = inventory
        .iter()
        .map(|id| {
            let result = if id == failing { conclusion } else { "success" };
            serde_json::json!({"job_id": id, "conclusion": result})
        })
        .collect();
    let plan_value = serde_json::to_value(plan)?;
    let mut request = merge_request(
        &plan_value,
        &plan_value["matrix"].clone(),
        &serde_json::to_value(&reports)?,
        &serde_json::Value::Array(jobs),
    );
    request["required_job_ids"] = serde_json::to_value(inventory)?;
    merge(&request)
}

/// Merge verdict over passing crates with `missing` never reporting.
pub(crate) fn merge_omitting_validator(
    plan: &Plan,
    inventory: &[String],
    missing: &str,
) -> Result<FinalReport, Box<dyn std::error::Error>> {
    let reports = passing_reports(plan)?;
    let jobs: Vec<serde_json::Value> = inventory
        .iter()
        .filter(|id| id.as_str() != missing)
        .map(|id| serde_json::json!({"job_id": id, "conclusion": "success"}))
        .collect();
    let plan_value = serde_json::to_value(plan)?;
    let mut request = merge_request(
        &plan_value,
        &plan_value["matrix"].clone(),
        &serde_json::to_value(&reports)?,
        &serde_json::Value::Array(jobs),
    );
    request["required_job_ids"] = serde_json::to_value(inventory)?;
    merge(&request)
}

/// Assert the verdict blames `failing` with `conclusion`.
pub(crate) fn assert_validator_blamed(
    report: &FinalReport,
    failing: &str,
    conclusion: JobConclusion,
    status: FinalStatus,
) {
    assert_eq!(report.status, status, "{failing} verdict");
    let blamed = report
        .required_job_results
        .iter()
        .find(|job| job.job_id == failing);
    assert_eq!(
        blamed.map(|job| job.conclusion),
        Some(conclusion),
        "{failing} blamed: {:?}",
        report.required_job_results
    );
}
