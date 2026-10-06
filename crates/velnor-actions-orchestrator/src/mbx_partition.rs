//! Canonical MBX tool context from the sole compiled installation authority.
//!
//! These bytes partition native receipts; they do not authenticate their origin.
//! Runtime admission must independently verify the complete tool manifest and
//! every historical receipt's repository, source, workflow, run and attempt.

use serde::Serialize;
use velnor_actions_contract::{CompiledSourceHelper, ToolCacheDescriptor, ToolCacheDomain};
use velnor_actions_mise::{
    PinnedTool, ToolCatalog,
    catalog::qualification::{
        DistributionHost, DistributionRequirement, DistributionTool, QualifiedDistribution,
    },
};
use velnor_actions_workflow_renderer::MiseSetup;

use crate::OrchestratorError;

/// Generation-time compatibility shape; constructed only from compiled owners.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ToolContext {
    schema: u32,
    descriptor: ToolCacheDescriptor,
    rust: RustContext,
    mbx: MbxContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RustContext {
    toolchain: String,
    selector: String,
    host: String,
    components: Vec<String>,
    targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MbxContext {
    qualification: String,
    source_repository: String,
    source_commit: String,
    source_tree: String,
    binary_sha256: String,
    abi: String,
}

impl ToolContext {
    /// Reconstruct both installation bytes and complete descriptor before binding.
    /// Runtime manifest verification remains required before native receipt use.
    pub(crate) fn for_installation(
        catalog: &ToolCatalog,
        descriptor: &ToolCacheDescriptor,
        installation: &CompiledSourceHelper,
        setup: &MiseSetup,
        version: &str,
    ) -> Result<Self, OrchestratorError> {
        descriptor.validate()?;
        let host = host_for_target(&descriptor.target)?;
        validate_setup(setup, descriptor, host, version)?;
        let rust_selector = catalog
            .tool_spec(catalog.compiler_tool())
            .map_err(owner_error)?;
        if descriptor.domain != ToolCacheDomain::Full
            || catalog.rust_host().target_triple() != descriptor.target
            || !descriptor.selectors.contains(&rust_selector)
            || !descriptor.selectors.contains(
                &catalog
                    .tool_spec(PinnedTool::MrBoxington)
                    .map_err(owner_error)?,
            )
        {
            return Err(invalid("mbx_partition_requires_complete_compiler_domain"));
        }
        let expected = velnor_actions_mise::catalog::tool_prepare::helper_for_tools(
            catalog,
            ToolCacheDomain::Full,
            host,
            &descriptor.selectors,
            version,
        )
        .map_err(owner_error)?;
        if installation != &expected {
            return Err(invalid("mbx_partition_installation_changed"));
        }
        let expected_descriptor =
            velnor_actions_workflow_renderer::tool_producer_steps::descriptor_for_record(
                &expected,
                &descriptor.runs_on,
                &descriptor.target,
                ToolCacheDomain::Full,
                setup,
                std::slice::from_ref(&expected),
            )?;
        if descriptor != &expected_descriptor {
            return Err(invalid("mbx_partition_tool_descriptor_changed"));
        }
        let mbx = QualifiedDistribution::require_for_generator(
            DistributionTool::Mbx,
            host,
            DistributionRequirement::MbxTransport,
        )
        .map_err(owner_error)?;
        Ok(Self::from_owners(catalog, descriptor, rust_selector, &mbx))
    }

    fn from_owners(
        catalog: &ToolCatalog,
        descriptor: &ToolCacheDescriptor,
        rust_selector: String,
        mbx: &QualifiedDistribution,
    ) -> Self {
        let options = catalog.rust_install_options();
        let mut targets = options.targets().to_vec();
        targets.push(catalog.rust_host().target_triple().to_owned());
        targets.sort();
        targets.dedup();
        Self {
            schema: 1,
            descriptor: descriptor.clone(),
            rust: RustContext {
                toolchain: catalog.rustup_toolchain(),
                selector: rust_selector,
                host: catalog.rust_host().target_triple().to_owned(),
                components: options.components().to_vec(),
                targets,
            },
            mbx: MbxContext {
                qualification: mbx.qualification_digest(),
                source_repository: mbx.source_repository().to_owned(),
                source_commit: mbx.source_commit().to_owned(),
                source_tree: mbx.source_tree().to_owned(),
                binary_sha256: mbx.binary_sha256().to_owned(),
                abi: mbx.abi().to_owned(),
            },
        }
    }

    /// Opaque canonical native partition context, never signing authority.
    pub(crate) fn canonical(&self) -> Result<String, OrchestratorError> {
        Ok(velnor_actions_contract::canonical_json_str(self)?)
    }
}

fn validate_setup(
    setup: &MiseSetup,
    descriptor: &ToolCacheDescriptor,
    host: DistributionHost,
    version: &str,
) -> Result<(), OrchestratorError> {
    let bootstrap = setup.bootstrap(ToolCacheDomain::Full, &descriptor.runs_on)?;
    let expected = velnor_actions_mise::catalog::mise_acquisition::helper_for_domain(
        ToolCacheDomain::Full,
        host,
        version,
    )
    .map_err(owner_error)?;
    let mise = QualifiedDistribution::require_for_generator(
        DistributionTool::Mise,
        host,
        DistributionRequirement::RequiresNoMiserc,
    )
    .map_err(owner_error)?;
    if bootstrap.helper != expected
        || bootstrap.target != descriptor.target
        || bootstrap.binary_sha256 != mise.binary_sha256()
        || setup.version != mise.version()
    {
        return Err(invalid("mbx_partition_bootstrap_changed"));
    }
    Ok(())
}

fn host_for_target(target: &str) -> Result<DistributionHost, OrchestratorError> {
    match target {
        "x86_64-unknown-linux-gnu" => Ok(DistributionHost::LinuxAmd64),
        "aarch64-unknown-linux-gnu" => Ok(DistributionHost::LinuxArm64),
        "aarch64-apple-darwin" => Ok(DistributionHost::MacosArm64),
        _ => Err(invalid("mbx_partition_unknown_tool_host")),
    }
}

fn owner_error(error: velnor_actions_mise::MiseError) -> OrchestratorError {
    invalid(&error.to_string())
}

fn invalid(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_owned(),
    }
}

#[cfg(test)]
#[path = "mbx_partition_tests.rs"]
mod tests;
