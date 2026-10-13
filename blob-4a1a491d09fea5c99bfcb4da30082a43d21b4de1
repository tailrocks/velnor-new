use std::fs;

use velnor_actions_orchestrator::prepare;

use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_build_tasks::{config_with_build, write_native_source_fixture};

#[test]
fn native_build_task_rejects_cargo_source_install_fallback() -> TestResult {
    let body = config_with_build("hosted");
    let repo = make_repo(&body)?;
    write_native_source_fixture(repo.path())?;
    let lock_path = repo.path().join("mise.lock");
    let lock = fs::read_to_string(&lock_path)?.replace(
        "backend = \"github:boltffi/boltffi\"",
        "backend = \"cargo:boltffi_cli\"",
    );
    fs::write(lock_path, lock)?;

    let error = prepare(repo.path()).expect_err("Cargo fallback backend must fail closed");
    assert!(
        error
            .to_string()
            .contains("build_task_tool_backend_unsupported"),
        "{error}"
    );
    Ok(())
}

#[test]
fn native_build_task_rejects_unsupported_mise_lockfile_versions() -> TestResult {
    let mut config = toml::from_str::<toml::Value>(&config_with_build("hosted"))?;
    let tasks = config
        .get_mut("workflow")
        .and_then(toml::Value::as_table_mut)
        .and_then(|workflow| workflow.get_mut("tasks"))
        .and_then(toml::Value::as_array_mut)
        .ok_or("workflow task array")?;
    tasks.retain(|task| task.get("kind").and_then(toml::Value::as_str) == Some("build"));
    let config = toml::to_string(&config)?;

    for version in [2, 4] {
        let repo = make_repo(&config)?;
        write_native_source_fixture(repo.path())?;
        let lock_path = repo.path().join("mise.lock");
        let source = fs::read_to_string(&lock_path)?;
        let version_field = "lockfile_version = 3";
        assert_eq!(source.matches(version_field).count(), 1);
        let lock = source.replace(version_field, &format!("lockfile_version = {version}"));
        toml::from_str::<toml::Value>(&lock)?;
        fs::write(lock_path, lock)?;

        let error = prepare(repo.path()).expect_err("unsupported Mise lock version must fail");
        assert!(
            error.to_string().contains("build_task_mise_lock_root"),
            "{error}"
        );
    }
    Ok(())
}

#[test]
fn native_build_task_requires_the_configured_request_in_lock_specifiers() -> TestResult {
    let repo = make_repo(&config_with_build("hosted"))?;
    write_native_source_fixture(repo.path())?;
    let lock_path = repo.path().join("mise.lock");
    let source = fs::read_to_string(&lock_path)?;
    let selector = "specifiers = [\"0.9.140\"]";
    assert_eq!(source.matches(selector).count(), 1);
    fs::write(
        lock_path,
        source.replace(selector, "specifiers = [\"0.9\"]"),
    )?;

    let error = prepare(repo.path()).expect_err("config request must be bound by lock specifiers");
    assert!(
        error.to_string().contains("build_task_tool_lock_mismatch"),
        "{error}"
    );
    Ok(())
}
