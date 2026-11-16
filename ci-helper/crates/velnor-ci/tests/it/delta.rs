// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;
use std::fs::{self, File, FileTimes};
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, SystemTime};

fn run(repo: &Repo, command: &str, base: &str) -> (Output, String) {
    let output_file = repo.root().join("delta-output");
    fs::remove_file(&output_file).ok();
    let mut command_line = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command_line.args(["delta", command]);
    if command != "prepare" {
        command_line.args([
            "--target-dir",
            "target",
            "--staging-dir",
            "delta",
            "--package",
            "app",
        ]);
    }
    command_line
        .current_dir(repo.root())
        .env("BASE_CACHE_KEY", base)
        .env("GITHUB_OUTPUT", &output_file)
        .env("RUNNER_TEMP", repo.root());
    let output = command_line
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let written = fs::read_to_string(output_file).unwrap_or_default();
    (output, written)
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn time(repo: &Repo, path: &str) -> SystemTime {
    fs::metadata(repo.root().join(path))
        .and_then(|m| m.modified())
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn set_time(repo: &Repo, path: &str, time: SystemTime) {
    File::options()
        .write(true)
        .open(repo.root().join(path))
        .and_then(|file| file.set_times(FileTimes::new().set_modified(time)))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn compiled(repo: &Repo) {
    repo.write_unstaged("velnor-run.log", "   Compiling app v0.1.0 (/work/app)\n");
}

#[test]
fn a_delta_survives_three_pushes_with_prior_artifacts_latest_inputs_and_original_times() {
    let repo = Repo::new("delta-generations");
    let (output, key) = run(&repo, "prepare", "base");
    success(&output);
    assert_eq!(key, "base8=cae66217\n");
    success(&run(&repo, "apply", "base").0);
    compiled(&repo);
    let built = time(&repo, "target/.velnor/delta-marker") + Duration::from_nanos(42_001);
    repo.write_unstaged("target/debug/first", "first");
    repo.write_unstaged("target/debug/deleted", "gone later");
    repo.write_unstaged("target/.velnor/inputs.tsv", "first manifest");
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
    for generation in [2, 3] {
        fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
        repo.write_unstaged("target/.velnor/inputs.tsv", "base manifest");
        success(&run(&repo, "apply", "base").0);
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
        let expected = if generation == 2 {
            "first manifest"
        } else {
            "second manifest"
        };
        assert_eq!(
            fs::read_to_string(repo.root().join("target/.velnor/inputs.tsv")).unwrap_or_default(),
            expected
        );
        if generation == 2 {
            fs::remove_file(repo.root().join("target/debug/deleted"))
                .unwrap_or_else(|e| unreachable!("{e}"));
            repo.write_unstaged("target/debug/second", "second");
            repo.write_unstaged("target/.velnor/inputs.tsv", "second manifest");
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
    }
}

#[test]
fn a_different_or_absent_base_discards_the_delta_before_it_can_overlay_artifacts() {
    let repo = Repo::new("delta-base");
    for base in ["other base", ""] {
        repo.write_unstaged("delta/base-key", "base");
        repo.write_unstaged("delta/files/debug/poison", "wrong base");
        repo.write_unstaged("target/debug/base", "base artifact");
        success(&run(&repo, "apply", base).0);
        assert!(!repo.root().join("delta").exists());
        assert!(!repo.root().join("target/debug/poison").exists());
        assert!(repo.root().join("target/debug/base").exists());
    }
    for identity in ["oversized", "fifo"] {
        let base = if identity == "oversized" {
            "b".repeat(513)
        } else {
            "base".to_owned()
        };
        if identity == "oversized" {
            repo.write_unstaged("delta/base-key", &base);
        } else {
            fs::create_dir_all(repo.root().join("delta")).unwrap_or_else(|e| unreachable!("{e}"));
            success(
                &Command::new("mkfifo")
                    .arg(repo.root().join("delta/base-key"))
                    .output()
                    .unwrap_or_else(|e| unreachable!("{e}")),
            );
        }
        repo.write_unstaged("delta/files/debug/poison", "invalid identity");
        repo.write_unstaged("target/debug/base", "base artifact");
        let mut writer = (identity == "fifo").then(|| {
            let mut writer = Command::new("tee")
                .arg(repo.root().join("delta/base-key"))
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap_or_else(|e| unreachable!("{e}"));
            writer
                .stdin
                .take()
                .unwrap_or_else(|| unreachable!("missing FIFO writer input"))
                .write_all(base.as_bytes())
                .unwrap_or_else(|e| unreachable!("{e}"));
            writer
        });
        let (output, _) = run(&repo, "apply", &base);
        if let Some(writer) = &mut writer {
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
        assert!(!repo.root().join("target/debug/poison").exists());
        assert!(repo.root().join("target/debug/base").exists());
        assert!(repo.root().join("target/.velnor/delta-marker").is_file());
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
    huge.set_len(500_000_000)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (output, save) = run(&repo, "stage", "base");
    success(&output);
    assert_eq!(save, "save=false\n");
    assert!(String::from_utf8_lossy(&output.stdout).contains("500000004"));
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
    assert_eq!(
        fs::read_to_string(repo.root().join("delta/base-key")).unwrap_or_default(),
        "base"
    );
}

#[test]
fn delta_symlinks_never_read_or_overwrite_files_outside_the_cache() {
    let repo = Repo::new("delta-links");
    repo.write_unstaged("outside/untouched", "safe");
    repo.write_unstaged("delta/base-key", "base");
    repo.write_unstaged("delta/files/debug/untouched", "poison");
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
    assert!(!run(&repo, "stage", "base").0.status.success());
    assert!(!repo.root().join("delta").exists());
}
