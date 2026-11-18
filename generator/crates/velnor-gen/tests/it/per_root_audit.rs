// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView};
use velnor_rust::{LOCK_PATH, pin_lock};

use crate::facts::TestResult;
use crate::lock::{COMMIT_FILE, SUMS};
use crate::pipeline::{view, workflow};

fn two_roots(version: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(
        &["Cargo.toml", "lab/Cargo.toml"],
        Some(b"full_run = \"nightly\"\n"),
    )?;
    repo.tracked_paths.insert(LOCK_PATH);
    repo.files.insert(
        LOCK_PATH,
        pin_lock(version, COMMIT_FILE, SUMS)?.into_bytes(),
    );
    repo.tracked_paths
        .insert(RepoPath::new(".velnor/rust-ci.toml")?);
    Ok(repo)
}

#[test]
fn a_helper_with_the_per_root_audit_audits_each_root_in_its_own_lane() -> TestResult {
    let text = crate::archive::plain_invocations(&workflow(&two_roots("0.1.82")?)?);
    assert!(!text.contains("rust-inputs-audit"), "{text}");
    assert!(
        !text.contains("cargo build --workspace --all-targets"),
        "{text}"
    );
    assert!(
        text.contains("run: velnor-ci inputs audit --root .\n"),
        "{text}"
    );
    assert!(
        text.contains("run: velnor-ci inputs audit --root lab\n"),
        "{text}"
    );
    Ok(())
}

#[test]
fn an_older_helper_keeps_the_separate_audit_job() -> TestResult {
    let text = crate::archive::plain_invocations(&workflow(&two_roots("0.1.81")?)?);
    assert!(text.contains("rust-inputs-audit:"), "{text}");
    assert!(
        text.contains("cargo build --workspace --all-targets"),
        "{text}"
    );
    assert!(!text.contains("--root"), "{text}");
    Ok(())
}
