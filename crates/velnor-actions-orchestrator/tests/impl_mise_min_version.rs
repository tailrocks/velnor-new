//! Mise minimum-version validation against the generator's pinned runtime.

use std::fs;

use velnor_actions_orchestrator::prepare;

use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_build_tasks::{config_with_build, write_native_source_fixture};
use crate::impl_schema2_verification_tasks::{
    config as verification_config, write_verification_task_config,
};

const INVALID_MINIMUMS: [&str; 9] = [
    "\"2026.10.8\"",
    "\"2026.010.7\"",
    "\"2026.10.7-01\"",
    "\"~2026.10.7\"",
    "2026",
    "{ hard = \"2026.10.8\", soft = \"2026.11.0\" }",
    "{ hard = \"2026.10.6\", soft = 2026 }",
    "{ soft = \"2026.10.6\", other = \"2026.10.7\" }",
    "{}",
];

#[test]
fn verification_minimum_is_typed_and_bounded_by_catalog() -> TestResult {
    for minimum in INVALID_MINIMUMS {
        let repo = make_repo(&verification_config("hosted"))?;
        write_verification_task_config(repo.path())?;
        replace_min_version(repo.path(), minimum)?;

        let error = prepare(repo.path()).expect_err("unsupported Mise minimum must fail closed");
        assert!(
            error.to_string().contains("verification_mise_min_version"),
            "{minimum}: {error}"
        );
    }
    Ok(())
}

#[test]
fn build_task_minimum_is_typed_and_bounded_by_catalog() -> TestResult {
    let config = build_only_config()?;
    for minimum in INVALID_MINIMUMS {
        let repo = make_repo(&config)?;
        write_native_source_fixture(repo.path())?;
        replace_min_version(repo.path(), minimum)?;

        let error = prepare(repo.path()).expect_err("unsupported Mise minimum must fail closed");
        assert!(
            error.to_string().contains("build_task_mise_min_version"),
            "{minimum}: {error}"
        );
    }
    Ok(())
}

#[test]
fn semver_minimums_at_or_below_catalog_are_accepted() -> TestResult {
    for minimum in [
        "\"2026.10.7\"",
        "\"2026.10.7-rc.1+build.2\"",
        "\"2026.10.6\"",
        "{ hard = \"2026.10.6\", soft = \"2026.11.0\" }",
        "{ soft = \"2026.11.0\" }",
    ] {
        let repo = make_repo(&verification_config("hosted"))?;
        write_verification_task_config(repo.path())?;
        replace_min_version(repo.path(), minimum)?;
        prepare(repo.path())?;
    }
    Ok(())
}

#[test]
fn build_task_minimum_object_is_accepted() -> TestResult {
    let config = build_only_config()?;
    let repo = make_repo(&config)?;
    write_native_source_fixture(repo.path())?;
    replace_min_version(
        repo.path(),
        "{ hard = \"2026.10.6\", soft = \"2026.11.0\" }",
    )?;
    prepare(repo.path())?;
    Ok(())
}

fn build_only_config() -> Result<String, Box<dyn std::error::Error>> {
    let mut value = toml::from_str::<toml::Value>(&config_with_build("hosted"))?;
    let tasks = value
        .get_mut("workflow")
        .and_then(toml::Value::as_table_mut)
        .and_then(|workflow| workflow.get_mut("tasks"))
        .and_then(toml::Value::as_array_mut)
        .ok_or("workflow task array")?;
    tasks.retain(|task| task.get("kind").and_then(toml::Value::as_str) == Some("build"));
    Ok(toml::to_string(&value)?)
}

fn replace_min_version(root: &std::path::Path, minimum: &str) -> TestResult {
    let path = root.join("mise.toml");
    let source = fs::read_to_string(&path)?;
    let fixture_pin = "min_version = \"2026.10.7\"";
    let replacement = format!("min_version = {minimum}");
    assert_eq!(
        source.matches(fixture_pin).count(),
        1,
        "fixture pin is unique"
    );
    if replacement == fixture_pin {
        return Ok(());
    }
    let updated = source.replace(fixture_pin, &replacement);
    assert_ne!(source, updated, "the test value replaces the fixture pin");
    fs::write(path, updated)?;
    Ok(())
}
