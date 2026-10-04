//! Compiled source authority for the fixed APT delivery roles.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledSourceHelper, CompiledSupportSource, ContractError, HelperInvocation,
    SourceBoundHelper, SourceBoundOperation, canonical_json_str, config::AptDeliveryConfig,
};

use super::{SUPPORT_PATHS, support_sources};

const CONFIG_PATH: &str = ".github/velnor/apt-delivery.jsonc";
const ADMISSION_PATH: &str = ".github/velnor/release_admission.py";

/// Native APT source operation selected by the typed workflow graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AptOperation {
    /// Download and verify the upstream package evidence.
    Verify,
    /// Reverify and sign a staged feed.
    Stage,
    /// Download and verify the immutable incoming artifact.
    IncomingTransport,
    /// Admit, download, guard, and deploy the staged Pages artifact.
    PagesAdmission,
    /// Check the terminal publication outcomes.
    Result,
}

/// Exact helper paths emitted by the APT source-owner records.
pub const HELPER_PATHS: &[&str] = &[
    ".github/velnor/apt_verify.sh",
    ".github/velnor/apt_stage.sh",
    ".github/velnor/apt_transport_incoming.sh",
    ".github/velnor/native_pages_admission.sh",
    ".github/velnor/apt_result.sh",
];

/// Compile one closed APT operation with its source-owner validated environment.
/// # Errors
/// Rejects altered credential scopes, role inputs or source closures.
pub fn compiled_helper(
    operation: AptOperation,
    config: &AptDeliveryConfig,
    version: &str,
    environment: BTreeMap<String, String>,
) -> Result<CompiledSourceHelper, ContractError> {
    config.validate(".velnor/config.toml")?;
    super::environment::validate(operation, &environment)?;
    Ok(compile(operation, config, version, None)?.with_environment(environment))
}

/// Compile immutable Pages admission with the authoritative shared source.
/// # Errors
/// Rejects foreign admission bytes and altered source, artifact or credential inputs.
pub fn compiled_pages_admission(
    config: &AptDeliveryConfig,
    version: &str,
    admission: &CompiledSupportSource,
    environment: BTreeMap<String, String>,
) -> Result<CompiledSourceHelper, ContractError> {
    config.validate(".velnor/config.toml")?;
    super::environment::validate(AptOperation::PagesAdmission, &environment)?;
    if environment.get("APPROVED_REPOSITORY") != Some(&config.consumer_repository)
        || environment.get("APPROVED_DEFAULT_BRANCH") != Some(&config.branch)
    {
        return Err(invalid("foreign_policy_identity"));
    }
    Ok(compile(
        AptOperation::PagesAdmission,
        config,
        version,
        Some(admission),
    )?
    .with_environment(environment))
}

fn compile(
    operation: AptOperation,
    config: &AptDeliveryConfig,
    version: &str,
    admission: Option<&CompiledSupportSource>,
) -> Result<CompiledSourceHelper, ContractError> {
    let source_operation = operation_for(operation)?;
    let source = wrapper_source(operation, config, version, admission)?;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor =
        SourceBoundHelper::compiled(source_operation, source_operation.path(), &digest)?;
    let invocation = HelperInvocation::compiled(descriptor, arguments(operation), Vec::new())?;
    CompiledSourceHelper::compiled(invocation, source)
}

fn wrapper_source(
    operation: AptOperation,
    config: &AptDeliveryConfig,
    version: &str,
    admission: Option<&CompiledSupportSource>,
) -> Result<String, ContractError> {
    let bundle = support_sources(version)?;
    let entry = bundle
        .files()
        .iter()
        .find(|file| file.path() == SUPPORT_PATHS[0])
        .ok_or_else(|| invalid("entry_source_missing"))?;
    let transport = bundle
        .files()
        .iter()
        .find(|file| file.path() == ".github/velnor/delivery_apt_transport.py")
        .ok_or_else(|| invalid("transport_source_missing"))?;
    let json = canonical_json_str(config)?;
    let policy = velnor_actions_contract::generated_source(version, &format!("{json}\n"))?;
    let expected = velnor_actions_contract::compiled_source_sha256(policy.as_bytes());
    if entry.source().matches("_APT_POLICY_SHA256 = None").count() != 1 {
        return Err(invalid("policy_digest_slot"));
    }
    let entry = entry.source().replacen(
        "_APT_POLICY_SHA256 = None",
        &format!("_APT_POLICY_SHA256 = '{expected}'"),
        1,
    );
    let source = template(operation, admission)?
        .replace("@@APT_ENTRY_SOURCE@@", &entry)
        .replace("@@APT_TRANSPORT_SOURCE@@", transport.source());
    let source = if let Some(admission) = admission {
        let marker = velnor_actions_contract::marker_for_version(version)?;
        if admission.path() != ADMISSION_PATH
            || admission.source().lines().next() != Some(marker.as_str())
        {
            return Err(invalid("admission_source_binding"));
        }
        source.replace("@@APT_SHARED_ADMISSION_SOURCE@@", admission.source())
    } else {
        source
    };
    if source.contains("@@APT_") {
        return Err(invalid("unresolved_wrapper_placeholder"));
    }
    velnor_actions_contract::generated_source(version, &source)
}

fn template(
    operation: AptOperation,
    admission: Option<&CompiledSupportSource>,
) -> Result<&'static str, ContractError> {
    match (operation, admission.is_some()) {
        (AptOperation::Verify, false) => Ok(include_str!("apt_verify.sh")),
        (AptOperation::Stage, false) => Ok(include_str!("apt_stage.sh")),
        (AptOperation::IncomingTransport, false) => Ok(include_str!("apt_transport_incoming.sh")),
        (AptOperation::PagesAdmission, true) => Ok(include_str!("apt_pages_admission.sh")),
        (AptOperation::Result, false) => Ok(include_str!("apt_result.sh")),
        (AptOperation::PagesAdmission, false) => Err(invalid("admission_source_required")),
        (_, true) => Err(invalid("unexpected_admission_source")),
    }
}

fn operation_for(operation: AptOperation) -> Result<SourceBoundOperation, ContractError> {
    let operation = match operation {
        AptOperation::Verify => SourceBoundOperation::AptVerify,
        AptOperation::Stage => SourceBoundOperation::AptStage,
        AptOperation::IncomingTransport => SourceBoundOperation::AptTransportIncoming,
        AptOperation::PagesAdmission => SourceBoundOperation::NativePagesAdmission,
        AptOperation::Result => SourceBoundOperation::AptResult,
    };
    let expected_path = operation.path();
    HELPER_PATHS
        .contains(&expected_path)
        .then_some(operation)
        .ok_or_else(|| invalid("operation_path_unowned"))
}

fn arguments(operation: AptOperation) -> Vec<String> {
    match operation {
        AptOperation::Verify => vec!["verify".to_owned(), CONFIG_PATH.to_owned()],
        AptOperation::Stage => vec!["stage".to_owned(), CONFIG_PATH.to_owned()],
        AptOperation::IncomingTransport => vec!["incoming".to_owned()],
        AptOperation::PagesAdmission | AptOperation::Result => Vec::new(),
    }
}

fn invalid(problem: &'static str) -> ContractError {
    ContractError::identity("apt_helper", problem)
}
