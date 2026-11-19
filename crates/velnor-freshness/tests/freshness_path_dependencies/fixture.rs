// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path};

use super::{FileSnapshot, assert_row, assert_row_detail, freshness_rows, snapshot_inputs};

pub(super) fn recursive_cross_workspace_target_path_dependencies_require_exact_lock_membership()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    create_recursive_fixture(&repo, NESTED_RECURSIVE_LOCK)?;
    assert_recursive_lock_membership_passes(&repo)?;
    assert_recursive_missing_leaf_fails_membership(&repo)?;
    assert_recursive_stale_root_lock_fails_membership(&repo)?;
    assert_recursive_reachable_extra_fails_membership(&repo)?;
    Ok(())
}

fn assert_recursive_lock_membership_passes(repo: &Path) -> Result<(), Box<dyn Error>> {
    let before = snapshot_recursive_inputs(repo)?;
    let rows = freshness_rows(repo)?;
    assert_row(&rows, "lock-staleness", "(lock-membership)", "pass");
    assert_row(&rows, "lock-staleness", "(lock-graph)", "pass");
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
    assert_eq!(snapshot_recursive_inputs(repo)?, before);
    Ok(())
}

fn assert_recursive_missing_leaf_fails_membership(repo: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        repo.join("crates/velnor-runner/Cargo.lock"),
        NESTED_RECURSIVE_LOCK_MISSING_LEAF,
    )?;
    let before = snapshot_recursive_inputs(repo)?;
    let rows = freshness_rows(repo)?;
    assert_row_detail(
        &rows,
        "lock-staleness",
        "crates/velnor-runner/(lock-membership)",
        "fail",
        Some("runner-leaf"),
    );
    assert_eq!(snapshot_recursive_inputs(repo)?, before);
    Ok(())
}

fn assert_recursive_stale_root_lock_fails_membership(repo: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        repo.join("crates/velnor-runner/crates/runner-leaf/Cargo.toml"),
        RECURSIVE_LEAF_MANIFEST_V2,
    )?;
    fs::write(
        repo.join("crates/velnor-runner/Cargo.lock"),
        NESTED_RECURSIVE_LOCK_V2,
    )?;
    let before = snapshot_recursive_inputs(repo)?;
    let rows = freshness_rows(repo)?;
    assert_row_detail(
        &rows,
        "lock-staleness",
        "(lock-membership)",
        "fail",
        Some("runner-leaf"),
    );
    assert_row(&rows, "lock-staleness", "(lock-graph)", "pass");
    assert_eq!(snapshot_recursive_inputs(repo)?, before);

    fs::write(repo.join("Cargo.lock"), RECURSIVE_ROOT_LOCK_V2)?;
    let before = snapshot_recursive_inputs(repo)?;
    let rows = freshness_rows(repo)?;
    assert_row(&rows, "lock-staleness", "(lock-membership)", "pass");
    assert_row(&rows, "lock-staleness", "(lock-graph)", "pass");
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
    assert_eq!(snapshot_recursive_inputs(repo)?, before);
    Ok(())
}

fn assert_recursive_reachable_extra_fails_membership(repo: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        repo.join("crates/velnor-runner/crates/runner-leaf/Cargo.toml"),
        RECURSIVE_LEAF_MANIFEST,
    )?;
    fs::write(repo.join("Cargo.lock"), RECURSIVE_ROOT_LOCK)?;
    fs::write(
        repo.join("crates/velnor-runner/Cargo.lock"),
        NESTED_RECURSIVE_LOCK_WITH_REACHABLE_EXTRA,
    )?;
    let before = snapshot_recursive_inputs(repo)?;
    let rows = freshness_rows(repo)?;
    assert_row_detail(
        &rows,
        "lock-staleness",
        "crates/velnor-runner/(lock-membership)",
        "fail",
        Some("undeclared-local"),
    );
    assert_row(
        &rows,
        "lock-staleness",
        "crates/velnor-runner/(lock-graph)",
        "pass",
    );
    assert_eq!(snapshot_recursive_inputs(repo)?, before);
    Ok(())
}

