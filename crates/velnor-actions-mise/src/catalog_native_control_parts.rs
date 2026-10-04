//! Private measured two-domain construction shared by distinct control purposes.

use std::collections::BTreeMap;

use super::{
    DistributionHost, MiseError, PinnedTool, QualifiedNativeLaunch, ToolCacheDomain, ToolCatalog,
    merge_launch_environment, qualified_native_execution_environment, qualified_toolset_digest,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NativeControlParts {
    pub(super) python: QualifiedNativeLaunch,
    pub(super) github: QualifiedNativeLaunch,
    pub(super) environment: BTreeMap<String, String>,
    pub(super) qualification_digest: String,
}

pub(super) fn qualified_control_parts(
    catalog: &ToolCatalog,
    host: DistributionHost,
) -> Result<NativeControlParts, MiseError> {
    let python = catalog.native_launch_context(host, ToolCacheDomain::Full, PinnedTool::Python)?;
    let github = catalog.native_launch_context(host, ToolCacheDomain::Planning, PinnedTool::Gh)?;
    let mut environment = qualified_native_execution_environment(
        catalog,
        host,
        ToolCacheDomain::Full,
        &[PinnedTool::Python],
    )?;
    merge_launch_environment(&[&python, &github], &mut environment)?;
    let mise = super::QualifiedDistribution::require_for_generator(
        super::DistributionTool::Mise,
        host,
        super::DistributionRequirement::RequiresNoMiserc,
    )?;
    let qualification_digest = qualified_toolset_digest(&[
        mise,
        python.distribution().clone(),
        github.distribution().clone(),
    ]);
    environment.insert(
        "VELNOR_QUALIFIED_TOOL_IDENTITY".to_owned(),
        format!("qualified-tools@{qualification_digest}"),
    );
    Ok(NativeControlParts {
        python,
        github,
        environment,
        qualification_digest,
    })
}
