// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;
use std::process::Command;

#[test]
fn native_validator_qualification_rejects_non_manual_events_before_io() -> Result<(), Box<dyn Error>>
{
    let temporary = tempfile::tempdir()?;
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/generator-release/source-qualification.sh");
    let output = Command::new("/bin/bash")
        .arg(script)
        .arg("qualify-linux-x64-native-validator-tools")
        .env("RUNNER_TEMP", temporary.path())
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("PATH", temporary.path())
        .output()?;

    assert_eq!(output.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires a workflow_dispatch event"));
    assert!(
        !temporary
            .path()
            .join("source-qualification-artifacts")
            .exists()
    );
    assert!(
        !temporary
            .path()
            .join("velnor-source-qualification-validator-archives")
            .exists()
    );
    assert!(
        !temporary
            .path()
            .join("velnor-source-qualification-validator-stage")
            .exists()
    );
    Ok(())
}