pub(super) fn root_lock_membership_authorizes_reachable_package_versions_by_manifest_identity()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    create_duplicate_version_fixture(&repo, DUPLICATE_VERSION_ROOT_LOCK)?;

    let before = snapshot_duplicate_version_inputs(&repo)?;
    let rows = freshness_rows(&repo)?;
    assert_row(&rows, "lock-staleness", "(lock-membership)", "pass");
    assert_row(&rows, "lock-staleness", "(lock-graph)", "pass");
    assert_eq!(snapshot_duplicate_version_inputs(&repo)?, before);

    fs::write(
        repo.join("Cargo.lock"),
        DUPLICATE_VERSION_ROOT_LOCK_WITH_REACHABLE_STALE_VERSION,
    )?;
    let before = snapshot_duplicate_version_inputs(&repo)?;
    let rows = freshness_rows(&repo)?;
    assert_row_detail(
        &rows,
        "lock-staleness",
        "(lock-membership)",
        "fail",
        Some("runner-leaf\", \"0.4.0\""),
    );
    assert_row(&rows, "lock-staleness", "(lock-graph)", "pass");
    assert_eq!(snapshot_duplicate_version_inputs(&repo)?, before);
    Ok(())
}

fn create_duplicate_version_fixture(repo: &Path, root_lock: &str) -> Result<(), Box<dyn Error>> {
    let app = repo.join("crates/root-app");
    let first = repo.join("crates/velnor-runner");
    let second = repo.join("crates/alternate-runner");
    fs::create_dir_all(&app)?;
    for path in [
        first.join("crates/velnor-runner-host"),
        first.join("crates/runner-leaf"),
        second.join("crates/runner-host"),
        second.join("crates/runner-leaf"),
        repo.join("crates/velnor-actions-contract"),
        repo.join(".velnor"),
    ] {
        fs::create_dir_all(path)?;
    }
    fs::write(repo.join(".velnor/freshness-inventory.json"), "{}\n")?;
    fs::write(repo.join("Cargo.toml"), DUPLICATE_VERSION_ROOT_MANIFEST)?;
    fs::write(app.join("Cargo.toml"), DUPLICATE_VERSION_APP_MANIFEST)?;
    fs::write(
        repo.join("crates/velnor-actions-contract/Cargo.toml"),
        "[package]\nname = \"velnor-actions-contract\"\nversion.workspace = true\nedition.workspace = true\n",
    )?;
    fs::write(first.join("Cargo.toml"), DUPLICATE_VERSION_FIRST_WORKSPACE)?;
    fs::write(
        first.join("crates/velnor-runner-host/Cargo.toml"),
        DUPLICATE_VERSION_FIRST_HOST,
    )?;
    fs::write(
        first.join("crates/runner-leaf/Cargo.toml"),
        DUPLICATE_VERSION_FIRST_LEAF,
    )?;
    fs::write(
        second.join("Cargo.toml"),
        DUPLICATE_VERSION_SECOND_WORKSPACE,
    )?;
    fs::write(
        second.join("crates/runner-host/Cargo.toml"),
        DUPLICATE_VERSION_SECOND_HOST,
    )?;
    fs::write(
        second.join("crates/runner-leaf/Cargo.toml"),
        DUPLICATE_VERSION_SECOND_LEAF,
    )?;
    fs::write(repo.join("Cargo.lock"), root_lock)?;
    fs::write(first.join("Cargo.lock"), DUPLICATE_VERSION_FIRST_LOCK)?;
    fs::write(second.join("Cargo.lock"), DUPLICATE_VERSION_SECOND_LOCK)?;
    Ok(())
}

