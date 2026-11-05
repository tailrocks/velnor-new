// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[test]
fn budget_capture_binds_job_identity_and_original_start() -> Result<(), Box<dyn std::error::Error>>
{
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let status = std::process::Command::new("python3")
        .arg(root.join("tests/release_budget_fixture.py"))
        .env("BUDGET_SCRIPT", root.join("src/release_budget.py"))
        .status()?;
    assert!(status.success());
    Ok(())
}
