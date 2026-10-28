// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::Value;

type FileSnapshot = Vec<(PathBuf, Vec<u8>)>;

#[test]
fn cross_workspace_path_dependency_direct() -> Result<(), Box<dyn Error>> {
    check_positive(false)
}

#[test]
fn cross_workspace_path_dependency_inherited_alias() -> Result<(), Box<dyn Error>> {
    check_positive(true)
}

fn check_positive(inherited: bool) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    create_fixture(&repo, inherited)?;
    let before = snapshot_inputs(&repo, &[])?;
    let rows = freshness_rows(&repo)?;
    assert_row(
        &rows,
        "lock-staleness",
        if inherited {
            "velnor-runner-host:dependencies:contract-alias"
        } else {
            "velnor-runner-host:dependencies:velnor-actions-contract"
        },
        "pass",
    );
    assert_row(
        &rows,
        "lock-staleness",
        "crates/velnor-runner/(lock-membership)",
        "pass",
    );
    assert_row(
        &rows,
        "lock-staleness",
        "crates/velnor-runner/(lock-graph)",
        "pass",
    );
    assert_eq!(snapshot_inputs(&repo, &[])?, before);
    Ok(())
}

#[test]
fn cross_workspace_path_dependency_rejects_escaping_target() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    let outside = temp.path().join("outside-contract");
    create_fixture(&repo, false)?;
    fs::create_dir_all(&outside)?;
    fs::write(
        outside.join("Cargo.toml"),
        "[package]\nname = \"velnor-actions-contract\"\nversion = \"0.1.69\"\nedition = \"2024\"\n",
    )?;
    write_host_path(&repo, "../../../../../outside-contract")?;
    let before = snapshot_inputs(&repo, &[outside.join("Cargo.toml")])?;
    let rows = freshness_rows(&repo)?;
    assert_row_detail(
        &rows,
        "lock-staleness",
        "velnor-runner-host:dependencies:velnor-actions-contract",
        "fail",
        Some("escapes the repository root"),
    );
    assert_eq!(
        snapshot_inputs(&repo, &[outside.join("Cargo.toml")])?,
        before
    );
    Ok(())
}

#[test]
fn cross_workspace_path_dependency_rejects_wrong_package_name() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    create_fixture(&repo, false)?;
    let wrong = repo.join("crates/wrong-contract");
    fs::create_dir_all(&wrong)?;
    fs::write(repo.join("Cargo.toml"), WRONG_NAME_ROOT_MANIFEST)?;
    fs::write(
        wrong.join("Cargo.toml"),
        "[package]\nname = \"different-contract\"\nversion.workspace = true\nedition.workspace = true\n",
    )?;
    fs::write(repo.join("Cargo.lock"), WRONG_NAME_ROOT_LOCK)?;
    write_host_path(&repo, "../../../wrong-contract")?;
    let before = snapshot_inputs(&repo, &[wrong.join("Cargo.toml")])?;
    let rows = freshness_rows(&repo)?;
    assert_row_detail(
        &rows,
        "lock-staleness",
        "velnor-runner-host:dependencies:velnor-actions-contract",
        "fail",
        Some("path target package name"),
    );
    assert_eq!(snapshot_inputs(&repo, &[wrong.join("Cargo.toml")])?, before);
    Ok(())
}

#[test]
fn cross_workspace_path_dependency_rejects_wrong_package_version() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    create_fixture(&repo, false)?;
    fs::write(repo.join("Cargo.toml"), WRONG_VERSION_ROOT_MANIFEST)?;
    fs::write(repo.join("Cargo.lock"), WRONG_VERSION_ROOT_LOCK)?;
    let before = snapshot_inputs(&repo, &[])?;
    let rows = freshness_rows(&repo)?;
    assert_row_detail(
        &rows,
        "lock-staleness",
        "velnor-runner-host:dependencies:velnor-actions-contract",
        "fail",
        Some("version \"0.1.68\" != exact requirement =0.1.69"),
    );
    assert_eq!(snapshot_inputs(&repo, &[])?, before);
    Ok(())
}

#[test]
fn cross_workspace_path_dependency_rejects_undiscovered_package() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    create_fixture(&repo, false)?;
    let undiscovered = repo.join("crates/undiscovered-contract");
    fs::create_dir_all(&undiscovered)?;
    fs::write(
        undiscovered.join("Cargo.toml"),
        "[package]\nname = \"velnor-actions-contract\"\nversion = \"0.1.69\"\nedition = \"2024\"\n",
    )?;
    write_host_path(&repo, "../../../undiscovered-contract")?;
    let before = snapshot_inputs(&repo, &[undiscovered.join("Cargo.toml")])?;
    let rows = freshness_rows(&repo)?;
    assert_row_detail(
        &rows,
        "lock-staleness",
        "velnor-runner-host:dependencies:velnor-actions-contract",
        "fail",
        Some("is not a discovered workspace package"),
    );
    assert_eq!(
        snapshot_inputs(&repo, &[undiscovered.join("Cargo.toml")])?,
        before
    );
    Ok(())
}

