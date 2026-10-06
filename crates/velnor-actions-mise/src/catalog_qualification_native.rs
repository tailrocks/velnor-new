//! Closed native distribution lookup and complete selected toolset identity.

use super::super::{PinnedTool, ToolCatalog};
use super::{DistributionHost, DistributionTool, NativeDistributionAudit, QualifiedDistribution};
use crate::MiseError;
use sha2::{Digest, Sha256};

impl QualifiedDistribution {
    /// Audit exact upstream native bytes without granting installed payload authority.
    /// # Errors
    /// Rejects missing artifact, launch or source qualification records.
    pub fn qualify_native(
        tool: DistributionTool,
        host: DistributionHost,
        version: &str,
    ) -> Result<NativeDistributionAudit, MiseError> {
        lookup_native(tool, host, version).map(NativeDistributionAudit::new)
    }

    /// Exact native distribution whose installation has been independently qualified.
    /// # Errors
    /// Fails when artifact, installation transforms, or launch paths remain unqualified.
    pub fn require_native(
        tool: DistributionTool,
        host: DistributionHost,
        version: &str,
    ) -> Result<Self, MiseError> {
        let distribution = lookup_native(tool, host, version)?;
        if distribution.tool != tool
            || distribution.host != host
            || distribution.selection_version != version
        {
            return Err(MiseError::Contract {
                problem: "native distribution identity mismatch".to_owned(),
            });
        }
        distribution.required_install_plan()?;
        distribution.required_installed_binary_path()?;
        Ok(distribution)
    }
}

fn lookup_native(
    tool: DistributionTool,
    host: DistributionHost,
    version: &str,
) -> Result<QualifiedDistribution, MiseError> {
    match tool {
        DistributionTool::ReleasePlz => super::release_plz_records::official(host, version),
        DistributionTool::CargoSemverChecks => super::semver_records::official(host, version),
        DistributionTool::Gh => super::gh_records::official(host, version),
        DistributionTool::OpenTofu => super::tofu_records::official(host, version),
        DistributionTool::Node => super::node_records::official(host, version),
        DistributionTool::Bun => super::bun_records::official(host, version),
        DistributionTool::Python => super::python_records::official(host, version),
        DistributionTool::Uv => super::uv_records::official(host, version),
        DistributionTool::Java => super::java_records::official(host, version),
        DistributionTool::Gradle => match version {
            super::gradle_records::VERSION => super::gradle_records::official(host, version),
            super::gradle_consumer_records::VERSION => {
                super::gradle_consumer_records::official(host, version)
            }
            super::gradle_bootstrap_records::VERSION => {
                super::gradle_bootstrap_records::official(host, version)
            }
            _ => Err(absent(tool, host, version)),
        },
        _ => Err(absent(tool, host, version)),
    }
}

impl ToolCatalog {
    /// Exact installed native distribution for one closed logical catalog slot.
    /// # Errors
    /// Rejects missing source, archive, launch or host installation qualification.
    pub fn native_distribution(
        &self,
        host: DistributionHost,
        tool: PinnedTool,
    ) -> Result<QualifiedDistribution, MiseError> {
        let distribution_tool = match tool {
            PinnedTool::Gh => DistributionTool::Gh,
            PinnedTool::ReleasePlz => DistributionTool::ReleasePlz,
            PinnedTool::Bun => DistributionTool::Bun,
            PinnedTool::Node => DistributionTool::Node,
            PinnedTool::Opentofu => DistributionTool::OpenTofu,
            PinnedTool::Python => DistributionTool::Python,
            PinnedTool::Uv => DistributionTool::Uv,
            PinnedTool::Java => DistributionTool::Java,
            PinnedTool::Gradle => DistributionTool::Gradle,
            PinnedTool::CargoSemverChecks => DistributionTool::CargoSemverChecks,
            _ => {
                return Err(MiseError::Contract {
                    problem: format!(
                        "selected tool distribution qualification absent: {}",
                        tool.tool_name()
                    ),
                });
            }
        };
        QualifiedDistribution::require_native(distribution_tool, host, self.version(tool))
    }

    /// Complete qualified distributions for every requested catalog tool.
    /// # Errors
    /// Rejects selected tools without exact artifact and installation qualification.
    pub fn qualified_distributions(
        &self,
        host: DistributionHost,
        tools: &[PinnedTool],
    ) -> Result<Vec<QualifiedDistribution>, MiseError> {
        let mut records = Vec::new();
        for tool in tools {
            let record = self.native_distribution(host, *tool)?;
            if !records
                .iter()
                .any(|existing: &QualifiedDistribution| existing.tool() == record.tool())
            {
                records.push(record);
            }
        }
        records.sort_by_key(QualifiedDistribution::selector);
        Ok(records)
    }
}

/// Canonical identity of the entire selected distribution set.
#[must_use]
pub fn qualified_toolset_digest(records: &[QualifiedDistribution]) -> String {
    let mut identities: Vec<_> = records
        .iter()
        .map(QualifiedDistribution::qualification_digest)
        .collect();
    identities.sort();
    identities.dedup();
    let mut digest = Sha256::new();
    digest.update(b"velnor-qualified-toolset-v1:");
    digest.update(identities.len().to_string().as_bytes());
    for identity in identities {
        digest.update(b":");
        digest.update(identity.as_bytes());
    }
    let digest = digest.finalize();
    encode_hex(&digest)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(*byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(*byte & 0x0f)]));
    }
    encoded
}

fn absent(tool: DistributionTool, host: DistributionHost, version: &str) -> MiseError {
    MiseError::Contract {
        problem: format!(
            "qualified native distribution absent: {tool:?} / {} / {version}",
            host.abi()
        ),
    }
}
