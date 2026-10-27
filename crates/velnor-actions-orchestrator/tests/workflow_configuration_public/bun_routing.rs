// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

use super::support::rendered_job_block;
use super::workflow_configuration_fixture;
use velnor_actions_orchestrator::{
    ExecutionMode, migrate_config, prepare, render_staged_tree, render_staged_tree_with,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

const BUN_JOB_IDS: [&str; 5] = [
    "bun-fixture-ui",
    "bun-fixture-web",
    "bun-fixture-web--unit",
    "bun-fixture-web--browser",
    "bun-fixture-web--browser--report",
];
const REQUIRED_BUN_JOB_IDS: [&str; 4] = [
    "bun-fixture-ui",
    "bun-fixture-web",
    "bun-fixture-web--unit",
    "bun-fixture-web--browser",
];

#[test]
fn schema1_bun_output_ignores_execution_dispatch() -> Result<(), Box<dyn Error>> {
    let repo = make_bun_repo()?;
    let prepared = prepare(repo.path())?;
    let baseline = render_staged_tree(&prepared)?;
    for mode in [
        ExecutionMode::Hosted,
        ExecutionMode::ScaleSet,
        ExecutionMode::Both,
    ] {
        assert_eq!(
            render_staged_tree_with(&prepared, Some(mode))?,
            baseline,
            "schema-1 output changed for {mode:?}"
        );
    }
    Ok(())
}

#[test]
fn schema2_bun_hosted_mode_keeps_five_hosted_jobs() -> Result<(), Box<dyn Error>> {
    let workflow = render_schema2_bun(ExecutionMode::Hosted)?;
    assert_bun_jobs(&workflow, ExecutionMode::Hosted)?;
    assert_hosted_controls(&workflow)?;
    assert_required_needs(&workflow, ExecutionMode::Hosted)?;
    assert_playwright_lane(
        &workflow,
        "bun-fixture-web--browser",
        "bun-fixture-web--browser--report",
        None,
    )?;
    Ok(())
}

#[test]
fn schema2_bun_scale_set_mode_keeps_five_jobs_on_exact_selector() -> Result<(), Box<dyn Error>> {
    let workflow = render_schema2_bun(ExecutionMode::ScaleSet)?;
    assert_bun_jobs(&workflow, ExecutionMode::ScaleSet)?;
    assert_hosted_controls(&workflow)?;
    assert_required_needs(&workflow, ExecutionMode::ScaleSet)?;
    assert_playwright_lane(
        &workflow,
        "bun-fixture-web--browser",
        "bun-fixture-web--browser--report",
        None,
    )?;
    Ok(())
}

#[test]
fn schema2_bun_both_mode_emits_one_hosted_job_and_keeps_handoffs() -> Result<(), Box<dyn Error>> {
    let workflow = render_schema2_bun(ExecutionMode::Both)?;
    assert_bun_jobs(&workflow, ExecutionMode::Both)?;
    assert_hosted_controls(&workflow)?;
    assert_required_needs(&workflow, ExecutionMode::Both)?;
    assert_playwright_lane(
        &workflow,
        "bun-fixture-web--browser",
        "bun-fixture-web--browser--report",
        None,
    )?;
    Ok(())
}

fn render_schema2_bun(mode: ExecutionMode) -> Result<String, Box<dyn Error>> {
    let repo = make_bun_repo()?;
    let migrated = migrate_config(repo.path(), 2, true)?;
    assert_eq!(
        fs::read_to_string(repo.path().join(".velnor/config.toml"))?,
        migrated
    );
    let prepared = prepare(repo.path())?;
    let rendered = render_staged_tree_with(&prepared, Some(mode))?;
    Ok(rendered
        .get(WORKFLOW_PATH)
        .ok_or("rendered CI workflow is missing")?
        .to_owned())
}

pub(super) fn make_bun_repo() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bun-workspace-suites/input");
    let config = fs::read_to_string(fixture.join(".velnor/config.toml"))?;
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    fs::remove_file(repo.path().join("Cargo.toml"))?;
    fs::remove_file(repo.path().join("src/lib.rs"))?;
    for (source, destination) in [
        ("package.json", "package.json"),
        ("bun.lock", "bun.lock"),
        ("mise.toml", "mise.toml"),
        (".velnor/playwright.lock", ".velnor/playwright.lock"),
        ("apps/web/package.json", "apps/web/package.json"),
        ("packages/ui/package.json", "packages/ui/package.json"),
    ] {
        copy_fixture_file(&fixture.join(source), &repo.path().join(destination))?;
    }
    Ok(repo)
}

fn copy_fixture_file(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    let parent = destination.parent().ok_or("fixture file has no parent")?;
    fs::create_dir_all(parent)?;
    fs::copy(source, destination)?;
    Ok(())
}

