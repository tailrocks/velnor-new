// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Environment protocol shared by the `velnor-freshness` binary and the
//! legacy `repo-policy-v1` internal op of `velnor-actions`.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::{BaseRef, CargoArtifactKind, Operation};

const ACTION_ENV: &str = "VELNOR_REPO_POLICY_ACTION";
const ROOT_ENV: &str = "VELNOR_REPO_POLICY_ROOT";
const UPSTREAM_ENV: &str = "VELNOR_REPO_POLICY_CHECK_UPSTREAM";
const ADVISORIES_ENV: &str = "VELNOR_REPO_POLICY_WITH_ADVISORIES";
const MANIFEST_PATH_ENV: &str = "VELNOR_REPO_POLICY_MANIFEST_PATH";
const BASE_REF_ENV: &str = "VELNOR_REPO_POLICY_BASE_REF";
const ARTIFACT_KIND_ENV: &str = "VELNOR_REPO_POLICY_ARTIFACT_KIND";

type Lookup<'a> = &'a dyn Fn(&str) -> Result<String, env::VarError>;

#[must_use]
pub fn gate_root() -> Option<PathBuf> {
    let action = env::var(ACTION_ENV).ok()?;
    if !matches!(
        action.as_str(),
        "freshness"
            | "toolchain-specs"
            | "mise-version"
            | "pin-mirror"
            | "workspace-members"
            | "library-members"
            | "test-members"
            | "artifact-executable"
    ) {
        return None;
    }
    let root = env::var_os(ROOT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    if !root.is_dir() {
        return None;
    }
    Some(root)
}

#[must_use]
pub fn run(root: &Path, program: &str) -> ExitCode {
    let operation = match operation_from(&|name| env::var(name)) {
        Ok(operation) => operation,
        Err(error) => return fail_internal(program, &error),
    };
    let code = crate::run(root, &operation).clamp(0, 255);
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn fail_internal(program: &str, problem: &str) -> ExitCode {
    eprintln!("{program}: internal request failed: {problem}");
    ExitCode::from(1)
}

pub(crate) fn operation_from(lookup: Lookup<'_>) -> Result<Operation, String> {
    match lookup(ACTION_ENV).as_deref() {
        Ok("freshness") => Ok(Operation::Freshness {
            check_upstream: optional_flag(lookup, UPSTREAM_ENV)?,
            with_advisories: optional_flag(lookup, ADVISORIES_ENV)?,
        }),
        Ok("toolchain-specs") => Ok(Operation::ToolchainSpecs),
        Ok("mise-version") => Ok(Operation::MiseVersion),
        Ok("pin-mirror") => Ok(Operation::PinMirror {
            base: optional_base_ref(lookup, BASE_REF_ENV)?,
        }),
        Ok("workspace-members") => Ok(Operation::WorkspaceMembers {
            libraries_only: false,
        }),
        Ok("library-members") => Ok(Operation::WorkspaceMembers {
            libraries_only: true,
        }),
        Ok("test-members") => Ok(Operation::WorkspaceTestMembers {
            manifest_path: optional_manifest_path(lookup, MANIFEST_PATH_ENV)?,
        }),
        Ok("artifact-executable") => Ok(Operation::CargoArtifactExecutable {
            messages_path: required_path(lookup, "VELNOR_REPO_POLICY_MESSAGES")?,
            manifest_path: required_path(lookup, "VELNOR_REPO_POLICY_MANIFEST")?,
            target_name: required_text(lookup, "VELNOR_REPO_POLICY_TARGET")?,
            kind: optional_artifact_kind(lookup, ARTIFACT_KIND_ENV)?,
        }),
        _ => Err("invalid repository-policy action".to_owned()),
    }
}

fn optional_manifest_path(lookup: Lookup<'_>, name: &str) -> Result<Option<PathBuf>, String> {
    match lookup(name) {
        Err(env::VarError::NotPresent) => Ok(None),
        Ok(value) if value.is_empty() => Ok(None),
        Ok(value) if !value.chars().any(char::is_control) => Ok(Some(PathBuf::from(value))),
        Ok(_) | Err(env::VarError::NotUnicode(_)) => Err(format!(
            "{name} must be a repository-relative Cargo.toml path"
        )),
    }
}

fn optional_base_ref(lookup: Lookup<'_>, name: &str) -> Result<Option<BaseRef>, String> {
    match lookup(name) {
        Err(env::VarError::NotPresent) => Ok(None),
        Ok(value) if value.is_empty() => Ok(None),
        Ok(value) => BaseRef::parse(&value)
            .map(Some)
            .map_err(|error| format!("{name}: {error}")),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{name} must be ASCII")),
    }
}

fn required_text(lookup: Lookup<'_>, name: &str) -> Result<String, String> {
    lookup(name)
        .ok()
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
        .ok_or_else(|| format!("{name} must contain nonempty text without control characters"))
}

fn required_path(lookup: Lookup<'_>, name: &str) -> Result<PathBuf, String> {
    required_text(lookup, name).map(PathBuf::from)
}

fn optional_artifact_kind(lookup: Lookup<'_>, name: &str) -> Result<CargoArtifactKind, String> {
    match lookup(name) {
        Err(env::VarError::NotPresent) => Ok(CargoArtifactKind::Bin),
        Ok(value) if value == "bin" => Ok(CargoArtifactKind::Bin),
        Ok(value) if value == "example" => Ok(CargoArtifactKind::Example),
        Ok(_) | Err(env::VarError::NotUnicode(_)) => {
            Err(format!("{name} must be absent, bin, or example"))
        }
    }
}

fn optional_flag(lookup: Lookup<'_>, name: &str) -> Result<bool, String> {
    match lookup(name) {
        Err(env::VarError::NotPresent) => Ok(false),
        Ok(value) if value == "0" => Ok(false),
        Ok(value) if value == "1" => Ok(true),
        Ok(_) | Err(env::VarError::NotUnicode(_)) => Err(format!("{name} must be absent, 0, or 1")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn parse(pairs: &[(&str, &str)]) -> Result<Operation, String> {
        let map: BTreeMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        operation_from(&|name| map.get(name).cloned().ok_or(env::VarError::NotPresent))
    }

    #[test]
    fn upstream_flag_reaches_the_freshness_operation() {
        let operation = parse(&[
            (ACTION_ENV, "freshness"),
            (UPSTREAM_ENV, "1"),
            (ADVISORIES_ENV, "0"),
        ]);
        assert_eq!(
            operation,
            Ok(Operation::Freshness {
                check_upstream: true,
                with_advisories: false,
            })
        );
    }

    #[test]
    fn flags_default_to_off_and_reject_other_values() {
        assert_eq!(
            parse(&[(ACTION_ENV, "freshness")]),
            Ok(Operation::Freshness {
                check_upstream: false,
                with_advisories: false,
            })
        );
        let error = parse(&[(ACTION_ENV, "freshness"), (UPSTREAM_ENV, "yes")]);
        assert_eq!(
            error,
            Err(format!("{UPSTREAM_ENV} must be absent, 0, or 1"))
        );
    }

    #[test]
    fn unknown_or_missing_action_is_rejected() {
        let expected = Err("invalid repository-policy action".to_owned());
        assert_eq!(parse(&[]), expected);
        assert_eq!(parse(&[(ACTION_ENV, "nope")]), expected);
    }

    #[test]
    fn artifact_executable_needs_messages_manifest_and_target() {
        let base = [(ACTION_ENV, "artifact-executable")];
        assert!(parse(&base).is_err());
        let full = parse(&[
            (ACTION_ENV, "artifact-executable"),
            ("VELNOR_REPO_POLICY_MESSAGES", "m.json"),
            ("VELNOR_REPO_POLICY_MANIFEST", "crates/x/Cargo.toml"),
            ("VELNOR_REPO_POLICY_TARGET", "x"),
            (ARTIFACT_KIND_ENV, "example"),
        ]);
        assert_eq!(
            full,
            Ok(Operation::CargoArtifactExecutable {
                messages_path: PathBuf::from("m.json"),
                manifest_path: PathBuf::from("crates/x/Cargo.toml"),
                target_name: "x".to_owned(),
                kind: CargoArtifactKind::Example,
            })
        );
    }
}
