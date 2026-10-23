//! Immutable release provenance for consumer-side helper acquisition.

use velnor_actions_contract::{
    RELEASE_MANIFEST_FILENAME, ReleaseManifest, ReleaseTarget, check_release_artifact,
};
use velnor_actions_workflow_renderer::EvidenceHelperProvenance;

use crate::{OrchestratorError, discover::Discovery};

/// Validated release provenance for the fixed evidence helper steps.
pub(crate) fn consumer_evidence_helper(
    label: &str,
    version: &str,
    discovery: &Discovery,
) -> Result<EvidenceHelperProvenance, OrchestratorError> {
    let target =
        ReleaseTarget::for_runner_label(label).ok_or_else(|| OrchestratorError::Contract {
            problem: format!("unsupported_target_for_runner:{label}"),
        })?;
    let manifest =
        validated_consumer_manifest(target, version, discovery.consumer_manifest_json.as_deref())?;
    EvidenceHelperProvenance::from_release_manifest(&manifest, label, version).map_err(Into::into)
}

pub(super) fn validated_consumer_manifest(
    target: ReleaseTarget,
    version: &str,
    json: Option<&str>,
) -> Result<ReleaseManifest, OrchestratorError> {
    let Some(json) = json else {
        return Err(OrchestratorError::Contract {
            problem: "consumer_requires_release_install:install an official velnor-actions release"
                .to_owned(),
        });
    };
    let manifest = ReleaseManifest::parse_json(json, RELEASE_MANIFEST_FILENAME)?;
    manifest.validate(RELEASE_MANIFEST_FILENAME)?;
    if manifest.version != version {
        return Err(OrchestratorError::Contract {
            problem: format!(
                "release_manifest_version_mismatch:{}:{version}",
                manifest.version
            ),
        });
    }
    crate::local_release_manifest::verify_current_exe_matches_consumer_manifest(&manifest)?;
    let record =
        manifest
            .record_for_target(target.triple())
            .ok_or_else(|| OrchestratorError::Contract {
                problem: format!("manifest_missing_target:{}", target.triple()),
            })?;
    // Defense in depth: re-bind the consumed record's URL even though
    // `validate` already bound every record (X1).
    check_release_artifact(
        &record.artifact,
        &manifest.version,
        &manifest.commit,
        target.triple(),
        RELEASE_MANIFEST_FILENAME,
        "targets.artifact",
    )?;
    Ok(manifest)
}
