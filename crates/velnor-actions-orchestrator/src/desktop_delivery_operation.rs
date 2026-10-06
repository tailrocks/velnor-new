//! Generation-time Swift operations with an immutable source closure.
#[path = "desktop_delivery_operation_inputs.rs"]
mod operation_inputs;
#[path = "desktop_delivery_operation_source.rs"]
mod operation_source;

use super::{NativeCompileDriver, NativeDesktopToolContext};
use serde_json::to_string;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, ContractError, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    config::{DesktopDeliveryConfig, SwiftInputs},
    workflow::native_tools::NativeCredentialScope,
};
use velnor_actions_mise::catalog::native_desktop::NativeTrustedOperation;
use velnor_actions_rust::native_ffi::NativeFfiArtifacts;
use velnor_actions_workflow_renderer::RenderError;
const SOURCE_ROOT_ENV: &str = "VELNOR_NATIVE_SOURCE_ROOT";
const SOURCE_SHA_ENV: &str = "VELNOR_NATIVE_SOURCE_SHA";
const GITHUB_SHA: &str = "${{ github.sha }}";
const WORKFLOW_SHA_SENTINEL: &str = "__VELNOR_WORKFLOW_GITHUB_SHA__";
const APPLE_API_KEY_PATH: &str = "${{ runner.temp }}/velnor/apple/AuthKey.p8";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeSwiftProfile {
    Release,
    Verification,
}
impl NativeSwiftProfile {
    fn path(self) -> &'static str {
        match self {
            Self::Release => velnor_actions_native::swift::DESKTOP_RELEASE_PROFILE,
            Self::Verification => velnor_actions_native::swift::DESKTOP_VERIFICATION_PROFILE,
        }
    }
    fn driver(self) -> NativeCompileDriver {
        match self {
            Self::Release => NativeCompileDriver::Cargo,
            Self::Verification => NativeCompileDriver::Mbx,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NativeSourceAuthority {
    Literal { root: String, sha: String },
    WorkflowGithubSha { root: String },
}
impl NativeSourceAuthority {
    pub(crate) fn literal(root: &str, sha: &str) -> Result<Self, RenderError> {
        validate_source_root(root)?;
        validate_sha(sha)?;
        Ok(Self::Literal {
            root: root.to_owned(),
            sha: sha.to_owned(),
        })
    }
    pub(crate) fn workflow_github_sha(root: &str) -> Result<Self, RenderError> {
        validate_source_root(root)?;
        Ok(Self::WorkflowGithubSha {
            root: root.to_owned(),
        })
    }
    fn validate(&self) -> Result<(), RenderError> {
        let (root, sha) = self.values();
        validate_source_root(root)?;
        if sha == GITHUB_SHA {
            Ok(())
        } else {
            validate_sha(sha)
        }
    }
    fn values(&self) -> (&str, &str) {
        match self {
            Self::Literal { root, sha } => (root, sha),
            Self::WorkflowGithubSha { root } => (root, GITHUB_SHA),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NativeSwiftStage {
    GenerateProject,
    BindingsCheck,
    Xcframework,
    Build(String, String),
    Verify(String, String, bool, Option<String>),
    SwiftBuild,
    SwiftTest,
    XcodeBuild,
    XcodeTest,
    UiTest,
    Format,
    Lint,
    Deadcode,
    SwiftHarnesses,
    Sign(String, String),
    State(String, String),
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct NativeSwiftVersionBuild<'a> {
    pub(crate) version: &'a str,
    pub(crate) build: &'a str,
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct NativeSwiftSignRequest<'a> {
    pub(crate) context: &'a NativeDesktopToolContext,
    pub(crate) profile: NativeSwiftProfile,
    pub(crate) generator_version: &'a str,
    pub(crate) version_build: NativeSwiftVersionBuild<'a>,
    pub(crate) inputs: &'a SwiftInputs,
    pub(crate) source: &'a NativeSourceAuthority,
    pub(crate) rust_artifacts: Option<&'a NativeFfiArtifacts>,
    pub(crate) policy: &'a DesktopDeliveryConfig,
}
struct NativeSwiftOperationRequest<'a> {
    context: &'a NativeDesktopToolContext,
    profile: NativeSwiftProfile,
    stage: NativeSwiftStage,
    inputs: &'a SwiftInputs,
    source: &'a NativeSourceAuthority,
    rust_artifacts: Option<&'a NativeFfiArtifacts>,
    generator_version: &'a str,
    signing: Option<(&'a str, &'a str)>,
}
pub(crate) fn compiled_swift_operation(
    context: &NativeDesktopToolContext,
    profile: NativeSwiftProfile,
    stage: NativeSwiftStage,
    inputs: &SwiftInputs,
    source: &NativeSourceAuthority,
    rust_artifacts: Option<&NativeFfiArtifacts>,
    generator_version: &str,
) -> Result<CompiledSourceHelper, RenderError> {
    if matches!(stage, NativeSwiftStage::Sign(..)) {
        return Err(invalid("native_swift_sign_requires_policy"));
    }
    compiled_swift_operation_inner(NativeSwiftOperationRequest {
        context,
        profile,
        stage,
        inputs,
        source,
        rust_artifacts,
        generator_version,
        signing: None,
    })
}
pub(crate) fn compiled_swift_sign_operation(
    request: NativeSwiftSignRequest<'_>,
) -> Result<CompiledSourceHelper, RenderError> {
    let signing = signing_policy(request.policy)?;
    compiled_swift_operation_inner(NativeSwiftOperationRequest {
        context: request.context,
        profile: request.profile,
        stage: NativeSwiftStage::Sign(
            request.version_build.version.to_owned(),
            request.version_build.build.to_owned(),
        ),
        inputs: request.inputs,
        source: request.source,
        rust_artifacts: request.rust_artifacts,
        generator_version: request.generator_version,
        signing: Some(signing),
    })
}
fn compiled_swift_operation_inner(
    request: NativeSwiftOperationRequest<'_>,
) -> Result<CompiledSourceHelper, RenderError> {
    request.source.validate()?;
    request
        .inputs
        .validate(".velnor/config.toml", "desktop.profile")
        .map_err(RenderError::Contract)?;
    operation_inputs::validate_stage_inputs(&request.stage, request.inputs)?;
    validate_artifacts(&request.stage, request.rust_artifacts)?;
    let bundle = velnor_actions_native::swift::support_sources(request.generator_version)
        .map_err(RenderError::Contract)?;
    let profile_file = velnor_actions_native::swift::projected_profile_file(
        request.profile.path(),
        request.inputs,
        request.generator_version,
    )
    .map_err(RenderError::Contract)?;
    let (source_root, source_sha) = request.source.values();
    let body = operation_source::wrapper_body(
        &bundle,
        &profile_file,
        &request.stage,
        source_root,
        source_sha,
        request.rust_artifacts,
    )?;
    let source_text = velnor_actions_contract::generated_source(request.generator_version, &body)
        .map_err(RenderError::Contract)?;
    let digest = sha256(&source_text);
    let descriptor = SourceBoundHelper::compiled(
        SourceBoundOperation::NativeSwiftExecution,
        format!(
            "{}{}.sh",
            SourceBoundOperation::NativeSwiftExecution.path(),
            digest
        )
        .as_str(),
        &digest,
    )
    .map_err(RenderError::Contract)?;
    let invocation = HelperInvocation::compiled(descriptor, Vec::new(), Vec::new())
        .map_err(RenderError::Contract)?;
    let environment = request.signing.map_or_else(
        || source_environment(request.source),
        |(team, certificate)| signing_environment(request.source, team, certificate),
    );
    let record = CompiledSourceHelper::compiled(invocation, source_text)
        .map_err(RenderError::Contract)?
        .with_environment(environment);
    bind_sdk(request.context, request.profile, request.stage, record)
}
fn bind_sdk(
    context: &NativeDesktopToolContext,
    profile: NativeSwiftProfile,
    stage: NativeSwiftStage,
    record: CompiledSourceHelper,
) -> Result<CompiledSourceHelper, RenderError> {
    let record = match trusted_operation(&stage) {
        Some(operation) => context.bind_trusted_execution(record, operation),
        None => context.bind_execution(record, profile.driver()),
    }?;
    if matches!(stage, NativeSwiftStage::Sign(..)) {
        require_signing_environment(&record)?;
    }
    Ok(record)
}
fn trusted_operation(stage: &NativeSwiftStage) -> Option<NativeTrustedOperation> {
    match stage {
        NativeSwiftStage::Sign(..) => Some(NativeTrustedOperation::AppleSignAndNotarize),
        NativeSwiftStage::State(..) => Some(NativeTrustedOperation::GithubReleaseInspection),
        _ => None,
    }
}
fn source_environment(source: &NativeSourceAuthority) -> BTreeMap<String, String> {
    let (root, sha) = source.values();
    BTreeMap::from([
        (SOURCE_ROOT_ENV.to_owned(), root.to_owned()),
        (SOURCE_SHA_ENV.to_owned(), sha.to_owned()),
    ])
}
fn signing_policy(policy: &DesktopDeliveryConfig) -> Result<(&str, &str), RenderError> {
    policy
        .validate(".velnor/config.toml")
        .map_err(RenderError::Contract)?;
    if !policy.enabled || !policy.sign_tags {
        return Err(invalid("native_swift_sign_policy_missing"));
    }
    match (&policy.team_id, &policy.certificate_sha256) {
        (Some(team), Some(certificate)) => Ok((team, certificate)),
        _ => Err(invalid("native_swift_signing_identity_missing")),
    }
}
fn signing_environment(
    source: &NativeSourceAuthority,
    team: &str,
    certificate: &str,
) -> BTreeMap<String, String> {
    let mut environment = source_environment(source);
    environment.extend([
        (String::from("EXPECTED_TEAM_ID"), team.to_owned()),
        (String::from("EXPECTED_CERT_SHA256"), certificate.to_owned()),
    ]);
    for key in NativeCredentialScope::AppleSigning.allowed_keys() {
        if matches!(*key, "EXPECTED_TEAM_ID" | "EXPECTED_CERT_SHA256") {
            continue;
        }
        let value = if *key == "APP_STORE_CONNECT_API_KEY_PATH" {
            APPLE_API_KEY_PATH.to_owned()
        } else {
            format!("${{{{ secrets.{key} }}}}")
        };
        environment.insert((*key).to_owned(), value);
    }
    environment
}
fn require_signing_environment(record: &CompiledSourceHelper) -> Result<(), RenderError> {
    let recipe = record
        .execution_recipe()
        .ok_or_else(|| invalid("native_swift_signing_recipe_missing"))?;
    if recipe.credential_scope() != NativeCredentialScope::AppleSigning {
        return Err(invalid("native_swift_signing_scope_missing"));
    }
    if NativeCredentialScope::AppleSigning
        .allowed_keys()
        .iter()
        .any(|key| !record.environment().contains_key(*key))
        || record
            .environment()
            .get("APP_STORE_CONNECT_API_KEY_PATH")
            .map(String::as_str)
            != Some(APPLE_API_KEY_PATH)
    {
        return Err(invalid("native_swift_signing_environment_missing"));
    }
    Ok(())
}
fn validate_artifacts(
    stage: &NativeSwiftStage,
    artifacts: Option<&NativeFfiArtifacts>,
) -> Result<(), RenderError> {
    if needs_artifacts(stage) && artifacts.is_none() {
        return Err(invalid("native_swift_artifact_receipt_missing"));
    }
    if needs_library(stage) && !artifacts.is_some_and(|value| value.library_path().is_some()) {
        return Err(invalid("native_swift_library_receipt_missing"));
    }
    Ok(())
}
fn needs_artifacts(stage: &NativeSwiftStage) -> bool {
    matches!(
        stage,
        NativeSwiftStage::BindingsCheck
            | NativeSwiftStage::Xcframework
            | NativeSwiftStage::Build(..)
    )
}
fn needs_library(stage: &NativeSwiftStage) -> bool {
    matches!(
        stage,
        NativeSwiftStage::Xcframework | NativeSwiftStage::Build(..)
    )
}
fn validate_source_root(root: &str) -> Result<(), RenderError> {
    (root == ".")
        .then_some(())
        .map_or_else(|| validate_relative_path(root), Ok)
}
fn validate_relative_path(path: &str) -> Result<(), RenderError> {
    let invalid_path = path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || velnor_actions_contract::normalize_posix_path(path)
            .ok()
            .as_deref()
            != Some(path);
    if invalid_path {
        return Err(invalid("native_swift_relative_path"));
    }
    Ok(())
}
fn validate_sha(sha: &str) -> Result<(), RenderError> {
    if sha.len() != 40
        || !sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid("native_swift_source_sha"));
    }
    Ok(())
}
fn literal(value: &str) -> Result<String, ContractError> {
    to_string(value).map_err(|error| invalid_contract(&error.to_string()))
}
fn sha256(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn invalid(problem: &'static str) -> RenderError {
    RenderError::InvalidWorkflow(problem.to_owned())
}
fn invalid_contract(problem: &str) -> ContractError {
    ContractError::identity("native_swift_operation", problem)
}
