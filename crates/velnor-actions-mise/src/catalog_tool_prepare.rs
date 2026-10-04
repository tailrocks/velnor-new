//! Pure catalog installation, reconstructed from compiled owner authority.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    ToolCacheDomain,
};

use super::{
    PinnedTool, ToolCatalog,
    qualification::{
        DistributionHost, DistributionRequirement, DistributionTool, QualifiedDistribution,
    },
};
use crate::{MISE_GLOBAL_FLAGS, MiseError};

#[path = "catalog_tool_prepare_body.rs"]
mod body;

#[path = "catalog_tool_prepare_config.rs"]
mod configuration;

/// Bind sorted exact tool selectors to their compiled installation owner.
/// # Errors
/// Rejects foreign catalogs/selectors, unsuitable domains and unqualified Mise.
pub fn helper_for_tools(
    catalog: &ToolCatalog,
    domain: ToolCacheDomain,
    host: DistributionHost,
    selectors: &[String],
    generator_version: &str,
) -> Result<CompiledSourceHelper, MiseError> {
    if domain == ToolCacheDomain::GradleBootstrap {
        let context = super::gradle_consumer::GradleConsumerContext::require(host)?;
        if catalog != &ToolCatalog::pinned() || selectors != context.selectors() {
            return Err(invalid());
        }
        return super::gradle_consumer::helper_for_host(host, generator_version);
    }
    let tools = validate(catalog, domain, host, selectors)?;
    if selectors.contains(&catalog.tool_spec(catalog.compiler_tool())?) {
        if catalog.rust_host().target_triple() != host.abi() {
            return Err(invalid());
        }
        let mut install = install_prefix();
        install.extend_from_slice(selectors);
        return super::rust_prepare::helper_for_install(
            catalog,
            super::rust_prepare::RustPrepareDomain::Tools,
            &install,
            generator_version,
        );
    }
    let mise = QualifiedDistribution::require_for_generator(
        DistributionTool::Mise,
        host,
        DistributionRequirement::RequiresNoMiserc,
    )?;
    compiled(catalog, domain, selectors, &tools, generator_version, &mise)
}

fn compiled(
    catalog: &ToolCatalog,
    domain: ToolCacheDomain,
    selectors: &[String],
    tools: &[PinnedTool],
    version: &str,
    mise: &QualifiedDistribution,
) -> Result<CompiledSourceHelper, MiseError> {
    let source = body::source(version)?;
    let operation = SourceBoundOperation::MiseToolPrepare;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &digest)
        .map_err(|error| contract(&error))?;
    let args = std::iter::once(argument(domain).to_owned())
        .chain(std::iter::once(mise.host().abi().to_owned()))
        .chain(std::iter::once(configuration::encode(
            catalog,
            mise.host(),
            tools,
        )?))
        .chain(selectors.iter().cloned())
        .collect();
    let invocation = HelperInvocation::compiled(descriptor, args, selectors.to_vec())
        .map_err(|error| contract(&error))?;
    let mut env: BTreeMap<_, _> = crate::command::ISOLATION_ENV
        .into_iter()
        .chain(crate::command::NO_AUTO_INSTALL_ENV)
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    env.extend(domain.home_environment());
    env.insert("MISE_DATA_DIR".to_owned(), domain.root().to_owned());
    env.insert(
        "VELNOR_MISE_SHA256".to_owned(),
        mise.binary_sha256().to_owned(),
    );
    env.insert(
        "VELNOR_TOOL_CACHE_IDENTITY".to_owned(),
        format!(
            "toolset@{}",
            velnor_actions_contract::digest_b3(selectors.join("\0").as_bytes())
        ),
    );
    let native_tools: Vec<_> = tools
        .iter()
        .copied()
        .filter(|tool| ToolCatalog::requires_native_host(*tool))
        .collect();
    let mut records = catalog.qualified_distributions(mise.host(), &native_tools)?;
    records.push(mise.clone());
    env.insert(
        "VELNOR_QUALIFIED_TOOL_IDENTITY".to_owned(),
        format!(
            "qualified-tools@{}",
            super::qualification::qualified_toolset_digest(&records)
        ),
    );
    Ok(CompiledSourceHelper::compiled(invocation, source)
        .map_err(|error| contract(&error))?
        .with_environment(env))
}

pub(super) fn authenticated_preparation_source(version: &str) -> Result<String, MiseError> {
    body::warm_source(version)
}

pub(super) fn preparation_source(version: &str) -> Result<String, MiseError> {
    body::source(version)
}

pub(super) fn encode_distributions(records: &[QualifiedDistribution]) -> Result<String, MiseError> {
    configuration::encode_records(records)
}

/// Reconstruct exact source, footprint and environment from the owner's factory.
/// # Errors
/// Rejects altered source bindings, arguments, footprints or owner environment.
pub fn record_for_invocation(
    invocation: &HelperInvocation,
    env: &BTreeMap<String, String>,
    generator_version: &str,
) -> Result<CompiledSourceHelper, MiseError> {
    if invocation.descriptor().operation() != SourceBoundOperation::MiseToolPrepare {
        return super::rust_prepare::record_for_invocation(invocation, env, generator_version);
    }
    if invocation.args().first().map(String::as_str)
        == Some(ToolCacheDomain::GradleBootstrap.name())
    {
        return super::gradle_consumer::record_for_invocation(invocation, env, generator_version);
    }
    let domain = domain_from_argument(invocation.args().first().ok_or_else(invalid)?)?;
    let host = host_from_argument(invocation.args().get(1).ok_or_else(invalid)?)?;
    let selectors = invocation.args().get(3..).ok_or_else(invalid)?;
    let expected = helper_for_tools(
        &ToolCatalog::pinned(),
        domain,
        host,
        selectors,
        generator_version,
    )?;
    verify_reconstruction(expected, invocation, env)
}

