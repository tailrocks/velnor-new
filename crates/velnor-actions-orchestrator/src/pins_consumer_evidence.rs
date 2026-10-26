// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::ReleaseTarget;
use velnor_actions_workflow_renderer::{EvidenceHelperProvenance, release_helpers};

use crate::{OrchestratorError, discover::Discovery};

pub(crate) fn consumer_evidence_helper(
    label: &str,
    version: &str,
    discovery: &Discovery,
) -> Result<EvidenceHelperProvenance, OrchestratorError> {
    let target =
        ReleaseTarget::for_runner_label(label).ok_or_else(|| OrchestratorError::Contract {
            problem: format!("unsupported_target_for_runner:{label}"),
        })?;
    let manifest = release_helpers::validate_consumer_manifest_for_version(
        target,
        version,
        discovery.consumer_manifest_json.as_deref(),
    )
    .map_err(crate::local_release_manifest::map_release_helper_error)?;
    EvidenceHelperProvenance::from_release_manifest(&manifest, label, version).map_err(Into::into)
}
