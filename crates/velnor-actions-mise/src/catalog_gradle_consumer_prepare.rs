//! Exact consumer preparation reconstructed from the closed SDK profile.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    compiled_source_sha256,
};

use super::super::{
    qualification::{
        DistributionHost, DistributionRequirement, DistributionTool, QualifiedDistribution,
        qualified_toolset_digest,
    },
    tool_prepare,
};
use super::{GradleConsumerContext, invalid};
use crate::MiseError;

/// Prepare the closed Java, consumer engine and wrapper bootstrap trio.
/// # Errors
/// Rejects unavailable host qualification or invalid generator source versions.
pub fn helper_for_host(
    host: DistributionHost,
    version: &str,
) -> Result<CompiledSourceHelper, MiseError> {
    let context = GradleConsumerContext::require(host)?;
    let mise = QualifiedDistribution::require_for_generator(
        DistributionTool::Mise,
        host,
        DistributionRequirement::RequiresNoMiserc,
    )?;
    let source = tool_prepare::preparation_source(version)?;
    let operation = SourceBoundOperation::MiseToolPrepare;
    let descriptor = SourceBoundHelper::compiled(
        operation,
        operation.path(),
        &compiled_source_sha256(source.as_bytes()),
    )
    .map_err(|error| tool_prepare::contract(&error))?;
    let selectors = context.selectors();
    let mut args = vec![
        context.domain().name().to_owned(),
        host.abi().to_owned(),
        tool_prepare::encode_distributions(&context.records())?,
    ];
    args.extend(selectors.iter().cloned());
    let invocation = HelperInvocation::compiled(descriptor, args, selectors.clone())
        .map_err(|error| tool_prepare::contract(&error))?;
    let mut environment: BTreeMap<_, _> = crate::ISOLATION_ENV
        .into_iter()
        .chain(crate::NO_AUTO_INSTALL_ENV)
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    environment.extend(context.environment());
    environment.insert(
        "VELNOR_MISE_SHA256".to_owned(),
        mise.binary_sha256().to_owned(),
    );
    environment.insert(
        "VELNOR_TOOL_CACHE_IDENTITY".to_owned(),
        format!(
            "toolset@{}",
            velnor_actions_contract::digest_b3(selectors.join("\0").as_bytes())
        ),
    );
    let mut records = context.records();
    records.push(mise);
    environment.insert(
        "VELNOR_QUALIFIED_TOOL_IDENTITY".to_owned(),
        format!("qualified-tools@{}", qualified_toolset_digest(&records)),
    );
    Ok(CompiledSourceHelper::compiled(invocation, source)
        .map_err(|error| tool_prepare::contract(&error))?
        .with_environment(environment))
}

/// Reconstruct complete source, selected roles, domain, footprint and environment.
/// # Errors
/// Rejects managed Gradle, changed selectors, source, paths, identity or environment.
pub fn record_for_invocation(
    invocation: &HelperInvocation,
    environment: &BTreeMap<String, String>,
    version: &str,
) -> Result<CompiledSourceHelper, MiseError> {
    if invocation.descriptor().operation() != SourceBoundOperation::MiseToolPrepare
        || invocation.args().first().map(String::as_str)
            != Some(velnor_actions_contract::ToolCacheDomain::GradleBootstrap.name())
    {
        return Err(invalid());
    }
    let host_argument = invocation.args().get(1).ok_or_else(invalid)?;
    let host = [
        DistributionHost::MacosArm64,
        DistributionHost::LinuxAmd64,
        DistributionHost::LinuxArm64,
    ]
    .into_iter()
    .find(|host| host.abi() == host_argument)
    .ok_or_else(invalid)?;
    let expected = helper_for_host(host, version)?;
    if expected.invocation() != invocation || expected.environment() != environment {
        return Err(invalid());
    }
    Ok(expected)
}