fn verify_reconstruction(
    expected: CompiledSourceHelper,
    invocation: &HelperInvocation,
    env: &BTreeMap<String, String>,
) -> Result<CompiledSourceHelper, MiseError> {
    if expected.invocation() != invocation || expected.environment() != env {
        return Err(invalid());
    }
    Ok(expected)
}

fn validate(
    catalog: &ToolCatalog,
    domain: ToolCacheDomain,
    host: DistributionHost,
    specs: &[String],
) -> Result<Vec<PinnedTool>, MiseError> {
    if specs.is_empty() || specs.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid());
    }
    let tools = selected_tools(catalog, host, specs)?;
    if tools.contains(&catalog.compiler_tool()) {
        return (domain == ToolCacheDomain::Full)
            .then_some(tools)
            .ok_or_else(invalid);
    }
    let pinned = ToolCatalog::pinned();
    if catalog != &pinned
        && catalog != &pinned.for_native_kind("native_xcode_project_ci")?
        && catalog != &pinned.for_native_source_kind("native_xcode_project_ci")?
    {
        return Err(invalid());
    }
    let allowed: &[PinnedTool] = match domain {
        ToolCacheDomain::Planning => &[
            PinnedTool::Gh,
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
            PinnedTool::Alint,
        ],
        ToolCacheDomain::NpmBootstrap => &[PinnedTool::Node],
        ToolCacheDomain::BunBootstrap => &[PinnedTool::Bun],
        ToolCacheDomain::TofuBootstrap => &[PinnedTool::Opentofu],
        ToolCacheDomain::GradleBootstrap => &[PinnedTool::Java, PinnedTool::Gradle],
        ToolCacheDomain::Full => return Ok(tools),
    };
    if tools.iter().any(|tool| !allowed.contains(tool)) {
        return Err(invalid());
    }
    Ok(tools)
}

fn selected_tools(
    catalog: &ToolCatalog,
    host: DistributionHost,
    specs: &[String],
) -> Result<Vec<PinnedTool>, MiseError> {
    specs
        .iter()
        .map(|spec| {
            let tool = PinnedTool::ALL
                .into_iter()
                .find(|tool| {
                    if matches!(tool, PinnedTool::Rust | PinnedTool::RustDesktop)
                        && *tool != catalog.compiler_tool()
                    {
                        return false;
                    }
                    if ToolCatalog::requires_native_host(*tool) {
                        let slot = match tool {
                            PinnedTool::Java => "graalvm-community-jdk",
                            _ => tool.tool_name(),
                        };
                        spec.starts_with(&format!("http:{slot}["))
                    } else {
                        catalog
                            .tool_spec(*tool)
                            .is_ok_and(|candidate| candidate == *spec)
                    }
                })
                .ok_or_else(invalid)?;
            if catalog.native_tool_spec(host, tool)? != *spec {
                return Err(invalid());
            }
            Ok(tool)
        })
        .collect()
}

pub(super) const fn argument(domain: ToolCacheDomain) -> &'static str {
    match domain {
        ToolCacheDomain::Planning => "planning",
        ToolCacheDomain::Full => "full",
        ToolCacheDomain::NpmBootstrap => "npm-bootstrap",
        ToolCacheDomain::BunBootstrap => "bun-bootstrap",
        ToolCacheDomain::TofuBootstrap => "tofu-bootstrap",
        ToolCacheDomain::GradleBootstrap => "gradle-bootstrap",
    }
}

fn domain_from_argument(value: &str) -> Result<ToolCacheDomain, MiseError> {
    [
        ToolCacheDomain::Planning,
        ToolCacheDomain::Full,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ]
    .into_iter()
    .find(|domain| argument(*domain) == value)
    .ok_or_else(invalid)
}

fn install_prefix() -> Vec<String> {
    std::iter::once("mise")
        .chain(MISE_GLOBAL_FLAGS.iter().copied())
        .chain(std::iter::once("install"))
        .map(str::to_owned)
        .collect()
}

fn host_from_argument(value: &str) -> Result<DistributionHost, MiseError> {
    [
        DistributionHost::LinuxAmd64,
        DistributionHost::LinuxArm64,
        DistributionHost::MacosArm64,
    ]
    .into_iter()
    .find(|host| host.abi() == value)
    .ok_or_else(invalid)
}

pub(super) fn contract(error: &velnor_actions_contract::ContractError) -> MiseError {
    MiseError::Contract {
        problem: error.to_string(),
    }
}

fn invalid() -> MiseError {
    MiseError::Contract {
        problem: "invalid_mise_tool_prepare_invocation".to_owned(),
    }
}

#[cfg(test)]
#[path = "catalog_tool_prepare_tests.rs"]
mod tests;
