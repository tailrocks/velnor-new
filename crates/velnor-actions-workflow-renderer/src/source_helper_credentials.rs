//! Credential admission for compiled source-helper records.
//!
//! The recipe is generation-time authority. Wire descriptors and ordinary
//! environment maps cannot select a credential scope or widen an operation.

use std::collections::BTreeMap;

use crate::{RenderError, toolchain_env};
use velnor_actions_contract::workflow::native_tools::NativeCredentialScope;
use velnor_actions_contract::{CompiledSourceHelper, SourceBoundOperation};
#[path = "source_helper_rustup.rs"]
mod rustup;

const GITHUB_TOKEN_EXPRESSION: &str = "${{ github.token }}";
const APT_PRIVATE_KEY_EXPRESSION: &str = "${{ secrets.APT_GPG_PRIVATE_KEY }}";
const APT_PASSPHRASE_EXPRESSION: &str = "${{ secrets.APT_GPG_PASSPHRASE }}";
const REGISTRY_TOKEN_EXPRESSION: &str = "${{ secrets.CARGO_REGISTRY_TOKEN }}";
const OIDC_URL_EXPRESSION: &str = "${{ env.ACTIONS_ID_TOKEN_REQUEST_URL }}";
const OIDC_TOKEN_EXPRESSION: &str = "${{ env.ACTIONS_ID_TOKEN_REQUEST_TOKEN }}";
const OCI_DOCKER_CONFIG_EXPRESSION: &str = "${{ runner.temp }}/velnor/oci-docker";
const APPLE_API_KEY_PATH_EXPRESSION: &str = "${{ runner.temp }}/velnor/apple/AuthKey.p8";

const HELPER_CONTROL_KEYS: &[&str] = &[
    "ACTIONS_CACHE_URL",
    "ACTIONS_RESULTS_URL",
    "ACTIONS_RUNTIME_URL",
    "GITHUB_ENV",
    "GITHUB_OUTPUT",
    "GITHUB_PATH",
    "GITHUB_STATE",
    "GITHUB_STEP_SUMMARY",
];

/// Operations whose compiled owner may perform fixed GitHub read-only checks.
///
/// Preparation, source production, and executable tool production are kept
/// outside this list even when their owner uses a managed `gh` binary.
const GITHUB_READ_ONLY_OPERATIONS: &[SourceBoundOperation] = &[
    SourceBoundOperation::AptVerify,
    SourceBoundOperation::AptTransportIncoming,
    SourceBoundOperation::NativePagesAdmission,
    SourceBoundOperation::NativePublishAdmission,
    SourceBoundOperation::NativePublishReceiptVerifier,
    SourceBoundOperation::NativeSwiftExecution,
    SourceBoundOperation::OciDelivery,
    SourceBoundOperation::ReleaseAdmissionDefaultBranch,
    SourceBoundOperation::RustReleaseForgePreflight,
    SourceBoundOperation::RustRegistryArtifactProof,
    SourceBoundOperation::RustReleaseReconcile,
    SourceBoundOperation::RustReleaseSourceSnapshot,
];

const GITHUB_ISSUE_WRITE_OPERATIONS: &[SourceBoundOperation] =
    &[SourceBoundOperation::VerificationObserver];

const APT_SIGNING_OPERATIONS: &[SourceBoundOperation] = &[SourceBoundOperation::AptStage];

const OCI_PUBLISH_OPERATIONS: &[SourceBoundOperation] = &[SourceBoundOperation::OciDelivery];

const APPLE_SIGNING_OPERATIONS: &[SourceBoundOperation] =
    &[SourceBoundOperation::NativeSwiftExecution];

/// Validate a compiled record's credential scope and exact owner environment.
/// # Errors
/// Rejects unsupported scopes, foreign operations, foreign OIDC bindings, and
/// credential-shaped environment keys outside the scope allowlist.
pub(super) fn validate_record(record: &CompiledSourceHelper) -> Result<(), RenderError> {
    rustup::validate(record)?;
    let scope = scope_of(record);
    let operation = record.invocation().descriptor().operation();
    validate_invocation_scope(scope, operation, record.invocation().args())?;
    if let Some(recipe) = record.execution_recipe() {
        validate_recipe(scope, operation, recipe.environment())?;
    }
    validate_environment(scope, operation, record.environment(), true)
}

