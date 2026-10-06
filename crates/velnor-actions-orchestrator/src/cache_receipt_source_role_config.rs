//! Closed producer role metadata; this descriptive projection grants no source authority.
use super::{OrchestratorError, invalid};
use serde_json::Value;
use velnor_actions_contract::{Job, SourceProducerRole, StepId};

pub(super) struct ProducerMetadata<'a> {
    pub(super) role: String,
    pub(super) descriptor: Value,
    pub(super) catalog: String,
    pub(super) save: &'a StepId,
}

pub(super) fn metadata(original: &Job) -> Result<ProducerMetadata<'_>, OrchestratorError> {
    if let Some(meta) = &original.tool_producer {
        return Ok(ProducerMetadata {
            role: format!("tool-{}", meta.descriptor.domain.name()),
            descriptor: serde_json::to_value(meta).map_err(|error| invalid(&error.to_string()))?,
            catalog: meta.descriptor.qualification_identity.clone(),
            save: &meta.save_step,
        });
    }
    if let Some(meta) = &original.source_producer {
        let role = match meta.role {
            SourceProducerRole::Cargo => "cargo",
            SourceProducerRole::Npm => "npm",
            SourceProducerRole::Bun => "bun",
            SourceProducerRole::Gradle => "gradle",
            SourceProducerRole::Tofu => "tofu",
        };
        return Ok(ProducerMetadata {
            role: role.into(),
            descriptor: serde_json::to_value(meta).map_err(|error| invalid(&error.to_string()))?,
            catalog: meta
                .tool_cache
                .as_ref()
                .ok_or_else(|| invalid("missing_catalog"))?
                .qualification_identity
                .clone(),
            save: &meta.save_step,
        });
    }
    if let Some(meta) = &original.mbx_producer {
        return Ok(ProducerMetadata {
            role: format!("mbx-{}", meta.descriptor.domain.name()),
            descriptor: serde_json::to_value(meta).map_err(|error| invalid(&error.to_string()))?,
            catalog: meta.descriptor.owner.qualification_identity.clone(),
            save: &meta.save_step,
        });
    }
    Err(invalid("missing_admitted_role"))
}
