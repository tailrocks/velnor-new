// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, FileTimes, Permissions};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::time::{Duration, SystemTime};

use crate::delta::{
    compiled, copy_tree, manifest, run, set_time, success, time, valid_delta, write_manifest,
};
use crate::io::Repo;

fn mode(repo: &Repo, path: &str) -> u32 {
    fs::metadata(repo.root().join(path))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions()
        .mode()
        & 0o7777
}

fn chmod(repo: &Repo, path: &str, mode: u32) {
    fs::set_permissions(repo.root().join(path), Permissions::from_mode(mode))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn base(repo: &Repo) {
    repo.write_unstaged("target/debug/executable", "same bytes");
    repo.write_unstaged("target/debug/file-to-directory", "was file");
    repo.write_unstaged("target/debug/directory-to-file/child", "was directory");
    fs::create_dir_all(repo.root().join("target/debug/deleted-empty"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.write_unstaged("target/debug/readonly/deleted", "remove me");
    readonly_parent(repo);
    chmod(repo, "target/debug/executable", 0o644);
    set_time(
        repo,
        "target/debug/executable",
        SystemTime::UNIX_EPOCH + Duration::from_secs(1),
    );
}

fn readonly_parent(repo: &Repo) {
    chmod(repo, "target/debug/readonly", 0o555);
    File::open(repo.root().join("target/debug/readonly"))
        .and_then(|file| {
            file.set_times(
                FileTimes::new().set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1)),
            )
        })
        .unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn deltas_round_trip_mode_changes_empty_directories_and_node_kind_transitions() {
    let repo = Repo::new("delta-tree-round-trip");
    base(&repo);
    success(&run(&repo, "apply", "base").0);
    chmod(&repo, "target/debug/executable", 0o755);
    fs::remove_file(repo.root().join("target/debug/file-to-directory"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(repo.root().join("target/debug/file-to-directory"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(repo.root().join("target/debug/directory-to-file"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.write_unstaged("target/debug/directory-to-file", "now file");
    fs::remove_dir(repo.root().join("target/debug/deleted-empty"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(repo.root().join("target/debug/new-empty"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    chmod(&repo, "target/debug/readonly", 0o700);
    fs::remove_file(repo.root().join("target/debug/readonly/deleted"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    readonly_parent(&repo);
    compiled(&repo);
    let (output, saved) = run(&repo, "stage", "base");
    success(&output);
    assert_eq!(saved, "save=true\n");
    copy_tree(&repo, "delta", "remote");
    fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    base(&repo);
    copy_tree(&repo, "remote", "delta");
    let (output, _) = run(&repo, "apply", "base");
    success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("applied"));
    assert!(!repo.root().join("target/debug/readonly/deleted").exists());
    assert_eq!(mode(&repo, "target/debug/readonly"), 0o555);
    assert_eq!(mode(&repo, "target/debug/executable"), 0o755);
    assert_eq!(
        time(&repo, "target/debug/executable"),
        SystemTime::UNIX_EPOCH + Duration::from_secs(1)
    );
    assert_eq!(
        fs::read_to_string(repo.root().join("target/debug/executable")).unwrap_or_default(),
        "same bytes"
    );
    assert!(repo.root().join("target/debug/file-to-directory").is_dir());
    assert_eq!(
        fs::read_to_string(repo.root().join("target/debug/directory-to-file")).unwrap_or_default(),
        "now file"
    );
    assert!(repo.root().join("target/debug/new-empty").is_dir());
    assert!(!repo.root().join("target/debug/deleted-empty").exists());
    assert!(!repo.root().join("delta").exists());
}

#[test]
fn delta_overlay_replaces_read_only_hardlinks_without_changing_their_peers() {
    let repo = Repo::new("delta-hardlinks");
    repo.write_unstaged("outside/peer", "base artifact");
    valid_delta(&repo);
    fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.write_unstaged("target/debug/placeholder", "base");
    fs::hard_link(
        repo.root().join("outside/peer"),
        repo.root().join("target/debug/current"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    chmod(&repo, "target/debug/current", 0o444);
    chmod(&repo, "target/debug", 0o555);
    success(&run(&repo, "apply", "base").0);
    assert_eq!(
        fs::read_to_string(repo.root().join("outside/peer")).unwrap_or_default(),
        "base artifact"
    );
    assert_eq!(mode(&repo, "outside/peer"), 0o444);
    assert_eq!(
        fs::read_to_string(repo.root().join("target/debug/current")).unwrap_or_default(),
        "current artifact"
    );
}

#[test]
fn corrupt_delta_payloads_are_rejected_whole_before_overlay_and_cold_runs_remain_stable() {
    let repo = Repo::new("delta-corruption");
    repo.write_unstaged("outside/untouched", "safe");
    valid_delta(&repo);
    copy_tree(&repo, "delta", "remote");
    for corruption in [
        "truncated",
        "missing-ancestor",
        "missing-payload",
        "node-kind",
        "missing-removals",
        "extra",
        "extra-empty-metadata",
        "hash",
        "mode",
        "mtime",
    ] {
        copy_tree(&repo, "remote", "delta");
        let inode = |path| {
            fs::metadata(repo.root().join(path))
                .unwrap_or_else(|e| unreachable!("{e}"))
                .ino()
        };
        assert_ne!(
            inode("delta/files/debug/current"),
            inode("remote/files/debug/current")
        );
        corrupt(&repo, corruption);
        fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
        repo.write_unstaged("target/debug/obsolete", "base artifact");
        fs::hard_link(
            repo.root().join("outside/untouched"),
            repo.root().join("target/debug/current"),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        let (output, _) = run(&repo, "apply", "base");
        success(&output);
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("building cold"),
            "{corruption}"
        );
        assert!(
            !repo.root().join("target/debug/current").exists(),
            "{corruption}"
        );
        assert!(!repo.root().join("delta").exists());
        assert_eq!(
            fs::read_to_string(repo.root().join("outside/untouched")).unwrap_or_default(),
            "safe"
        );
        success(&run(&repo, "apply", "base").0);
        assert!(!repo.root().join("target/debug/current").exists());
        repo.write_unstaged("target/debug/current", "healthy cold build");
        compiled(&repo);
        let (output, saved) = run(&repo, "stage", "base");
        success(&output);
        assert_eq!(saved, "save=true\n");
    }
}

fn corrupt(repo: &Repo, corruption: &str) {
    match corruption {
        "missing-payload" => fs::remove_dir_all(repo.root().join("delta/files"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        "truncated" => {
            fs::write(repo.root().join("delta/files/debug/current"), b"short")
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        "extra-empty-metadata" => {
            fs::create_dir_all(repo.root().join("delta/files/.velnor/unlisted"))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        "extra" => repo.write_unstaged("delta/files/debug/extra", "unlisted"),
        _ => {
            let mut metadata = manifest(repo);
            match corruption {
                "missing-ancestor" => {
                    metadata["nodes"]
                        .as_object_mut()
                        .unwrap_or_else(|| unreachable!("manifest nodes"))
                        .remove("debug");
                }
                "node-kind" => {
                    metadata["nodes"]["debug/current"] = serde_json::json!({
                        "kind": "Directory",
                        "mode": metadata["nodes"]["debug/current"]["mode"],
                        "modified": metadata["nodes"]["debug/current"]["modified"]
                    });
                }
                "missing-removals" => {
                    metadata
                        .as_object_mut()
                        .unwrap_or_else(|| unreachable!("manifest object"))
                        .remove("removed");
                }
                "hash" => {
                    let digest = &mut metadata["nodes"]["debug/current"]["digest"][0];
                    let original = digest
                        .as_u64()
                        .unwrap_or_else(|| unreachable!("digest byte"));
                    *digest = serde_json::json!((original + 1) % 256);
                }
                "mode" => metadata["nodes"]["debug/current"]["mode"] = serde_json::json!(0),
                "mtime" => metadata["nodes"]["debug/current"]["modified"] = serde_json::json!(0),
                _ => unreachable!("unknown corruption"),
            }
            write_manifest(repo, &metadata);
        }
    }
}

#[test]
fn unsupported_build_outputs_decline_delta_saving_without_failing_the_build() {
    for kind in ["symlink", "fifo"] {
        let repo = Repo::new("delta-unsupported-output");
        repo.write_unstaged("outside/untouched", "safe");
        success(&run(&repo, "apply", "base").0);
        let path = repo.root().join("target/unsupported");
        if kind == "symlink" {
            std::os::unix::fs::symlink(repo.root().join("outside/untouched"), &path)
                .unwrap_or_else(|e| unreachable!("{e}"));
        } else {
            success(
                &std::process::Command::new("mkfifo")
                    .arg(&path)
                    .output()
                    .unwrap_or_else(|e| unreachable!("{e}")),
            );
        }
        compiled(&repo);
        let (output, saved) = run(&repo, "stage", "base");
        success(&output);
        assert_eq!(saved, "save=false\n");
        assert!(String::from_utf8_lossy(&output.stdout).contains("::notice::"));
        assert!(!repo.root().join("delta").exists());
        assert_eq!(
            fs::read_to_string(repo.root().join("outside/untouched")).unwrap_or_default(),
            "safe"
        );
    }
}
