// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::Path};

use velnor_actions_mise::source_preview::verify_only_command;

#[test]
fn runtime_verifier_requires_fixed_program_and_valid_identity() -> Result<(), Box<dyn Error>> {
    let source = "a".repeat(40);
    let command = verify_only_command(
        &source,
        123,
        2,
        Path::new("/usr/bin/gh"),
        Path::new("/tmp"),
        "test_token",
    )?;
    let argv = command.argv();
    assert_eq!(argv[0], "/usr/bin/python3");
    assert_eq!(argv[1], "-I");
    assert_eq!(argv[2], "-S");
    assert_eq!(argv[3], "-c");
    assert_eq!(argv[8], source.as_str());
    assert_eq!(argv[9], "123");
    assert_eq!(argv[10], "2");
    assert_eq!(argv[11], "/usr/bin/gh");
    assert_eq!(argv[12], "/tmp");
    assert!(
        command
            .full_env()
            .iter()
            .any(|(key, value)| key == "GH_TOKEN" && value == "test_token")
    );
    assert!(
        verify_only_command(
            "bad",
            123,
            2,
            Path::new("/usr/bin/gh"),
            Path::new("/tmp"),
            "test_token"
        )
        .is_err()
    );
    assert!(
        verify_only_command(
            &source,
            0,
            2,
            Path::new("/usr/bin/gh"),
            Path::new("/tmp"),
            "test_token"
        )
        .is_err()
    );
    assert!(
        verify_only_command(
            &source,
            123,
            2,
            Path::new("gh"),
            Path::new("/tmp"),
            "test_token"
        )
        .is_err()
    );
    assert!(
        verify_only_command(
            &source,
            123,
            2,
            Path::new("/usr/bin/gh"),
            Path::new("/tmp"),
            ""
        )
        .is_err()
    );
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn runtime_verifier_rejects_missing_credentials_without_output() -> Result<(), Box<dyn Error>> {
    let command = verify_only_command(
        &"a".repeat(40),
        123,
        2,
        Path::new("/usr/bin/gh"),
        Path::new("/tmp"),
        "test_token",
    )?;
    let argv = command.argv();
    let output = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .env_clear()
        .output()?;
    assert!(
        !output.status.success(),
        "runtime verification accepted missing credentials"
    );
    assert!(
        output.stdout.is_empty(),
        "runtime verification emitted unauthenticated identity"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("source_preview_admission_rejected"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
