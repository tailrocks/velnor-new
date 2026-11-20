// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};
use velnor_test_support::real_cargo;

use crate::delta::success;
use crate::io::Repo;

const SOURCE_A: &str = "fn main() {}\n";
const SOURCE_B: &str = "compile_error!(\"uncompiled future source\");\nfn main() {}\n";

pub(super) fn helper(repo: &Repo, arguments: &[&str], expected: Option<&str>) -> (Output, String) {
    helper_at(repo, arguments, expected, repo.root())
}

pub(super) fn helper_at(
    repo: &Repo,
    arguments: &[&str],
    expected: Option<&str>,
    directory: &Path,
) -> (Output, String) {
    let outputs = repo.root().join(".git/velnor-source-output");
    fs::write(&outputs, "").unwrap_or_else(|error| unreachable!("{error}"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(arguments)
        .current_dir(directory)
        .env("BASE_CACHE_KEY", "base")
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_OUTPUT", &outputs)
        .env_remove("EXPECTED_SOURCE_IDENTITY");
    strip_mirror_overrides(&mut command);
    if let Some(expected) = expected {
        command.env("EXPECTED_SOURCE_IDENTITY", expected);
    }
    let output = command
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    let values = fs::read_to_string(outputs).unwrap_or_default();
    (output, values)
}

pub(super) fn prepare(repo: &Repo) -> String {
    prepared(helper(
        repo,
        &[
            "delta",
            "prepare",
            "--target-dir",
            "target",
            "--staging-dir",
            "delta",
        ],
        None,
    ))
}

fn prepared((output, values): (Output, String)) -> String {
    success(&output);
    values
        .lines()
        .find_map(|line| line.strip_prefix("source_identity="))
        .unwrap_or_else(|| unreachable!("missing source identity: {values}"))
        .to_owned()
}

pub(super) fn delta(repo: &Repo, verb: &str, expected: Option<&str>) -> String {
    let (output, values) = helper(
        repo,
        &[
            "delta",
            verb,
            "--target-dir",
            "target",
            "--staging-dir",
            "delta",
            "--package",
            "app",
        ],
        expected,
    );
    success(&output);
    values
}

/// Takes a child env off the job's crates-mirror. On CI the job's `VELNOR_CARGO_CONFIG` (passed as
/// `--config` by the mirror cargo wrapper) and `CARGO_REGISTRIES_*` / `CARGO_SOURCE_*` replace
/// crates-io with `velnor-crates-mirror` ahead of the repo's own `.cargo/config.toml`, so the
/// directory source that vendors `regdep` is never found (main job 136117859373, delta.rs:88).
///
/// The wrapper also sits first on `PATH` as `cargo` and fails with `missing VELNOR_CARGO_WRAPPER`
/// once its variables are gone (main run 44699691623), so the wrapper's directory leaves `PATH` too
/// and the helper's own `cargo` resolves to the real one.
pub(super) fn strip_mirror_overrides(command: &mut Command) {
    let wrapper = std::env::var_os("VELNOR_CARGO_WRAPPER").map(std::path::PathBuf::from);
    for name in ["VELNOR_CARGO_CONFIG", "VELNOR_CARGO_WRAPPER"] {
        command.env_remove(name);
    }
    for (key, value) in std::env::vars_os() {
        let name = key.to_string_lossy();
        let mirror = name.starts_with("VELNOR_CARGO_")
            || name.starts_with("CARGO_REGISTRIES_")
            || name.starts_with("CARGO_SOURCE_")
            || (name == "CARGO" && wrapper.as_deref() == Some(Path::new(&value)));
        if mirror {
            command.env_remove(&key);
        }
    }
    if let Some(directory) = wrapper.as_deref().and_then(Path::parent) {
        let path = std::env::var_os("PATH").unwrap_or_default();
        command.env("PATH", without_directory(&path, directory));
    }
}

fn without_directory(path: &std::ffi::OsStr, directory: &Path) -> std::ffi::OsString {
    let kept = std::env::split_paths(path).filter(|entry| entry != directory);
    std::env::join_paths(kept).unwrap_or_else(|_| path.to_owned())
}

#[test]
fn the_job_mirror_wrapper_directory_leaves_the_child_path() {
    let path = std::env::join_paths(["/runner/velnor-cargo", "/usr/bin", "/opt/rust/bin"])
        .unwrap_or_else(|error| unreachable!("{error}"));
    let kept = without_directory(&path, Path::new("/runner/velnor-cargo"));
    let kept: Vec<_> = std::env::split_paths(&kept).collect();
    assert_eq!(kept, [Path::new("/usr/bin"), Path::new("/opt/rust/bin")]);
}

pub(super) fn cargo(repo: &Repo) -> Output {
    let mut command = Command::new(real_cargo());
    command
        .args(["build", "--offline", "--verbose", "--color", "never"])
        .current_dir(repo.root())
        .env("CARGO_TARGET_DIR", repo.root().join("target"))
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER");
    strip_mirror_overrides(&mut command);
    command
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"))
}

pub(super) fn fixture(name: &str) -> (Repo, String) {
    let repo = Repo::new(name);
    repo.write_all(&[
        (
            "Cargo.toml",
            "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n",
        ),
        (
            "Cargo.lock",
            "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n",
        ),
        ("src/main.rs", SOURCE_A),
    ]);
    repo.set_mtime(
        "src/main.rs",
        SystemTime::UNIX_EPOCH + Duration::from_secs(1),
    );
    let expected = prepare(&repo);
    assert_eq!(expected.len(), 64);
    delta(&repo, "apply", None);
    let output = cargo(&repo);
    success(&output);
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(log.contains("Compiling app"), "{log}");
    repo.write_unstaged("velnor-run.log", &log);
    success(
        &Command::new(repo.root().join("target/debug/app"))
            .output()
            .unwrap_or_else(|error| unreachable!("{error}")),
    );
    (repo, expected)
}

pub(super) fn save_inputs(repo: &Repo) {
    success(&helper(repo, &["inputs", "save"], None).0);
}

pub(super) fn next_push(repo: &Repo) -> Output {
    fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|error| unreachable!("{error}"));
    delta(repo, "apply", None);
    success(&helper(repo, &["inputs", "restore"], None).0);
    cargo(repo)
}

#[test]
fn unchanged_sources_reuse_the_compilation_when_checkout_times_change() {
    let (repo, expected) = fixture("delta-source-unchanged");
    repo.set_mtime(
        "src/main.rs",
        SystemTime::UNIX_EPOCH + Duration::from_secs(2),
    );
    assert_eq!(prepare(&repo), expected);
    save_inputs(&repo);
    assert_eq!(delta(&repo, "stage", Some(&expected)), "save=true\n");
    let output = next_push(&repo);
    success(&output);
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(log.contains("Fresh app"), "{log}");
    success(
        &Command::new(repo.root().join("target/debug/app"))
            .output()
            .unwrap_or_else(|error| unreachable!("{error}")),
    );
}

#[test]
fn post_build_source_mutation_cannot_cache_a_future_compile_error_as_fresh() {
    let (repo, expected) = fixture("delta-source-future");
    let original_time = repo.mtime("src/main.rs");
    repo.write_unstaged("src/main.rs", SOURCE_B);
    repo.set_mtime("src/main.rs", original_time);
    save_inputs(&repo);
    let decision = delta(&repo, "stage", Some(&expected));
    let output = next_push(&repo);
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "future source was falsely fresh: {log}"
    );
    assert!(log.contains("uncompiled future source"), "{log}");
    assert_eq!(decision, "save=false\n");
}

