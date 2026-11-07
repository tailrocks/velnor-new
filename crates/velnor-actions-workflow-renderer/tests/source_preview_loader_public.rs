// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::{error::Error, path::Path, process::Command};

#[test]
fn verified_loader_rejects_tampering_and_executes_only_verified_bytes() -> Result<(), Box<dyn Error>>
{
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("/usr/bin/python3")
        .arg(root.join("tests/source_preview_loader_scenarios.py"))
        .arg(root)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
