// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::error::Error;

use velnor_actions_workflow_renderer::quote_run_arg;

fn run_quoted(value: &str, env: &[(&str, &str)]) -> Result<Vec<u8>, Box<dyn Error>> {
    let command = format!("/usr/bin/printf %s {}", quote_run_arg(value)?);
    let mut shell = std::process::Command::new("/bin/bash");
    shell.args(["-c", &command]).env_clear();
    for (name, val) in env {
        shell.env(name, val);
    }
    let output = shell.output()?;
    assert!(
        output.status.success(),
        "generated shell failed for {value:?}"
    );
    Ok(output.stdout)
}

#[test]
fn literal_arguments_with_apostrophes_quotes_and_dollar_signs_round_trip_through_bash()
-> Result<(), Box<dyn Error>> {
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
        assert_eq!(run_quoted(value, &[])?, value.as_bytes(), "value {value:?}");
    }
    Ok(())
}

#[test]
fn dollar_names_are_shell_expansions_not_literals() -> Result<(), Box<dyn Error>> {
    let env = [("HOME", "/velnor-home")];
    assert_eq!(run_quoted("$HOME", &env)?, b"/velnor-home");
    assert_eq!(run_quoted("a$HOME/x", &env)?, b"a/velnor-home/x");
    assert_eq!(run_quoted("a$b", &env)?, b"a");
    Ok(())
}
