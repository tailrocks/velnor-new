// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;

use super::bun_routing::make_bun_repo;
use super::support::rendered_job_block;
use velnor_actions_orchestrator::{
    ExecutionMode, migrate_config, prepare, render_staged_tree_with,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

const ASSETS_JOB_ID: &str = "bun-fixture-web--assets";
const APP_KEY: &str = "FIXTURE_APP_PRIVATE_KEY";
const PRIVATE_TOKEN: &str = "VELNOR_PRIVATE_FETCH_TOKEN";
const PRIVATE_NPM_TOKEN: &str = "VELNOR_NPM_READ_TOKEN";
const PRIVATE_TOKEN_OUTPUT: &str = "steps.deps-token.outputs.token";
const HOSTED_RUNNER: &str = "ubuntu-26.04";

#[test]
fn private_asset_app_key_minting_stays_on_hosted_producer() -> Result<(), Box<dyn Error>> {
    for mode in [
        ExecutionMode::ScaleSet,
        ExecutionMode::Hosted,
        ExecutionMode::Both,
    ] {
        let workflow = render_bun_with_private_asset(mode)?;
        let producer = rendered_job_block(&workflow, ASSETS_JOB_ID)?;
        assert_job_runner(&workflow, ASSETS_JOB_ID, HOSTED_RUNNER)?;
        assert!(producer.contains(APP_KEY));
        assert!(producer.contains("create-github-app-token"));
        assert_eq!(
            needs_for_block(producer)?,
            BTreeSet::from(["plan".to_owned()])
        );

        for consumer_id in [
            "bun-fixture-ui",
            "bun-fixture-web",
            "bun-fixture-web--unit",
            "bun-fixture-web--browser",
            "bun-fixture-web--browser--report",
        ] {
            let consumer = rendered_job_block(&workflow, consumer_id)?;
            assert!(
                !consumer.contains(APP_KEY),
                "{consumer_id} received app key"
            );
            assert!(
                !consumer.contains(PRIVATE_TOKEN) && !consumer.contains(PRIVATE_TOKEN_OUTPUT),
                "{consumer_id} received a private installation token"
            );
            assert!(
                !consumer.contains("create-github-app-token"),
                "{consumer_id} can mint a private installation token"
            );
        }

        assert_member_handoff(&workflow, "bun-fixture-ui")?;
        assert_member_handoff(&workflow, "bun-fixture-web")?;
        assert_suite_handoff(&workflow, "bun-fixture-web--unit", "bun-fixture-web")?;
        assert_suite_handoff(&workflow, "bun-fixture-web--browser", "bun-fixture-web")?;
        assert_report_handoff(&workflow, "bun-fixture-web")?;
    }

    Ok(())
}

fn assert_member_handoff(workflow: &str, member_id: &str) -> Result<(), Box<dyn Error>> {
    let producer_id = format!("{member_id}--assets");
    let archive = format!("velnor-pw-assets-{member_id}.tar.gz");
    let stage = format!("${{{{ runner.temp }}}}/velnor/bun-assets/{member_id}");
    let producer = rendered_job_block(workflow, &producer_id)?;
    assert_job_runner(workflow, &producer_id, HOSTED_RUNNER)?;
    assert!(producer.contains(APP_KEY));
    assert!(producer.contains("create-github-app-token"));
    assert_eq!(
        needs_for_block(producer)?,
        BTreeSet::from(["plan".to_owned()])
    );
    assert!(producer.contains("Bundle verified private assets"));
    assert!(producer.contains(&archive));
    assert!(producer.contains(&format!("{stage}/{archive}")));

    let consumer = rendered_job_block(workflow, member_id)?;
    assert_eq!(
        needs_for_block(consumer)?,
        BTreeSet::from(["plan".to_owned(), producer_id])
    );
    assert!(consumer.contains(&archive));
    assert!(consumer.contains(&stage));
    let import = workflow_step(consumer, "Import and verify private assets")?;
    let verify = workflow_step(consumer, "Verify Bun workspace")?;
    let registry = workflow_step(consumer, "Install dependencies (registry phase)")?;
    assert!(consumer.find(import) < consumer.find(verify));
    assert!(consumer.find(import) < consumer.find(registry));
    assert!(registry.contains(PRIVATE_NPM_TOKEN));
    assert_eq!(consumer.matches(PRIVATE_NPM_TOKEN).count(), 1);
    Ok(())
}

fn assert_suite_handoff(
    workflow: &str,
    suite_id: &str,
    member_id: &str,
) -> Result<(), Box<dyn Error>> {
    let producer_id = format!("{member_id}--assets");
    let archive = format!("velnor-pw-assets-{member_id}.tar.gz");
    let stage = format!("${{{{ runner.temp }}}}/velnor/bun-assets/{member_id}");
    let producer = rendered_job_block(workflow, &producer_id)?;
    assert!(producer.contains("Bundle verified private assets"));
    assert!(producer.contains(&archive));
    assert!(producer.contains(&format!("{stage}/{archive}")));
    let suite = rendered_job_block(workflow, suite_id)?;
    assert_eq!(
        needs_for_block(suite)?,
        BTreeSet::from(["plan".to_owned(), producer_id])
    );
    assert!(suite.contains(&archive));
    assert!(suite.contains(&stage));
    let import = workflow_step(suite, "Import and verify private assets")?;
    let verify = workflow_step(suite, "Verify Bun workspace")?;
    let registry = workflow_step(suite, "Install dependencies (registry phase)")?;
    assert!(suite.find(import) < suite.find(verify));
    assert!(suite.find(import) < suite.find(registry));
    assert!(registry.contains(PRIVATE_NPM_TOKEN));
    assert_eq!(suite.matches(PRIVATE_NPM_TOKEN).count(), 1);
    Ok(())
}

fn assert_report_handoff(workflow: &str, member_id: &str) -> Result<(), Box<dyn Error>> {
    let producer_id = format!("{member_id}--assets");
    let archive = format!("velnor-pw-assets-{member_id}.tar.gz");
    let stage = format!("${{{{ runner.temp }}}}/velnor/bun-assets/{member_id}");
    let report = rendered_job_block(workflow, "bun-fixture-web--browser--report")?;
    assert_eq!(
        needs_for_block(report)?,
        BTreeSet::from([
            "plan".to_owned(),
            producer_id,
            "bun-fixture-web--browser".to_owned(),
        ])
    );
    assert!(report.contains(&archive));
    assert!(report.contains(&stage));
    assert!(report.contains("Import and verify private assets"));
    Ok(())
}

fn assert_job_runner(workflow: &str, job_id: &str, expected: &str) -> Result<(), Box<dyn Error>> {
    let block = rendered_job_block(workflow, job_id)?;
    let actual = block
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("runs-on: "))
        .ok_or_else(|| format!("rendered job {job_id} has no runner"))?;
    assert_eq!(actual, expected, "runner selector for {job_id}");
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
npm_scopes = ["@fixture"]

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

fn workflow_step<'a>(block: &'a str, name: &str) -> Result<&'a str, Box<dyn Error>> {
    block
        .split("\n      - ")
        .find(|step| {
            step.lines()
                .any(|line| line.trim() == format!("name: {name}"))
        })
        .ok_or_else(|| format!("workflow step is missing: {name}").into())
}
