// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, FileTimes};
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use super::{audit, own_units, unit, workspace_below_the_root};

#[test]
fn a_later_workspaces_target_creation_does_not_stale_a_repository_read() {
    let repo = workspace_below_the_root();
    repo.write(
        "second/Cargo.toml",
        "[package]\nname = \"second\"\nedition = \"2024\"\n[workspace]\n",
    );
    repo.write("second/src/lib.rs", "");
    repo.write("second/Cargo.lock", "version = 4\n");
    own_units(&repo, "rust/target/debug", &[]);
    let output = "rust/target/debug/build/lib-f000000000000008/output";
    unit(&repo, output, "cargo:rerun-if-changed=../..\n");
    let built = UNIX_EPOCH + Duration::from_secs(3_000_000_001);
    repo.set_mtime(output, built);
    File::open(repo.root().join("second"))
        .and_then(|file| file.set_times(FileTimes::new().set_modified(built)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    for (hash, fingerprint) in [("001", "lib-second"), ("002", "test-lib-second")] {
        unit(
            &repo,
            &format!("second/target/debug/deps/second-f000000000000{hash}.d"),
            "out.d: src/lib.rs\n",
        );
        unit(
            &repo,
            &format!("second/target/debug/.fingerprint/second-f000000000000{hash}/{fingerprint}"),
            "",
        );
    }
    File::open(repo.root().join("second"))
        .and_then(|file| {
            file.set_times(
                FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(4_000_000_000)),
            )
        })
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(repo.mtime("second") > built);

    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        ".\tlib\n"
    );
}

