// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;
use std::fs::{self, File, FileTimes};
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, SystemTime};

pub(super) fn run(repo: &Repo, command: &str, base: &str) -> (Output, String) {
    let identity = (command == "stage").then(|| {
        let (output, prepared) = run(repo, "prepare", base);
        success(&output);
        prepared
            .lines()
            .find_map(|line| line.strip_prefix("source_identity="))
            .unwrap_or_default()
            .to_owned()
    });
    let output_file = repo.root().join("delta-output");
    fs::remove_file(&output_file).ok();
    let mut command_line = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command_line.args(["delta", command]);
    command_line.args([
        "--target-dir",
        "target",
        "--staging-dir",
        "delta",
        "--package",
        "app",
    ]);
    command_line
        .current_dir(repo.root())
        .env("BASE_CACHE_KEY", base)
        .env("GITHUB_OUTPUT", &output_file)
        .env("RUNNER_TEMP", repo.root());
    if let Some(identity) = identity {
        command_line.env("EXPECTED_SOURCE_IDENTITY", identity);
    }
    let output = command_line
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let written = fs::read_to_string(output_file).unwrap_or_default();
    (output, written)
}

pub(super) fn copy_tree(repo: &Repo, from: &str, to: &str) {
    if repo.root().join(to).exists() {
        fs::remove_dir_all(repo.root().join(to)).unwrap_or_else(|e| unreachable!("{e}"));
    }
    success(
        &Command::new("cp")
            .args(["-pR"])
            .arg(repo.root().join(from))
            .arg(repo.root().join(to))
            .output()
            .unwrap_or_else(|e| unreachable!("{e}")),
    );
}

pub(super) fn manifest(repo: &Repo) -> serde_json::Value {
    serde_json::from_slice(
        &fs::read(repo.root().join("delta/manifest.json")).unwrap_or_else(|e| unreachable!("{e}")),
    )
    .unwrap_or_else(|e| unreachable!("{e}"))
}

pub(super) fn write_manifest(repo: &Repo, value: &serde_json::Value) {
    repo.write_unstaged(
        "delta/manifest.json",
        &serde_json::to_string(value).unwrap_or_else(|e| unreachable!("{e}")),
    );
}

