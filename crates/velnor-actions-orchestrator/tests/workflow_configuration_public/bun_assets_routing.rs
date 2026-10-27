// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;

use super::bun_assets_routing_support::{
    assert_mixed_report_artifacts, configure_mixed_profiles, mixed_rendered_job_ids,
};
use super::bun_routing::make_bun_repo;
use super::support::rendered_job_block;
use velnor_actions_orchestrator::{
    ExecutionMode, migrate_config, prepare, render_staged_tree_with,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

const MEMBER_ID: &str = "bun-fixture-web";
const ASSETS_JOB_ID: &str = "bun-fixture-web--assets";
const SUITE_IDS: [&str; 2] = ["bun-fixture-web--unit", "bun-fixture-web--browser"];

#[test]
fn schema2_bun_both_routes_private_asset_handoffs_once() -> Result<(), Box<dyn Error>> {
    assert_private_asset_handoffs(ExecutionMode::Both)
}

#[test]
fn schema2_bun_scale_set_routes_private_asset_handoffs_once() -> Result<(), Box<dyn Error>> {
    let workflow = render_bun_with_private_asset(ExecutionMode::ScaleSet)?;
    assert_private_asset_handoffs_for(&workflow)?;
    assert_job_runner(&workflow, ASSETS_JOB_ID, "ubuntu-26.04")?;
    for id in [
        "bun-fixture-web--unit",
        "bun-fixture-web--browser",
        "bun-fixture-web--browser--report",
    ] {
        assert_job_runner(&workflow, id, "[velnor, ubuntu-26.04-scale-set]")?;
    }
    Ok(())
}

fn assert_private_asset_handoffs(mode: ExecutionMode) -> Result<(), Box<dyn Error>> {
    let workflow = render_bun_with_private_asset(mode)?;
    assert_private_asset_handoffs_for(&workflow)
}

fn assert_private_asset_handoffs_for(workflow: &str) -> Result<(), Box<dyn Error>> {
    let member_id = MEMBER_ID;
    let assets_job_id = ASSETS_JOB_ID;
    let archive_name = format!("velnor-pw-assets-{member_id}.tar.gz");
    let stage_dir = format!("${{{{ runner.temp }}}}/velnor/bun-assets/{member_id}");
    let producer = rendered_job_block(workflow, assets_job_id)?;
    let archive_path = format!("{stage_dir}/{archive_name}");
    assert_eq!(asset_archive_path(producer)?, archive_path.as_str());
    assert!(has_yaml_value(producer, "archive", "false"));
    assert!(!has_yaml_value(producer, "name", &archive_name));
    assert!(has_yaml_value(
        producer,
        "name",
        "Bundle verified private assets"
    ));
    assert_named_asset_step_id(producer, "Bundle verified private assets", member_id)?;

    for suite_id in SUITE_IDS {
        let suite = rendered_job_block(workflow, suite_id)?;
        assert_eq!(
            needs_for_block(suite)?,
            BTreeSet::from(["plan".to_owned(), assets_job_id.to_owned()])
        );
        assert!(has_yaml_value(suite, "name", &archive_name));
        assert!(has_yaml_value(suite, "path", &stage_dir));
        assert_named_asset_step_id(suite, "Prepare private asset handoff", member_id)?;
        assert_named_asset_step_id(suite, "Import and verify private assets", member_id)?;
    }

    assert_browser_report_handoff(
        workflow,
        (
            "bun-fixture-web--browser",
            "bun-fixture-web--browser--report",
        ),
        (assets_job_id, "unused-other-assets-job"),
        member_id,
        "unused-other-member",
        &archive_name,
        &stage_dir,
    )?;
    Ok(())
}

fn render_bun_with_private_asset(mode: ExecutionMode) -> Result<String, Box<dyn Error>> {
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
    let prepared = prepare(repo.path())?;
    let rendered = render_staged_tree_with(&prepared, Some(mode))?;
    rendered
        .get(WORKFLOW_PATH)
        .map(str::to_owned)
        .ok_or_else(|| "rendered CI workflow is missing".into())
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
        let repo = make_mixed_private_assets_repo()?;
        configure_mixed_profiles(repo.path(), default_profile, suite_profile)?;
        let prepared = prepare(repo.path())?;
        let rendered = render_staged_tree_with(&prepared, None)?;
        let workflow = rendered
            .get(WORKFLOW_PATH)
            .ok_or("rendered CI workflow is missing")?;
        assert_mixed_profile_routing(workflow, default_runner, suite_runner)?;
    }
    Ok(())
}

fn make_mixed_private_assets_repo() -> Result<tempfile::TempDir, Box<dyn Error>> {
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
    migrate_config(repo.path(), 2, true)?;
    Ok(repo)
}

fn assert_mixed_profile_routing(
    workflow: &str,
    default_runner: &str,
    suite_runner: &str,
) -> Result<(), Box<dyn Error>> {
    let expected: BTreeSet<String> = [
        "bun-fixture-ui",
        "bun-fixture-ui--assets",
        MEMBER_ID,
        "bun-fixture-web--unit",
        "bun-fixture-web--browser",
        "bun-fixture-web--browser--report",
        ASSETS_JOB_ID,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let actual: BTreeSet<String> = mixed_rendered_job_ids(workflow)
        .into_iter()
        .filter(|id| id.starts_with("bun-"))
        .collect();
    assert_eq!(actual, expected);
    for id in [
        "bun-fixture-ui",
        MEMBER_ID,
        "bun-fixture-web--browser--report",
    ] {
        assert_job_runner(workflow, id, default_runner)?;
    }
    for id in ["bun-fixture-ui--assets", ASSETS_JOB_ID] {
        assert_job_runner(workflow, id, "ubuntu-26.04")?;
    }
    for id in SUITE_IDS {
        assert_job_runner(workflow, id, suite_runner)?;
    }
    let archive_name = format!("velnor-pw-assets-{MEMBER_ID}.tar.gz");
    let stage_dir = format!("${{{{ runner.temp }}}}/velnor/bun-assets/{MEMBER_ID}");
    let producer = rendered_job_block(workflow, ASSETS_JOB_ID)?;
    assert_eq!(
        asset_archive_path(producer)?,
        format!("{stage_dir}/{archive_name}")
    );
    assert_named_asset_step_id(producer, "Bundle verified private assets", MEMBER_ID)?;
    for suite_id in SUITE_IDS {
        let suite = rendered_job_block(workflow, suite_id)?;
        assert_eq!(
            needs_for_block(suite)?,
            BTreeSet::from(["plan".to_owned(), ASSETS_JOB_ID.to_owned()])
        );
        assert!(has_yaml_value(suite, "name", &archive_name));
        assert!(has_yaml_value(suite, "path", &stage_dir));
        assert_named_asset_step_id(suite, "Prepare private asset handoff", MEMBER_ID)?;
        assert_named_asset_step_id(suite, "Import and verify private assets", MEMBER_ID)?;
    }
    assert_browser_report_handoff(
        workflow,
        (
            "bun-fixture-web--browser",
            "bun-fixture-web--browser--report",
        ),
        (ASSETS_JOB_ID, "illegal-other-assets"),
        MEMBER_ID,
        "illegal-other-member",
        &archive_name,
        &stage_dir,
    )?;
    assert_mixed_report_artifacts(workflow)?;
    Ok(())
}
