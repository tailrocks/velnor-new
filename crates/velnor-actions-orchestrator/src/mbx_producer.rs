//! Fixed native MBX writer drafts reconstructed from captured compiled owners.
use crate::{
    OrchestratorError,
    mbx_admission_source::{MbxProducerSources, compiled_mbx_sources},
};
use velnor_actions_contract::{
    CacheMode, CompiledSourceHelper, Job, JobTimeout, MbxExportDescriptor, PermissionLevel,
    Permissions, PureMbxProducer, StepId, ToolProducerSelection,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::cache_producer_workflow::CacheProducerRecipe;
use velnor_actions_workflow_renderer::{MiseSetup, cache_mbx_roles};

/// Complete reviewable computation; grants no hosted publication or warm admission.
/// Source records come only from the independent fixed native source factory.
#[derive(Debug, Clone)]
pub struct DraftMbxProducer {
    job: Job,
    sources: MbxProducerSources,
    catalog: ToolCatalog,
    setup: MiseSetup,
    version: String,
}

impl DraftMbxProducer {
    /// Exact downstream writer computation, including its closed cache transport.
    #[must_use]
    pub fn original(&self) -> &Job {
        &self.job
    }

    /// Entire independent owner source closure in execution order.
    #[must_use]
    pub fn source_helpers(&self) -> &[CompiledSourceHelper] {
        self.sources.source_helpers()
    }

    /// Captured generator marker; publication never substitutes a different version.
    #[must_use]
    pub fn generator_version(&self) -> &str {
        &self.version
    }

    /// Render an independently reconstructed source publication candidate.
    /// This returns reviewable source only, never an activated writer or warm grant.
    /// # Errors
    /// Rejects changed owner records and structural recipes before source creation.
    pub fn receipt_candidate(
        &self,
    ) -> Result<crate::cache_producer_workflow::DraftCacheProducerWorkflow, OrchestratorError> {
        self.validate()?;
        let context = velnor_actions_workflow_renderer::WorkflowDocumentContext {
            generator_version: self.version.clone(),
            source_helpers: self.source_helpers().to_vec(),
            native_pages_approvals: Vec::new(),
            native_publish_approvals: Vec::new(),
            action_credential_approvals: Vec::new(),
        };
        let recipe =
            velnor_actions_workflow_renderer::cache_producer_workflow::admit_cache_producer_recipe(
                self.original(),
                &self.setup,
                &context,
            )?;
        let sources =
            crate::cache_receipt_source::draft_cache_receipt_sources_with_mbx(&recipe, self)?;
        crate::cache_producer_workflow::render_cache_producer_workflow_draft(&recipe, &sources)
    }

    /// Reconstruct the complete source factory and job before recipe admission.
    /// # Errors
    /// Rejects any changed source, environment, cohort, step or scheduling boundary.
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        let metadata = self
            .job
            .mbx_producer
            .as_ref()
            .ok_or_else(|| invalid("missing_role"))?;
        self.sources
            .validate(metadata, &self.catalog, &self.setup, &self.version)?;
        let fresh_sources =
            compiled_mbx_sources(metadata, &self.catalog, &self.setup, &self.version)?;
        if fresh_sources.source_helpers() != self.sources.source_helpers()
            || writer_job(metadata, &fresh_sources)? != self.job
        {
            return Err(invalid("fixed_factory_changed"));
        }
        Ok(())
    }

    /// Independently bind a structural draft to its complete original source owner.
    /// This check grants no immutable publication or authenticated artifact authority.
    /// # Errors
    /// Rejects any substituted original job, source closure or generator marker.
    pub(crate) fn validate_recipe(
        &self,
        recipe: &CacheProducerRecipe,
    ) -> Result<(), OrchestratorError> {
        self.validate()?;
        if recipe.original() != self.original()
            || recipe.source_helpers() != self.source_helpers()
            || recipe.generator_version() != self.generator_version()
        {
            return Err(invalid("foreign_structural_recipe"));
        }
        Ok(())
    }
}

/// Build a deterministic writer draft from typed native identity and compiled owners.
/// Unknown artifact, historical-origin or publication authority stays cold in the owner.
/// # Errors
/// Rejects unsupported native qualification, foreign selections and changed owner inputs.
pub fn draft_mbx_producer(
    descriptor: &MbxExportDescriptor,
    selection: &ToolProducerSelection,
    catalog: &ToolCatalog,
    setup: &MiseSetup,
    version: &str,
) -> Result<DraftMbxProducer, OrchestratorError> {
    let metadata = metadata(descriptor, selection)?;
    let sources = compiled_mbx_sources(&metadata, catalog, setup, version)?;
    sources.validate(&metadata, catalog, setup, version)?;
    let draft = DraftMbxProducer {
        job: writer_job(&metadata, &sources)?,
        sources,
        catalog: catalog.clone(),
        setup: setup.clone(),
        version: version.to_owned(),
    };
    draft.validate()?;
    Ok(draft)
}

fn writer_job(
    metadata: &PureMbxProducer,
    sources: &MbxProducerSources,
) -> Result<Job, OrchestratorError> {
    metadata.validate()?;
    Ok(Job {
        cache_mode: Some(CacheMode::Write),
        display_name: format!(
            "Publish {} native MBX state",
            metadata.descriptor.domain.name()
        ),
        runs_on: metadata.descriptor.runs_on.clone(),
        timeout_minutes: JobTimeout::CRATE,
        needs: metadata.needs(),
        condition: Some(metadata.condition()),
        permissions: Some(Permissions {
            contents: PermissionLevel::None,
            actions: PermissionLevel::Read,
            ..Permissions::default()
        }),
        environment: None,
        source_producer: None,
        tool_producer: None,
        mbx_producer: Some(metadata.clone()),
        native_pages_deploy: None,
        native_publish: None,
        outputs: cache_mbx_roles::producer_outputs(metadata),
        steps: cache_mbx_roles::draft_producer_steps(metadata, sources.source_helpers())?,
    })
}

fn metadata(
    descriptor: &MbxExportDescriptor,
    selection: &ToolProducerSelection,
) -> Result<PureMbxProducer, OrchestratorError> {
    let metadata = PureMbxProducer {
        descriptor: descriptor.clone(),
        selection: selection.clone(),
        installation_step: StepId::new("mbx-install")?,
        admission_step: StepId::new("mbx-admit")?,
        verification_step: StepId::new("mbx-verify")?,
        save_step: StepId::new("mbx-save")?,
        publication_step: StepId::new("mbx-publication")?,
        report_step: StepId::new("mbx-report")?,
    };
    metadata.validate()?;
    Ok(metadata)
}

fn invalid(reason: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("mbx_producer_{reason}"),
    }
}
