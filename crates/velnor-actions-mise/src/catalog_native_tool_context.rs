//! Sealed native launch locations derived from measured distribution records.
//!
//! This generation-time record grants no runtime admission. Before the first
//! mutable cached launch, callers must authenticate the full tool receipt or
//! run the existing cold source preparation chain.

use std::collections::BTreeMap;

use velnor_actions_contract::ToolCacheDomain;

use super::{
    PinnedTool, ToolCatalog,
    qualification::{
        DistributionHost, DistributionRequirement, DistributionTool, QualifiedDistribution,
        QualifiedLaunchEntry, qualified_toolset_digest,
    },
};
use crate::MiseError;

/// Fixed Python control plus independent planning GitHub execution authority.
#[path = "catalog_native_snapshot_context.rs"]
pub mod snapshot_control;

/// Fixed release admission control, separate from snapshot source production.
#[path = "catalog_native_admission_context.rs"]
pub mod admission_control;

#[path = "catalog_native_control_parts.rs"]
mod control_parts;

/// SDK-owned execution locations; no deserialization or caller construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedNativeLaunch {
    tool: PinnedTool,
    domain: ToolCacheDomain,
    distribution: QualifiedDistribution,
    install_relative_root: &'static str,
    launch: QualifiedLaunchEntry,
    launch_relative_path: &'static str,
}

impl ToolCatalog {
    /// Resolve native execution authority for an explicit host and cache owner.
    /// # Errors
    /// Rejects wrong domains and missing measured installation or launch records.
    pub fn native_launch_context(
        &self,
        host: DistributionHost,
        domain: ToolCacheDomain,
        tool: PinnedTool,
    ) -> Result<QualifiedNativeLaunch, MiseError> {
        validate_domain(domain, tool)?;
        let distribution = self.native_distribution(host, tool)?;
        let install_relative_root = distribution.required_install_plan()?.root_relative_path();
        let launch_relative_path = distribution.required_installed_binary_path()?;
        let launch = distribution
            .launch_entries()
            .iter()
            .find(|entry| {
                entry.installed_relative_path() == Some(launch_relative_path)
                    && entry.sha256() == distribution.binary_sha256()
            })
            .copied()
            .ok_or_else(|| invalid("native primary launch record absent"))?;
        if !launch_relative_path
            .strip_prefix(install_relative_root)
            .is_some_and(|suffix| suffix.starts_with('/'))
        {
            return Err(invalid("native launch escapes measured installation root"));
        }
        Ok(QualifiedNativeLaunch {
            tool,
            domain,
            distribution,
            install_relative_root,
            launch,
            launch_relative_path,
        })
    }
}

impl QualifiedNativeLaunch {
    /// Closed logical tool.
    #[must_use]
    pub const fn tool(&self) -> PinnedTool {
        self.tool
    }
    /// Exact cache ownership.
    #[must_use]
    pub const fn domain(&self) -> ToolCacheDomain {
        self.domain
    }
    /// Explicit measured host.
    #[must_use]
    pub const fn host(&self) -> DistributionHost {
        self.distribution.host()
    }
    /// Fixed workflow tokenized Mise root, never selected by repository data.
    #[must_use]
    pub const fn owned_root(&self) -> &'static str {
        self.domain.root()
    }
    /// Measured HTTP backend root relative to the fixed Mise data directory.
    #[must_use]
    pub const fn install_relative_root(&self) -> &'static str {
        self.install_relative_root
    }
    /// Measured primary launch location relative to the fixed Mise directory.
    #[must_use]
    pub const fn launch_relative_path(&self) -> &'static str {
        self.launch_relative_path
    }
    /// Complete fixed workflow executable argument.
    #[must_use]
    pub fn executable(&self) -> String {
        format!("{}/{}", self.owned_root(), self.launch_relative_path)
    }
    /// Complete measured installation root.
    #[must_use]
    pub fn install_root(&self) -> String {
        format!("{}/{}", self.owned_root(), self.install_relative_root)
    }
    /// Exact qualified installation selector.
    #[must_use]
    pub const fn selector(&self) -> &'static str {
        self.distribution.selector()
    }
    /// Primary executable or launcher input digest.
    #[must_use]
    pub const fn launch_sha256(&self) -> &'static str {
        self.launch.sha256()
    }
    /// Complete immutable distribution identity, including the launch closure.
    #[must_use]
    pub fn qualification_digest(&self) -> String {
        self.distribution.qualification_digest()
    }
    /// Complete sealed record for comparison with cache descriptor authority.
    #[must_use]
    pub const fn distribution(&self) -> &QualifiedDistribution {
        &self.distribution
    }
    /// Measured executable and interpreter input closure.
    #[must_use]
    pub const fn launch_entries(&self) -> &'static [QualifiedLaunchEntry] {
        self.distribution.launch_entries()
    }
    /// Fixed qualified home and PATH directories derived from the measured root.
    /// # Errors
    /// Rejects absent installer qualification.
    pub fn environment(&self) -> Result<BTreeMap<String, String>, MiseError> {
        Ok(self
            .distribution
            .required_install_plan()?
            .environment()
            .iter()
            .map(|entry| {
                let root = self.install_root();
                let path = if entry.relative_path().is_empty() {
                    root
                } else {
                    format!("{root}/{}", entry.relative_path())
                };
                (entry.name().to_owned(), path)
            })
            .collect())
    }
}

