// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{bun_routing::make_bun_repo, support::rendered_job_block};
use std::{collections::BTreeSet, error::Error, fmt::Write as _, fs, path::Path};
use velnor_actions_orchestrator::{
    ExecutionMode, migrate_config, prepare, render_staged_tree_with,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

const MEMBER_ID: &str = "bun-fixture-web";
const ASSETS_JOB_ID: &str = "bun-fixture-web--assets";
const SUITE_IDS: [&str; 2] = ["bun-fixture-web--unit", "bun-fixture-web--browser"];
const BROWSER_SUITE_ID: &str = "bun-fixture-web--browser";
const BROWSER_REPORT_ID: &str = "bun-fixture-web--browser--report";
const MIXED_BUN_JOB_IDS: [&str; 6] = [
    "bun-fixture-ui",
    MEMBER_ID,
    "bun-fixture-web--unit",
    BROWSER_SUITE_ID,
    BROWSER_REPORT_ID,
    ASSETS_JOB_ID,
];

#[test]
fn schema2_bun_both_routes_private_asset_handoffs_per_lane() -> Result<(), Box<dyn Error>> {
    let workflow = render_bun_with_private_asset()?;
    let mut archive_names = Vec::new();
    for suffix in ["__hosted", "__local"] {
        let producer = rendered_job_block(&workflow, &format!("{ASSETS_JOB_ID}{suffix}"))?;
        archive_names.push(
            asset_archive_path(producer)?
                .rsplit('/')
                .next()
                .ok_or("paired Bun assets producer has no archive filename")?,
        );
    }
    assert_eq!(
        archive_names.iter().collect::<BTreeSet<_>>().len(),
        2,
        "paired assets producers must publish distinct archives"
    );
    for (suffix, other_suffix) in [("__hosted", "__local"), ("__local", "__hosted")] {
        let member_id = format!("{MEMBER_ID}{suffix}");
        let other_member_id = format!("{MEMBER_ID}{other_suffix}");
        let assets_job_id = format!("{ASSETS_JOB_ID}{suffix}");
        let archive_name = format!("velnor-pw-assets-{member_id}.tar.gz");
        let stage_dir = format!("${{{{ runner.temp }}}}/velnor/bun-assets/{member_id}");
        let producer = rendered_job_block(&workflow, &assets_job_id)?;
        let archive_path = format!("{stage_dir}/{archive_name}");
        assert_eq!(asset_archive_path(producer)?, archive_path.as_str());
        assert!(has_yaml_value(producer, "archive", "false"));
        assert!(!has_yaml_value(producer, "name", &archive_name));
        assert!(has_yaml_value(
            producer,
            "name",
            "Bundle verified private assets"
        ));
        assert_named_asset_step_id(producer, "Bundle verified private assets", &member_id)?;

        for suite_base_id in SUITE_IDS {
            let suite_id = format!("{suite_base_id}{suffix}");
            let suite = rendered_job_block(&workflow, &suite_id)?;
            let needs = needs_for_block(suite)?;
            assert_eq!(
                needs,
                BTreeSet::from(["plan".to_owned(), assets_job_id.clone()])
            );
            assert!(has_yaml_value(suite, "name", &archive_name));
            assert!(has_yaml_value(suite, "path", &stage_dir));
            assert!(!suite.contains(&format!("{other_member_id}.tar.gz")));
            assert_named_asset_step_id(suite, "Prepare private asset handoff", &member_id)?;
            assert_named_asset_step_id(suite, "Import and verify private assets", &member_id)?;
            assert_asset_id_count(suite, &other_member_id, 0);
            assert!(!needs.contains(&format!("{ASSETS_JOB_ID}{other_suffix}")));
        }

        assert_browser_report_handoff(
            &workflow,
            (
                &format!("{MEMBER_ID}--browser{suffix}"),
                &format!("{MEMBER_ID}--browser--report{suffix}"),
            ),
            (&assets_job_id, &format!("{ASSETS_JOB_ID}{other_suffix}")),
            &member_id,
            &other_member_id,
            &archive_name,
            &stage_dir,
        )?;
    }
    Ok(())
}

#[test]
fn schema2_bun_mixed_profiles_keep_private_asset_handoffs() -> Result<(), Box<dyn Error>> {
    for (default_profile, suite_profile, default_runner, suite_runner) in [
        (
            "hosted",
            "local",
            "ubuntu-26.04",
            "[velnor, ubuntu-26.04-scale-set]",
        ),
        (
            "local",
            "hosted",
            "[velnor, ubuntu-26.04-scale-set]",
            "ubuntu-26.04",
        ),
    ] {
        let repo = make_private_assets_repo()?;
        configure_mixed_execution(repo.path(), default_profile, suite_profile)?;
        let prepared = prepare(repo.path())?;
        let rendered = render_staged_tree_with(&prepared, None)?;
        let workflow = rendered
            .get(WORKFLOW_PATH)
            .ok_or("rendered CI workflow is missing")?;
        assert_mixed_bun_workflow(workflow, default_runner, suite_runner)?;
    }
    Ok(())
}

fn make_private_assets_repo() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let repo = make_bun_repo()?;
    let config_path = repo.path().join(".velnor/config.toml");
    let mut config = fs::read_to_string(&config_path)?;
    config.push_str(
        r#"
[private_sources]
owner = "fixture-owner"
app_id_variable = "FIXTURE_APP_ID"
app_private_key_secret = "FIXTURE_APP_PRIVATE_KEY"

[[private_sources.release_assets]]
id = "fixture-assets"
repository = "fixture-owner/fixture-assets"
tag = "v1.0.0"
asset = "fixture-assets.tar.gz"
extract_to = "fixture-assets"
"#,
    );
    fs::write(&config_path, config)?;
    fs::write(
        repo.path().join(".velnor/deps.lock"),
        r#"schema = 1

[[release_assets]]
id = "fixture-assets"
repository = "fixture-owner/fixture-assets"
tag = "v1.0.0"
commit = "0123456789abcdef0123456789abcdef01234567"
asset = "fixture-assets.tar.gz"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
extract_to = "fixture-assets"
"#,
    )?;
    let migrated = migrate_config(repo.path(), 2, true)?;
    assert_eq!(fs::read_to_string(config_path)?, migrated);
    Ok(repo)
}

