// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::CiError;

use super::crates_mirror_args::with_mirror_config;
use super::fetch::FORK_HINT;

pub(super) fn run_fetches(
    cargo: &Path,
    environment: &[(OsString, OsString)],
    mirror: Option<&Path>,
    manifests: &[PathBuf],
    neutral: Option<&Path>,
) -> Result<(), CiError> {
    let with_token = neutral.is_some();
    for manifest in manifests {
        let mut command = Command::new(cargo);
        command
            .env_clear()
            .envs(environment.iter().map(|(k, v)| (k, v)));
        let directory = manifest
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        let mut arguments: Vec<OsString> = vec!["fetch".into(), "--locked".into()];
        let neutralised = if let Some(neutral) = neutral {
            let cargo_home = environment
                .iter()
                .find(|(name, _)| name == "CARGO_HOME")
                .map_or_default(|(_, home)| PathBuf::from(home));
            let checkout = directory.unwrap_or(Path::new("."));
            let absolute = std::fs::canonicalize(manifest)
                .or_else(|_| std::path::absolute(manifest))
                .map_err(|e| CiError::new(format!("cannot resolve {}: {e}", manifest.display())))?;
            command.current_dir(neutral);
            arguments.extend(["--manifest-path".into(), absolute.into()]);
            super::fetch_credentials::neutralise(&[checkout, neutral], &cargo_home)?
        } else {
            if let Some(directory) = directory {
                command.current_dir(directory);
            }
            arguments.extend([
                "--manifest-path".into(),
                manifest.file_name().unwrap_or(manifest.as_os_str()).into(),
            ]);
            super::fetch_credentials::Neutralised::default()
        };
        command.envs(neutralised.environment.iter().map(|(k, v)| (k, v)));
        if with_token {
            arguments.extend(["--config".into(), "net.offline=false".into()]);
            for config in &neutralised.config {
                arguments.extend(["--config".into(), config.into()]);
            }
        } else {
            arguments.push("--offline".into());
        }
        command.args(match mirror {
            Some(mirror) => with_mirror_config(mirror.as_os_str(), arguments),
            None => arguments,
        });
        let status = command
            .status()
            .map_err(|e| CiError::new(format!("cannot run {}: {e}", cargo.display())))?;
        if !status.success() {
            let why = if with_token { "" } else { FORK_HINT };
            return Err(CiError::new(format!(
                "cargo fetch for {} failed with {status}{}{why}",
                manifest.display(),
                if with_token { "" } else { "; " }
            )));
        }
    }
    Ok(())
}
