// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use serde_json::{Value, json};
use velnor_runner::exec::{AdapterRegistry, JobCancel, JobResult};

use crate::exec_builder::{execute, job, literal, mapping, run, with};
use crate::exec_support::Scratch;

fn run_defaults(pairs: &[(&str, &str)]) -> Value {
    let entries: Vec<(&str, Value)> = pairs
        .iter()
        .map(|(name, value)| (*name, literal(value)))
        .collect();
    mapping(&[("run", mapping(&entries))])
}

#[tokio::test]
async fn a_later_defaults_entry_with_run_replaces_the_whole_run_dictionary() -> Result<()> {
    let scratch = Scratch::new()?;
    std::fs::create_dir_all(scratch.workspace().join("sub")).context("create the directory")?;
    let message = with(
        job(vec![run("a", "echo shell-ok\npwd")]),
        "Defaults",
        json!([
            run_defaults(&[("shell", "pwsh")]),
            run_defaults(&[("working-directory", "sub")])
        ]),
    );
    let (finished, lines) = execute(
        &message,
        &scratch,
        &AdapterRegistry::empty(),
        JobCancel::new(),
    )
    .await?;
    assert_eq!(finished.result, JobResult::Succeeded, "{lines:?}");
    assert!(lines.iter().any(|line| line == "shell-ok"), "{lines:?}");
    assert!(lines.iter().any(|line| line.ends_with("/sub")), "{lines:?}");
    Ok(())
}
