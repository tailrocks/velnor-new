//! Compiled source authority for an isolated OpenTofu provider producer.
//!
//! The producer has no repository checkout and never runs a provider. The
//! native init process verifies the captured lockfile, then this owner copies
//! only selected provider package directories into a fresh output.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog, catalog::qualification::DistributionHost};

use crate::{OrchestratorError, tofu_cache_source::ProviderExportDescriptor};

const MAX_LOCK_BYTES: usize = 16 * 1024;
const MAX_SELECTIONS: usize = 128;
const MAX_ENCODED_ARG_BYTES: usize = 64 * 1024;
const BODY: &str = include_str!("tofu_producer_source.sh");

/// Build the one compiled source record used by a pure provider producer.
///
/// Lock bytes and the synthetic configuration are octal encoded so the
/// invocation carries no control characters. Runtime paths stay in the
/// step environment; they never become source bytes or shell fragments.
/// # Errors
/// Rejects an unqualified catalog, malformed descriptor, oversized literal,
/// or invalid generated source record.
pub(crate) fn compiled_helper(
    descriptor: &ProviderExportDescriptor,
    target: &str,
    catalog: &ToolCatalog,
    candidate: &str,
    output: &str,
    generator_version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let tofu_version = catalog.version(PinnedTool::Opentofu);
    let host = host_for_target(target)?;
    let distribution = catalog.native_distribution(host, PinnedTool::Opentofu)?;
    distribution.required_install_plan()?;
    let binary_path = distribution.required_installed_binary_path()?;
    if !velnor_actions_contract::is_supported_target(target)
        || candidate.is_empty()
        || output.is_empty()
        || generator_version != env!("CARGO_PKG_VERSION")
        || candidate.chars().any(char::is_control)
        || output.chars().any(char::is_control)
    {
        return Err(contract("tofu_provider_export_binding"));
    }
    let parsed =
        crate::tofu_cache_source::descriptor_from_lock(&descriptor.root, &descriptor.lock_content);
    if parsed.as_ref() != Some(descriptor) {
        return Err(contract("tofu_provider_export_descriptor_mismatch"));
    }
    qualify_descriptor(descriptor, tofu_version)?;
    let config = synthetic_config(&descriptor.selections)?;
    let lock = octal(descriptor.lock_content.as_bytes())?;
    let config = octal(config.as_bytes())?;
    let root_key = velnor_actions_tofu::key_for_root(&descriptor.root);
    let arguments = vec![
        target.to_owned(),
        lock,
        config,
        tofu_version.to_owned(),
        root_key,
        binary_path.to_owned(),
    ];
    let source = velnor_actions_contract::generated_source(env!("CARGO_PKG_VERSION"), BODY)
        .map_err(OrchestratorError::from)?;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let helper = SourceBoundHelper::compiled(
        SourceBoundOperation::TofuProviderExport,
        SourceBoundOperation::TofuProviderExport.path(),
        &digest,
    )
    .map_err(OrchestratorError::from)?;
    let selectors = catalog.native_tool_specs(host, &[PinnedTool::Opentofu])?;
    let invocation = HelperInvocation::compiled(helper, arguments, selectors)
        .map_err(OrchestratorError::from)?;
    CompiledSourceHelper::compiled(invocation, source).map_err(OrchestratorError::from)
}

fn host_for_target(target: &str) -> Result<DistributionHost, OrchestratorError> {
    match target {
        "x86_64-unknown-linux-gnu" => Ok(DistributionHost::LinuxAmd64),
        "aarch64-unknown-linux-gnu" => Ok(DistributionHost::LinuxArm64),
        "aarch64-apple-darwin" => Ok(DistributionHost::MacosArm64),
        _ => Err(contract("tofu_provider_export_host_unqualified")),
    }
}

/// Bind only the runtime cache paths owned by the producer job.
///
/// Descriptor bytes remain literal invocation arguments; these values are
/// transport paths supplied by the fixed job constructor.
pub(crate) fn producer_environment(
    candidate: Option<&str>,
    output: &str,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut environment = BTreeMap::new();
    if let Some(candidate) = candidate {
        validate_runtime_path(candidate)?;
        environment.insert(
            "VELNOR_TOFU_PROVIDER_CANDIDATE".to_owned(),
            candidate.to_owned(),
        );
    }
    validate_runtime_path(output)?;
    environment.insert("VELNOR_TOFU_PROVIDER_OUTPUT".to_owned(), output.to_owned());
    Ok(environment)
}

