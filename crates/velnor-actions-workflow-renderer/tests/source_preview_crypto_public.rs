// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::Path, process::Command};

#[test]
fn pinned_gh_verifies_real_production_signature_offline() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("python3")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(root.join(
            "crates/velnor-actions-workflow-renderer/tests/source_preview_crypto_scenarios.py",
        ))
        .arg(&root)
        .output()?;
    assert!(
        output.status.success(),
        "actual gh signature verification failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