fn snapshot_duplicate_version_inputs(repo: &Path) -> Result<FileSnapshot, Box<dyn Error>> {
    snapshot_inputs(
        repo,
        &[
            repo.join("crates/root-app/Cargo.toml"),
            repo.join("crates/alternate-runner/Cargo.toml"),
            repo.join("crates/alternate-runner/Cargo.lock"),
            repo.join("crates/alternate-runner/crates/runner-host/Cargo.toml"),
            repo.join("crates/alternate-runner/crates/runner-leaf/Cargo.toml"),
            repo.join("crates/velnor-runner/crates/velnor-runner-host/Cargo.toml"),
            repo.join("crates/velnor-runner/crates/runner-leaf/Cargo.toml"),
        ],
    )
}

fn create_recursive_fixture(repo: &Path, nested_lock: &str) -> Result<(), Box<dyn Error>> {
    let app = repo.join("crates/root-app");
    let nested = repo.join("crates/velnor-runner");
    let host = nested.join("crates/velnor-runner-host");
    let leaf = nested.join("crates/runner-leaf");
    let contract = repo.join("crates/velnor-actions-contract");
    for path in [&app, &host, &leaf, &contract] {
        fs::create_dir_all(path)?;
    }
    fs::create_dir_all(repo.join(".velnor"))?;
    fs::write(repo.join(".velnor/freshness-inventory.json"), "{}\n")?;
    fs::write(repo.join("Cargo.toml"), RECURSIVE_ROOT_MANIFEST)?;
    fs::write(app.join("Cargo.toml"), RECURSIVE_APP_MANIFEST)?;
    fs::write(
        contract.join("Cargo.toml"),
        "[package]\nname = \"velnor-actions-contract\"\nversion.workspace = true\nedition.workspace = true\n",
    )?;
    fs::write(nested.join("Cargo.toml"), RECURSIVE_NESTED_MANIFEST)?;
    fs::write(host.join("Cargo.toml"), RECURSIVE_HOST_MANIFEST)?;
    fs::write(leaf.join("Cargo.toml"), RECURSIVE_LEAF_MANIFEST)?;
    fs::write(repo.join("Cargo.lock"), RECURSIVE_ROOT_LOCK)?;
    fs::write(nested.join("Cargo.lock"), nested_lock)?;
    Ok(())
}

fn snapshot_recursive_inputs(repo: &Path) -> Result<FileSnapshot, Box<dyn Error>> {
    snapshot_inputs(
        repo,
        &[
            repo.join("crates/root-app/Cargo.toml"),
            repo.join("crates/velnor-runner/crates/velnor-runner-host/Cargo.toml"),
            repo.join("crates/velnor-runner/crates/runner-leaf/Cargo.toml"),
        ],
    )
}

const RECURSIVE_ROOT_MANIFEST: &str = "[workspace]\nmembers = [\"crates/root-app\", \"crates/velnor-actions-contract\"]\nexclude = [\"crates/velnor-runner\"]\nresolver = \"3\"\n\n[workspace.package]\nversion = \"0.1.69\"\nedition = \"2024\"\n";
const RECURSIVE_APP_MANIFEST: &str = "[package]\nname = \"runner-app\"\nversion.workspace = true\nedition.workspace = true\n\n[target.'cfg(unix)'.dependencies]\nhost-alias = { package = \"velnor-runner-host\", path = \"../velnor-runner/crates/velnor-runner-host\" }\n";
const RECURSIVE_NESTED_MANIFEST: &str = "[workspace]\nmembers = [\"crates/velnor-runner-host\", \"crates/runner-leaf\"]\nresolver = \"3\"\n\n[workspace.dependencies]\nleaf-alias = { package = \"runner-leaf\", version = \"=0.3.0\", path = \"crates/runner-leaf\" }\n";
const RECURSIVE_HOST_MANIFEST: &str = "[package]\nname = \"velnor-runner-host\"\nversion = \"0.2.0\"\nedition = \"2024\"\n\n[dependencies]\nleaf-alias = { workspace = true }\n";
const RECURSIVE_LEAF_MANIFEST: &str =
    "[package]\nname = \"runner-leaf\"\nversion = \"0.3.0\"\nedition = \"2024\"\n";
const RECURSIVE_LEAF_MANIFEST_V2: &str =
    "[package]\nname = \"runner-leaf\"\nversion = \"0.3.1\"\nedition = \"2024\"\n";
