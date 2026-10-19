//! Validation for the native Rust binary preview configuration.

use super::{
    RustBinaryPreviewCompanion, RustBinaryPreviewConfig, RustBinaryPreviewProducer,
    RustBinaryPreviewProduct,
};
use crate::config::is_valid_mise_task_name;
use crate::config::{CheckPlatform, CheckSystemTool};
use crate::errors::ContractError;
use std::collections::BTreeSet;

#[path = "binary_release_preview_inventory.rs"]
pub(super) mod inventory;
use inventory::paths_overlap;
pub(crate) use inventory::validate_static_release_inventory;

impl RustBinaryPreviewConfig {
    /// Validate all enabled preview policy fields and closed inventories.
    ///
    /// Disabled preview policies retain defaults and may be configured ahead
    /// of activation, but partial active policies fail closed.
    ///
    /// # Errors
    ///
    /// Returns a configuration error for an unsafe or incomplete policy.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let prefix = "stacks.rust.binary_release.preview";
        if (self.enabled && !valid_version_env(&self.version_env))
            || (!self.version_env.is_empty() && !valid_version_env(&self.version_env))
        {
            return Err(ContractError::config(
                file,
                format!("{prefix}.version_env"),
                "bad_preview_version_env",
            ));
        }
        if self.environment != "preview-publish" {
            return Err(ContractError::config(
                file,
                format!("{prefix}.environment"),
                "preview_environment_must_be_preview_publish",
            ));
        }
        self.producer.validate(file, self.enabled)?;
        inventory::validate_inventory(file, self)?;
        Ok(())
    }
}

impl RustBinaryPreviewProducer {
    fn validate(&self, file: &str, required: bool) -> Result<(), ContractError> {
        let key = "stacks.rust.binary_release.preview.producer";
        validate_task_name(file, &self.task, required, &format!("{key}.task"))?;
        validate_producer_paths(file, self, key)?;
        validate_producer_limits(file, self, key)?;
        validate_producer_inputs(file, self, required, key)?;
        validate_producer_tools(file, self, required, key)?;
        CheckSystemTool::validate_set(
            &self.system_tools,
            match self.runner {
                super::VerificationRunner::LinuxX64 => CheckPlatform::LinuxX64,
                super::VerificationRunner::MacosArm64
                | super::VerificationRunner::Macos26Arm64
                | super::VerificationRunner::Xcode27Arm64 => CheckPlatform::MacosArm64,
            },
            file,
            key,
        )
    }
}

fn validate_task_name(
    file: &str,
    task: &str,
    required: bool,
    key: &str,
) -> Result<(), ContractError> {
    if task.is_empty() && required {
        return Err(config_error(file, key, "required_when_enabled"));
    }
    if !task.is_empty() && !is_valid_mise_task_name(task) {
        return Err(config_error(file, key, "bad_mise_task"));
    }
    Ok(())
}

fn validate_producer_paths(
    file: &str,
    producer: &RustBinaryPreviewProducer,
    key: &str,
) -> Result<(), ContractError> {
    if !producer.source.validate() {
        return Err(config_error(
            file,
            &format!("{key}.source"),
            "bad_preview_source",
        ));
    }
    for (field, path) in [
        ("output_directory", producer.output_directory.as_str()),
        ("manifest", producer.manifest.as_str()),
    ] {
        if !valid_repo_path(path, false) {
            return Err(config_error(
                file,
                &format!("{key}.{field}"),
                "bad_preview_path",
            ));
        }
    }
    if !producer
        .manifest
        .starts_with(&format!("{}/", producer.output_directory))
    {
        return Err(config_error(
            file,
            &format!("{key}.manifest"),
            "preview_manifest_outside_output_directory",
        ));
    }
    Ok(())
}

fn validate_producer_limits(
    file: &str,
    producer: &RustBinaryPreviewProducer,
    key: &str,
) -> Result<(), ContractError> {
    if !(1..=RustBinaryPreviewProducer::MAX_TIMEOUT_MINUTES).contains(&producer.timeout_minutes) {
        return Err(config_error(
            file,
            &format!("{key}.timeout_minutes"),
            "bad_preview_timeout",
        ));
    }
    if !(1..=64).contains(&producer.build_jobs) {
        return Err(config_error(
            file,
            &format!("{key}.build_jobs"),
            "bad_preview_build_jobs",
        ));
    }
    Ok(())
}

fn validate_producer_inputs(
    file: &str,
    producer: &RustBinaryPreviewProducer,
    required: bool,
    key: &str,
) -> Result<(), ContractError> {
    if producer
        .inputs
        .iter()
        .any(|path| !valid_repo_path(path, false))
        || !unique(&producer.inputs)
    {
        return Err(config_error(
            file,
            &format!("{key}.inputs"),
            "bad_preview_inputs",
        ));
    }
    if producer
        .inputs
        .iter()
        .any(|input| paths_overlap(input, &producer.output_directory))
    {
        return Err(config_error(
            file,
            &format!("{key}.inputs"),
            "preview_output_overlaps_source_inputs",
        ));
    }
    if required && producer.inputs.is_empty() {
        return Err(config_error(
            file,
            key,
            "preview_task_requires_declared_inputs_and_tools",
        ));
    }
    if required && !producer.inputs.contains(&producer.source.mise_config) {
        return Err(config_error(
            file,
            &format!("{key}.inputs"),
            "preview_inputs_must_include_mise_config",
        ));
    }
    Ok(())
}

fn validate_producer_tools(
    file: &str,
    producer: &RustBinaryPreviewProducer,
    required: bool,
    key: &str,
) -> Result<(), ContractError> {
    if producer.tools.iter().any(|tool| !valid_identifier(tool)) || !unique(&producer.tools) {
        return Err(config_error(
            file,
            &format!("{key}.tools"),
            "bad_preview_tools",
        ));
    }
    if required && producer.tools.is_empty() {
        return Err(config_error(
            file,
            key,
            "preview_task_requires_declared_inputs_and_tools",
        ));
    }
    Ok(())
}

fn config_error(file: &str, key: &str, problem: &str) -> ContractError {
    ContractError::config(file, key, problem)
}

fn valid_version_env(name: &str) -> bool {
    name.ends_with("_VERSION_OVERRIDE")
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && ![
            "GITHUB_", "ACTIONS_", "RUNNER_", "VELNOR_", "MISE_", "CARGO_", "RUST_", "BASH_",
            "LD_", "DYLD_",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_asset_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 180
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && !matches!(value, "." | "..")
}

fn valid_repo_path(value: &str, allow_root: bool) -> bool {
    (allow_root && value == ".")
        || (!value.is_empty()
            && value.len() <= 1024
            && !value.starts_with('/')
            && !value.ends_with('/')
            && value.split('/').all(|part| {
                !part.is_empty()
                    && part != "."
                    && part != ".."
                    && part.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                    })
            }))
}

fn unique(values: &[String]) -> bool {
    values.iter().collect::<BTreeSet<_>>().len() == values.len()
}