fn render_bun_with_private_asset() -> Result<String, Box<dyn Error>> {
    let repo = make_private_assets_repo()?;
    let prepared = prepare(repo.path())?;
    let rendered = render_staged_tree_with(&prepared, Some(ExecutionMode::Both))?;
    rendered
        .get(WORKFLOW_PATH)
        .map(str::to_owned)
        .ok_or_else(|| "rendered CI workflow is missing".into())
}

fn configure_mixed_execution(
    repo: &Path,
    default_profile: &str,
    suite_profile: &str,
) -> Result<(), Box<dyn Error>> {
    let config_path = repo.join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?;
    assert!(!config.lines().any(|line| line.starts_with("mode = ")));
    let previous = "default_profile = \"hosted\"";
    let replacement = format!("default_profile = \"{default_profile}\"");
    assert_eq!(config.matches(previous).count(), 1);
    let mut config = config.replace(previous, &replacement);
    write!(
        config,
        r#"

[execution.overrides."bun-fixture-web--unit"]
profile = "{suite_profile}"
role = "verification"

[execution.overrides."bun-fixture-web--browser"]
profile = "{suite_profile}"
role = "verification"
"#
    )?;
    fs::write(config_path, config)?;
    Ok(())
}

fn assert_mixed_bun_workflow(
    workflow: &str,
    default_runner: &str,
    suite_runner: &str,
) -> Result<(), Box<dyn Error>> {
    let actual: BTreeSet<String> = rendered_job_ids(workflow)
        .into_iter()
        .filter(|id| id.starts_with("bun-"))
        .collect();
    assert_eq!(
        actual,
        MIXED_BUN_JOB_IDS
            .iter()
            .map(|id| (*id).to_owned())
            .collect::<BTreeSet<_>>()
    );
    for id in [
        "bun-fixture-ui",
        MEMBER_ID,
        ASSETS_JOB_ID,
        BROWSER_REPORT_ID,
    ] {
        assert_job_runner(workflow, id, default_runner)?;
    }
    for id in SUITE_IDS {
        assert_job_runner(workflow, id, suite_runner)?;
    }

    let archive_name = format!("velnor-pw-assets-{MEMBER_ID}.tar.gz");
    let stage_dir = format!("${{{{ runner.temp }}}}/velnor/bun-assets/{MEMBER_ID}");
    let producer = rendered_job_block(workflow, ASSETS_JOB_ID)?;
    let archive_path = format!("{stage_dir}/{archive_name}");
    assert_eq!(asset_archive_path(producer)?, archive_path.as_str());
    assert_named_asset_step_id(producer, "Bundle verified private assets", MEMBER_ID)?;

    for suite_id in SUITE_IDS {
        let suite = rendered_job_block(workflow, suite_id)?;
        assert_eq!(
            needs_for_block(suite)?,
            BTreeSet::from(["plan".to_owned(), ASSETS_JOB_ID.to_owned()])
        );
        assert_private_asset_handoff(suite, MEMBER_ID, &archive_name, &stage_dir)?;
    }

    let report = rendered_job_block(workflow, BROWSER_REPORT_ID)?;
    assert_eq!(
        needs_for_block(report)?,
        BTreeSet::from([
            "plan".to_owned(),
            ASSETS_JOB_ID.to_owned(),
            BROWSER_SUITE_ID.to_owned(),
        ])
    );
    assert!(report.contains(&format!("needs.{BROWSER_SUITE_ID}.result")));
    assert_private_asset_handoff(report, MEMBER_ID, &archive_name, &stage_dir)?;

    let browser_suite = rendered_job_block(workflow, BROWSER_SUITE_ID)?;
    assert_single_lane_artifacts(browser_suite, report, BROWSER_SUITE_ID);
    Ok(())
}

