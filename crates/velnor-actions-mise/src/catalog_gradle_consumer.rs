//! Closed consumer engine, wrapper bootstrap and Community Java installation.

use std::collections::BTreeMap;

use velnor_actions_contract::ToolCacheDomain;

use super::qualification::{
    DistributionHost, DistributionTool, JAVA_SELECTION_VERSION, QualifiedDistribution,
    qualified_toolset_digest,
};
use crate::MiseError;

#[path = "catalog_gradle_consumer_prepare.rs"]
mod prepare;
pub use prepare::{helper_for_host, record_for_invocation};

#[path = "catalog_gradle_consumer_recipe.rs"]
mod recipe;

/// Distinct installation roles; managed Gradle has no consumer authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradleConsumerRole {
    /// Exact Community Java runtime.
    Java,
    /// Engine selected by the consumer wrapper.
    Engine,
    /// Release which generated the consumer wrapper.
    Bootstrap,
}

/// Compiled installation and launch paths for one exact selected role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GradleConsumerLaunch {
    distribution: QualifiedDistribution,
    install_relative_root: &'static str,
    executable_relative_path: &'static str,
}

impl GradleConsumerLaunch {
    fn require(
        tool: DistributionTool,
        host: DistributionHost,
        version: &str,
    ) -> Result<Self, MiseError> {
        let distribution = QualifiedDistribution::require_native(tool, host, version)?;
        let install_relative_root = distribution.required_install_plan()?.root_relative_path();
        let executable_relative_path = distribution.required_installed_binary_path()?;
        if !executable_relative_path
            .strip_prefix(install_relative_root)
            .is_some_and(|suffix| suffix.starts_with('/'))
            || !distribution.launch_entries().iter().any(|entry| {
                entry.installed_relative_path() == Some(executable_relative_path)
                    && entry.sha256() == distribution.binary_sha256()
            })
        {
            return Err(invalid());
        }
        Ok(Self {
            distribution,
            install_relative_root,
            executable_relative_path,
        })
    }

    /// Complete selected distribution identity and launch closure.
    #[must_use]
    pub const fn distribution(&self) -> &QualifiedDistribution {
        &self.distribution
    }

    /// Qualified installation root under the fixed Gradle bootstrap owner.
    #[must_use]
    pub fn install_root(&self) -> String {
        format!(
            "{}/{}",
            ToolCacheDomain::GradleBootstrap.root(),
            self.install_relative_root
        )
    }

    /// Qualified executable under the fixed Gradle bootstrap owner.
    #[must_use]
    pub fn executable(&self) -> String {
        format!(
            "{}/{}",
            ToolCacheDomain::GradleBootstrap.root(),
            self.executable_relative_path
        )
    }
}

/// Sealed exact consumer profile; no URL, version, domain or path overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GradleConsumerContext {
    java: GradleConsumerLaunch,
    engine: GradleConsumerLaunch,
    bootstrap: GradleConsumerLaunch,
    java_home: String,
}

impl GradleConsumerContext {
    /// Select the sole qualified consumer profile for one explicit host.
    /// # Errors
    /// Rejects hosts lacking measured installation or launch records.
    pub fn require(host: DistributionHost) -> Result<Self, MiseError> {
        let java =
            GradleConsumerLaunch::require(DistributionTool::Java, host, JAVA_SELECTION_VERSION)?;
        let engine = GradleConsumerLaunch::require(
            DistributionTool::Gradle,
            host,
            super::gradle::GRADLE_WRAPPER_VERSION,
        )?;
        let bootstrap = GradleConsumerLaunch::require(
            DistributionTool::Gradle,
            host,
            super::gradle::BOOTSTRAP_VERSION,
        )?;
        let home = java
            .distribution
            .required_install_plan()?
            .environment()
            .iter()
            .find(|entry| entry.name() == "JAVA_HOME")
            .ok_or_else(invalid)?;
        let java_home = if home.relative_path().is_empty() {
            java.install_root()
        } else {
            format!("{}/{}", java.install_root(), home.relative_path())
        };
        if java.executable() != format!("{java_home}/bin/java") {
            return Err(invalid());
        }
        Ok(Self {
            java,
            engine,
            bootstrap,
            java_home,
        })
    }

    /// Explicit measured host.
    #[must_use]
    pub const fn host(&self) -> DistributionHost {
        self.java.distribution.host()
    }

    /// Sole cache owner admitted by this profile.
    #[must_use]
    pub const fn domain(&self) -> ToolCacheDomain {
        ToolCacheDomain::GradleBootstrap
    }

    /// Fixed owned installation root.
    #[must_use]
    pub const fn owned_root(&self) -> &'static str {
        self.domain().root()
    }

    /// Exact role without managed-tool substitution.
    #[must_use]
    pub const fn launch(&self, role: GradleConsumerRole) -> &GradleConsumerLaunch {
        match role {
            GradleConsumerRole::Java => &self.java,
            GradleConsumerRole::Engine => &self.engine,
            GradleConsumerRole::Bootstrap => &self.bootstrap,
        }
    }

    /// Qualified Community Java home.
    #[must_use]
    pub fn java_home(&self) -> &str {
        &self.java_home
    }

    /// Full distribution identity, host, cache domain and launch paths.
    #[must_use]
    pub fn identity(&self) -> String {
        let records = self.records();
        let bytes = format!(
            "velnor-gradle-consumer-v1\0{}\0{}\0{}\0{}\0{}\0{}",
            self.host().abi(),
            self.domain().name(),
            self.owned_root(),
            qualified_toolset_digest(&records),
            self.java_home,
            [&self.java, &self.engine, &self.bootstrap]
                .iter()
                .map(|launch| format!("{}\0{}", launch.install_root(), launch.executable()))
                .collect::<Vec<_>>()
                .join("\0")
        );
        format!(
            "gradle-consumer@{}",
            velnor_actions_contract::digest_b3(bytes.as_bytes())
        )
    }

    /// Exact sorted selectors for all three installation roles.
    #[must_use]
    pub fn selectors(&self) -> Vec<String> {
        let mut selectors: Vec<_> = self
            .records()
            .iter()
            .map(|record| record.selector().to_owned())
            .collect();
        selectors.sort();
        selectors
    }

    /// Immutable selected Java, engine and bootstrap records, in role order.
    #[must_use]
    pub const fn distributions(&self) -> [&QualifiedDistribution; 3] {
        [
            &self.java.distribution,
            &self.engine.distribution,
            &self.bootstrap.distribution,
        ]
    }

    /// Fixed Java launch environment for both Gradle roles.
    #[must_use]
    pub fn environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("MISE_DATA_DIR".to_owned(), self.owned_root().to_owned()),
            ("JAVA_HOME".to_owned(), self.java_home.clone()),
            (
                "VELNOR_GRADLE_CONSUMER_IDENTITY".to_owned(),
                self.identity(),
            ),
        ])
    }

    pub(super) fn records(&self) -> Vec<QualifiedDistribution> {
        let mut records = vec![
            self.java.distribution.clone(),
            self.engine.distribution.clone(),
            self.bootstrap.distribution.clone(),
        ];
        records.sort_by_key(QualifiedDistribution::selector);
        records
    }
}

pub(super) fn invalid() -> MiseError {
    MiseError::Contract {
        problem: "invalid_gradle_consumer_profile".to_owned(),
    }
}

#[cfg(test)]
#[path = "catalog_gradle_consumer_tests.rs"]
mod tests;
