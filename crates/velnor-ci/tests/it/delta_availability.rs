// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use crate::delta::{compiled, run, success, valid_delta};
use crate::delta_source::prepare;
use crate::io::Repo;

type Tree = BTreeMap<PathBuf, (Option<Vec<u8>>, SystemTime, u32)>;

fn tree(root: &Path) -> Tree {
    let mut result = Tree::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(path) = pending.pop() {
        let full = root.join(&path);
        let stat = fs::symlink_metadata(&full).unwrap_or_else(|error| unreachable!("{error}"));
        let bytes = if stat.is_dir() {
            for entry in fs::read_dir(&full).unwrap_or_else(|error| unreachable!("{error}")) {
                let entry = entry.unwrap_or_else(|error| unreachable!("{error}"));
                pending.push(path.join(entry.file_name()));
            }
            None
        } else {
            assert!(stat.is_file());
            Some(fs::read(&full).unwrap_or_else(|error| unreachable!("{error}")))
        };
        result.insert(
            path,
            (
                bytes,
                stat.modified()
                    .unwrap_or_else(|error| unreachable!("{error}")),
                stat.permissions().mode(),
            ),
        );
    }
    result
}

#[test]
fn a_partial_staging_write_declines_saving_without_changing_the_successful_build() {
    let repo = Repo::new("delta-partial-write");
    repo.write("source", "source input");
    success(&run(&repo, "apply", "base").0);
    for index in 0..16 {
        repo.write_unstaged(
            &format!("target/debug/artifact-{index}"),
            &"x".repeat(16_384),
        );
    }
    compiled(&repo);
    let expected = prepare(&repo);
    let before = tree(&repo.root().join("target"));
    let outputs = repo.root().join(".git/limited-stage-output");
    let output = Command::new("/bin/sh")
        .args(["-c", "trap '' XFSZ; ulimit -f 1; exec \"$@\"", "stage"])
        .arg(env!("CARGO_BIN_EXE_velnor-ci"))
        .args([
            "delta",
            "stage",
            "--target-dir",
            "target",
            "--staging-dir",
            "delta",
            "--package",
            "app",
        ])
        .current_dir(repo.root())
        .env("BASE_CACHE_KEY", "base")
        .env("EXPECTED_SOURCE_IDENTITY", expected)
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_OUTPUT", &outputs)
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"));
    success(&output);
    assert_eq!(
        fs::read_to_string(outputs).unwrap_or_default(),
        "save=false\n"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("::notice::"));
    assert!(!repo.root().join("delta").exists());
    assert_eq!(tree(&repo.root().join("target")), before);
}

#[test]
fn a_linked_target_skips_the_delta_and_preserves_the_linked_build_tree() {
    for restored in ["missing", "valid", "corrupt", "tracked"] {
        let repo = Repo::new("delta-linked-target");
        valid_delta(&repo);
        if restored == "missing" {
            fs::remove_dir_all(repo.root().join("delta"))
                .unwrap_or_else(|error| unreachable!("{error}"));
        } else if restored == "corrupt" {
            repo.write_unstaged("delta/manifest.json", "invalid manifest");
        } else if restored == "tracked" {
            repo.write("delta/source", "tracked source input");
        }
        fs::rename(repo.root().join("target"), repo.root().join("real-target"))
            .unwrap_or_else(|error| unreachable!("{error}"));
        std::os::unix::fs::symlink("real-target", repo.root().join("target"))
            .unwrap_or_else(|error| unreachable!("{error}"));
        let before = tree(&repo.root().join("real-target"));
        let (output, _) = run(&repo, "apply", "base");
        if restored == "tracked" {
            assert!(!output.status.success());
            assert_eq!(
                fs::read_to_string(repo.root().join("delta/source")).unwrap_or_default(),
                "tracked source input"
            );
        } else {
            success(&output);
            assert!(String::from_utf8_lossy(&output.stdout).contains("::notice::"));
            let (output, values) = run(&repo, "stage", "base");
            success(&output);
            assert_eq!(values, "save=false\n");
            assert!(!repo.root().join("delta").exists());
        }
        assert_eq!(
            fs::read_link(repo.root().join("target"))
                .unwrap_or_else(|error| unreachable!("{error}")),
            Path::new("real-target")
        );
        assert_eq!(tree(&repo.root().join("real-target")), before);
    }
}