fn assert_private_asset_handoff(
    block: &str,
    member_id: &str,
    archive_name: &str,
    stage_dir: &str,
) -> Result<(), Box<dyn Error>> {
    assert!(has_yaml_value(block, "name", archive_name));
    assert!(has_yaml_value(block, "path", stage_dir));
    assert_named_asset_step_id(block, "Prepare private asset handoff", member_id)?;
    assert_named_asset_step_id(block, "Import and verify private assets", member_id)?;
    Ok(())
}

fn assert_single_lane_artifacts(suite: &str, report: &str, suite_id: &str) {
    let blob_name = format!("velnor-pw-blob-{suite_id}-leg-${{{{ matrix.leg }}}}");
    let blob_pattern = format!("velnor-pw-blob-{suite_id}-leg-*");
    let results_name = format!("velnor-pw-results-{suite_id}-leg-${{{{ matrix.leg }}}}");
    let report_name = format!("velnor-pw-report-{suite_id}");
    assert!(suite.contains(&format!("name: {blob_name}")));
    assert!(suite.contains(&format!("name: {results_name}")));
    assert!(report.contains(&format!("pattern: {blob_pattern}")));
    assert!(report.contains(&format!("name: {report_name}")));
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

fn asset_archive_path(block: &str) -> Result<&str, Box<dyn Error>> {
    block
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("path: ")
                .map(|path| path.trim_matches(|character| character == '\'' || character == '"'))
                .filter(|path| path.contains("/velnor-pw-assets-") && path.ends_with(".tar.gz"))
        })
        .ok_or_else(|| "Bun assets producer has no archive path".into())
}

fn has_yaml_value(block: &str, key: &str, expected: &str) -> bool {
    let prefix = format!("{key}: ");
    block
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            line.strip_prefix("- ")
                .unwrap_or(line)
                .strip_prefix(&prefix)
        })
        .any(|value| {
            value.trim_matches(|character| character == '\'' || character == '"') == expected
        })
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

fn assert_browser_report_handoff(
    workflow: &str,
    browser_ids: (&str, &str),
    assets_job_ids: (&str, &str),
    member_id: &str,
    other_member_id: &str,
    archive_name: &str,
    stage_dir: &str,
) -> Result<(), Box<dyn Error>> {
    let (browser_suite_id, report_id) = browser_ids;
    let (assets_job_id, other_assets_job_id) = assets_job_ids;
    let report = rendered_job_block(workflow, report_id)?;
    let needs = needs_for_block(report)?;
    assert_eq!(
        needs,
        BTreeSet::from([
            "plan".to_owned(),
            assets_job_id.to_owned(),
            browser_suite_id.to_owned(),
        ])
    );
    assert!(!needs.contains(other_assets_job_id));
    assert!(has_yaml_value(report, "name", archive_name));
    assert!(has_yaml_value(report, "path", stage_dir));
    assert!(!report.contains(&format!("{other_member_id}.tar.gz")));
    assert_named_asset_step_id(report, "Prepare private asset handoff", member_id)?;
    assert_named_asset_step_id(report, "Import and verify private assets", member_id)?;
    assert_asset_id_count(report, other_member_id, 0);
    Ok(())
}

fn assert_asset_id_count(block: &str, member_id: &str, expected: usize) {
    let count = block
        .lines()
        .filter_map(|line| line.trim().strip_prefix("VELNOR_BUN_ASSET_ID: "))
        .filter(|value| {
            value.trim_matches(|character| character == '\'' || character == '"') == member_id
        })
        .count();
    assert_eq!(count, expected, "VELNOR_BUN_ASSET_ID: {member_id}");
}

fn assert_named_asset_step_id(
    block: &str,
    step_name: &str,
    member_id: &str,
) -> Result<(), Box<dyn Error>> {
    let step = block
        .split("\n      - ")
        .find(|step| has_yaml_value(step, "name", step_name))
        .ok_or_else(|| format!("missing Bun asset handoff step: {step_name}"))?;
    assert_asset_id_count(step, member_id, 1);
    Ok(())
}
