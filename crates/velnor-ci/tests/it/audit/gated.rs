// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::{audit, own_units_in, unit, workspace_below_the_root};
use crate::io::Repo;

const DEBUG: &str = "rust/target/debug";
const TRIPLE: &str = "rust/target/x86_64-unknown-linux-gnu/debug";
const INACTIVE: &str = "cfg(target_os = \"no-such-os\")";
const ACTIVE: &str = "cfg(not(target_os = \"no-such-os\"))";

fn package(name: &str, extra: &str) -> String {
    format!("[package]\nname = \"{name}\"\nedition = \"2024\"\n\n{extra}")
}

fn finish(repo: &Repo) {
    finish_for(repo, TRIPLE);
}

fn finish_for(repo: &Repo, triple: &str) {
    repo.git(&["add", "--all"]);
    own_units_in(repo, DEBUG, triple, &[]);
}

fn direct(platform: &str) -> Repo {
    let repo = workspace_below_the_root();
    unit(
        &repo,
        "rust/app/Cargo.toml",
        &package(
            "app",
            &format!("[target.'{platform}'.build-dependencies]\nlib = {{ path = \"../lib\" }}\n"),
        ),
    );
    finish(&repo);
    repo
}

fn transitive(platform: &str) -> Repo {
    let repo = workspace_below_the_root();
    unit(
        &repo,
        "rust/app/Cargo.toml",
        &package(
            "app",
            "[build-dependencies]\nmacros = { path = \"../macros\" }\n",
        ),
    );
    unit(
        &repo,
        "rust/macros/Cargo.toml",
        &package(
            "macros",
            &format!(
                "[lib]\nproc-macro = true\n\n[target.'{platform}'.dependencies]\nlib = {{ path = \"../lib\" }}\n"
            ),
        ),
    );
    finish(&repo);
    repo
}

fn unowned(platform: &str) -> Repo {
    let repo = workspace_below_the_root();
    unit(
        &repo,
        "rust/app/Cargo.toml",
        &package("app", "[dependencies]\nmid = { path = \"../../mid\" }\n"),
    );
    unit(
        &repo,
        "mid/Cargo.toml",
        &package(
            "mid",
            &format!(
                "[target.'{platform}'.build-dependencies]\nlib = {{ path = \"../rust/lib\" }}\n"
            ),
        ),
    );
    unit(&repo, "mid/src/lib.rs", "");
    unit(&repo, ".velnor/rust-ci.toml", "exclude = [\"mid\"]\n");
    finish(&repo);
    built_mid(&repo);
    repo
}

fn built_mid(repo: &Repo) {
    unit(
        repo,
        &format!("{TRIPLE}/deps/mid-f0000000000000aa.d"),
        "out.d: ../mid/src/lib.rs\n",
    );
    unit(
        repo,
        &format!("{TRIPLE}/.fingerprint/mid-f0000000000000aa/lib-mid"),
        "",
    );
}

fn behind_an_inactive_edge() -> Repo {
    let repo = workspace_below_the_root();
    unit(
        &repo,
        "rust/app/Cargo.toml",
        &package(
            "app",
            &format!("[target.'{INACTIVE}'.dependencies]\nmid = {{ path = \"../../mid\" }}\n"),
        ),
    );
    unit(
        &repo,
        "mid/Cargo.toml",
        &package(
            "mid",
            "[build-dependencies]\nlib = { path = \"../rust/lib\" }\n",
        ),
    );
    unit(&repo, "mid/src/lib.rs", "");
    unit(&repo, ".velnor/rust-ci.toml", "exclude = [\"mid\"]\n");
    finish(&repo);
    repo
}

fn behind_a_target_edge(triple: &str) -> Repo {
    let repo = workspace_below_the_root();
    unit(
        &repo,
        "rust/app/Cargo.toml",
        &package(
            "app",
            "[target.'cfg(windows)'.dependencies]\nmid = { path = \"../../mid\" }\n",
        ),
    );
    unit(
        &repo,
        "mid/Cargo.toml",
        &package(
            "mid",
            "[build-dependencies]\nlib = { path = \"../rust/lib\" }\n",
        ),
    );
    unit(&repo, "mid/src/lib.rs", "");
    unit(&repo, ".velnor/rust-ci.toml", "exclude = [\"mid\"]\n");
    finish_for(&repo, triple);
    repo
}

fn passes(repo: &Repo) {
    let output = audit(repo, &["--write"]);
    assert!(output.status.success(), "{output:?}");
}