fn validate_invocation_scope(
    scope: NativeCredentialScope,
    operation: SourceBoundOperation,
    args: &[String],
) -> Result<(), RenderError> {
    if operation != SourceBoundOperation::OciDelivery {
        return Ok(());
    }
    if scope == NativeCredentialScope::Anonymous {
        return Ok(());
    }
    if args.len() != 4 {
        return Err(invalid("oci_scope_args"));
    }
    let phase = args.first().map(String::as_str);
    let admitted = match scope {
        NativeCredentialScope::GithubReadOnly => matches!(
            phase,
            Some("verify" | "source" | "artifact" | "publish-admission" | "index-receipt")
        ),
        NativeCredentialScope::OciRegistryPublish => {
            matches!(phase, Some("admission" | "assembly" | "platform-publish"))
        }
        _ => true,
    };
    if admitted {
        validate_oci_argument_shape(scope, phase, &args[1..])
    } else {
        Err(invalid("oci_scope_phase"))
    }
}

fn validate_oci_argument_shape(
    scope: NativeCredentialScope,
    phase: Option<&str>,
    values: &[String],
) -> Result<(), RenderError> {
    let all_identity = values.iter().all(|value| value != "-");
    let source_shape = values[0] != "-" && values[1] == "-" && values[2] != "-";
    let empty_shape = values.iter().all(|value| value == "-");
    let valid = match (scope, phase) {
        (NativeCredentialScope::GithubReadOnly, Some("verify" | "publish-admission")) => {
            all_identity
        }
        (NativeCredentialScope::GithubReadOnly, Some("source"))
        | (NativeCredentialScope::OciRegistryPublish, Some("assembly")) => source_shape,
        (NativeCredentialScope::GithubReadOnly, Some("artifact" | "index-receipt"))
        | (NativeCredentialScope::OciRegistryPublish, Some("admission")) => empty_shape,
        (NativeCredentialScope::OciRegistryPublish, Some("platform-publish")) => all_identity,
        _ => false,
    };
    valid.then_some(()).ok_or_else(|| invalid("oci_scope_args"))
}

/// Validate the exact environment copied into the rendered helper step.
/// # Errors
/// Rejects an environment that is not permitted by the compiled scope.
pub(super) fn validate_step(
    record: &CompiledSourceHelper,
    environment: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    validate_record(record)?;
    if record.environment() != environment {
        return Err(invalid("environment_mismatch"));
    }
    Ok(())
}

