// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{Duration, SystemTime};

use crate::delta::success;
use crate::delta_source::{delta, fixture, helper, next_push, prepare, save_inputs};

const SOURCE_A: &str = "fn main() {}\n";
const SOURCE_B: &str = "compile_error!(\"uncompiled future source\");\nfn main() {}\n";

#[test]
fn a_saved_future_source_cannot_be_staged_after_the_built_source_returns() {
    let (repo, expected) = fixture("delta-interleaved-inputs");
    let original = repo.mtime("src/main.rs");
    repo.write_unstaged("src/main.rs", SOURCE_B);
    repo.set_mtime("src/main.rs", original);
    save_inputs(&repo);
    repo.write_unstaged("src/main.rs", SOURCE_A);
    repo.set_mtime("src/main.rs", original);
    let decision = delta(&repo, "stage", Some(&expected));
    repo.write_unstaged("src/main.rs", SOURCE_B);
    repo.set_mtime("src/main.rs", original);
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
fn changed_snapshot_rows_cannot_restore_unbuilt_sources_to_old_times() {
    let (repo, _) = fixture("inputs-bound-rows");
    save_inputs(&repo);
    let path = repo.root().join("target/.velnor/inputs.tsv");
    let original = fs::read_to_string(&path).unwrap_or_else(|e| unreachable!("{e}"));
    repo.write_unstaged("src/main.rs", SOURCE_B);
    save_inputs(&repo);
    let changed = fs::read_to_string(&path).unwrap_or_else(|e| unreachable!("{e}"));
    let rows = changed
        .lines()
        .filter(|line| line.starts_with("src/main.rs\t"))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    let mut forged = original
        .lines()
        .map(|line| {
            if line.starts_with("src/main.rs\t") {
                rows[0]
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    forged.push('\n');
    fs::write(&path, forged).unwrap_or_else(|e| unreachable!("{e}"));
    let changed_time = SystemTime::UNIX_EPOCH + Duration::from_secs(4);
    repo.set_mtime("src/main.rs", changed_time);
    success(&helper(&repo, &["inputs", "restore"], None).0);
    assert_eq!(
        repo.mtime("src/main.rs"),
        changed_time,
        "unbound rows restored a timestamp"
    );
    assert!(
        !repo.root().join("target/debug/app").exists(),
        "invalid inputs did not discard cached build artifacts"
    );
}

#[test]
fn missing_snapshot_binding_cannot_authorize_a_delta_save() {
    let (repo, expected) = fixture("inputs-binding-required");
    save_inputs(&repo);
    let path = repo.root().join("target/.velnor/inputs.tsv");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| unreachable!("{e}"));
    let rows = text
        .lines()
        .filter(|line| line.starts_with("src/main.rs\t") || line.starts_with("Cargo."))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&path, rows).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(delta(&repo, "stage", Some(&expected)), "save=false\n");
    assert!(!repo.root().join("delta").exists());
}

#[test]
fn tracked_staging_inputs_survive_apply_stage_and_git_clean_with_path_aliases() {
    for alias in ["relative", "absolute", "parent"] {
        for verb in ["apply", "stage"] {
            let (repo, _) = fixture("tracked-delta-staging");
            let sentinel = "target.velnor-delta/sentinel";
            repo.write(sentinel, "tracked input must survive\n");
            repo.make_executable(sentinel);
            repo.commit();
            let expected = prepare(&repo);
            save_inputs(&repo);
            let time = repo.mtime(sentinel);
            let staging = match alias {
                "absolute" => repo.root().join("target.velnor-delta"),
                "parent" => repo.root().join("src/../target.velnor-delta"),
                _ => "target.velnor-delta".into(),
            };
            let (output, values) = helper(
                &repo,
                &[
                    "delta",
                    verb,
                    "--target-dir",
                    "target",
                    "--staging-dir",
                    staging.to_str().unwrap_or_else(|| unreachable!()),
                    "--package",
                    "app",
                ],
                Some(&expected),
            );
            assert!(
                repo.root().join("target/debug/app").is_file(),
                "collision changed the build directory"
            );
            assert!(
                !output.status.success() || values.contains("save=false"),
                "tracked staging was accepted: {}",
                String::from_utf8_lossy(&output.stdout)
            );
            repo.git(&["clean", "-fdx"]);
            assert_eq!(
                fs::read_to_string(repo.root().join(sentinel)).unwrap_or_default(),
                "tracked input must survive\n"
            );
            assert_eq!(repo.mtime(sentinel), time);
            assert_eq!(
                fs::metadata(repo.root().join(sentinel))
                    .unwrap_or_else(|e| unreachable!("{e}"))
                    .permissions()
                    .mode()
                    & 0o777,
                0o755
            );
            let diff = Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(repo.root())
                .output()
                .unwrap_or_else(|e| unreachable!("{e}"));
            success(&diff);
            assert!(
                diff.stdout.is_empty(),
                "{}",
                String::from_utf8_lossy(&diff.stdout)
            );
        }
    }
}