fn needs_the_host_units_of_lib(repo: &Repo) {
    let output = audit(repo, &["--write"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("for lib (rust/lib/Cargo.toml)"),
        "{output:?}"
    );
}

#[test]
fn a_build_dependency_under_an_inactive_cfg_needs_no_host_units() {
    passes(&direct(INACTIVE));
}

#[test]
fn a_build_dependency_under_an_inactive_triple_needs_no_host_units() {
    passes(&direct("x86_64-no-such-vendor-none"));
}

#[test]
fn a_build_dependency_under_a_cfg_that_rustflags_can_set_still_needs_its_host_units() {
    needs_the_host_units_of_lib(&direct("cfg(tokio_unstable)"));
}

#[test]
fn a_dependency_of_a_host_package_under_an_inactive_cfg_needs_no_host_units() {
    passes(&transitive(INACTIVE));
}

#[test]
fn a_dependency_of_a_host_package_under_an_active_cfg_needs_its_host_units() {
    needs_the_host_units_of_lib(&transitive(ACTIVE));
}

#[test]
fn a_package_outside_the_workspaces_with_an_inactive_build_dependency_needs_no_host_units() {
    passes(&unowned(INACTIVE));
}

#[test]
fn a_package_outside_the_workspaces_reached_only_through_an_inactive_edge_hosts_nothing() {
    passes(&behind_an_inactive_edge());
}

#[test]
fn an_ordinary_edge_that_holds_for_the_build_target_not_the_host_needs_its_host_units() {
    needs_the_host_units_of_lib(&behind_a_target_edge(
        "rust/target/x86_64-pc-windows-gnu/debug",
    ));
}

#[test]
fn a_path_dependency_behind_a_target_edge_is_required_for_the_built_target() {
    let output = audit(
        &behind_a_target_edge("rust/target/x86_64-pc-windows-gnu/debug"),
        &["--write"],
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("path dependency mid (mid/Cargo.toml"),
        "{output:?}"
    );
}

#[test]
fn an_ordinary_edge_that_holds_for_no_built_platform_needs_no_host_units() {
    passes(&behind_a_target_edge(TRIPLE));
}

#[test]
fn a_built_target_rustc_cannot_name_keeps_its_ordinary_edges_active() {
    needs_the_host_units_of_lib(&behind_a_target_edge("rust/target/custom-spec/debug"));
}

#[test]
fn a_target_folder_with_no_current_unit_adds_no_platform() {
    let repo = behind_a_target_edge(TRIPLE);
    unit(&repo, "rust/target/x86_64-pc-windows-gnu/CACHEDIR.TAG", "");
    passes(&repo);
}

#[test]
fn the_rustc_the_environment_selects_is_the_one_queried() {
    let repo = direct(INACTIVE);
    let shims = repo.root().join("env-shims");
    fs::create_dir_all(&shims).unwrap_or_else(|e| unreachable!("{e}"));
    let rustc = shims.join("configured-rustc");
    fs::write(
        &rustc,
        "#!/bin/sh\necho 'configured compiler used' >&2\nexit 1\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&rustc, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let output = super::velnor_ci()
        .current_dir(repo.root())
        .env("RUSTC", &rustc)
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("configured compiler used"),
        "{output:?}"
    );
}

#[test]
fn a_package_outside_the_workspaces_with_an_active_build_dependency_needs_its_host_units() {
    needs_the_host_units_of_lib(&unowned(ACTIVE));
}

fn audit_with_rustc(repo: &Repo, script: &str) -> std::process::Output {
    let shims = repo.root().join("shims");
    fs::create_dir_all(&shims).unwrap_or_else(|e| unreachable!("{e}"));
    let rustc = shims.join("rustc");
    fs::write(&rustc, script).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&rustc, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let path = format!(
        "{}:{}",
        shims.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    super::velnor_ci()
        .current_dir(repo.root())
        .env("PATH", path)
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_rustc_that_prints_no_host_triple_fails_the_audit() {
    let output = audit_with_rustc(&direct(INACTIVE), "#!/bin/sh\necho 'release: 1.99.1'\n");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no host triple"),
        "{output:?}"
    );
}

#[test]
fn rustc_is_queried_from_the_workspace_root_so_its_own_toolchain_answers() {
    let script = "#!/bin/sh\ncase \"$PWD\" in\n*/rust) ;;\n*) echo 'toolchain not installed here' >&2; exit 1;;\nesac\ncase \"$1\" in\n-vV) echo 'host: x86_64-unknown-linux-gnu';;\n*) printf 'unix\\ntarget_os=\"linux\"\\n';;\nesac\n";
    let output = audit_with_rustc(&direct(INACTIVE), script);
    assert!(output.status.success(), "{output:?}");
}
