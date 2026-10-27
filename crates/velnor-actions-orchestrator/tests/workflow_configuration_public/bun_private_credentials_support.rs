// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;

use super::bun_routing::make_bun_repo;
use super::support::rendered_job_block;
use velnor_actions_orchestrator::migrate_config;

pub(super) fn private_bun_repo() -> Result<tempfile::TempDir, Box<dyn Error>> {
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
    Ok(repo)
}

pub(super) fn set_execution(
    root: &std::path::Path,
    default_profile: &str,
    overrides: &[(&str, &str)],
) -> Result<(), Box<dyn Error>> {
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(root.join(".velnor/config.toml"))?)?;
    let execution = config
        .get_mut("execution")
        .and_then(toml::Value::as_table_mut)
        .ok_or("execution table is missing")?;
    execution.insert(
        "default_profile".to_owned(),
        toml::Value::String(default_profile.to_owned()),
    );
    execution.remove("mode");
    let mut override_tables = toml::map::Map::new();
    for (job_id, profile) in overrides {
        override_tables.insert(
            (*job_id).to_owned(),
            toml::Value::Table(toml::map::Map::from_iter([
                (
                    "profile".to_owned(),
                    toml::Value::String((*profile).to_owned()),
                ),
                (
                    "role".to_owned(),
                    toml::Value::String("verification".to_owned()),
                ),
            ])),
        );
    }
    execution.insert("overrides".to_owned(), toml::Value::Table(override_tables));
    fs::write(root.join(".velnor/config.toml"), toml::to_string(&config)?)?;
    Ok(())
}

pub(super) fn assert_member_handoff(workflow: &str, member_id: &str) -> Result<(), Box<dyn Error>> {
    let producer_id = format!("{member_id}--assets");
    let archive = format!("velnor-pw-assets-{member_id}.tar.gz");
    let stage = format!("${{{{ runner.temp }}}}/velnor/bun-assets/{member_id}");
    let producer = rendered_job_block(workflow, &producer_id)?;
    let member = rendered_job_block(workflow, member_id)?;
    assert_eq!(needs(producer)?, BTreeSet::from(["plan".to_owned()]));
    assert_eq!(
        needs(member)?,
        BTreeSet::from(["plan".to_owned(), producer_id])
    );
    assert!(producer.contains(&archive));
    assert!(member.contains(&archive));
    assert!(member.contains(&stage));
    let import = member
        .find("Import and verify private assets")
        .ok_or("member import step is missing")?;
    let verify = member
        .find("Verify Bun workspace")
        .ok_or("member verification step is missing")?;
    let registry = member
        .find("Install dependencies (registry phase)")
        .ok_or("member registry step is missing")?;
    assert!(import < verify && import < registry);
    Ok(())
}

pub(super) fn assert_runner(
    workflow: &str,
    id: &str,
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    let block = rendered_job_block(workflow, id)?;
    let actual = block
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("runs-on: "))
        .ok_or_else(|| format!("rendered job {id} has no runner"))?;
    assert_eq!(actual, expected, "runner selector for {id}");
    Ok(())
}

fn needs(block: &str) -> Result<BTreeSet<String>, Box<dyn Error>> {
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