#[test]
fn cross_workspace_path_dependency_rejects_undeclared_local_lock_entry()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    create_fixture(&repo, false)?;
    let nested_lock = repo.join("crates/velnor-runner/Cargo.lock");
    fs::write(
        &nested_lock,
        format!("{NESTED_LOCK}\n[[package]]\nname = \"orphan-local\"\nversion = \"0.2.0\"\n"),
    )?;
    let before = snapshot_inputs(&repo, &[])?;
    let rows = freshness_rows(&repo)?;
    assert_row_detail(
        &rows,
        "lock-staleness",
        "crates/velnor-runner/(lock-membership)",
        "fail",
        Some("orphan-local"),
    );
    assert_row_detail(
        &rows,
        "lock-staleness",
        "crates/velnor-runner/(lock-graph)",
        "fail",
        Some("unreachable locked package orphan-local"),
    );
    assert_eq!(snapshot_inputs(&repo, &[])?, before);
    Ok(())
}

#[test]
fn freshness_reports_a_missing_workspace_member_once() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    create_fixture(&repo, false)?;
    fs::write(repo.join("Cargo.toml"), MISSING_MEMBER_ROOT_MANIFEST)?;
    let before = snapshot_inputs(&repo, &[])?;
    let rows = freshness_rows(&repo)?;
    assert_row_detail(
        &rows,
        "lock-staleness",
        "/crates/missing-contract",
        "fail",
        Some("workspace member manifest not found"),
    );
    assert_eq!(snapshot_inputs(&repo, &[])?, before);
    Ok(())
}

fn create_fixture(repo: &Path, inherited: bool) -> Result<(), Box<dyn Error>> {
    let contract = repo.join("crates/velnor-actions-contract");
    let nested = repo.join("crates/velnor-runner");
    let host = nested.join("crates/velnor-runner-host");
    fs::create_dir_all(contract.join("src"))?;
    fs::create_dir_all(&host)?;
    fs::create_dir_all(repo.join(".velnor"))?;
    fs::write(repo.join(".velnor/freshness-inventory.json"), "{}\n")?;
    fs::write(repo.join("Cargo.toml"), ROOT_MANIFEST)?;
    fs::write(
        contract.join("Cargo.toml"),
        "[package]\nname = \"velnor-actions-contract\"\nversion.workspace = true\nedition.workspace = true\n",
    )?;
    fs::write(
        nested.join("Cargo.toml"),
        if inherited {
            NESTED_INHERITED_MANIFEST
        } else {
            NESTED_DIRECT_MANIFEST
        },
    )?;
    fs::write(
        host.join("Cargo.toml"),
        if inherited {
            HOST_INHERITED_MANIFEST
        } else {
            HOST_DIRECT_MANIFEST
        },
    )?;
    fs::write(repo.join("Cargo.lock"), ROOT_LOCK)?;
    fs::write(nested.join("Cargo.lock"), NESTED_LOCK)?;
    Ok(())
}

fn write_host_path(repo: &Path, path: &str) -> Result<(), Box<dyn Error>> {
    let host = repo.join("crates/velnor-runner/crates/velnor-runner-host/Cargo.toml");
    fs::write(
        host,
        format!(
            "[package]\nname = \"velnor-runner-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nvelnor-actions-contract = {{ version = \"=0.1.69\", path = \"{path}\" }}\n"
        ),
    )?;
    Ok(())
}

fn freshness_rows(repo: &Path) -> Result<Vec<Value>, Box<dyn Error>> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/check-freshness.sh")
        .canonicalize()?;
    let output = Command::new("/bin/bash")
        .arg(script)
        .arg("--root")
        .arg(repo)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("VELNOR_ACTIONS_BIN", env!("CARGO_BIN_EXE_velnor-actions"))
        .output()?;
    let stdout = String::from_utf8(output.stdout)?;
    let rows = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("row: "))
        .map(serde_json::from_str)
        .collect::<Result<Vec<Value>, _>>()?;
    Ok(rows)
}

fn assert_row(rows: &[Value], check: &str, subject: &str, status: &str) {
    assert_row_detail(rows, check, subject, status, None);
}

