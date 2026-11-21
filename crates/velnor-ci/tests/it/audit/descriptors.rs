// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;

use nix::sys::resource::{Resource, getrlimit, setrlimit};
use velnor_test_support::real_cargo;

use super::inventory::build;
use super::{own_units, unit, workspace_below_the_root};
use crate::io::Repo;

const DEPTH: usize = 120;
const SIBLINGS: usize = 8;
const OUTPUT: &str = "rust/target/debug/build/lib-f000000000000008/output";
const SOFT_LIMIT_WITHOUT_RAISE: u32 = 256;
const TIGHT_LIMIT: u32 = 40;
const RAISE_NEEDS_HARD_LIMIT: u64 = 1024;

fn hard_open_file_limit() -> u64 {
    getrlimit(Resource::RLIMIT_NOFILE).map_or(0, |(_, hard)| hard)
}

fn assert_the_raise_can_run() {
    let hard = hard_open_file_limit();
    assert!(
        hard >= RAISE_NEEDS_HARD_LIMIT,
        "the hard open-file limit {hard} is below {RAISE_NEEDS_HARD_LIMIT}, so the soft limit cannot be raised past {TIGHT_LIMIT} and the raise path would not run"
    );
}

fn deep_repository() -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../..\n");
    let mut directory = repo.root().join("deep");
    for _ in 0..DEPTH {
        for sibling in 0..SIBLINGS {
            fs::create_dir_all(directory.join(format!("a{sibling}")))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        directory = directory.join("z");
    }
    fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    build(&repo, OUTPUT, None);
    repo
}