fn validate_domain(domain: ToolCacheDomain, tool: PinnedTool) -> Result<(), MiseError> {
    let allowed = match domain {
        ToolCacheDomain::Full => matches!(
            tool,
            PinnedTool::Bun
                | PinnedTool::Node
                | PinnedTool::Opentofu
                | PinnedTool::Python
                | PinnedTool::Uv
                | PinnedTool::Java
                | PinnedTool::Gradle
                | PinnedTool::ReleasePlz
                | PinnedTool::CargoSemverChecks
        ),
        ToolCacheDomain::NpmBootstrap => tool == PinnedTool::Node,
        ToolCacheDomain::BunBootstrap => tool == PinnedTool::Bun,
        ToolCacheDomain::TofuBootstrap => tool == PinnedTool::Opentofu,
        // The consumer profile owns two distinct Gradle releases. A logical
        // managed Gradle slot cannot select either consumer execution role.
        ToolCacheDomain::GradleBootstrap => tool == PinnedTool::Java,
        ToolCacheDomain::Planning => tool == PinnedTool::Gh,
    };
    if !allowed {
        return Err(invalid(
            "native launch cache owner does not admit selected tool",
        ));
    }
    Ok(())
}

fn invalid(problem: &str) -> MiseError {
    MiseError::Contract {
        problem: problem.to_owned(),
    }
}

/// Compiler-free native execution environment from complete owned qualification.
/// # Errors
/// Rejects missing native installation records, wrong domains and unpublished Mise.
pub fn qualified_native_execution_environment(
    catalog: &ToolCatalog,
    host: DistributionHost,
    domain: ToolCacheDomain,
    tools: &[PinnedTool],
) -> Result<BTreeMap<String, String>, MiseError> {
    if tools.is_empty() {
        return Err(invalid("native execution requires a selected tool"));
    }
    let mut selectors = Vec::new();
    for tool in tools {
        selectors.push(
            catalog
                .native_launch_context(host, domain, *tool)?
                .selector(),
        );
    }
    selectors.sort_unstable();
    selectors.dedup();
    let mise = QualifiedDistribution::require_for_generator(
        DistributionTool::Mise,
        host,
        DistributionRequirement::RequiresNoMiserc,
    )?;
    let mut records = catalog.qualified_distributions(host, tools)?;
    records.push(mise.clone());
    let mut environment: BTreeMap<_, _> = crate::command::ISOLATION_ENV
        .into_iter()
        .chain(crate::command::NO_AUTO_INSTALL_ENV)
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    environment.extend(domain.home_environment());
    environment.extend([
        ("MISE_DATA_DIR".to_owned(), domain.root().to_owned()),
        (
            "VELNOR_MISE_SHA256".to_owned(),
            mise.binary_sha256().to_owned(),
        ),
        (
            "VELNOR_TOOL_CACHE_IDENTITY".to_owned(),
            format!(
                "toolset@{}",
                velnor_actions_contract::digest_b3(selectors.join("\0").as_bytes())
            ),
        ),
        (
            "VELNOR_QUALIFIED_TOOL_IDENTITY".to_owned(),
            format!("qualified-tools@{}", qualified_toolset_digest(&records)),
        ),
    ]);
    merge_native_environment(catalog, host, domain, tools, &mut environment)?;
    Ok(environment)
}

pub(in crate::catalog) fn merge_native_environment(
    catalog: &ToolCatalog,
    host: DistributionHost,
    domain: ToolCacheDomain,
    tools: &[PinnedTool],
    environment: &mut BTreeMap<String, String>,
) -> Result<(), MiseError> {
    let mut tools = tools.to_vec();
    tools.sort_by_key(|tool| tool.tool_name());
    tools.dedup();
    let launches: Vec<_> = tools
        .into_iter()
        .map(|tool| catalog.native_launch_context(host, domain, tool))
        .collect::<Result<_, _>>()?;
    merge_launch_environment(&launches.iter().collect::<Vec<_>>(), environment)
}

fn merge_launch_environment(
    launches: &[&QualifiedNativeLaunch],
    environment: &mut BTreeMap<String, String>,
) -> Result<(), MiseError> {
    let mut paths = Vec::new();
    for launch in launches {
        for (key, value) in launch.environment()? {
            if key == "PATH" {
                if !paths.contains(&value) {
                    paths.push(value);
                }
            } else if environment
                .get(&key)
                .is_some_and(|existing| existing != &value)
            {
                return Err(invalid("conflicting measured native environment"));
            } else {
                environment.insert(key, value);
            }
        }
    }
    paths.push("/usr/bin:/bin:/usr/sbin:/sbin".to_owned());
    environment.insert("PATH".to_owned(), paths.join(":"));
    Ok(())
}

#[cfg(test)]
#[path = "catalog_native_tool_context_tests.rs"]
mod tests;
