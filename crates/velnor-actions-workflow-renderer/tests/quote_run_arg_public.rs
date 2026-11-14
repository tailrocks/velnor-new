// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::error::Error;

use velnor_actions_workflow_renderer::quote_run_arg;

#[test]
fn quoted_arguments_with_apostrophes_round_trip_through_bash() -> Result<(), Box<dyn Error>> {
    for value in [
        "'",
        "''",
        "a'b",
        "a'b c",
        "'leading",
        "trailing'",
        "$",
        "a$ b",
        "$'x",
        "\\",
        "a\\b",
        "\"",
        "a'\"b",
        "a\\'b",
        "'$\"\\x'",
    ] {
        let command = format!("/usr/bin/printf %s {}", quote_run_arg(value)?);
        let output = std::process::Command::new("/bin/bash")
            .args(["-c", &command])
            .env_clear()
            .output()?;
        assert!(
            output.status.success(),
            "generated shell failed for {value:?}"
        );
        assert_eq!(output.stdout, value.as_bytes());
    }
    Ok(())
}