pub(super) fn valid_delta(repo: &Repo) {
    repo.write_unstaged("target/debug/obsolete", "base artifact");
    success(&run(repo, "apply", "base").0);
    fs::remove_file(repo.root().join("target/debug/obsolete"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.write_unstaged("target/debug/current", "current artifact");
    compiled(repo);
    let (output, saved) = run(repo, "stage", "base");
    success(&output);
    assert_eq!(saved, "save=true\n");
}

pub(super) fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn time(repo: &Repo, path: &str) -> SystemTime {
    fs::metadata(repo.root().join(path))
        .and_then(|m| m.modified())
        .unwrap_or_else(|e| unreachable!("{e}"))
}

pub(super) fn set_time(repo: &Repo, path: &str, time: SystemTime) {
    File::options()
        .write(true)
        .open(repo.root().join(path))
        .and_then(|file| file.set_times(FileTimes::new().set_modified(time)))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

pub(super) fn compiled(repo: &Repo) {
    repo.write_unstaged("velnor-run.log", "   Compiling app v0.1.0 (/work/app)\n");
    velnor_ci::io::save_inputs(repo.root()).unwrap_or_else(|error| unreachable!("{error}"));
}

#[test]
fn a_delta_survives_three_pushes_with_prior_artifacts_latest_inputs_and_original_times() {
    let repo = Repo::new("delta-generations");
    let (output, key) = run(&repo, "prepare", "base");
    success(&output);
    assert!(key.starts_with("base8=cae66217\nsource_identity="));
    success(&run(&repo, "apply", "base").0);
    repo.write("source", "first source");
    compiled(&repo);
    let built = time(&repo, "target/.velnor/delta-marker") + Duration::from_nanos(42_001);
    repo.write_unstaged("target/debug/first", "first");
    repo.write_unstaged("target/debug/deleted", "gone later");
    let mut expected_inputs = fs::read_to_string(repo.root().join("target/.velnor/inputs.tsv"))
        .unwrap_or_else(|error| unreachable!("{error}"));
    set_time(&repo, "target/debug/first", built);
    set_time(&repo, "target/.velnor/inputs.tsv", built);
    let (output, save) = run(&repo, "stage", "base");
    success(&output);
    assert_eq!(save, "save=true\n");
    assert_eq!(time(&repo, "delta/files/debug/first"), built);
    assert_eq!(
        fs::read_to_string(repo.root().join("delta/files/debug/first")).unwrap_or_default(),
        "first"
    );
    copy_tree(&repo, "delta", "remote");
    for generation in [2, 3] {
        fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
        repo.write_unstaged("target/.velnor/inputs.tsv", "base manifest");
        copy_tree(&repo, "remote", "delta");
        success(&run(&repo, "apply", "base").0);
        assert!(!repo.root().join("delta").exists());
        assert_eq!(time(&repo, "target/debug/first"), built);
        assert_eq!(
            fs::read_to_string(repo.root().join("target/debug/first")).unwrap_or_default(),
            "first"
        );
        if generation == 3 {
            assert_eq!(
                fs::read_to_string(repo.root().join("target/debug/second")).unwrap_or_default(),
                "second"
            );
        }
        assert_eq!(
            fs::read_to_string(repo.root().join("target/.velnor/inputs.tsv")).unwrap_or_default(),
            expected_inputs
        );
        if generation == 2 {
            fs::remove_file(repo.root().join("target/debug/deleted"))
                .unwrap_or_else(|e| unreachable!("{e}"));
            repo.write_unstaged("target/debug/second", "second");
            repo.write_unstaged("source", "second source");
            velnor_ci::io::save_inputs(repo.root()).unwrap_or_else(|error| unreachable!("{error}"));
            expected_inputs = fs::read_to_string(repo.root().join("target/.velnor/inputs.tsv"))
                .unwrap_or_else(|error| unreachable!("{error}"));
        }
        let (output, save) = run(&repo, "stage", "base");
        success(&output);
        assert_eq!(save, "save=true\n");
        assert!(repo.root().join("delta/files/debug/first").is_file());
        assert!(repo.root().join("delta/files/debug/second").is_file());
        assert!(!repo.root().join("delta/files/debug/deleted").exists());
        assert_eq!(time(&repo, "delta/files/debug/first"), built);
        for (artifact, expected) in [("first", "first"), ("second", "second")] {
            assert_eq!(
                fs::read_to_string(repo.root().join("delta/files/debug").join(artifact))
                    .unwrap_or_default(),
                expected
            );
        }
        copy_tree(&repo, "delta", "remote");
    }
}

#[test]
fn a_different_or_absent_base_discards_the_delta_before_it_can_overlay_artifacts() {
    let repo = Repo::new("delta-base");
    let base512 = "b".repeat(512);
    valid_delta(&repo);
    copy_tree(&repo, "delta", "remote");
    for identity in ["other base", "", &base512, "fifo", "oversized"] {
        copy_tree(&repo, "remote", "delta");
        let mut metadata = manifest(&repo);
        metadata["base"] = serde_json::Value::String(
            if identity == "fifo" { "base" } else { identity }.to_owned(),
        );
        write_manifest(&repo, &metadata);
        let path = repo.root().join("delta/manifest.json");
        let mut writer = None;
        if identity == "oversized" {
            File::create(&path)
                .and_then(|file| file.set_len(16 * 1024 * 1024 + 1))
                .unwrap_or_else(|e| unreachable!("{e}"));
        } else if identity == "fifo" {
            fs::remove_file(&path).unwrap_or_else(|e| unreachable!("{e}"));
            success(
                &Command::new("mkfifo")
                    .arg(&path)
                    .output()
                    .unwrap_or_else(|e| unreachable!("{e}")),
            );
            let mut child = Command::new("tee")
                .arg(&path)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap_or_else(|e| unreachable!("{e}"));
            child
                .stdin
                .take()
                .unwrap_or_else(|| unreachable!("missing FIFO input"))
                .write_all(&serde_json::to_vec(&metadata).unwrap_or_else(|e| unreachable!("{e}")))
                .unwrap_or_else(|e| unreachable!("{e}"));
            writer = Some(child);
        }
        fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
        repo.write_unstaged("target/debug/base", "base artifact");
        let expected = if identity.is_empty() { "" } else { "base" };
        let (output, _) = run(&repo, "apply", expected);
        if let Some(mut writer) = writer {
            if writer
                .try_wait()
                .unwrap_or_else(|e| unreachable!("{e}"))
                .is_none()
            {
                writer.kill().unwrap_or_else(|e| unreachable!("{e}"));
            }
            writer.wait().unwrap_or_else(|e| unreachable!("{e}"));
        }
        success(&output);
        assert!(!repo.root().join("delta").exists());
        assert!(!repo.root().join("target/debug/current").exists());
        assert!(repo.root().join("target/.velnor/delta-marker").is_file());
        if matches!(identity, "other base" | "") || identity == base512 {
            assert!(repo.root().join("target/debug/base").is_file());
        }
    }
}

#[test]
fn no_workspace_compilation_and_oversized_deltas_save_nothing_before_copying() {
    let repo = Repo::new("delta-no-work");
    for log in [
        "",
        "Finished dev profile\n",
        "Compiling dependency v1.0.0\n",
    ] {
        success(&run(&repo, "apply", "base").0);
        repo.write_unstaged("velnor-run.log", log);
        let (output, save) = run(&repo, "stage", "base");
        success(&output);
        assert_eq!(save, "save=false\n");
        assert!(!repo.root().join("delta").exists());
    }
    success(&run(&repo, "apply", "base").0);
    compiled(&repo);
    let huge =
        File::create(repo.root().join("target/huge")).unwrap_or_else(|e| unreachable!("{e}"));
    huge.set_len(499_999_995)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (output, save) = run(&repo, "stage", "base");
    success(&output);
    assert_eq!(save, "save=false\n");
    assert!(String::from_utf8_lossy(&output.stdout).contains("::notice::"));
    assert!(!repo.root().join("delta").exists());
    fs::remove_file(repo.root().join("target/huge")).unwrap_or_else(|e| unreachable!("{e}"));
    success(&run(&repo, "apply", "base").0);
    compiled(&repo);
    repo.write_unstaged("target/debug/current", "current");
    for path in ["delta/junk", "delta/files/debug/incremental/junk"] {
        repo.write_unstaged(path, "");
        File::options()
            .write(true)
            .open(repo.root().join(path))
            .and_then(|file| file.set_len(500_000_001))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let (output, save) = run(&repo, "stage", "base");
    success(&output);
    assert_eq!(save, "save=true\n");
    assert!(!repo.root().join("delta/junk").exists());
    assert!(!repo.root().join("delta/files/debug/incremental").exists());
    assert_eq!(
        fs::read_to_string(repo.root().join("delta/files/debug/current")).unwrap_or_default(),
        "current"
    );
    assert_eq!(manifest(&repo)["base"], "base");
}

#[test]
fn delta_symlinks_never_read_or_overwrite_files_outside_the_cache() {
    let repo = Repo::new("delta-links");
    repo.write_unstaged("outside/untouched", "safe");
    valid_delta(&repo);
    fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(
        repo.root().join("outside"),
        repo.root().join("target/debug"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    success(&run(&repo, "apply", "base").0);
    assert_eq!(
        fs::read_to_string(repo.root().join("outside/untouched")).unwrap_or_default(),
        "safe"
    );
    assert!(!repo.root().join("delta").exists());
    std::os::unix::fs::symlink(
        repo.root().join("outside/untouched"),
        repo.root().join("target/link"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    compiled(&repo);
    let (output, saved) = run(&repo, "stage", "base");
    success(&output);
    assert_eq!(saved, "save=false\n");
    assert!(String::from_utf8_lossy(&output.stdout).contains("::notice::"));
    assert!(!repo.root().join("delta").exists());
}
