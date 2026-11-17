// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File};
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, SystemTime};

use crate::delta::{
    compiled, copy_tree, manifest, run, set_time, success, time, valid_delta, write_manifest,
};
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
        if generation > 1 {
            copy_tree(&repo, "remote", "delta");
        }
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
        staged_outputs(&repo, generation);
        copy_tree(&repo, "delta", "remote");
        fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    }
}

fn staged_outputs(repo: &Repo, generation: u32) {
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
}

#[test]
fn invalid_removals_cannot_delete_outside_files_and_invalid_inventories_refuse_saving() {
    let repo = Repo::new("delta-removal-validation");
    repo.write_unstaged("outside/untouched", "safe");
    let absolute = repo.root().join("outside/untouched");
    valid_delta(&repo);
    copy_tree(&repo, "delta", "remote");
    for removal in [
        "../outside/untouched".to_owned(),
        absolute.to_string_lossy().into_owned(),
        "link/untouched".to_owned(),
    ] {
        copy_tree(&repo, "remote", "delta");
        let mut metadata = manifest(&repo);
        metadata["removed"] = serde_json::json!(["debug/valid", removal]);
        write_manifest(&repo, &metadata);
        if repo.root().join("target").exists() {
            fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
        }
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
    for invalid in ["missing", "invalid json", "oversized", "fifo"] {
        success(&run(&repo, "apply", "base").0);
        invalid_metadata(&repo, "target/.velnor/delta-baseline.json", invalid);
        compiled(&repo);
        let (output, saved) = run(&repo, "stage", "base");
        success(&output);
        assert_eq!(saved, "save=false\n");
        assert!(!repo.root().join("delta").exists());
    }
}

#[test]
fn bounded_manifest_metadata_rejects_fifos_and_oversized_files() {
    let repo = Repo::new("delta-bounded-manifest");
    repo.write_unstaged("outside/untouched", "safe");
    let absolute = repo.root().join("outside/untouched");
    valid_delta(&repo);
    copy_tree(&repo, "delta", "remote");
    let valid =
        fs::read(repo.root().join("delta/manifest.json")).unwrap_or_else(|e| unreachable!("{e}"));
    for invalid in ["fifo", "oversized"] {
        copy_tree(&repo, "remote", "delta");
        let writer = bounded_metadata(&repo, invalid, &valid);
        fs::remove_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
        repo.write_unstaged("target/debug/obsolete", "base artifact");
        let (output, _) = run(&repo, "apply", "base");
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
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("building cold"),
            "{invalid}"
        );
        assert!(
            !repo.root().join("target/debug/current").exists(),
            "{invalid}"
        );
        assert_eq!(fs::read_to_string(&absolute).unwrap_or_default(), "safe");
        assert!(!repo.root().join("delta").exists());
    }
}

#[test]
fn inherited_metadata_nodes_are_replaced_without_following_nested_links() {
    let repo = Repo::new("delta-inherited-metadata");
    repo.write_unstaged("outside/untouched", "safe");
    success(&run(&repo, "apply", "base").0);
    inherited_metadata_is_replaced(&repo);
}

fn inherited_metadata_is_replaced(repo: &Repo) {
    let names = [
        "target/.velnor/delta-marker",
        "target/.velnor/delta-baseline.json",
    ];
    for name in names {
        for inherited in ["fifo", "symlink", "directory"] {
            invalid_metadata(repo, name, inherited);
            let reader = (inherited == "fifo").then(|| {
                File::options()
                    .read(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(repo.root().join(name))
                    .unwrap_or_else(|e| unreachable!("{e}"))
            });
            success(&run(repo, "apply", "base").0);
            drop(reader);
            assert_eq!(
                fs::read_to_string(repo.root().join("outside/untouched")).unwrap_or_default(),
                "safe"
            );
            for rewritten in names {
                assert!(
                    fs::symlink_metadata(repo.root().join(rewritten))
                        .unwrap_or_else(|e| unreachable!("{e}"))
                        .is_file(),
                    "{rewritten} must be replaced with a regular file after inheriting {inherited} at {name}"
                );
            }
            compiled(repo);
            let (output, saved) = run(repo, "stage", "base");
            success(&output);
            assert_eq!(saved, "save=true\n");
        }
    }
}

fn invalid_metadata(repo: &Repo, name: &str, invalid: &str) {
    let path = repo.root().join(name);
    if path.is_dir() {
        fs::remove_dir_all(&path).unwrap_or_else(|e| unreachable!("{e}"));
    }
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => unreachable!("{error}"),
    }
    match invalid {
        "missing" => {}
        "directory" => {
            fs::create_dir_all(&path).unwrap_or_else(|e| unreachable!("{e}"));
            std::os::unix::fs::symlink(repo.root().join("outside"), path.join("nested"))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
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

fn bounded_metadata(repo: &Repo, invalid: &str, valid: &[u8]) -> Option<std::process::Child> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let path = repo.root().join("delta/manifest.json");
    if invalid == "oversized" {
        let mut bytes = valid.to_vec();
        bytes.resize(16 * 1024 * 1024 + 1, b' ');
        fs::write(path, bytes).unwrap_or_else(|e| unreachable!("{e}"));
        return None;
    }
    invalid_metadata(repo, "delta/manifest.json", "fifo");
    let mut writer = Command::new("tee")
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    writer
        .stdin
        .take()
        .unwrap_or_else(|| unreachable!("FIFO input"))
        .write_all(valid)
        .unwrap_or_else(|e| unreachable!("{e}"));
    Some(writer)
}
