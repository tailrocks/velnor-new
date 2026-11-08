// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, process::Command};

pub(crate) fn materialized_source(runs: &[String]) -> Result<String, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let env_file = temp.path().join("env");
    fs::write(&env_file, "")?;
    let mut directory = String::new();
    for run in runs {
        let output = Command::new("/bin/bash")
            .args(["--noprofile", "--norc", "-e", "-o", "pipefail", "-c", run])
            .env("RUNNER_TEMP", temp.path())
            .env("GITHUB_ENV", &env_file)
            .env("VELNOR_FIXED_SCRIPT_DIR", &directory)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "materializer failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        if let Some(value) = fs::read_to_string(&env_file)?
            .lines()
            .find_map(|line| line.strip_prefix("VELNOR_FIXED_SCRIPT_DIR="))
        {
            value.clone_into(&mut directory);
        }
    }
    Ok(fs::read_to_string(format!("{directory}/script.sh"))?)
}

pub(crate) fn materializer_runs(workflow: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let mut runs = Vec::new();
    let mut active = false;
    for line in workflow.lines() {
        if line.contains("name: Materialize fixed script") {
            active = true;
        } else if active
            && line.trim_start().starts_with("- name:")
            && !line.contains("name: Write fixed authority source script.sh")
        {
            break;
        }
        if active && let Some(value) = line.trim_start().strip_prefix("run: ") {
            runs.push(serde_json::from_str(value)?);
        }
    }
    if runs.is_empty() {
        return Err("missing materializers".into());
    }
    Ok(runs)
}

pub(crate) fn expose_materialized_sources(workflow: &str) -> Result<String, Box<dyn Error>> {
    let mut output = String::new();
    let mut runs = Vec::new();
    let mut collecting = false;
    let mut executor = false;
    for line in workflow.lines() {
        if line.contains("name: Materialize fixed script") {
            runs.clear();
            collecting = true;
        }
        if line.contains("name: Materialize fixed image authority implementation") {
            collecting = false;
            executor = true;
        }
        if let Some(value) = line.trim_start().strip_prefix("run: ") {
            if collecting {
                runs.push(serde_json::from_str(value)?);
            }
            if executor {
                let indent = line.len() - line.trim_start().len();
                output.push_str(&" ".repeat(indent));
                output.push_str("run: ");
                output.push_str(&serde_json::to_string(&materialized_source(&runs)?)?);
                output.push('\n');
                executor = false;
                continue;
            }
        }
        output.push_str(line);
        output.push('\n');
    }
    Ok(output)
}