fn velnor_ci_under(repo: &Repo, limits: &str, args: &[&str], path: Option<&str>) -> Output {
    let mut command = super::command("sh");
    command
        .arg("-c")
        .arg(format!("{limits} && exec \"$0\" \"$@\""))
        .arg(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(args)
        .current_dir(repo.root())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_BUILD_DIR")
        .env_remove("VELNOR_AUDIT_MAX_ENTRIES");
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn audit_under(repo: &Repo, limits: &str) -> Output {
    velnor_ci_under(repo, limits, &["inputs", "audit", "--write"], None)
}

fn cargo_script(metadata: &str, build: &str) -> (Repo, String) {
    let bin = Repo::empty("cargo-bin");
    let cargo = bin.root().join("cargo");
    let real = real_cargo();
    fs::write(
        &cargo,
        format!(
            "#!/bin/sh\ncase \" $* \" in *\" metadata \"*) {metadata}exec \"{}\" \"$@\";; esac\n{build}\n",
            real.display()
        ),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let path = format!(
        "{}:{}",
        bin.root().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    (bin, path)
}

fn fake_cargo() -> (Repo, String) {
    cargo_script(
        "",
        &format!("ulimit -Sn\ntouch -m -t 206601010000 {OUTPUT}"),
    )
}

fn receipt(repo: &Repo) -> PathBuf {
    repo.root().join(format!("{OUTPUT}.velnor-inputs.json"))
}

#[test]
fn a_deep_tree_with_pending_siblings_audits_once_the_open_file_limit_is_raised() {
    assert_the_raise_can_run();
    let repo = deep_repository();
    let audited = audit_under(&repo, &format!("ulimit -Sn {TIGHT_LIMIT}"));
    assert!(audited.status.success(), "{audited:?}");
    assert!(repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn exhausting_the_open_file_limit_names_the_depth_and_the_limit() {
    let repo = deep_repository();
    let audited = audit_under(
        &repo,
        &format!("ulimit -Sn {TIGHT_LIMIT} && ulimit -Hn {TIGHT_LIMIT}"),
    );
    assert_eq!(audited.status.code(), Some(1), "{audited:?}");
    let stderr = String::from_utf8_lossy(&audited.stderr);
    assert!(
        stderr.contains("directory tree deeper than ")
            && stderr.contains(&format!(
                " levels with pending siblings exceeds the open-file limit ({TIGHT_LIMIT}); raise the open-file limit"
            )),
        "{stderr}"
    );
}

#[test]
fn a_build_that_exhausts_the_open_file_limit_runs_cargo_warns_and_treats_the_tree_as_untrusted() {
    let repo = deep_repository();
    assert!(
        receipt(&repo).exists(),
        "a complete build records its inputs"
    );
    let (_bin, path) = fake_cargo();
    let built = velnor_ci_under(
        &repo,
        &format!("ulimit -Sn {TIGHT_LIMIT} && ulimit -Hn {TIGHT_LIMIT}"),
        &["inputs", "build", "--", "cargo", "build"],
        Some(&path),
    );
    assert!(built.status.success(), "{built:?}");
    let stderr = String::from_utf8_lossy(&built.stderr);
    assert!(
        stderr.contains("velnor-ci: WARNING: directory tree deeper than ")
            && stderr.contains(&format!(
                " levels with pending siblings exceeds the open-file limit ({TIGHT_LIMIT}); raise the open-file limit"
            )),
        "{stderr}"
    );
    assert!(
        String::from_utf8_lossy(&built.stdout)
            .lines()
            .any(|line| line == TIGHT_LIMIT.to_string()),
        "cargo did not run under the original limit: {built:?}"
    );
    assert!(
        !receipt(&repo).exists(),
        "an incomplete capture kept the previous inputs record"
    );
}

#[test]
fn a_deep_tree_is_captured_completely_by_a_build_that_raised_the_open_file_limit() {
    assert_the_raise_can_run();
    let repo = deep_repository();
    let (_bin, path) = fake_cargo();
    let built = velnor_ci_under(
        &repo,
        &format!("ulimit -Sn {TIGHT_LIMIT}"),
        &["inputs", "build", "--", "cargo", "build"],
        Some(&path),
    );
    assert!(built.status.success(), "{built:?}");
    assert!(
        !String::from_utf8_lossy(&built.stderr).contains("WARNING"),
        "{built:?}"
    );
    assert!(
        receipt(&repo).exists(),
        "the build did not record its inputs: {built:?}"
    );
}

#[test]
fn the_build_command_sees_the_open_file_limit_the_process_started_with() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../..\n");
    let built = velnor_ci_under(
        &repo,
        &format!("ulimit -Sn {SOFT_LIMIT_WITHOUT_RAISE}"),
        &["inputs", "build", "--", "sh", "-c", "ulimit -Sn"],
        None,
    );
    assert!(built.status.success(), "{built:?}");
    assert!(
        String::from_utf8_lossy(&built.stdout)
            .lines()
            .any(|line| line == SOFT_LIMIT_WITHOUT_RAISE.to_string()),
        "the build command inherited the raised limit: {built:?}"
    );
}

fn shallow_repository() -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../..\n");
    build(&repo, OUTPUT, None);
    repo
}

fn metadata_logging_cargo() -> (Repo, String, PathBuf) {
    let log = std::env::temp_dir().join(format!("velnor-metadata-limits-{}", std::process::id()));
    let _ = fs::remove_file(&log);
    let (bin, path) = cargo_script(&format!("ulimit -Sn >> \"{}\"; ", log.display()), "");
    (bin, path, log)
}

fn logged_limits(log: &Path) -> Vec<String> {
    let text = fs::read_to_string(log)
        .unwrap_or_else(|e| unreachable!("the metadata command never ran: {e}"));
    let _ = fs::remove_file(log);
    text.lines().map(str::to_owned).collect()
}

#[test]
fn setup_subprocesses_of_an_audit_run_under_the_original_open_file_limit() {
    let repo = shallow_repository();
    let (_bin, path, log) = metadata_logging_cargo();
    let audited = velnor_ci_under(
        &repo,
        &format!("ulimit -Sn {SOFT_LIMIT_WITHOUT_RAISE}"),
        &["inputs", "audit", "--write"],
        Some(&path),
    );
    assert!(audited.status.success(), "{audited:?}");
    let limits = logged_limits(&log);
    assert!(!limits.is_empty(), "no setup subprocess ran");
    assert!(
        limits
            .iter()
            .all(|limit| limit == &SOFT_LIMIT_WITHOUT_RAISE.to_string()),
        "a setup subprocess ran under the raised limit: {limits:?}"
    );
}

#[test]
fn setup_subprocesses_of_a_build_run_under_the_original_open_file_limit() {
    let repo = shallow_repository();
    let (_bin, path, log) = metadata_logging_cargo();
    let built = velnor_ci_under(
        &repo,
        &format!("ulimit -Sn {SOFT_LIMIT_WITHOUT_RAISE}"),
        &["inputs", "build", "--", "cargo", "build"],
        Some(&path),
    );
    assert!(built.status.success(), "{built:?}");
    let limits = logged_limits(&log);
    assert!(!limits.is_empty(), "no setup subprocess ran");
    assert!(
        limits
            .iter()
            .all(|limit| limit == &SOFT_LIMIT_WITHOUT_RAISE.to_string()),
        "a setup subprocess ran under the raised limit: {limits:?}"
    );
}

fn soft_open_file_limit() -> u64 {
    getrlimit(Resource::RLIMIT_NOFILE).map_or(0, |(soft, _)| soft)
}

fn lower_soft_limit_below_the_hard_limit() -> u64 {
    let hard = hard_open_file_limit();
    assert!(
        hard > u64::from(SOFT_LIMIT_WITHOUT_RAISE),
        "the hard open-file limit {hard} leaves no room above {SOFT_LIMIT_WITHOUT_RAISE} to observe a raise"
    );
    setrlimit(
        Resource::RLIMIT_NOFILE,
        u64::from(SOFT_LIMIT_WITHOUT_RAISE),
        hard,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    soft_open_file_limit()
}

fn options(max_directory_entries: usize) -> velnor_ci::io::AuditOptions {
    velnor_ci::io::AuditOptions {
        write: true,
        root: None,
        max_directory_entries,
        inherit_target_dir: false,
    }
}

#[test]
fn the_public_audit_call_returns_with_the_callers_open_file_limit_unchanged() {
    let repo = shallow_repository();
    let before = lower_soft_limit_below_the_hard_limit();
    let written = velnor_ci::io::audit_inputs(repo.root(), &options(1_000_000));
    assert!(written.is_ok(), "{written:?}");
    assert_eq!(soft_open_file_limit(), before);
}

#[test]
fn the_public_audit_call_restores_the_open_file_limit_when_the_scan_fails() {
    let repo = shallow_repository();
    let before = lower_soft_limit_below_the_hard_limit();
    let refused = velnor_ci::io::audit_inputs(repo.root(), &options(1));
    assert!(
        refused
            .as_ref()
            .is_err_and(|error| error.to_string().contains("freshness scan limit")),
        "{refused:?}"
    );
    assert_eq!(soft_open_file_limit(), before);
}
