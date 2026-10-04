//! Immutable review evidence for detached MBX source computations.

use velnor_actions_contract::canonical::{canonical_json_bytes, digest_b3};
use velnor_actions_workflow_renderer::RenderedFile;

use super::MbxFinalization;
use crate::OrchestratorError;

impl MbxFinalization {
    /// Preserve complete proposed source bytes without creating an executable workflow.
    pub(crate) fn review_files(
        &self,
        version: &str,
    ) -> Result<Vec<RenderedFile>, OrchestratorError> {
        self.domains.iter().map(|draft| {
            let cold_reasons: Vec<_> = self.unsupported.iter()
                .filter(|cold| cold.domain == draft.descriptor.domain && cold.job_id == draft.job_id)
                .map(|cold| cold.reason.as_str())
                .collect();
            let source = serde_json::json!({
                "schema": 1,
                "generator_version": version,
                "status": "proposed_source",
                "runtime_admission": "unqualified",
                "restore_access": "read",
                "producer_access": "write",
                "cold_reasons": cold_reasons,
                "job_id": draft.job_id,
                "descriptor": draft.descriptor,
                "tool_context": draft.tool_context,
                "restore": draft.restore,
                "export": draft.export,
                "upload": draft.upload,
                "producer": draft.producer,
            });
            let identity = digest_b3(&canonical_json_bytes(&source)?);
            let json = serde_json::to_string_pretty(&source).map_err(|error| {
                OrchestratorError::Contract { problem: error.to_string() }
            })?;
            let body = format!(
                "# Proposed MBX source draft\n\nRuntime admission: unqualified.\n\n```json\n{json}\n```\n"
            );
            Ok(RenderedFile {
                path: format!(".github/velnor/mbx-source-draft-{identity}.md"),
                bytes: velnor_actions_contract::generated_source(version, &body)?,
            })
        }).collect()
    }
}