fn qualify_descriptor(
    descriptor: &ProviderExportDescriptor,
    tofu_version: &str,
) -> Result<(), OrchestratorError> {
    if tofu_version != velnor_actions_mise::OPENTOFU_VERSION
        || descriptor.lock_content.is_empty()
        || descriptor.lock_content.len() > MAX_LOCK_BYTES
        || descriptor.lock_content.as_bytes().contains(&0)
        || descriptor.selections.is_empty()
        || descriptor.selections.len() > MAX_SELECTIONS
        || descriptor
            .selections
            .iter()
            .any(|(source, version)| !safe_source(source) || !safe_version(version))
    {
        return Err(contract("tofu_provider_export_unqualified"));
    }
    let mut seen = BTreeMap::new();
    for (source, version) in &descriptor.selections {
        if seen.insert(source, version).is_some() {
            return Err(contract("tofu_provider_export_duplicate_selection"));
        }
    }
    Ok(())
}

fn synthetic_config(selections: &[(String, String)]) -> Result<String, OrchestratorError> {
    let mut config = String::from("terraform {\n  required_providers {\n");
    for (index, (source, version)) in selections.iter().enumerate() {
        if !safe_source(source) || !safe_version(version) {
            return Err(contract("tofu_provider_export_bad_selection"));
        }
        config.push_str(&format!(
            "    v{index} = {{\n      source = \"{source}\"\n      version = \"= {version}\"\n    }}\n"
        ));
    }
    config.push_str("  }\n}\n");
    Ok(config)
}

fn octal(bytes: &[u8]) -> Result<String, OrchestratorError> {
    let mut encoded = String::with_capacity(bytes.len() * 4);
    for byte in bytes {
        encoded.push('\\');
        encoded.push(char::from(b'0' + (byte >> 6)));
        encoded.push(char::from(b'0' + ((byte >> 3) & 7)));
        encoded.push(char::from(b'0' + (byte & 7)));
    }
    if encoded.len() > MAX_ENCODED_ARG_BYTES {
        return Err(contract("tofu_provider_export_argument_too_large"));
    }
    Ok(encoded)
}

fn safe_source(source: &str) -> bool {
    let mut parts = source.split('/');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (
            Some("registry.opentofu.org" | "registry.terraform.io"),
            Some(namespace),
            Some(provider),
            None
        ) if !namespace.is_empty()
            && !provider.is_empty()
            && namespace.bytes().all(source_segment_byte)
            && provider.bytes().all(source_segment_byte)
    )
}

fn source_segment_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
}

fn safe_version(version: &str) -> bool {
    if version.is_empty() || version.len() > 128 || !version.as_bytes()[0].is_ascii_digit() {
        return false;
    }
    if !version.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'+' | b'-')
    }) {
        return false;
    }
    let Some(core) = version.split(['+', '-']).next() else {
        return false;
    };
    let components: Vec<_> = core.split('.').collect();
    components.len() == 3
        && components.iter().all(|component| {
            !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
        })
        && (version.len() == core.len() || version.len() > core.len() + 1)
}

fn validate_runtime_path(path: &str) -> Result<(), OrchestratorError> {
    if path.is_empty()
        || path.chars().any(char::is_control)
        || path.contains("$(")
        || path.contains('`')
    {
        return Err(contract("tofu_provider_export_runtime_path"));
    }
    let runner_prefix = "${{ runner.temp }}";
    let path = match path.strip_prefix(runner_prefix) {
        Some(rest) => rest,
        None => path,
    };
    if !path.starts_with('/')
        || path.split('/').enumerate().any(|(index, segment)| {
            (index == 0 && !segment.is_empty())
                || (index > 0 && (segment.is_empty() || segment == "." || segment == ".."))
        })
    {
        return Err(contract("tofu_provider_export_runtime_path"));
    }
    Ok(())
}

fn contract(problem: impl Into<String>) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.into(),
    }
}

#[cfg(test)]
#[path = "tofu_producer_source_tests.rs"]
mod tests;
