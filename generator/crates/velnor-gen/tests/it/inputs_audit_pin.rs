// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::pipeline::workflow;

#[test]
fn an_inputs_audit_requires_a_helper_with_pre_build_inventories() -> TestResult {
    for version in ["0.1.79", "0.1.80"] {
        let mut repo = crate::lock::locked(version)?;
        repo.files.insert(
            RepoPath::new(".velnor/rust-ci.toml")?,
            b"full_run = \"nightly\"\n".to_vec(),
        );
        let error = workflow(&repo).err().ok_or("old helper accepted")?;
        assert!(error.to_string().contains("pin-helper 0.1.81"), "{error}");
    }
    let mut repo = crate::lock::locked("0.1.81")?;
    repo.files.insert(
        RepoPath::new(".velnor/rust-ci.toml")?,
        b"full_run = \"nightly\"\n".to_vec(),
    );
    let text = crate::archive::plain_invocations(&workflow(&repo)?);
    assert!(
        text.contains("velnor-ci run -- velnor-ci inputs build -- cargo-clippy"),
        "{text}"
    );
    assert!(
        text.contains("velnor-ci run -- velnor-ci inputs build -- cargo-nextest nextest run"),
        "{text}"
    );
    assert!(text.contains("run: velnor-ci inputs audit\n"), "{text}");
    let mut off = crate::lock::locked("0.1.80")?;
    off.files.insert(
        RepoPath::new(".velnor/rust-ci.toml")?,
        b"full_run = \"off\"\n".to_vec(),
    );
    off.tracked_paths
        .insert(RepoPath::new(".velnor/rust-ci.toml")?);
    let text = workflow(&off)?;
    assert!(!text.contains("inputs audit"), "{text}");
    assert!(!text.contains("inputs build"), "{text}");
    Ok(())
}
