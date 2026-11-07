// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::{error::Error, path::Path, process::Command};

#[test]
fn protected_preview_identity_survives_plan_report_and_required_boundaries()
-> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("python3")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(root.join("crates/velnor-actions-cli/tests/preview_cross_job_identity_scenarios.py"))
        .arg(&root)
        .arg(env!("CARGO_BIN_EXE_velnor-actions"))
        .arg(env!("CARGO_PKG_VERSION"))
        .output()?;
    assert!(
        output.status.success(),
        "protected preview Plan crossed an identity boundary: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
