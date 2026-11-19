// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use crate::io::Repo;

fn tracked_target_input_refusal_preserves_state(sentinel: &str, restored: &str) {
    let repo = Repo::new("tracked-delta-target");
    repo.write_unstaged(sentinel, "tracked input must survive\n");
    repo.make_executable(sentinel);
    repo.git(&["add", "--force", sentinel]);
    let time = repo.mtime(sentinel);
    let outputs = repo.root().join(".git/target-path-output");
    fs::write(&outputs, "existing=retained\n").unwrap_or_else(|e| unreachable!("{e}"));
    if restored == "corrupt" {
        repo.write_unstaged("delta/manifest.json", "invalid manifest");
        repo.write_unstaged("delta/sentinel", "staged input must survive\n");
    }
    for alias in ["relative", "absolute", "parent"] {
        for verb in ["apply", "prepare", "stage"] {
            let target = match alias {
                "absolute" => repo.root().join("target"),
                "parent" => repo.root().join("nested/../target"),
                _ => "target".into(),
            };
            let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
                .args(["delta", verb, "--target-dir"])
                .arg(target)
                .args(["--staging-dir", "delta", "--package", "app"])
                .current_dir(repo.root())
                .env("BASE_CACHE_KEY", "base")
                .env("GITHUB_OUTPUT", &outputs)
                .env("RUNNER_TEMP", repo.root())
                .output()
                .unwrap_or_else(|e| unreachable!("{e}"));
            assert_eq!(
                fs::read_to_string(repo.root().join(sentinel)).unwrap_or_default(),
                "tracked input must survive\n",
                "{verb} changed {sentinel} through {alias} with {restored} delta"
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
            assert!(!output.status.success(), "{output:?}");
            assert_eq!(
                fs::read_to_string(&outputs).unwrap_or_default(),
                "existing=retained\n"
            );
            if restored == "corrupt" {
                assert_eq!(
                    fs::read_to_string(repo.root().join("delta/sentinel")).unwrap_or_default(),
                    "staged input must survive\n"
                );
            } else {
                assert!(!repo.root().join("delta").exists());
            }
        }
    }
}

#[test]
fn tracked_delta_marker_survives_missing_restore_refusals() {
    tracked_target_input_refusal_preserves_state("target/.velnor/delta-marker", "missing");
}

#[test]
fn tracked_delta_marker_survives_corrupt_restore_refusals() {
    tracked_target_input_refusal_preserves_state("target/.velnor/delta-marker", "corrupt");
}

#[test]
fn tracked_delta_baseline_survives_missing_restore_refusals() {
    tracked_target_input_refusal_preserves_state("target/.velnor/delta-baseline.json", "missing");
}

#[test]
fn tracked_delta_baseline_survives_corrupt_restore_refusals() {
    tracked_target_input_refusal_preserves_state("target/.velnor/delta-baseline.json", "corrupt");
}

#[test]
fn tracked_arbitrary_target_input_survives_missing_restore_refusals() {
    tracked_target_input_refusal_preserves_state("target/nested/input.rs", "missing");
}

#[test]
fn tracked_arbitrary_target_input_survives_corrupt_restore_refusals() {
    tracked_target_input_refusal_preserves_state("target/nested/input.rs", "corrupt");
}