const RECURSIVE_ROOT_LOCK: &str = "version = 4\n\n[[package]]\nname = \"runner-app\"\nversion = \"0.1.69\"\ndependencies = [\n \"velnor-runner-host 0.2.0\",\n]\n\n[[package]]\nname = \"velnor-actions-contract\"\nversion = \"0.1.69\"\n\n[[package]]\nname = \"velnor-runner-host\"\nversion = \"0.2.0\"\ndependencies = [\n \"runner-leaf 0.3.0\",\n]\n\n[[package]]\nname = \"runner-leaf\"\nversion = \"0.3.0\"\n";
const NESTED_RECURSIVE_LOCK: &str = "version = 4\n\n[[package]]\nname = \"velnor-runner-host\"\nversion = \"0.2.0\"\ndependencies = [\n \"runner-leaf 0.3.0\",\n]\n\n[[package]]\nname = \"runner-leaf\"\nversion = \"0.3.0\"\n";
const NESTED_RECURSIVE_LOCK_MISSING_LEAF: &str = "version = 4\n\n[[package]]\nname = \"velnor-runner-host\"\nversion = \"0.2.0\"\ndependencies = [\n \"runner-leaf 0.3.0\",\n]\n";
const NESTED_RECURSIVE_LOCK_V2: &str = "version = 4\n\n[[package]]\nname = \"velnor-runner-host\"\nversion = \"0.2.0\"\ndependencies = [\n \"runner-leaf 0.3.1\",\n]\n\n[[package]]\nname = \"runner-leaf\"\nversion = \"0.3.1\"\n";
const RECURSIVE_ROOT_LOCK_V2: &str = "version = 4\n\n[[package]]\nname = \"runner-app\"\nversion = \"0.1.69\"\ndependencies = [\n \"velnor-runner-host 0.2.0\",\n]\n\n[[package]]\nname = \"velnor-actions-contract\"\nversion = \"0.1.69\"\n\n[[package]]\nname = \"velnor-runner-host\"\nversion = \"0.2.0\"\ndependencies = [\n \"runner-leaf 0.3.1\",\n]\n\n[[package]]\nname = \"runner-leaf\"\nversion = \"0.3.1\"\n";
const NESTED_RECURSIVE_LOCK_WITH_REACHABLE_EXTRA: &str = "version = 4\n\n[[package]]\nname = \"velnor-runner-host\"\nversion = \"0.2.0\"\ndependencies = [\n \"runner-leaf 0.3.0\",\n]\n\n[[package]]\nname = \"runner-leaf\"\nversion = \"0.3.0\"\ndependencies = [\n \"undeclared-local 0.4.0\",\n]\n\n[[package]]\nname = \"undeclared-local\"\nversion = \"0.4.0\"\n";
const DUPLICATE_VERSION_ROOT_MANIFEST: &str = "[workspace]\nmembers = [\"crates/root-app\", \"crates/velnor-actions-contract\"]\nexclude = [\"crates/velnor-runner\", \"crates/alternate-runner\"]\nresolver = \"3\"\n\n[workspace.package]\nversion = \"0.1.69\"\nedition = \"2024\"\n";
const DUPLICATE_VERSION_APP_MANIFEST: &str = "[package]\nname = \"runner-app\"\nversion.workspace = true\nedition.workspace = true\n\n[target.'cfg(unix)'.dependencies]\nfirst-host = { package = \"runner-host-one\", path = \"../velnor-runner/crates/velnor-runner-host\" }\nsecond-host = { package = \"runner-host-two\", path = \"../alternate-runner/crates/runner-host\" }\n";
const DUPLICATE_VERSION_FIRST_WORKSPACE: &str = "[workspace]\nmembers = [\"crates/velnor-runner-host\", \"crates/runner-leaf\"]\nresolver = \"3\"\n\n[workspace.dependencies]\nleaf-alias = { package = \"runner-leaf\", version = \"=0.3.0\", path = \"crates/runner-leaf\" }\n";
const DUPLICATE_VERSION_SECOND_WORKSPACE: &str = "[workspace]\nmembers = [\"crates/runner-host\", \"crates/runner-leaf\"]\nresolver = \"3\"\n\n[workspace.dependencies]\nleaf-alias = { package = \"runner-leaf\", version = \"=0.5.0\", path = \"crates/runner-leaf\" }\n";
const DUPLICATE_VERSION_FIRST_HOST: &str = "[package]\nname = \"runner-host-one\"\nversion = \"0.2.0\"\nedition = \"2024\"\n\n[dependencies]\nleaf-alias = { workspace = true }\n";
const DUPLICATE_VERSION_SECOND_HOST: &str = "[package]\nname = \"runner-host-two\"\nversion = \"0.2.0\"\nedition = \"2024\"\n\n[dependencies]\nleaf-alias = { workspace = true }\n";
const DUPLICATE_VERSION_FIRST_LEAF: &str =
    "[package]\nname = \"runner-leaf\"\nversion = \"0.3.0\"\nedition = \"2024\"\n";
