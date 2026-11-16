// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::cli::CheckConfigurationsArgs;
use crate::configurations::{Configuration, ResolvedFeatures, requests, resolved};
use crate::error::CiError;

pub(super) fn check(args: &CheckConfigurationsArgs) -> Result<String, CiError> {
    let configurations = match &args.configurations {
        Some(path) => {
            let bytes = fs::read(path).map_err(|e| {
                CiError::new(format!(
                    "cannot read configurations {}: {e}",
                    path.display()
                ))
            })?;
            requests(&bytes)?
        }
        None => vec![Configuration::default()],
    };
    let mut resolutions: Vec<ResolvedFeatures> = Vec::new();
    for configuration in &configurations {
        let output = cargo(&args.manifest_path, configuration, "metadata")
            .output()
            .map_err(|e| CiError::new(format!("cannot run cargo metadata: {e}")))?;
        if !output.status.success() {
            return Err(CiError::new(format!(
                "cargo metadata for {} failed with {}: {}",
                configuration.name,
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        resolutions.push(resolved(&output.stdout).map_err(|e| e.context(&configuration.name))?);
    }
    let mut representatives: Vec<usize> = Vec::new();
    for (index, configuration) in configurations.iter().enumerate() {
        let representative = representatives.iter().copied().find(|&previous| {
            resolutions[previous].equivalent(
                &resolutions[index],
                &configurations[previous],
                configuration,
            )
        });
        if let Some(previous) = representative {
            println!(
                "{} -> {}",
                configuration.name, configurations[previous].name
            );
        } else {
            representatives.push(index);
            println!("{} -> {}", configuration.name, configuration.name);
        }
    }
    println!(
        "{} configurations, {} checks",
        configurations.len(),
        representatives.len()
    );
    for index in representatives {
        let configuration = &configurations[index];
        let status = cargo(&args.manifest_path, configuration, "check")
            .status()
            .map_err(|e| CiError::new(format!("cannot run cargo check: {e}")))?;
        if !status.success() {
            return Err(CiError::new(format!(
                "cargo check for {} failed with {status}",
                configuration.name
            )));
        }
    }
    Ok(String::new())
}

fn cargo(manifest: &Path, configuration: &Configuration, operation: &str) -> Command {
    let mut command = Command::new("cargo");
    command.arg(operation);
    if operation == "metadata" {
        command.args(["--format-version", "1"]);
    } else {
        command.args(["--workspace", "--all-targets"]);
    }
    command
        .args(["--locked", "--offline", "--manifest-path"])
        .arg(manifest);
    if configuration.no_default_features {
        command.arg("--no-default-features");
    }
    if !configuration.features.is_empty() {
        command
            .arg("--features")
            .arg(configuration.features.join(","));
    }
    command
}
