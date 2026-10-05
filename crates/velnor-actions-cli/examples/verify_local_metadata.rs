//! Native metadata reader used by `scripts/verify-local.sh`.

#[cfg(not(test))]
use std::io::{self, Read};

use serde_json::Value;

/// Select sorted package names from Cargo's metadata JSON.
pub(crate) fn package_names(
    metadata_json: &str,
    libraries_only: bool,
) -> Result<Vec<String>, String> {
    let metadata: Value = serde_json::from_str(metadata_json)
        .map_err(|error| format!("parse cargo metadata JSON: {error}"))?;
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| "cargo metadata JSON is missing the packages array".to_owned())?;
    let mut names = Vec::with_capacity(packages.len());
    for package in packages {
        if libraries_only && !has_library_target(package)? {
            continue;
        }
        let name = package
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "cargo metadata package is missing its name".to_owned())?;
        names.push(name.to_owned());
    }
    names.sort_unstable();
    Ok(names)
}

/// Match Python's former `"lib" in target.get("kind", [])` selection.
fn has_library_target(package: &Value) -> Result<bool, String> {
    let targets = package
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| "cargo metadata package is missing its targets array".to_owned())?;
    Ok(targets.iter().any(|target| {
        target
            .get("kind")
            .and_then(Value::as_array)
            .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("lib")))
    }))
}

#[cfg(not(test))]
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("verify-local metadata: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(test))]
fn run() -> Result<(), String> {
    let selection = std::env::args()
        .nth(1)
        .ok_or_else(|| "expected `packages` or `libraries` selection".to_owned())?;
    let libraries_only = match selection.as_str() {
        "packages" => false,
        "libraries" => true,
        _ => return Err(format!("unknown selection `{selection}`")),
    };
    let mut metadata_json = String::new();
    io::stdin()
        .read_to_string(&mut metadata_json)
        .map_err(|error| format!("read cargo metadata JSON: {error}"))?;
    let names = package_names(&metadata_json, libraries_only)?;
    println!("{}", names.join(" "));
    Ok(())
}
