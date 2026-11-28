// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::{audit, own_units, unit, velnor_ci, workspace_below_the_root};
use crate::io::Repo;

const TARGET: &str = "rust/target/debug";
const APP_DEPS: &str = "rust/target/debug/deps/app-f000000000000001.d";
const LIB_DEPS: &str = "rust/target/debug/deps/lib-f000000000000005.d";
const LIB_OUTPUT: &str = "rust/target/debug/build/lib-f000000000000008";
const APP_OUTPUT: &str = "rust/target/debug/build/app-f000000000000004/output";
const SOMEWHERE: &str = "/velnor-test-elsewhere";

struct Fixture {
    repo: Repo,
    alias: Repo,
}

impl std::ops::Deref for Fixture {
    type Target = Repo;

    fn deref(&self) -> &Repo {
        &self.repo
    }
}

impl Fixture {
    fn lib_output(&self, leaf: &str) -> String {
        format!("{}/repo/{LIB_OUTPUT}/{leaf}", self.alias.root().display())
    }
}

fn units() -> Fixture {
    let repo = workspace_below_the_root();
    own_units(&repo, TARGET, &[]);
    unit(&repo, APP_OUTPUT, "cargo:rerun-if-changed=build.rs\n");
    super::inventory::build(&repo, APP_OUTPUT, None);
    unit(
        &repo,
        &format!("{TARGET}/.fingerprint/lib-f000000000000005/lib-lib.json"),
        "{\"deps\":[[0,\"build_script_build\",false,1]]}",
    );
    unit(
        &repo,
        &format!("{TARGET}/.fingerprint/lib-f000000000000008/run-build-script-build-script-build"),
        "0100000000000000",
    );
    let alias = Repo::empty("alias");
    std::os::unix::fs::symlink(repo.root(), alias.root().join("repo"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    Fixture { repo, alias }
}

fn recorded(repo: &Repo, root_output: &str) {
    unit(repo, &format!("{LIB_OUTPUT}/root-output"), root_output);
}

fn fake_compiler(repo: &Repo, prints: &str) -> std::path::PathBuf {
    let compiler = repo.root().join("compiler.sh");
    fs::write(&compiler, format!("#!/bin/sh\n{prints}\n")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(
        &compiler,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    compiler
}

fn lock(repo: &Repo) -> Option<String> {
    fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock")).ok()
}

fn written(repo: &Repo) -> String {
    let result = velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_HOME", format!("{SOMEWHERE}/cargo-home"))
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(result.status.success(), "{result:?}");
    lock(repo).unwrap_or_default()
}

#[test]
fn a_dep_info_read_outside_the_repository_in_no_output_directory_refuses_the_audit() {
    let repo = units();
    unit(
        &repo,
        APP_DEPS,
        &format!("out.d: app/src/lib.rs {SOMEWHERE}/docs/x.md\n"),
    );
    let refused = audit(&repo, &["--write"]);
    let message = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(1), "{message}");
    assert!(message.contains("outside the repository"), "{message}");
    assert!(
        message.contains(&format!("{SOMEWHERE}/docs/x.md")),
        "{message}"
    );
    assert!(lock(&repo).is_none());
}

#[test]
fn a_dep_info_read_under_the_own_output_directory_of_an_undeclared_build_script_credits_a_dot_line()
{
    let repo = units();
    let out = repo.lib_output("out");
    recorded(&repo, &format!("{out}\n"));
    unit(
        &repo,
        LIB_DEPS,
        &format!("out.d: lib/src/lib.rs {out}/o.txt\n"),
    );
    assert_eq!(written(&repo), ".\tlib\n");
}

#[test]
fn a_dep_info_read_under_the_cargo_home_registry_is_audited() {
    let repo = units();
    unit(
        &repo,
        APP_DEPS,
        &format!(
            "out.d: app/src/lib.rs {SOMEWHERE}/cargo-home/registry/src/index/dep-1.0.0/src/lib.rs\n"
        ),
    );
    assert!(!written(&repo).contains("dep-1.0.0"));
}

#[test]
fn an_undeclared_build_script_without_a_root_output_credits_a_dot_line_to_its_package() {
    let repo = units();
    fs::remove_file(repo.root().join(format!("{LIB_OUTPUT}/root-output")))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(written(&repo), ".\tlib\n");
}

#[test]
fn a_package_reading_the_output_directory_of_another_package_refuses_the_audit() {
    let repo = units();
    let out = repo.lib_output("out");
    recorded(&repo, &format!("{out}\n"));
    unit(
        &repo,
        APP_DEPS,
        &format!("out.d: app/src/lib.rs {out}/o.txt\n"),
    );
    let refused = audit(&repo, &["--write"]);
    let message = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(1), "{message}");
    assert!(message.contains("outside the repository"), "{message}");
    assert!(lock(&repo).is_none());
}

#[test]
fn a_root_output_outside_the_unit_own_output_directory_is_missing_and_never_disables_the_refusal() {
    let own = |repo: &Fixture| repo.lib_output("other");
    let recordings: [&dyn Fn(&Fixture) -> String; 6] = [
        &|_| String::new(),
        &|_| "  \n".to_owned(),
        &|_| "relative/out\n".to_owned(),
        &|_| "/\n".to_owned(),
        &|_| format!("{SOMEWHERE}/out/lib\n"),
        &own,
    ];
    for recording in recordings {
        let repo = units();
        let text = recording(&repo);
        recorded(&repo, &text);
        unit(
            &repo,
            APP_DEPS,
            &format!("out.d: app/src/lib.rs {SOMEWHERE}/docs/x.md\n"),
        );
        let refused = audit(&repo, &["--write"]);
        let message = String::from_utf8_lossy(&refused.stderr);
        assert_eq!(refused.status.code(), Some(1), "{text:?} {message}");
        assert!(message.contains("outside the repository"), "{message}");
        unit(&repo, APP_DEPS, "out.d: app/src/lib.rs\n");
        assert_eq!(written(&repo), ".\tlib\n", "{text:?}");
    }
}

#[test]
fn a_root_output_of_the_filesystem_root_refuses_an_outside_read() {
    let repo = units();
    recorded(&repo, "/\n");
    unit(
        &repo,
        LIB_DEPS,
        &format!("out.d: lib/src/lib.rs {SOMEWHERE}/docs/x.md\n"),
    );
    let refused = audit(&repo, &["--write"]);
    let message = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(1), "{message}");
    assert!(lock(&repo).is_none());
}

#[test]
fn a_sysroot_the_compiler_does_not_print_never_makes_a_read_explained() {
    for prints in ["", "echo", "echo relative/sysroot", "echo /"] {
        let repo = units();
        let compiler = fake_compiler(&repo, prints);
        unit(
            &repo,
            APP_DEPS,
            &format!("out.d: app/src/lib.rs {SOMEWHERE}/docs/x.md\n"),
        );
        let refused = velnor_ci()
            .current_dir(repo.root())
            .env("CARGO_HOME", format!("{SOMEWHERE}/cargo-home"))
            .env("RUSTC", &compiler)
            .args(["inputs", "audit", "--write"])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        let message = String::from_utf8_lossy(&refused.stderr);
        assert_eq!(refused.status.code(), Some(1), "{prints:?} {message}");
        assert!(message.contains("outside the repository"), "{message}");
        assert!(lock(&repo).is_none());
    }
}

#[test]
fn a_root_output_above_its_bound_refuses_the_audit() {
    let repo = units();
    recorded(&repo, &format!("{SOMEWHERE}/{}\n", "x".repeat(64 << 10)));
    let refused = audit(&repo, &["--write"]);
    let message = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(1), "{message}");
    assert!(
        message.contains("root-output file exceeds 8192 bytes"),
        "{message}"
    );
    assert!(lock(&repo).is_none());
}

