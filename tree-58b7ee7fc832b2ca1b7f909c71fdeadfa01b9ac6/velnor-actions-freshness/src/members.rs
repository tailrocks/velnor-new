//! Bounded Cargo package listing for the local verification script.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use crate::process::run_bounded;

const METADATA_CAP: usize = 512 * 1024;
const METADATA_TIMEOUT: Duration = Duration::from_secs(30);

/// Print sorted Cargo package names through the existing private CLI operation.
pub(crate) fn print_workspace_members(root: &Path, libraries_only: bool) -> i32 {
    match cargo_members(root, libraries_only) {
        Ok(members) => {
            println!("{}", members.join(" "));
            0
        }
        Err(error) => {
            eprintln!("repository policy: {error}");
            1
        }
    }
}

fn cargo_members(root: &Path, libraries_only: bool) -> Result<Vec<String>, String> {
    let stdout = cargo_metadata(root)?;
    let metadata: Value = serde_json::from_slice(&stdout)
        .map_err(|error| format!("cargo metadata output was invalid JSON ({error})"))?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or_else(|| "cargo metadata had no packages array".to_owned())?;
    let mut names = packages
        .iter()
        .filter(|package| !libraries_only || has_library_target(package))
        .filter_map(|package| package["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    if names.is_empty() {
        return Err("cargo metadata returned no matching package names".to_owned());
    }
    Ok(names)
}

fn cargo_metadata(root: &Path) -> Result<Vec<u8>, String> {
    let output = run_bounded(
        Command::new("cargo")
            .args([
                "metadata",
                "--locked",
                "--no-deps",
                "--format-version",
                "1",
                "--offline",
            ])
            .current_dir(root),
        METADATA_CAP,
        METADATA_TIMEOUT,
    )
    .map_err(|error| format!("cargo metadata failed ({error})"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output.stdout)
}

fn has_library_target(package: &Value) -> bool {
    package["targets"].as_array().is_some_and(|targets| {
        targets.iter().any(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("lib")))
        })
    })
}
