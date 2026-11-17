// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File};
use std::time::{Duration, SystemTime};

use crate::delta::{compiled, run, set_time, success, time};
use crate::io::Repo;

const OUT: &str = "target/debug/build/app/out";

fn base(repo: &Repo) {
    for (name, bytes) in [
        ("obsolete", "gone"),
        ("generated", "old bytes"),
        ("unchanged", "base"),
    ] {
        let path = format!("{OUT}/{name}");
        repo.write_unstaged(&path, bytes);
        set_time(repo, &path, SystemTime::UNIX_EPOCH + Duration::from_secs(1));
    }
}

fn changed_outputs(repo: &Repo) {
    fs::remove_file(repo.root().join(format!("{OUT}/obsolete")))
        .unwrap_or_else(|e| unreachable!("{e}"));
    for (name, bytes) in [
        ("generated", "new bytes"),
        ("new", "old timestamp"),
        ("incremental", "regular file"),
    ] {
        let path = format!("{OUT}/{name}");
        repo.write_unstaged(&path, bytes);
        set_time(repo, &path, SystemTime::UNIX_EPOCH + Duration::from_secs(1));
    }
    repo.write_unstaged("target/debug/incremental/ignored", "excluded");
    repo.write_unstaged("target/x86_64-target/debug/incremental/ignored", "excluded");
    repo.write_unstaged(
        "target/debug/build/other/out/incremental/kept",
        "deep output",
    );
    set_time(
        repo,
        "target/debug/build/other/out/incremental/kept",
        SystemTime::UNIX_EPOCH + Duration::from_secs(1),
    );
}

#[test]
fn cumulative_deltas_preserve_exact_outputs_even_when_timestamps_hide_changes() {
    let repo = Repo::new("delta-content-identity");
    for generation in 1..=3 {
        base(&repo);
        success(&run(&repo, "apply", "base").0);
        if generation == 1 {
            changed_outputs(&repo);
        } else {
            assert!(!repo.root().join(format!("{OUT}/obsolete")).exists());
            for (name, bytes) in [("new", "old timestamp"), ("incremental", "regular file")] {
                let path = format!("{OUT}/{name}");
                assert_eq!(
                    fs::read_to_string(repo.root().join(&path)).unwrap_or_default(),
                    bytes
                );
                assert_eq!(
                    time(&repo, &path),
                    SystemTime::UNIX_EPOCH + Duration::from_secs(1)
                );
            }
            let expected = if generation == 2 {
                "new bytes"
            } else {
                "old bytes"
            };
            assert_eq!(
                fs::read_to_string(repo.root().join(format!("{OUT}/generated")))
                    .unwrap_or_default(),
                expected
            );
            if generation == 2 {
                repo.write_unstaged(&format!("{OUT}/generated"), "old bytes");
                set_time(
                    &repo,
                    &format!("{OUT}/generated"),
                    SystemTime::UNIX_EPOCH + Duration::from_secs(1),
                );
            }
        }
        compiled(&repo);
        let (output, saved) = run(&repo, "stage", "base");
        success(&output);
        assert_eq!(saved, "save=true\n");
        assert!(!repo.root().join("delta/files/debug/incremental").exists());
        assert!(
            !repo
                .root()
                .join("delta/files/x86_64-target/debug/incremental")
                .exists()
        );
        assert_eq!(
            fs::read_to_string(
                repo.root()
                    .join("delta/files/debug/build/other/out/incremental/kept")
            )
            .unwrap_or_default(),
            "deep output"
        );
        assert_eq!(
            time(&repo, "delta/files/debug/build/other/out/incremental/kept"),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1)
        );
        assert!(
            !repo
                .root()
                .join("delta/files/debug/build/app/out/unchanged")
                .exists()
        );
        assert_eq!(
            repo.root()
                .join("delta/files/debug/build/app/out/generated")
                .exists(),
            generation == 1
        );
        assert_eq!(
            fs::read_to_string(repo.root().join(format!("{OUT}/unchanged"))).unwrap_or_default(),
            "base"
        );
        fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    }
}

#[test]
fn invalid_removals_cannot_delete_outside_files_and_invalid_inventories_refuse_saving() {
    let repo = Repo::new("delta-removal-validation");
    repo.write_unstaged("outside/untouched", "safe");
    let absolute = repo.root().join("outside/untouched");
    for removal in [
        "../outside/untouched".to_owned(),
        absolute.to_string_lossy().into_owned(),
        "link/untouched".to_owned(),
    ] {
        repo.write_unstaged("delta/base-key", "base");
        repo.write_unstaged(
            "delta/removed.json",
            &serde_json::to_string(&["debug/valid", &removal]).unwrap_or_default(),
        );
        repo.write_unstaged("delta/files/debug/current", "current");
        repo.write_unstaged("target/debug/valid", "base");
        if removal == "link/untouched" {
            std::os::unix::fs::symlink(
                repo.root().join("outside"),
                repo.root().join("target/link"),
            )
            .unwrap_or_else(|e| unreachable!("{e}"));
        }
        success(&run(&repo, "apply", "base").0);
        assert_eq!(fs::read_to_string(&absolute).unwrap_or_default(), "safe");
        assert!(!repo.root().join("delta").exists());
        fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    }
    for invalid in ["fifo", "oversized"] {
        repo.write_unstaged("delta/base-key", "base");
        invalid_metadata(&repo, "delta/removed.json", invalid);
        success(&run(&repo, "apply", "base").0);
        assert_eq!(fs::read_to_string(&absolute).unwrap_or_default(), "safe");
        assert!(!repo.root().join("delta").exists());
    }
    for invalid in ["missing", "invalid json", "oversized", "fifo"] {
        success(&run(&repo, "apply", "base").0);
        invalid_metadata(&repo, "target/.velnor/delta-baseline.json", invalid);
        compiled(&repo);
        let (output, saved) = run(&repo, "stage", "base");
        success(&output);
        assert_eq!(saved, "save=false\n");
        assert!(!repo.root().join("delta").exists());
    }
    for inherited in ["symlink", "fifo"] {
        invalid_metadata(&repo, "target/.velnor/delta-baseline.json", inherited);
        success(&run(&repo, "apply", "base").0);
        assert_eq!(fs::read_to_string(&absolute).unwrap_or_default(), "safe");
        assert!(
            fs::symlink_metadata(repo.root().join("target/.velnor/delta-baseline.json"))
                .unwrap_or_else(|e| unreachable!("{e}"))
                .is_file()
        );
        compiled(&repo);
        let (output, saved) = run(&repo, "stage", "base");
        success(&output);
        assert_eq!(saved, "save=true\n");
    }
}

fn invalid_metadata(repo: &Repo, name: &str, invalid: &str) {
    let path = repo.root().join(name);
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => unreachable!("{error}"),
    }
    match invalid {
        "missing" => {}
        "symlink" => std::os::unix::fs::symlink(repo.root().join("outside/untouched"), &path)
            .unwrap_or_else(|e| unreachable!("{e}")),
        "fifo" => success(
            &std::process::Command::new("mkfifo")
                .arg(&path)
                .output()
                .unwrap_or_else(|e| unreachable!("{e}")),
        ),
        "oversized" => File::create(&path)
            .and_then(|file| file.set_len(16 * 1024 * 1024 + 1))
            .unwrap_or_else(|e| unreachable!("{e}")),
        text => fs::write(&path, text).unwrap_or_else(|e| unreachable!("{e}")),
    }
}
