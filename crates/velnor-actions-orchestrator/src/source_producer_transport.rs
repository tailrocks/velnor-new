//! One literal Cargo source authority for reader and producer transport.

use velnor_actions_contract::{SourceProducer, SourceProducerRole, Step, StepId, StepKind};

use crate::OrchestratorError;

const CARGO_SOURCE_NAMESPACE: &str = "velnor-v4-cargo-source-public-";
const CARGO_HOME: &str = "${{ runner.temp }}/velnor/cargo";

/// Opaque immutable public Cargo source identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Source3 {
    identity: String,
}

impl Source3 {
    /// Admit only literal keys belonging to the public Cargo source namespace.
    pub(crate) fn new(identity: String) -> Result<Self, OrchestratorError> {
        if !identity.starts_with(CARGO_SOURCE_NAMESPACE)
            || identity.len() == CARGO_SOURCE_NAMESPACE.len()
            || identity.len() > velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES
            || !identity
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(reject("invalid_cargo_source_identity"));
        }
        Ok(Self { identity })
    }

    /// Exact literal cohort shared by every reader and its producer.
    pub(crate) fn identity(&self) -> &str {
        &self.identity
    }

    /// Restore only snapshots within this exact source cohort.
    pub(crate) fn restore(&self) -> Result<Step, OrchestratorError> {
        let key = format!(
            "{}-lookup-${{{{github.run_id}}}}-${{{{github.run_attempt}}}}",
            self.identity()
        );
        let mut step = cache_step(true, &key, &[format!("{}-snapshot-", self.identity())])?;
        step.id = Some(StepId::new("velnor-sources-cache").map_err(contract_error)?);
        Ok(step)
    }

    /// Publish the canonical source subset with the producer's verified gate.
    pub(crate) fn save(&self, producer: &SourceProducer) -> Result<Step, OrchestratorError> {
        self.validate_producer(producer)?;
        let mut step = cache_step(false, &producer.save_key(), &[])?;
        step.id = Some(producer.save_step.clone());
        step.condition = Some(producer.save_condition());
        Ok(step)
    }

    /// Prove exact attempted publication without downloading cached content.
    pub(crate) fn publication(&self, producer: &SourceProducer) -> Result<Step, OrchestratorError> {
        self.validate_producer(producer)?;
        let mut step = cache_step(true, &producer.save_key(), &[])?;
        step.name = "Verify Cargo source publication".to_owned();
        step.id = Some(producer.publication_step.clone());
        step.condition = Some(producer.publication_condition());
        let StepKind::Action { with, .. } = &mut step.kind else {
            return Err(reject("invalid_cargo_source_transport"));
        };
        with.insert("lookup-only".to_owned(), "true".to_owned());
        with.remove("restore-keys");
        Ok(step)
    }

    fn validate_producer(&self, producer: &SourceProducer) -> Result<(), OrchestratorError> {
        producer.validate().map_err(contract_error)?;
        if producer.role != SourceProducerRole::Cargo || producer.source_identity != self.identity {
            return Err(reject("foreign_cargo_source_producer"));
        }
        Ok(())
    }
}

fn cache_step(restore: bool, key: &str, prefixes: &[String]) -> Result<Step, OrchestratorError> {
    use velnor_actions_actionlint::{
        PinnedActionRef,
        actions::{CACHE_ACTION_SHA, CACHE_ACTION_VERSION},
    };
    use velnor_actions_mise::cache_sources::{sources_cache_paths, validate_sources_subset};
    let wrap = |error: velnor_actions_mise::MiseError| OrchestratorError::Contract {
        problem: error.to_string(),
    };
    let paths = sources_cache_paths(CARGO_HOME).map_err(wrap)?;
    validate_sources_subset(&paths, CARGO_HOME).map_err(wrap)?;
    let uses = PinnedActionRef::new(
        "actions/cache",
        Some(if restore { "restore" } else { "save" }),
        CACHE_ACTION_SHA,
        CACHE_ACTION_VERSION,
    )
    .map_err(OrchestratorError::from)?
    .uses_value();
    let mut step = velnor_actions_workflow_renderer::steps::cache_action_step(
        restore, &uses, "sources", key, prefixes, &paths,
    )
    .map_err(OrchestratorError::from)?;
    step.name = if restore {
        "Restore Cargo sources"
    } else {
        "Save Cargo sources"
    }
    .to_owned();
    Ok(step)
}