const DUPLICATE_VERSION_SECOND_LEAF: &str =
    "[package]\nname = \"runner-leaf\"\nversion = \"0.5.0\"\nedition = \"2024\"\n";
const DUPLICATE_VERSION_FIRST_LOCK: &str = "version = 4\n\n[[package]]\nname = \"runner-host-one\"\nversion = \"0.2.0\"\ndependencies = [\n \"runner-leaf 0.3.0\",\n]\n\n[[package]]\nname = \"runner-leaf\"\nversion = \"0.3.0\"\n";
const DUPLICATE_VERSION_SECOND_LOCK: &str = "version = 4\n\n[[package]]\nname = \"runner-host-two\"\nversion = \"0.2.0\"\ndependencies = [\n \"runner-leaf 0.5.0\",\n]\n\n[[package]]\nname = \"runner-leaf\"\nversion = \"0.5.0\"\n";
const DUPLICATE_VERSION_ROOT_LOCK: &str = "version = 4\n\n[[package]]\nname = \"runner-app\"\nversion = \"0.1.69\"\ndependencies = [\n \"runner-host-one 0.2.0\",\n \"runner-host-two 0.2.0\",\n]\n\n[[package]]\nname = \"velnor-actions-contract\"\nversion = \"0.1.69\"\n\n[[package]]\nname = \"runner-host-one\"\nversion = \"0.2.0\"\ndependencies = [\n \"runner-leaf 0.3.0\",\n]\n\n[[package]]\nname = \"runner-host-two\"\nversion = \"0.2.0\"\ndependencies = [\n \"runner-leaf 0.5.0\",\n]\n\n[[package]]\nname = \"runner-leaf\"\nversion = \"0.3.0\"\n\n[[package]]\nname = \"runner-leaf\"\nversion = \"0.5.0\"\n";
const DUPLICATE_VERSION_ROOT_LOCK_WITH_REACHABLE_STALE_VERSION: &str = "version = 4\n\n[[package]]\nname = \"runner-app\"\nversion = \"0.1.69\"\ndependencies = [\n \"runner-host-one 0.2.0\",\n \"runner-host-two 0.2.0\",\n]\n\n[[package]]\nname = \"velnor-actions-contract\"\nversion = \"0.1.69\"\n\n[[package]]\nname = \"runner-host-one\"\nversion = \"0.2.0\"\ndependencies = [\n \"runner-leaf 0.3.0\",\n \"runner-leaf 0.4.0\",\n]\n\n[[package]]\nname = \"runner-host-two\"\nversion = \"0.2.0\"\ndependencies = [\n \"runner-leaf 0.5.0\",\n]\n\n[[package]]\nname = \"runner-leaf\"\nversion = \"0.3.0\"\n\n[[package]]\nname = \"runner-leaf\"\nversion = \"0.4.0\"\n\n[[package]]\nname = \"runner-leaf\"\nversion = \"0.5.0\"\n";