#[test]
fn saves_require_the_pre_execution_identity_from_the_step_environment() {
    let (repo, expected) = fixture("delta-source-required");
    save_inputs(&repo);
    for value in [
        None,
        Some(""),
        Some("not-a-digest"),
        Some("0123456789abcdef"),
    ] {
        assert_eq!(delta(&repo, "stage", value), "save=false\n");
        assert!(!repo.root().join("delta").exists());
    }
    assert_eq!(delta(&repo, "stage", Some(&expected)), "save=true\n");
}

#[test]
fn source_identity_covers_modes_links_absence_and_parent_configuration() {
    for mutation in ["mode", "link", "absent", "parent"] {
        let repo = Repo::new("delta-source-dimensions");
        repo.write_all(&[
            ("workspace/src.rs", "original"),
            (".cargo/config.toml", "[build]\n"),
        ]);
        std::os::unix::fs::symlink("src.rs", repo.root().join("workspace/link"))
            .unwrap_or_else(|error| unreachable!("{error}"));
        repo.git(&["add", "workspace/link"]);
        let original = prepare(&repo);
        let nested = || {
            prepared(helper_at(
                &repo,
                &[
                    "delta",
                    "prepare",
                    "--target-dir",
                    "target",
                    "--staging-dir",
                    "delta",
                ],
                None,
                &repo.root().join("workspace"),
            ))
        };
        assert_eq!(nested(), original);
        match mutation {
            "mode" => fs::set_permissions(
                repo.root().join("workspace/src.rs"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap_or_else(|error| unreachable!("{error}")),
            "link" => {
                fs::remove_file(repo.root().join("workspace/link"))
                    .unwrap_or_else(|error| unreachable!("{error}"));
                std::os::unix::fs::symlink("other.rs", repo.root().join("workspace/link"))
                    .unwrap_or_else(|error| unreachable!("{error}"));
            }
            "absent" => fs::remove_file(repo.root().join("workspace/src.rs"))
                .unwrap_or_else(|error| unreachable!("{error}")),
            _ => repo.write_unstaged(".cargo/config.toml", "[build]\njobs=2\n"),
        }
        assert_ne!(nested(), original, "{mutation}");
    }
}

#[test]
fn source_identity_declines_preexisting_tracked_node_kind_mismatches() {
    for kind in ["file", "link"] {
        let repo = Repo::new("delta-source-kind");
        repo.write_all(&[("src.rs", "original")]);
        std::os::unix::fs::symlink("src.rs", repo.root().join("link"))
            .unwrap_or_else(|error| unreachable!("{error}"));
        repo.git(&["add", "link"]);
        let original = prepare(&repo);
        assert_eq!(original.len(), 64);
        fs::remove_file(repo.root().join(kind_path(kind)))
            .unwrap_or_else(|error| unreachable!("{error}"));
        if kind == "file" {
            std::os::unix::fs::symlink("link", repo.root().join("src.rs"))
                .unwrap_or_else(|error| unreachable!("{error}"));
        } else {
            repo.write_unstaged("link", "replacement");
        }
        assert!(prepare(&repo).is_empty(), "{kind}");
        delta(&repo, "apply", None);
        repo.write_unstaged("velnor-run.log", "Compiling app v0.1.0\n");
        assert_eq!(delta(&repo, "stage", Some(&original)), "save=false\n");
    }
}

fn kind_path(kind: &str) -> &str {
    if kind == "file" { "src.rs" } else { "link" }
}