#[test]
fn the_read_only_roots_are_computed_once_per_audit() {
    let repo = units();
    let log = repo.root().join("compiler.log");
    let compiler = fake_compiler(
        &repo,
        &format!("echo run >> {}\necho /velnor-test-sysroot", log.display()),
    );
    for (file, source) in [(APP_DEPS, "app"), (LIB_DEPS, "lib")] {
        unit(
            &repo,
            file,
            &format!(
                "out.d: {source}/src/lib.rs {SOMEWHERE}/cargo-home/registry/a/{source}.rs {SOMEWHERE}/cargo-home/registry/b/{source}.rs\n"
            ),
        );
    }
    let result = velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_HOME", format!("{SOMEWHERE}/cargo-home"))
        .env("RUSTC", &compiler)
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(result.status.success(), "{result:?}");
    let runs = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(runs.lines().count(), 1, "{runs}");
}

#[test]
fn a_sysroot_naming_a_parent_directory_never_makes_a_read_explained() {
    let repo = units();
    let compiler = fake_compiler(&repo, "echo ..");
    let parent = fs::canonicalize(repo.root())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .parent()
        .map_or_default(std::path::Path::to_path_buf);
    unit(
        &repo,
        APP_DEPS,
        &format!("out.d: app/src/lib.rs {}/elsewhere.md\n", parent.display()),
    );
    let refused = velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_HOME", format!("{SOMEWHERE}/cargo-home"))
        .env("RUSTC", &compiler)
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let message = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(1), "{message}");
    assert!(message.contains("outside the repository"), "{message}");
    assert!(lock(&repo).is_none());
}
