//! Source-only acquisition record for the independent fresh compiler purpose.

use crate::source_intent_cold_root::{SourceIntentColdLeaf, SourceIntentColdRoot};

use super::{
    AcquisitionOutputPurpose, BTreeMap, DistributionHost, DistributionRequirement,
    DistributionTool, MiseError, ProvisioningMode, QualifiedDistribution, configuration, host_name,
    invalid, source_with_roots,
};

/// Immutable acquisition stage; no cache domain or standalone helper authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIntentMiseAcquisition {
    source: String,
    args: Vec<String>,
    environment: BTreeMap<String, String>,
    distribution: QualifiedDistribution,
}

impl SourceIntentMiseAcquisition {
    /// Complete owner-generated executable source.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Exact literal arguments; never interpreted as shell code.
    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Exact qualified record, including archive and source provenance.
    #[must_use]
    pub const fn distribution(&self) -> &QualifiedDistribution {
        &self.distribution
    }

    /// Fixed owner environment, with no workflow cache identity.
    #[must_use]
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
}

/// Acquire the sole `SourceIntent` host from an owned `NoMiserc` distribution.
/// # Errors
/// Rejects absent publication or incompatible artifact provisioning.
pub fn source_intent_acquisition(
    root: SourceIntentColdRoot,
    version: &str,
) -> Result<SourceIntentMiseAcquisition, MiseError> {
    let distribution = QualifiedDistribution::require_for_generator(
        DistributionTool::Mise,
        DistributionHost::LinuxAmd64,
        DistributionRequirement::RequiresNoMiserc,
    )?;
    compiled(root, version, distribution)
}

fn compiled(
    root: SourceIntentColdRoot,
    version: &str,
    distribution: QualifiedDistribution,
) -> Result<SourceIntentMiseAcquisition, MiseError> {
    if distribution.tool() != DistributionTool::Mise
        || distribution.host() != DistributionHost::LinuxAmd64
        || distribution.provisioning_mode() != ProvisioningMode::MiseNoMisercExclusiveConfig
    {
        return Err(invalid());
    }
    let relative = format!(
        "{}/{}",
        root.relative_to_runner_temp(),
        SourceIntentColdLeaf::Mise.relative()
    );
    let source = source_with_roots(
        version,
        &BTreeMap::from([("source-intent", relative)]),
        AcquisitionOutputPurpose::SourceIntent,
    )?;
    let (key, value) = root.namespace_environment();
    let environment = BTreeMap::from([
        (key.to_owned(), value.to_owned()),
        (
            "MISE_DATA_DIR".to_owned(),
            root.leaf_expression(SourceIntentColdLeaf::Mise),
        ),
    ]);
    let args = vec![
        "source-intent".to_owned(),
        host_name(distribution.host()).to_owned(),
        configuration(&distribution)?.to_string(),
    ];
    Ok(SourceIntentMiseAcquisition {
        source,
        args,
        environment,
        distribution,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_intent_namespace_is_independent_and_requires_owned_publication()
    -> Result<(), MiseError> {
        let root = SourceIntentColdRoot::root_linux();
        assert!(source_intent_acquisition(root, "0.1.0").is_err());
        let fixture = QualifiedDistribution::owned_test_fixture(DistributionHost::LinuxAmd64)?;
        let record = compiled(root, "0.1.0", fixture)?;
        assert!(
            record
                .source()
                .contains("velnor-control/source-intent/mise")
        );
        assert!(!record.source().contains("velnor/mise"));
        assert!(!record.environment().keys().any(|key| key.contains("CACHE")));
        assert_eq!(record.args()[0], "source-intent");
        Ok(())
    }
}