fn assert_bun_jobs(workflow: &str, mode: ExecutionMode) -> Result<(), Box<dyn Error>> {
    let expected: BTreeSet<String> = BUN_JOB_IDS.iter().map(|id| (*id).to_owned()).collect();
    let actual: BTreeSet<String> = rendered_job_ids(workflow)
        .into_iter()
        .filter(|id| id.starts_with("bun-"))
        .collect();
    assert_eq!(actual, expected, "unexpected Bun job IDs for {mode:?}");
    match mode {
        ExecutionMode::Hosted | ExecutionMode::Both => {
            for id in BUN_JOB_IDS {
                assert_job_runner(workflow, id, "ubuntu-26.04")?;
            }
        }
        ExecutionMode::ScaleSet => {
            for id in BUN_JOB_IDS {
                assert_job_runner(workflow, id, "[velnor, ubuntu-26.04-scale-set]")?;
            }
        }
    }
    Ok(())
}

fn rendered_job_ids(workflow: &str) -> BTreeSet<String> {
    workflow
        .lines()
        .skip_while(|line| *line != "jobs:")
        .skip(1)
        .filter_map(|line| line.strip_prefix("  "))
        .filter_map(|line| line.strip_suffix(':'))
        .filter(|id| !id.starts_with(' '))
        .map(str::to_owned)
        .collect()
}

fn assert_job_runner(workflow: &str, id: &str, expected: &str) -> Result<(), Box<dyn Error>> {
    let block = rendered_job_block(workflow, id)?;
    let actual = block
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("runs-on: "))
        .ok_or_else(|| format!("rendered job {id} has no runner"))?;
    assert_eq!(actual, expected, "runner selector for {id}");
    Ok(())
}

fn assert_hosted_controls(workflow: &str) -> Result<(), Box<dyn Error>> {
    let ids = rendered_job_ids(workflow);
    for id in ["plan", "jactionlint", "required"] {
        assert!(ids.contains(id), "control job {id} is missing");
        assert!(!ids.contains(&format!("{id}__hosted")));
        assert!(!ids.contains(&format!("{id}__local")));
        assert_job_runner(workflow, id, "ubuntu-26.04")?;
    }
    Ok(())
}

fn assert_required_needs(workflow: &str, mode: ExecutionMode) -> Result<(), Box<dyn Error>> {
    let block = rendered_job_block(workflow, "required")?;
    let mut expected: BTreeSet<String> = match mode {
        ExecutionMode::Both => REQUIRED_BUN_JOB_IDS
            .iter()
            .map(|id| (*id).to_owned())
            .collect(),
        ExecutionMode::Hosted | ExecutionMode::ScaleSet => REQUIRED_BUN_JOB_IDS
            .iter()
            .map(|id| (*id).to_owned())
            .collect(),
    };
    expected.extend(["plan".to_owned(), "jactionlint".to_owned()]);
    assert_eq!(needs_for_block(block)?, expected);
    let gate = rendered_job_block(workflow, "required-gate")?;
    assert_eq!(
        needs_for_block(gate)?,
        BTreeSet::from(["required".to_owned()])
    );
    Ok(())
}

fn assert_playwright_lane(
    workflow: &str,
    suite_id: &str,
    report_id: &str,
    other_suite: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let suite = rendered_job_block(workflow, suite_id)?;
    let report = rendered_job_block(workflow, report_id)?;
    assert_eq!(
        needs_for_block(report)?,
        BTreeSet::from(["plan".to_owned(), suite_id.to_owned()])
    );
    assert!(report.contains(&format!("needs.{suite_id}.result")));
    if let Some(base_id) = suite_id
        .strip_suffix("__hosted")
        .or_else(|| suite_id.strip_suffix("__local"))
    {
        assert!(!report.contains(&format!("needs.{base_id}.result")));
    }
    let blob_name = format!("velnor-pw-blob-{suite_id}-leg-${{{{ matrix.leg }}}}");
    let blob_pattern = format!("velnor-pw-blob-{suite_id}-leg-*");
    let results_name = format!("velnor-pw-results-{suite_id}-leg-${{{{ matrix.leg }}}}");
    let report_name = format!("velnor-pw-report-{suite_id}");
    assert!(suite.contains(&format!("name: {blob_name}")));
    assert!(suite.contains(&format!("name: {results_name}")));
    assert!(report.contains(&format!("pattern: {blob_pattern}")));
    assert!(report.contains(&format!("name: {report_name}")));
    if let Some(other_suite) = other_suite {
        assert!(!suite.contains(&format!("velnor-pw-blob-{other_suite}-leg-")));
        assert!(!report.contains(&format!("velnor-pw-blob-{other_suite}-leg-")));
    }
    Ok(())
}

fn needs_for_block(block: &str) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let section = block
        .split("    needs:\n")
        .nth(1)
        .ok_or("rendered job has no needs field")?;
    Ok(section
        .lines()
        .take_while(|line| line.starts_with("      - "))
        .filter_map(|line| line.trim().strip_prefix("- "))
        .map(str::to_owned)
        .collect())
}