#[test]
fn a_newer_descendant_of_an_explicit_target_directory_prevents_writing_the_inputs_lock() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    let output = "rust/target/debug/build/lib-f000000000000008/output";
    unit(
        &repo,
        output,
        "cargo:rerun-if-changed=../target/generated\n",
    );
    let built = UNIX_EPOCH + Duration::from_secs(3_000_000_001);
    repo.set_mtime(output, built);
    unit(&repo, "rust/target/generated/nested/value.txt", "newer");
    repo.set_mtime(
        "rust/target/generated/nested/value.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    let old = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
    for directory in ["rust/target/generated", "rust/target/generated/nested"] {
        File::open(repo.root().join(directory))
            .and_then(|file| file.set_times(FileTimes::new().set_modified(old)))
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(repo.mtime(directory), old);
    }

    let refused = audit(&repo, &["--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_target_directory_containing_tracked_inputs_cannot_hide_changed_sources() {
    let repo = workspace_below_the_root();
    let target = "debug";
    own_units(&repo, target, &[]);
    let output = format!("{target}/build/lib-f000000000000008/output");
    unit(&repo, &output, "cargo:rerun-if-changed=../../shared\n");
    repo.set_mtime(&output, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    repo.set_mtime(
        "shared/build.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    let refused = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .env("CARGO_TARGET_DIR", repo.root())
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn newer_configured_target_artifacts_do_not_hide_a_repository_read() {
    let repo = workspace_below_the_root();
    let target = "custom-target/debug";
    own_units(&repo, target, &[]);
    let output = format!("{target}/build/lib-f000000000000008/output");
    unit(&repo, &output, "cargo:rerun-if-changed=../..\n");
    repo.set_mtime(&output, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    for artifact in ["custom-target/newer-artifact", "rust/target/newer-artifact"] {
        unit(&repo, artifact, "");
        repo.set_mtime(artifact, UNIX_EPOCH + Duration::from_secs(4_000_000_000));
    }
    let written = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .env("CARGO_TARGET_DIR", repo.root().join("custom-target"))
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        ".\tlib\n"
    );
}

#[test]
fn repeated_directory_declarations_share_the_audit_scan_budget() {
    let repo = workspace_below_the_root();
    populate(&repo, "shared/many", 50_001);
    repo.write("shared/many/read.txt", "read");
    repo.set_mtime(
        "shared/many/read.txt",
        UNIX_EPOCH + Duration::from_secs(2_500_000_000),
    );
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    let earlier = format!("{target}/build/app-f000000000000018/output");
    unit(
        &repo,
        &earlier,
        "cargo:rerun-if-changed=../../shared/many\n",
    );
    repo.set_mtime(&earlier, UNIX_EPOCH + Duration::from_secs(2_000_000_000));
    for (hash, declared) in [("008", "../../shared/many"), ("018", "../../shared/./many")] {
        let output = format!("{target}/build/lib-f000000000000{hash}/output");
        unit(
            &repo,
            &output,
            &format!("cargo:rerun-if-changed={declared}\n"),
        );
        repo.set_mtime(&output, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    }
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        "shared/many\tlib\n"
    );
}

#[test]
fn distinct_declared_trees_cannot_exceed_the_audit_scan_budget() {
    let repo = workspace_below_the_root();
    for directory in ["shared/first", "shared/second"] {
        populate(&repo, directory, 50_001);
    }
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    for (hash, declared) in [
        ("008", "../../shared/first"),
        ("018", "../../shared/second"),
    ] {
        let output = format!("{target}/build/lib-f000000000000{hash}/output");
        unit(
            &repo,
            &output,
            &format!("cargo:rerun-if-changed={declared}\n"),
        );
        repo.set_mtime(&output, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    }
    let refused = audit(&repo, &["--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("freshness scan limit"),
        "{refused:?}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

fn populate(repo: &crate::io::Repo, directory: &str, count: usize) {
    let directory = repo.root().join(directory);
    fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    let seed = directory.join("seed");
    fs::write(&seed, "").unwrap_or_else(|e| unreachable!("{e}"));
    for index in 0..count {
        fs::hard_link(&seed, directory.join(index.to_string()))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
}

#[test]
fn a_deleted_tracked_dep_info_input_prevents_writing_the_inputs_lock() {
    let repo = workspace_below_the_root();
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    unit(
        &repo,
        &format!("{target}/deps/app-f000000000000001.d"),
        "out.d: app/src/lib.rs ../shared/greeting.txt\n",
    );
    fs::remove_file(repo.root().join("shared/greeting.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"));

    let deleted = audit(&repo, &["--write"]);
    assert_eq!(deleted.status.code(), Some(1), "{deleted:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_deleted_declared_directory_with_tracked_inputs_prevents_writing_the_inputs_lock() {
    let repo = workspace_below_the_root();
    repo.write("shared/assets/nested/logo.txt", "original");
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    unit(
        &repo,
        &format!("{target}/build/lib-f000000000000008/output"),
        "cargo:rerun-if-changed=../../shared/assets\n",
    );
    fs::remove_dir_all(repo.root().join("shared/assets")).unwrap_or_else(|e| unreachable!("{e}"));

    let deleted = audit(&repo, &["--write"]);
    assert_eq!(deleted.status.code(), Some(1), "{deleted:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_changed_file_inside_a_declared_directory_prevents_writing_the_inputs_lock() {
    let repo = workspace_below_the_root();
    repo.write("shared/assets/nested/logo.txt", "original");
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    let output = format!("{target}/build/lib-f000000000000008/output");
    unit(
        &repo,
        &output,
        "cargo:rerun-if-changed=../../shared/assets\n",
    );
    let old = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
    repo.set_mtime(&output, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    for directory in ["shared/assets", "shared/assets/nested"] {
        File::open(repo.root().join(directory))
            .and_then(|file| file.set_times(FileTimes::new().set_modified(old)))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    repo.write_unstaged("shared/assets/nested/logo.txt", "changed");
    repo.set_mtime(
        "shared/assets/nested/logo.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    assert_eq!(repo.mtime("shared/assets"), old);
    assert_eq!(repo.mtime("shared/assets/nested"), old);

    let changed = audit(&repo, &["--write"]);
    assert_eq!(changed.status.code(), Some(1), "{changed:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}
