// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::pipeline::workflow;

#[test]
fn an_inputs_audit_records_pre_build_inventories() -> TestResult {
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
    Ok(())
}
