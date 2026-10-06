//! Closed MBX data admission source owner. Draft evidence never grants publication.
use crate::OrchestratorError;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, PureMbxProducer, SourceBoundHelper,
    SourceBoundOperation,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::catalog::mbx_action_authority::QualifiedMbxAction;
use velnor_actions_mise::catalog::qualification::{
    DistributionRequirement, DistributionTool, QualifiedDistribution,
};
use velnor_actions_workflow_renderer::MiseSetup;

/// Captured source-owner state; callers cannot construct an authenticated origin.
/// Qualification must establish acquisition, live artifact metadata and retained
/// origin receipts together before adding an admitted variant.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MbxCapturedSourceInputs {
    UnsupportedPublication {
        owner_unavailable: Option<String>,
        action_unavailable: Option<String>,
        action: Option<QualifiedMbxAction>,
    },
}

/// Complete private source closure, distinct from executable/tool receipt grants.
#[derive(Debug, Clone)]
pub(crate) struct MbxProducerSources {
    metadata: PureMbxProducer,
    inputs: MbxCapturedSourceInputs,
    version: String,
    records: [CompiledSourceHelper; 4],
}

impl MbxProducerSources {
    pub(crate) fn source_helpers(&self) -> &[CompiledSourceHelper; 4] {
        &self.records
    }

    /// Freshly rebuild source, invocation, environment and metadata from owner input.
    pub(crate) fn validate(
        &self,
        metadata: &PureMbxProducer,
        catalog: &ToolCatalog,
        setup: &MiseSetup,
        version: &str,
    ) -> Result<(), OrchestratorError> {
        let fresh = compiled_mbx_sources(metadata, catalog, setup, version)?;
        if self.metadata != fresh.metadata
            || self.inputs != fresh.inputs
            || self.version != fresh.version
            || self.records != fresh.records
        {
            return Err(invalid("source_owner_changed"));
        }
        Ok(())
    }
}

/// Compile an honest review draft from the fixed source owner.
/// Catalog setup grants no artifact authority. No immutable MBX publication has
/// completed source/binary, same-run service metadata and historical qualification.
pub(crate) fn compiled_mbx_sources(
    metadata: &PureMbxProducer,
    _catalog: &ToolCatalog,
    _setup: &MiseSetup,
    version: &str,
) -> Result<MbxProducerSources, OrchestratorError> {
    metadata.validate()?;
    if version != env!("CARGO_PKG_VERSION") {
        return Err(invalid("generator_version"));
    }
    let inputs = capture_inputs(metadata)?;
    let records = reconstruct(metadata, &inputs, version)?;
    Ok(MbxProducerSources {
        metadata: metadata.clone(),
        inputs,
        version: version.to_owned(),
        records,
    })
}

fn capture_inputs(
    metadata: &PureMbxProducer,
) -> Result<MbxCapturedSourceInputs, OrchestratorError> {
    let host = crate::workloads::host_for_runner(&metadata.descriptor.runs_on)?;
    let owner = QualifiedDistribution::require_for_generator(
        DistributionTool::Mbx,
        host,
        DistributionRequirement::MbxTransport,
    );
    let owner_unavailable = match owner {
        Err(error) => Some(error.to_string()),
        Ok(owner) => {
            let identity = &metadata.descriptor.owner;
            if identity.version != owner.version()
                || identity.binary_sha256 != owner.binary_sha256()
                || identity.source_sha != owner.source_commit()
                || identity.qualification_identity != owner.qualification_digest()
            {
                return Err(invalid("owner_identity_changed"));
            }
            owner.required_install_plan()?;
            None
        }
    };
    let (action, action_unavailable) = match QualifiedMbxAction::require_comparison_export() {
        Err(error) => (None, Some(error.to_string())),
        Ok(action) => {
            if metadata.descriptor.action_sha != action.source_commit() {
                return Err(invalid("action_identity_changed"));
            }
            (Some(action), None)
        }
    };
    // Native acquisition alone cannot establish actual service-authenticated
    // same-run metadata, historical receipts and immutable published source records.
    Ok(MbxCapturedSourceInputs::UnsupportedPublication {
        owner_unavailable,
        action_unavailable,
        action,
    })
}

fn reconstruct(
    metadata: &PureMbxProducer,
    inputs: &MbxCapturedSourceInputs,
    version: &str,
) -> Result<[CompiledSourceHelper; 4], OrchestratorError> {
    let MbxCapturedSourceInputs::UnsupportedPublication {
        owner_unavailable,
        action_unavailable,
        action,
    } = inputs;
    let identity = metadata.descriptor.identity()?;
    let captured = velnor_actions_contract::canonical_json_str(&(
        &identity,
        owner_unavailable,
        action_unavailable,
        action.map(QualifiedMbxAction::qualification_digest),
        "artifact-origin-publication-unqualified",
    ))?;
    Ok([
        record(
            SourceBoundOperation::MbxProducerPrepare,
            PREPARE,
            &captured,
            version,
        )?,
        record(
            SourceBoundOperation::MbxArtifactAdmission,
            ADMISSION,
            &captured,
            version,
        )?,
        record(
            SourceBoundOperation::MbxBundleVerify,
            VERIFY,
            &captured,
            version,
        )?,
        record(
            SourceBoundOperation::MbxProducerReport,
            REPORT,
            &captured,
            version,
        )?,
    ])
}

fn record(
    operation: SourceBoundOperation,
    body: &str,
    identity: &str,
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let source = velnor_actions_contract::generated_source(version, body)?;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let helper = SourceBoundHelper::compiled(operation, operation.path(), &digest)?;
    let invocation = HelperInvocation::compiled(helper, vec![identity.to_owned()], Vec::new())?;
    Ok(CompiledSourceHelper::compiled(invocation, source)?)
}

fn invalid(reason: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("mbx_admission_source_{reason}"),
    }
}

// Explicit draft outputs. These do not download, execute or interpret transported
// data, mint provenance, request credentials, or create a bundle/cache payload.
const PREPARE: &str = "set -euo pipefail\nprintf 'prepared=false\\nerror=mbx_publication_unqualified\\n' >> \"$GITHUB_OUTPUT\"\n";
const ADMISSION: &str = "set -euo pipefail\nprintf 'admitted=false\\nuseful=false\\nerror=mbx_artifact_origin_unqualified\\n' >> \"$GITHUB_OUTPUT\"\n";
const VERIFY: &str = "set -euo pipefail\nprintf 'verified=false\\nerror=mbx_native_owner_unqualified\\n' >> \"$GITHUB_OUTPUT\"\n";
const REPORT: &str = "set -euo pipefail\nprintf 'cache_available=false\\nverified=false\\nsourceidentity=\\nerror=mbx_publication_unqualified\\n' >> \"$GITHUB_OUTPUT\"\n";

#[cfg(test)]
#[path = "mbx_admission_source_tests.rs"]
mod tests;
