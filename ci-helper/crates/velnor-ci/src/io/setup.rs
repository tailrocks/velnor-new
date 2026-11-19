// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use velnor_gen::io::read_files;
use velnor_model::{LOCK_OUTPUT_PREFIX, MisePlatform, RepoPath, RootSlug, ToolPins, Toolchain};
use velnor_rust::{
    BUILD_DRIVER, CONFIG_PATH, CompatInputs, METADATA_PATH, TOOLCHAIN_PLAIN, TOOLCHAIN_TOML,
    build_profile, ci_debuginfo, compat, lock_key, mise_files, recorded_members, tool_pins,
    toolchain,
};

use super::build_inputs::{image_identity, release_of, repository_profile};
use super::{append_outputs, append_to, current_dir};
use crate::error::CiError;
use crate::setup::{
    Missing, check_tools, env_lines, missing_toolchain, output_lines, parse_reported,
};

const ENV_VARIABLE: &str = "GITHUB_ENV";
const CARGO_DIRECTORY: &str = ".cargo";
const CARGO_CONFIG: &str = "config.toml";
pub(super) const OWNED_MARKER: &str = "# velnor-ci owns this file and rewrites it on every prepare";

pub(super) fn prepare() -> Result<String, CiError> {
    let root = current_dir()?;
    let wanted: Vec<RepoPath> = mise_files().into_iter().chain([CONFIG_PATH]).collect();
    let files = read_files(&root, &wanted).map_err(|e| CiError::new(e.to_string()))?;
    let pins = tool_pins(&files).map_err(|e| CiError::new(e.to_string()))?;
    let debuginfo = ci_debuginfo(&files).map_err(|e| CiError::new(e.to_string()))?;
    if !pins.tools.is_empty() {
        check_pinned_tools(&root, &pins)?;
    }
    let declared = read_files(&root, &[TOOLCHAIN_PLAIN, TOOLCHAIN_TOML])
        .map_err(|e| CiError::new(e.to_string()))?;
    if let Some(declared) = toolchain(&declared)
        .map_err(|e| CiError::new(e.to_string()))?
        .filter(|declared| declared.path.is_none())
    {
        install_declared(&root, &declared)?;
    }
    let rustc_verbose = rustc_verbose()?;
    let profile = build_profile(debuginfo, release_of(&rustc_verbose)?);
    let inputs = CompatInputs {
        rustc_verbose,
        target: env_text("CARGO_BUILD_TARGET"),
        profile: profile.clone(),
        rustflags: env_text("RUSTFLAGS").unwrap_or_default(),
        workspace: root.display().to_string(),
        driver: BUILD_DRIVER.to_owned(),
        repo_profile: repository_profile(&root)?,
        image: image_identity()?,
    };
    let digest = compat(&inputs);
    write_cargo_config(&root, profile.cargo_config)?;
    append_to(ENV_VARIABLE, &env_lines(&profile))?;
    append_outputs(&output_lines(&digest))?;
    append_outputs(&lock_output_lines(&root)?)?;
    Ok(String::new())
}

fn lock_output_lines(root: &Path) -> Result<String, CiError> {
    let recorded = read_files(root, &[METADATA_PATH]).map_err(|e| CiError::new(e.to_string()))?;
    let members = recorded_members(recorded.get(&METADATA_PATH).map(Vec::as_slice))
        .map_err(|e| CiError::new(e.to_string()))?;
    let owned: BTreeSet<String> = members.values().flatten().cloned().collect();
    members
        .keys()
        .map(|manifest| {
            let lock = manifest.sibling(&RepoPath::from_static("Cargo.lock"));
            let mut read = read_files(root, std::slice::from_ref(&lock))
                .map_err(|e| CiError::new(e.to_string()))?;
            let digest = lock_key(read.remove(&lock).as_deref(), &owned);
            Ok(format!(
                "{LOCK_OUTPUT_PREFIX}{}={digest}\n",
                RootSlug::of(manifest)
            ))
        })
        .collect()
}

fn check_pinned_tools(root: &Path, pins: &ToolPins) -> Result<(), CiError> {
    let output = Command::new("mise")
        .args(["ls", "--current", "--json"])
        .current_dir(root)
        .output()
        .map_err(|e| CiError::new(format!("cannot run mise: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "mise ls failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    check_tools(
        pins,
        &parse_reported(&String::from_utf8_lossy(&output.stdout))?,
        MisePlatform::new(std::env::consts::OS, std::env::consts::ARCH),
    )
}

fn install_declared(root: &std::path::Path, declared: &Toolchain) -> Result<(), CiError> {
    let host = host_triple(root)?;
    let survey = || -> Result<Missing, CiError> {
        Ok(missing_toolchain(
            declared,
            &host,
            &tool_output(root, "rustup", &["component", "list"])?,
            &tool_output(root, "rustup", &["target", "list", "--installed"])?,
        ))
    };
    let missing = survey()?;
    if missing.is_empty() {
        return Ok(());
    }
    for (kind, names) in [
        ("component", &missing.components),
        ("target", &missing.targets),
    ] {
        if !names.is_empty() {
            let mut args = vec![kind, "add"];
            args.extend(names.iter().map(String::as_str));
            tool_output(root, "rustup", &args)?;
        }
    }
    survey()?.ensure_none()
}

fn tool_output(root: &std::path::Path, program: &str, args: &[&str]) -> Result<String, CiError> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| CiError::new(format!("cannot run {program}: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn host_triple(root: &std::path::Path) -> Result<String, CiError> {
    tool_output(root, "rustc", &["-vV"])?
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or_else(|| CiError::new("rustc -vV does not report a host".to_owned()))
}

pub(super) fn check_metadata() -> Result<String, CiError> {
    velnor_gen::io::check_metadata(&current_dir()?).map_err(|e| CiError::new(e.to_string()))?;
    Ok(String::new())
}

fn env_text(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn rustc_verbose() -> Result<String, CiError> {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| CiError::new(format!("cannot run rustc: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "rustc -vV failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| CiError::new("rustc -vV did not print UTF-8".to_owned()))
}

fn write_cargo_config(root: &Path, text: &str) -> Result<(), CiError> {
    let directory = root
        .parent()
        .ok_or_else(|| CiError::new(format!("{} has no parent directory", root.display())))?
        .join(CARGO_DIRECTORY);
    let file = directory.join(CARGO_CONFIG);
    let failed = |what: &str, e: std::io::Error| {
        CiError::new(format!("cannot {what} {}: {e}", file.display()))
    };
    match std::fs::read_to_string(&file) {
        Ok(existing) if !existing.starts_with(OWNED_MARKER) => {
            return if text.is_empty() {
                Ok(())
            } else {
                Err(CiError::new(format!(
                    "{} exists and was not written by velnor-ci",
                    file.display()
                )))
            };
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(failed("read", e)),
    }
    if text.is_empty() {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(failed("remove", e)),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(&directory).map_err(|e| failed("create the directory of", e))?;
    let staged = directory.join(format!("{CARGO_CONFIG}.velnor-{}", std::process::id()));
    std::fs::write(&staged, format!("{OWNED_MARKER}\n{text}"))
        .and_then(|()| std::fs::rename(&staged, &file))
        .map_err(|e| failed("write", e))
}
