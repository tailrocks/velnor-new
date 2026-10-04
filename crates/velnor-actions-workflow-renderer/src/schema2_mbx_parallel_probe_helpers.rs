//! Narrow renderer helpers for the MBX parallel qualification jobs.

use std::collections::BTreeMap;

use super::{MbxQualificationPins, Role};
use velnor_actions_contract::{PermissionLevel, Permissions, PullRequestCachePolicy};
use crate::cache_steps::MBX_ACTION_NAME;
use crate::render::RenderContext;
use crate::steps;
use crate::yaml::Yaml;
use crate::RenderError;

pub(super) fn permission_yaml(permissions: Option<Permissions>) -> Yaml {
    let actions = match permissions.map(|permissions| permissions.actions) {
        Some(PermissionLevel::Write) => "write",
        _ => "read",
    };
    mapping(&[("contents", "read"), ("actions", actions)])
}

pub(super) fn qualification_job_env(request: &MbxQualificationPins, role: Role) -> Yaml {
    Yaml::Map(vec![
        ("MBX_GC_AUTO".to_owned(), Yaml::str("1")),
        (
            "MBX_VERSION".to_owned(),
            Yaml::str(request.mbx_version.clone()),
        ),
        (
            "RUSTUP_TOOLCHAIN".to_owned(),
            Yaml::str(request.rust_version.clone()),
        ),
        ("MBX_CACHE_SCOPE".to_owned(), Yaml::str(role.scope())),
        (
            "MBX_QUALIFICATION_ACTION_REF".to_owned(),
            Yaml::str(request.mbx_action_uses.clone()),
        ),
        (
            "MBX_QUALIFICATION_PHASE_FILE".to_owned(),
            Yaml::str("${{ runner.temp }}/mbx-cache-evidence/phases.tsv"),
        ),
        (
            "MBX_QUALIFICATION_IMPORT_RECEIPT".to_owned(),
            Yaml::str("${{ runner.temp }}/mbx-cache-evidence/import-receipt.txt"),
        ),
        (
            "MBX_QUALIFICATION_EXPORT_RECEIPT".to_owned(),
            Yaml::str("${{ runner.temp }}/mbx-cache-evidence/export-receipt.txt"),
        ),
        (
            "MBX_QUALIFICATION_SAMPLE_INTERVAL".to_owned(),
            Yaml::str("5"),
        ),
        (
            "MBX_QUALIFICATION_FINALIZER_WAIT".to_owned(),
            Yaml::str("6"),
        ),
    ])
}

fn mapping(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

pub(super) fn validate_pins(request: &MbxQualificationPins) -> Result<(), RenderError> {
    request.mise_setup.validate()?;
    steps::validate_uses(&request.mbx_action_uses)?;
    if !request
        .mbx_action_uses
        .starts_with(&format!("{MBX_ACTION_NAME}@"))
    {
        return Err(RenderError::BadActionRef(format!(
            "not_mbx_action:{}",
            request.mbx_action_uses
        )));
    }
    validate_exact_version(&request.mbx_version, "mbx")?;
    validate_exact_version(&request.rust_version, "rust")
}

fn validate_exact_version(version: &str, kind: &str) -> Result<(), RenderError> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!(
            "bad_{kind}_version:{version}"
        )))
    }
}

pub(super) fn step_context() -> RenderContext {
    RenderContext {
        generator_version: "0.0.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.0.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor".to_owned(),
        checkout_uses: super::super::features::CHECKOUT_USES.to_owned(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        pull_request_cache_policy: PullRequestCachePolicy::ReadOnly,
        plan_consumer_env: BTreeMap::new(),
    }
}