/// Apply the normal scrub while retaining only the admitted scope's exact bindings.
/// # Errors
/// Rejects the record or supplied environment before producing the overlay.
pub(super) fn scrubbed_environment(
    record: &CompiledSourceHelper,
    environment: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, RenderError> {
    validate_step(record, environment)?;
    let mut scrubbed = toolchain_env::with_credential_scrub(environment);
    scrubbed.insert("RUSTUP_AUTO_INSTALL".to_owned(), "0".to_owned());
    for key in scope_of(record).allowed_keys() {
        if let Some(value) = environment.get(*key) {
            scrubbed.insert((*key).to_owned(), value.clone());
        }
    }
    Ok(scrubbed)
}

fn scope_of(record: &CompiledSourceHelper) -> NativeCredentialScope {
    record
        .execution_recipe()
        .map_or(NativeCredentialScope::Anonymous, |recipe| {
            recipe.credential_scope()
        })
}

fn validate_recipe(
    scope: NativeCredentialScope,
    operation: SourceBoundOperation,
    environment: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    validate_scope_operation(scope, operation)?;
    validate_environment(
        scope,
        operation,
        environment,
        scope != NativeCredentialScope::AppleSigning,
    )
}

fn validate_scope_operation(
    scope: NativeCredentialScope,
    operation: SourceBoundOperation,
) -> Result<(), RenderError> {
    if scope == NativeCredentialScope::Anonymous {
        return if matches!(
            operation,
            SourceBoundOperation::RustRegistryPublish
                | SourceBoundOperation::RustForgePublish
                | SourceBoundOperation::RustRegistryArtifactProof
                | SourceBoundOperation::RustReleasePrepareForge
                | SourceBoundOperation::RustReleaseSourceSnapshot
        ) {
            Err(invalid("publisher_scope_missing"))
        } else {
            Ok(())
        };
    }
    if matches!(
        scope,
        NativeCredentialScope::RustRegistryPublishOidc
            | NativeCredentialScope::RustRegistryPublishBootstrap
    ) {
        return if operation == SourceBoundOperation::RustRegistryPublish {
            Ok(())
        } else {
            Err(invalid("registry_scope_operation"))
        };
    }
    if scope == NativeCredentialScope::GithubReleasePublish {
        return if matches!(
            operation,
            SourceBoundOperation::RustForgePublish | SourceBoundOperation::RustReleasePrepareForge
        ) {
            Ok(())
        } else {
            Err(invalid("forge_scope_operation"))
        };
    }
    if scope == NativeCredentialScope::GithubReadOnly
        && GITHUB_READ_ONLY_OPERATIONS.contains(&operation)
    {
        return Ok(());
    }
    if scope == NativeCredentialScope::GithubReadOnly {
        return Err(invalid("github_scope_operation"));
    }
    if scope == NativeCredentialScope::GithubIssueWrite
        && GITHUB_ISSUE_WRITE_OPERATIONS.contains(&operation)
    {
        return Ok(());
    }
    if scope == NativeCredentialScope::GithubIssueWrite {
        return Err(invalid("github_issue_write_scope_operation"));
    }
    if scope == NativeCredentialScope::AptSigning && APT_SIGNING_OPERATIONS.contains(&operation) {
        return Ok(());
    }
    if scope == NativeCredentialScope::AptSigning {
        return Err(invalid("apt_scope_operation"));
    }
    if scope == NativeCredentialScope::OciRegistryPublish
        && OCI_PUBLISH_OPERATIONS.contains(&operation)
    {
        return Ok(());
    }
    if scope == NativeCredentialScope::OciRegistryPublish {
        return Err(invalid("oci_scope_operation"));
    }
    if scope == NativeCredentialScope::AppleSigning && APPLE_SIGNING_OPERATIONS.contains(&operation)
    {
        return Ok(());
    }
    if scope == NativeCredentialScope::AppleSigning {
        return Err(invalid("apple_scope_operation"));
    }
    Err(invalid("credential_scope_unreviewed"))
}

fn validate_environment(
    scope: NativeCredentialScope,
    operation: SourceBoundOperation,
    environment: &BTreeMap<String, String>,
    require_bindings: bool,
) -> Result<(), RenderError> {
    validate_scope_operation(scope, operation)?;
    for (key, value) in environment {
        if is_apple_credential_key(key) {
            if scope != NativeCredentialScope::AppleSigning {
                return Err(invalid("apple_credential_scope"));
            }
            validate_apple_binding(key, value)?;
            continue;
        }
        if key == "DOCKER_CONFIG" && scope != NativeCredentialScope::OciRegistryPublish {
            return Err(invalid("oci_config_scope"));
        }
        if let Some(expected) = credential_binding(scope, key) {
            if value != expected {
                return Err(invalid("credential_binding"));
            }
            continue;
        }
        if is_credential_key(key) {
            return Err(invalid("credential_environment"));
        }
        if value.contains("github.token")
            || value.contains("secrets.")
            || value.contains("ACTIONS_ID_TOKEN_REQUEST_")
        {
            return Err(invalid("credential_expression"));
        }
    }
    if require_bindings {
        for key in scope.allowed_keys() {
            if credential_binding(scope, key)
                .is_none_or(|expected| environment.get(*key).map(String::as_str) != Some(expected))
            {
                if scope == NativeCredentialScope::AppleSigning && environment.contains_key(*key) {
                    continue;
                }
                return Err(invalid("credential_missing"));
            }
        }
    }
    Ok(())
}

fn validate_apple_binding(key: &str, value: &str) -> Result<(), RenderError> {
    match key {
        "EXPECTED_TEAM_ID" if is_apple_team_id(value) => Ok(()),
        "EXPECTED_CERT_SHA256" if is_apple_certificate(value) => Ok(()),
        "APP_STORE_CONNECT_API_KEY_PATH" if value == APPLE_API_KEY_PATH_EXPRESSION => Ok(()),
        "DEVELOPER_ID_APPLICATION"
        | "DEVELOPER_ID_APPLICATION_P12_BASE64"
        | "DEVELOPER_ID_APPLICATION_P12_PASSWORD"
        | "APP_STORE_CONNECT_API_KEY_P8"
        | "APP_STORE_CONNECT_KEY_ID"
        | "APP_STORE_CONNECT_ISSUER_ID"
            if value == secret_expression(key) =>
        {
            Ok(())
        }
        _ => Err(invalid("apple_credential_binding")),
    }
}

fn secret_expression(key: &str) -> String {
    format!("${{{{ secrets.{key} }}}}")
}

fn is_apple_credential_key(key: &str) -> bool {
    matches!(
        key,
        "EXPECTED_TEAM_ID"
            | "EXPECTED_CERT_SHA256"
            | "DEVELOPER_ID_APPLICATION"
            | "DEVELOPER_ID_APPLICATION_P12_BASE64"
            | "DEVELOPER_ID_APPLICATION_P12_PASSWORD"
            | "APP_STORE_CONNECT_API_KEY_P8"
            | "APP_STORE_CONNECT_API_KEY_PATH"
            | "APP_STORE_CONNECT_KEY_ID"
            | "APP_STORE_CONNECT_ISSUER_ID"
    ) || key.starts_with("APP_STORE_CONNECT_")
        || key.starts_with("DEVELOPER_ID_")
}

fn is_apple_team_id(value: &str) -> bool {
    value.len() == 10
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

fn is_apple_certificate(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn credential_binding(scope: NativeCredentialScope, key: &str) -> Option<&'static str> {
    use NativeCredentialScope::{
        AptSigning, GithubIssueWrite, GithubReadOnly, GithubReleasePublish, OciRegistryPublish,
        RustRegistryPublishBootstrap, RustRegistryPublishOidc,
    };
    match (scope, key) {
        (
            GithubIssueWrite | GithubReadOnly | GithubReleasePublish | OciRegistryPublish,
            "GH_TOKEN",
        ) => Some(GITHUB_TOKEN_EXPRESSION),
        (AptSigning, "APT_GPG_PRIVATE_KEY") => Some(APT_PRIVATE_KEY_EXPRESSION),
        (AptSigning, "APT_GPG_PASSPHRASE") => Some(APT_PASSPHRASE_EXPRESSION),
        (OciRegistryPublish, "DOCKER_CONFIG") => Some(OCI_DOCKER_CONFIG_EXPRESSION),
        (RustRegistryPublishBootstrap, "CARGO_REGISTRY_TOKEN") => Some(REGISTRY_TOKEN_EXPRESSION),
        (RustRegistryPublishOidc, "ACTIONS_ID_TOKEN_REQUEST_URL") => Some(OIDC_URL_EXPRESSION),
        (RustRegistryPublishOidc, "ACTIONS_ID_TOKEN_REQUEST_TOKEN") => Some(OIDC_TOKEN_EXPRESSION),
        _ => None,
    }
}

fn is_credential_key(key: &str) -> bool {
    HELPER_CONTROL_KEYS.contains(&key)
        || velnor_actions_contract::is_secret_env_name(key)
        || toolchain_env::is_denied_credential_key(key)
        || key.starts_with("ACTIONS_ID_TOKEN_REQUEST_")
        || toolchain_env::is_denied_endpoint_key(key)
}

fn invalid(problem: &str) -> RenderError {
    RenderError::BadCommand(format!("source_helper_credentials:{problem}"))
}
