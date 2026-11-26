// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::exec::ExecError;
use crate::native::mise_lock::is_valid_mise_version;

const CACHE_MUST_BE_FALSE: &str = "jdx/mise-action input 'cache' must be false";
const BOOLEAN_INPUTS: [&str; 11] = [
    "auto_update",
    "fetch_from_github",
    "reshim",
    "install",
    "bootstrap",
    "env",
    "wings_enabled",
    "export_path",
    "experimental",
    "add_shims_to_path",
    "monorepo",
];

#[derive(Debug)]
pub(super) struct MiseInputs {
    pub(super) version: String,
    pub(super) sha256: String,
    pub(super) install_args: String,
}

impl MiseInputs {
    pub(super) fn parse(inputs: &BTreeMap<String, String>) -> Result<Self, ExecError> {
        let mut parsed = Self {
            version: String::new(),
            sha256: String::new(),
            install_args: String::new(),
        };
        let mut cache_disabled = false;
        for (name, value) in inputs {
            match name.to_ascii_lowercase().as_str() {
                "version" => value.trim().clone_into(&mut parsed.version),
                "sha256" => value.trim().clone_into(&mut parsed.sha256),
                "install_args" => value.trim().clone_into(&mut parsed.install_args),
                "cache" if matches!(value.trim(), "false" | "False" | "FALSE") => {
                    cache_disabled = true;
                }
                "cache" => return Err(ExecError::Unsupported(CACHE_MUST_BE_FALSE.to_owned())),
                other if value.trim().is_empty() && BOOLEAN_INPUTS.contains(&other) => {
                    return Err(ExecError::Invalid(format!(
                        "Input does not meet YAML 1.2 \"Core Schema\" specification: {other}\nSupport boolean input list: `true | True | TRUE | false | False | FALSE`"
                    )));
                }
                _ if value.trim().is_empty() => {}
                other => {
                    return Err(ExecError::Unsupported(format!(
                        "jdx/mise-action input '{other}' is not supported by velnor-runner"
                    )));
                }
            }
        }
        if !cache_disabled {
            return Err(ExecError::Unsupported(CACHE_MUST_BE_FALSE.to_owned()));
        }
        if !is_valid_mise_version(&parsed.version) {
            return Err(ExecError::Invalid(format!(
                "mise version '{}' is not an exact YYYY.M.D version",
                parsed.version
            )));
        }
        Ok(parsed)
    }
}
