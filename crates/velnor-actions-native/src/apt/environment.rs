//! Environment roles for the fixed APT helper records.

use std::collections::BTreeMap;

use super::AptOperation;
use velnor_actions_contract::ContractError;

const GH_TOKEN: &str = "${{ github.token }}";
const SUITE: &str = "${{ inputs.channel || 'stable' }}";
const MODE: &str = "${{ inputs.mode || 'publish' }}";
const VERSION: &str = "${{ inputs.version || '' }}";
const COMMIT: &str = "${{ inputs.commit || '' }}";
const GPG_KEY: &str = "${{ secrets.APT_GPG_PRIVATE_KEY }}";
const GPG_PASSPHRASE: &str = "${{ secrets.APT_GPG_PASSPHRASE }}";

const CONTEXT_KEYS: &[&str] = &[
    "GITHUB_REPOSITORY",
    "GITHUB_SHA",
    "GITHUB_RUN_ID",
    "GITHUB_RUN_ATTEMPT",
    "GITHUB_EVENT_NAME",
    "GITHUB_REF",
    "GITHUB_SERVER_URL",
    "GITHUB_API_URL",
];
const CONTEXT_VALUES: [(&str, &str); 8] = [
    ("GITHUB_REPOSITORY", "${{ github.repository }}"),
    ("GITHUB_SHA", "${{ github.sha }}"),
    ("GITHUB_RUN_ID", "${{ github.run_id }}"),
    ("GITHUB_RUN_ATTEMPT", "${{ github.run_attempt }}"),
    ("GITHUB_EVENT_NAME", "${{ github.event_name }}"),
    ("GITHUB_REF", "${{ github.ref }}"),
    ("GITHUB_SERVER_URL", "https://github.com"),
    ("GITHUB_API_URL", "https://api.github.com"),
];

pub(super) fn validate(
    operation: AptOperation,
    environment: &BTreeMap<String, String>,
) -> Result<(), ContractError> {
    let context = if operation == AptOperation::Result {
        &[][..]
    } else {
        CONTEXT_KEYS
    };
    let expected = keys(operation);
    if environment.len() != expected.len() + context.len()
        || expected
            .iter()
            .chain(context.iter())
            .any(|key| !environment.contains_key(*key))
        || environment.values().any(|value| {
            value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control)
        })
    {
        return Err(invalid("environment_shape"));
    }
    if operation != AptOperation::Result {
        for (key, value) in CONTEXT_VALUES {
            exact(environment, key, value)?;
        }
    }
    match operation {
        AptOperation::Verify => verify(environment),
        AptOperation::Stage => stage(environment),
        AptOperation::IncomingTransport => incoming(environment),
        AptOperation::PagesAdmission => pages(environment),
        AptOperation::Result => result(environment),
    }
}

fn keys(operation: AptOperation) -> &'static [&'static str] {
    match operation {
        AptOperation::Verify => &["CHANNEL", "INPUT_VERSION", "INPUT_COMMIT", "GH_TOKEN"],
        AptOperation::Stage => &[
            "INPUT_SUITE",
            "DELIVERY_MODE",
            "APT_GPG_PRIVATE_KEY",
            "APT_GPG_PASSPHRASE",
        ],
        AptOperation::IncomingTransport => &["ARTIFACT_ID", "ARTIFACT_DIGEST", "GH_TOKEN"],
        AptOperation::PagesAdmission => &[
            "INPUT_SUITE",
            "DELIVERY_MODE",
            "EXPECTED_ARTIFACT_ID",
            "EXPECTED_ARTIFACT_DIGEST",
            "FULL_CI_RESULT",
            "APPROVED_REPOSITORY",
            "APPROVED_SOURCE_SHA",
            "APPROVED_DEFAULT_BRANCH",
            "ADMISSION_EVENT_POLICY",
            "GH_TOKEN",
        ],
        AptOperation::Result => &["PUBLICATION_ELIGIBLE", "VERIFY", "ADMIT", "STAGE", "DEPLOY"],
    }
}

fn verify(environment: &BTreeMap<String, String>) -> Result<(), ContractError> {
    exact(environment, "CHANNEL", SUITE)?;
    exact(environment, "INPUT_VERSION", VERSION)?;
    exact(environment, "INPUT_COMMIT", COMMIT)?;
    exact(environment, "GH_TOKEN", GH_TOKEN)
}