fn contract_error(error: velnor_actions_contract::ContractError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}

fn reject(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn producer(source: &Source3) -> SourceProducer {
        let step = |id| StepId::new(id).expect("step id");
        SourceProducer {
            role: SourceProducerRole::Cargo,
            selection: velnor_actions_contract::ToolProducerSelection {
                unconditional: true,
                ..Default::default()
            },
            tool_cache: None,
            source_identity: source.identity().to_owned(),
            verification_step: step("velnor-rust-source-verify"),
            restore_step: step("velnor-sources-cache"),
            save_step: step("velnor-rust-source-save"),
            publication_step: step("velnor-rust-source-publication"),
            report_step: step("velnor-rust-source-report"),
        }
    }

    #[test]
    fn publication_proves_exact_save_key_with_verified_metadata() {
        let source = Source3::new(format!("{CARGO_SOURCE_NAMESPACE}abc123")).expect("identity");
        let producer = producer(&source);
        let save = source.save(&producer).expect("save");
        let lookup = source.publication(&producer).expect("lookup");
        assert_eq!(save.id, Some(producer.save_step.clone()));
        assert_eq!(save.condition, Some(producer.save_condition()));
        assert_eq!(lookup.id, Some(producer.publication_step.clone()));
        assert_eq!(lookup.condition, Some(producer.publication_condition()));
        let StepKind::Action { with: saved, .. } = save.kind else {
            panic!("save action");
        };
        let StepKind::Action { with: looked, .. } = lookup.kind else {
            panic!("lookup action");
        };
        assert_eq!(saved["key"], producer.save_key());
        assert_eq!(saved["key"], looked["key"]);
        assert_eq!(saved["path"], looked["path"]);
        assert_eq!(looked["lookup-only"], "true");
        assert!(!looked.contains_key("restore-keys"));
    }

    #[test]
    fn producer_cannot_rebind_identity_or_native_role() {
        let source = Source3::new(format!("{CARGO_SOURCE_NAMESPACE}abc123")).expect("identity");
        let mut producer = producer(&source);
        producer.source_identity.push_str("-foreign");
        assert!(source.save(&producer).is_err());
        producer.source_identity = source.identity().to_owned();
        producer.role = SourceProducerRole::Npm;
        assert!(source.publication(&producer).is_err());
    }

    #[test]
    fn literal_identity_excludes_legacy_and_expression_keys() {
        for identity in [
            "velnor-v3-sources-x86_64-unknown-linux-gnu",
            "velnor-v4-cargo-source-public-",
            "velnor-v4-cargo-source-public-${{ github.sha }}",
            "velnor-v4-cargo-source-public-a/b",
        ] {
            assert!(Source3::new(identity.to_owned()).is_err(), "{identity}");
        }
        assert!(Source3::new(format!("{CARGO_SOURCE_NAMESPACE}abc_123.def")).is_ok());
    }

    #[test]
    fn reader_and_writer_share_the_owned_credential_free_subset() {
        let source = Source3::new(format!("{CARGO_SOURCE_NAMESPACE}abc123")).expect("identity");
        let restore = source.restore().expect("restore");
        let save = source.save(&producer(&source)).expect("save");
        let inputs = |step: Step| match step.kind {
            StepKind::Action { with, .. } => with,
            _ => panic!("cache transport must be an action"),
        };
        let reader = inputs(restore);
        let writer = inputs(save);
        assert_eq!(reader["path"], writer["path"]);
        let paths = velnor_actions_mise::cache_sources::sources_cache_paths(CARGO_HOME)
            .expect("canonical subset");
        assert_eq!(reader["path"], paths.join("\n"));
        assert_eq!(
            reader["restore-keys"],
            format!("{}-snapshot-", source.identity())
        );
        assert!(!reader["restore-keys"].contains("velnor-v3"));
    }
}