fn assert_row_detail(
    rows: &[Value],
    check: &str,
    subject: &str,
    status: &str,
    detail: Option<&str>,
) {
    let matching = rows
        .iter()
        .filter(|row| row["check"] == check && row["subject"] == subject)
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "expected one {check} row for {subject}: {rows:?}"
    );
    assert_eq!(
        matching[0]["status"], status,
        "row: {}; rows: {rows:?}",
        matching[0]
    );
    assert!(matching[0]["detail"].is_string(), "row: {}", matching[0]);
    if let Some(detail) = detail {
        assert!(
            matching[0]["detail"]
                .as_str()
                .is_some_and(|value| value.contains(detail)),
            "row: {}; expected detail containing {detail:?}",
            matching[0]
        );
    }
}

fn snapshot_inputs(repo: &Path, additional: &[PathBuf]) -> Result<FileSnapshot, Box<dyn Error>> {
    let mut paths = [
        "Cargo.toml",
        "Cargo.lock",
        ".velnor/freshness-inventory.json",
        "crates/velnor-actions-contract/Cargo.toml",
        "crates/velnor-runner/Cargo.toml",
        "crates/velnor-runner/Cargo.lock",
        "crates/velnor-runner/crates/velnor-runner-host/Cargo.toml",
    ]
    .into_iter()
    .map(|relative| repo.join(relative))
    .collect::<Vec<_>>();
    paths.extend_from_slice(additional);
    paths
        .into_iter()
        .map(|path| Ok((path.clone(), fs::read(path)?)))
        .collect()
}

const ROOT_MANIFEST: &str = "[workspace]\nmembers = [\"crates/velnor-actions-contract\"]\nexclude = [\"crates/velnor-runner\"]\nresolver = \"3\"\n\n[workspace.package]\nversion = \"0.1.69\"\nedition = \"2024\"\n";
const MISSING_MEMBER_ROOT_MANIFEST: &str = "[workspace]\nmembers = [\"crates/velnor-actions-contract\", \"crates/missing-contract\"]\nexclude = [\"crates/velnor-runner\"]\nresolver = \"3\"\n\n[workspace.package]\nversion = \"0.1.69\"\nedition = \"2024\"\n";
const NESTED_DIRECT_MANIFEST: &str =
    "[workspace]\nmembers = [\"crates/velnor-runner-host\"]\nresolver = \"3\"\n";
const NESTED_INHERITED_MANIFEST: &str = "[workspace]\nmembers = [\"crates/velnor-runner-host\"]\nresolver = \"3\"\n\n[workspace.dependencies]\ncontract-alias = { package = \"velnor-actions-contract\", version = \"=0.1.69\", path = \"../velnor-actions-contract\" }\n";
const HOST_DIRECT_MANIFEST: &str = "[package]\nname = \"velnor-runner-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nvelnor-actions-contract = { version = \"=0.1.69\", path = \"../../../velnor-actions-contract\" }\n";
const HOST_INHERITED_MANIFEST: &str = "[package]\nname = \"velnor-runner-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ncontract-alias = { workspace = true }\n";
const ROOT_LOCK: &str =
    "version = 4\n\n[[package]]\nname = \"velnor-actions-contract\"\nversion = \"0.1.69\"\n";
const NESTED_LOCK: &str = "version = 4\n\n[[package]]\nname = \"velnor-runner-host\"\nversion = \"0.1.0\"\ndependencies = [\n \"velnor-actions-contract 0.1.69\",\n]\n\n[[package]]\nname = \"velnor-actions-contract\"\nversion = \"0.1.69\"\n";
const WRONG_NAME_ROOT_MANIFEST: &str = "[workspace]\nmembers = [\"crates/wrong-contract\"]\nexclude = [\"crates/velnor-runner\"]\nresolver = \"3\"\n\n[workspace.package]\nversion = \"0.1.69\"\nedition = \"2024\"\n";
const WRONG_NAME_ROOT_LOCK: &str =
    "version = 4\n\n[[package]]\nname = \"different-contract\"\nversion = \"0.1.69\"\n";
const WRONG_VERSION_ROOT_MANIFEST: &str = "[workspace]\nmembers = [\"crates/velnor-actions-contract\"]\nexclude = [\"crates/velnor-runner\"]\nresolver = \"3\"\n\n[workspace.package]\nversion = \"0.1.68\"\nedition = \"2024\"\n";
const WRONG_VERSION_ROOT_LOCK: &str =
    "version = 4\n\n[[package]]\nname = \"velnor-actions-contract\"\nversion = \"0.1.68\"\n";