fn stage(environment: &BTreeMap<String, String>) -> Result<(), ContractError> {
    exact(environment, "INPUT_SUITE", SUITE)?;
    exact(environment, "DELIVERY_MODE", MODE)?;
    exact(environment, "APT_GPG_PRIVATE_KEY", GPG_KEY)?;
    exact(environment, "APT_GPG_PASSPHRASE", GPG_PASSPHRASE)
}

fn incoming(environment: &BTreeMap<String, String>) -> Result<(), ContractError> {
    exact(
        environment,
        "ARTIFACT_ID",
        "${{ needs.verify.outputs.artifact_id }}",
    )?;
    exact(
        environment,
        "ARTIFACT_DIGEST",
        "${{ needs.verify.outputs.artifact_digest }}",
    )?;
    exact(environment, "GH_TOKEN", GH_TOKEN)
}

fn pages(environment: &BTreeMap<String, String>) -> Result<(), ContractError> {
    exact(environment, "INPUT_SUITE", SUITE)?;
    exact(environment, "DELIVERY_MODE", MODE)?;
    exact(
        environment,
        "EXPECTED_ARTIFACT_ID",
        "${{ needs.stage.outputs.artifact_id }}",
    )?;
    exact(
        environment,
        "EXPECTED_ARTIFACT_DIGEST",
        "${{ needs.stage.outputs.artifact_digest }}",
    )?;
    exact(environment, "FULL_CI_RESULT", "${{ needs.admit.result }}")?;
    exact(environment, "APPROVED_SOURCE_SHA", "${{ github.sha }}")?;
    exact(environment, "ADMISSION_EVENT_POLICY", "default-branch")?;
    exact(environment, "GH_TOKEN", GH_TOKEN)?;
    repository(environment.get("APPROVED_REPOSITORY"))?;
    branch(environment.get("APPROVED_DEFAULT_BRANCH"))
}

fn result(environment: &BTreeMap<String, String>) -> Result<(), ContractError> {
    expression(environment, "PUBLICATION_ELIGIBLE")?;
    for (key, value) in [
        ("VERIFY", "${{ needs.verify.result }}"),
        ("ADMIT", "${{ needs.admit.result }}"),
        ("STAGE", "${{ needs.stage.result }}"),
        ("DEPLOY", "${{ needs.deploy.result }}"),
    ] {
        exact(environment, key, value)?;
    }
    Ok(())
}

fn exact(
    environment: &BTreeMap<String, String>,
    key: &str,
    expected: &str,
) -> Result<(), ContractError> {
    (environment.get(key).map(String::as_str) == Some(expected))
        .then_some(())
        .ok_or_else(|| invalid("environment_binding"))
}

fn expression(environment: &BTreeMap<String, String>, key: &str) -> Result<(), ContractError> {
    let value = environment
        .get(key)
        .ok_or_else(|| invalid("environment_binding"))?;
    (value.starts_with("${{") && value.ends_with("}}"))
        .then_some(())
        .ok_or_else(|| invalid("environment_expression"))
}

fn repository(value: Option<&String>) -> Result<(), ContractError> {
    let Some(value) = value else {
        return Err(invalid("repository_binding"));
    };
    let mut parts = value.split('/');
    let owner = parts.next().ok_or_else(|| invalid("repository_binding"))?;
    let name = parts.next().ok_or_else(|| invalid("repository_binding"))?;
    if parts.next().is_some()
        || owner.is_empty()
        || owner.len() > 39
        || owner.starts_with('-')
        || owner.ends_with('-')
        || owner.contains("--")
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || name.is_empty()
        || name.len() > 100
        || !name.starts_with(|value: char| value.is_ascii_alphanumeric())
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        || name.contains("..")
    {
        return Err(invalid("repository_binding"));
    }
    Ok(())
}

fn branch(value: Option<&String>) -> Result<(), ContractError> {
    let Some(value) = value else {
        return Err(invalid("branch_binding"));
    };
    if !velnor_actions_contract::is_valid_branch_name(value) {
        return Err(invalid("branch_binding"));
    }
    Ok(())
}

fn invalid(problem: &'static str) -> ContractError {
    ContractError::identity("apt_helper", problem)
}
