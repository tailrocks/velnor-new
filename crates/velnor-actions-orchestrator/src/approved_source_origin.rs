//! Private SOURCE producer. Publication never grants runtime or SDK authority.

use super::{invalid, source};
use crate::OrchestratorError;

const RECORDS: &str = include_str!("../../../scripts/source_publication_records.py");
const BRIDGE: &str = include_str!("approved_source_origin.py");
const SEMVER: &str = include_str!("../../../scripts/source_semver_capsule.py");
const CAPSULE: &str = include_str!("../../../scripts/source_proof_capsule.py");

/// Closed producer, retaining the canonical publication owner's capability.
/// Its Python closure must be included by the source owner with the existing
/// source codec/materializer. A JSON descriptor cannot construct this value.
pub(super) struct ApprovedSourceProducer {
    foundation: source::PublishedFoundationSource,
}

impl ApprovedSourceProducer {
    /// Private compiled inclusion; this does not execute or admit a snapshot.
    pub(super) fn python_source(&self) -> Result<String, OrchestratorError> {
        let reference = serde_json::to_string(self.foundation.action_reference())
            .map_err(|_| invalid("approved_source_reference_encoding"))?;
        // The canonical reviewed records are compiled bytes, never imported
        // from a caller-controlled Python path. Separate globals prevent their
        // helper names from replacing the source engine's validation functions.
        let records = serde_json::to_string(RECORDS)
            .map_err(|_| invalid("approved_source_records_encoding"))?;
        let semver = serde_json::to_string(SEMVER)
            .map_err(|_| invalid("approved_source_capsule_encoding"))?;
        let capsule = CAPSULE.replace(
            "from source_semver_capsule import archive_proof, require, sha, git_object",
            "",
        );
        let capsule = serde_json::to_string(&capsule)
            .map_err(|_| invalid("approved_source_capsule_encoding"))?;
        Ok(format!(
            "_approved_records = {{'__name__': '_velnor_compiled_reviewed_sources'}}\nexec(compile({records}, '<velnor-reviewed-source-owner>', 'exec'), _approved_records)\n\
             _approved_capsule = {{'__name__': '_velnor_compiled_source_capsule'}}\nexec(compile({semver}, '<velnor-semver-capsule>', 'exec'), _approved_capsule)\nexec(compile({capsule}, '<velnor-proof-capsule>', 'exec'), _approved_capsule)\n\
             def _compiled_approved_source_projection():\n    return ('velnor-approved-source-owner-v1', 'source-only', {reference}, _approved_records['reviewed_source_revision'], _approved_capsule['validate_semver_receipt'])\n\
             {BRIDGE}"
        ))
    }
}

/// Genuine closed SOURCE owner. Independent of SDK/native qualification.
pub(super) fn approved_source_producer() -> Result<ApprovedSourceProducer, OrchestratorError> {
    Ok(ApprovedSourceProducer {
        foundation: source::published_foundation_source()?,
    })
}

/// No compiled producer/transport proof currently grants a runtime snapshot.
/// Keeping this separate prevents SOURCE publication activating runtime trust.
pub(super) fn runtime_approved_source_producer() -> Option<ApprovedSourceProducer> {
    None
}
