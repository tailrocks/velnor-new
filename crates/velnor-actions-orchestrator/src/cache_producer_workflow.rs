//! Fixed signing workflow drafts owned by the closed receipt source factory.
use crate::{
    OrchestratorError,
    cache_receipt_source::{
        CacheReceiptPublication, CacheReceiptSources, CacheReceiptTransportLayout,
    },
};
use std::collections::BTreeMap;
use velnor_actions_contract::{CompiledSourceHelper, Job, PermissionLevel, Step, StepId, StepKind};
use velnor_actions_workflow_renderer::cache_producer_workflow::CacheProducerRecipe;
use velnor_actions_workflow_renderer::{
    RenderError, RenderedFile, WorkflowDocumentContext, yaml::Yaml,
};

const CACHE_PRODUCER_ATTEST_USES: &str = "actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6";
const CACHE_PRODUCER_PREDICATE_TYPE: &str = "https://velnor.dev/cache-producer/v1";
const MANIFEST_ID: &str = "velnor-cache-receipt-manifest";
const ATTEST_ID: &str = "velnor-cache-receipt-attest";
const BUNDLE_ID: &str = "velnor-cache-receipt-bundle";

/// Reviewable publication candidate. It grants no caller reference or warm admission.
#[derive(Debug, Clone)]
pub struct DraftCacheProducerWorkflow {
    /// Fixed pure callee source for immutable publication.
    pub file: RenderedFile,
}

/// Emit a fixed pure callee from an admitted recipe and its exact owner-issued sources.
/// # Errors
/// Rejects substituted sources, mismatched recipes and missing compiled helper authority.
pub fn render_cache_producer_workflow_draft(
    recipe: &CacheProducerRecipe,
    sources: &CacheReceiptSources,
) -> Result<DraftCacheProducerWorkflow, OrchestratorError> {
    if !sources.admits(recipe) {
        return Err(OrchestratorError::Contract {
            problem: "cache_receipt_sources_foreign_recipe".into(),
        });
    }
    let original = recipe.original();
    let digest = velnor_actions_contract::cache_producer_recipe_digest(original)?;
    let job = insert_receipt(
        original,
        sources.manifest(),
        sources.bundle(),
        sources.transport_layout(),
    )?;
    let mut records = recipe.source_helpers().to_vec();
    records.extend([sources.manifest().clone(), sources.bundle().clone()]);
    let context = WorkflowDocumentContext {
        generator_version: recipe.generator_version().into(),
        source_helpers: records,
        native_pages_approvals: Vec::new(),
        native_publish_approvals: Vec::new(),
        action_credential_approvals: Vec::new(),
    };
    let document = workflow_yaml(&job, &context)?;
    let bytes = velnor_actions_workflow_renderer::marker::with_marker(
        recipe.generator_version(),
        &velnor_actions_workflow_renderer::cache_producer_workflow::render_cache_producer_document(
            document,
        ),
    )?;
    Ok(DraftCacheProducerWorkflow {
        file: RenderedFile {
            path: format!(".github/workflows/velnor-cache-producer-{digest}.yml"),
            bytes,
        },
    })
}

/// Activate only actual immutable source publication. No recipe is qualified yet.
/// # Errors
/// Every current request stays cold until the publication owner supplies evidence.
pub fn render_published_cache_producer_workflow(
    _recipe: &CacheProducerRecipe,
    _sources: &CacheReceiptSources,
    _publication: &CacheReceiptPublication,
) -> Result<DraftCacheProducerWorkflow, OrchestratorError> {
    Err(OrchestratorError::Contract {
        problem: "cache_producer_publication_unqualified".into(),
    })
}

fn insert_receipt(
    original: &Job,
    manifest: &CompiledSourceHelper,
    bundle: &CompiledSourceHelper,
    layout: &CacheReceiptTransportLayout,
) -> Result<Job, RenderError> {
    let save_id = original
        .tool_producer
        .as_ref()
        .map(|meta| &meta.save_step)
        .or_else(|| {
            original
                .source_producer
                .as_ref()
                .map(|meta| &meta.save_step)
        })
        .or_else(|| original.mbx_producer.as_ref().map(|meta| &meta.save_step))
        .ok_or_else(|| invalid("missing_save_binding"))?;
    let index = original
        .steps
        .iter()
        .position(|step| step.id.as_ref() == Some(save_id))
        .ok_or_else(|| invalid("missing_save_step"))?;
    let condition = original.steps[index]
        .condition
        .clone()
        .ok_or_else(|| invalid("missing_save_condition"))?;
    let root = format!("${{{{ runner.temp }}}}/velnor/{}", layout.evidence_root());
    let additions = receipt_steps(manifest, bundle, &root, &condition)?;
    let mut job = original.clone();
    let StepKind::Action { with, .. } = &mut job.steps[index].kind else {
        return Err(invalid("save_not_action"));
    };
    let payload = with
        .get_mut("path")
        .ok_or_else(|| invalid("missing_payload"))?;
    *payload = layout.transport_paths().into();
    job.steps[index].condition = Some(format!(
        "{condition} && steps.{BUNDLE_ID}.outcome == 'success'"
    ));
    job.steps.splice(index..index, additions);
    job.needs.clear();
    job.condition =
        Some(velnor_actions_contract::workflow::cache_trust::CACHE_TRUSTED_PUSH_EXPR.into());
    let permissions = job
        .permissions
        .as_mut()
        .ok_or_else(|| invalid("missing_permissions"))?;
    permissions.id_token = PermissionLevel::Write;
    permissions.attestations = PermissionLevel::Write;
    Ok(job)
}

fn receipt_steps(
    manifest: &CompiledSourceHelper,
    bundle: &CompiledSourceHelper,
    root: &str,
    condition: &str,
) -> Result<Vec<Step>, RenderError> {
    let make = |name, id, record: &CompiledSourceHelper| {
        let mut step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
            name,
            record,
            record.environment().clone(),
        )?;
        step.id = Some(StepId::new(id).map_err(RenderError::Contract)?);
        step.condition = Some(condition.into());
        Ok::<_, RenderError>(step)
    };
    let attest = Step {
        id: Some(StepId::new(ATTEST_ID).map_err(RenderError::Contract)?),
        name: "Attest complete cache payload manifest".into(),
        condition: Some(condition.into()),
        kind: StepKind::Action {
            uses: CACHE_PRODUCER_ATTEST_USES.into(),
            with: BTreeMap::from([
                ("subject-path".into(), format!("{root}/manifest.json")),
                ("predicate-path".into(), format!("{root}/predicate.json")),
                (
                    "predicate-type".into(),
                    CACHE_PRODUCER_PREDICATE_TYPE.into(),
                ),
                ("show-summary".into(), "false".into()),
            ]),
            env: BTreeMap::new(),
        },
    };
    Ok(vec![
        make(
            "Create complete cache producer statement",
            MANIFEST_ID,
            manifest,
        )?,
        attest,
        make("Store public cache producer bundle", BUNDLE_ID, bundle)?,
    ])
}

fn permissions_yaml(job: &Job) -> Yaml {
    let mut permissions = vec![
        ("id-token".into(), Yaml::str("write")),
        ("attestations".into(), Yaml::str("write")),
    ];
    if job
        .permissions
        .as_ref()
        .is_some_and(|permissions| permissions.actions == PermissionLevel::Read)
    {
        permissions.push(("actions".into(), Yaml::str("read")));
    }
    Yaml::Map(permissions)
}

fn workflow_yaml(job: &Job, context: &WorkflowDocumentContext) -> Result<Yaml, RenderError> {
    let steps = job
        .steps
        .iter()
        .map(|step| {
            velnor_actions_workflow_renderer::cache_producer_workflow::render_cache_producer_step(
                step,
                &context.source_helpers,
                &job.runs_on,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let outputs = job
        .outputs
        .iter()
        .map(|output| (output.name.clone(), Yaml::str(output.value.expression())))
        .collect();
    let producer = Yaml::Map(vec![
        ("name".into(), Yaml::str(job.display_name.clone())),
        ("runs-on".into(), Yaml::str(job.runs_on.clone())),
        (
            "timeout-minutes".into(),
            Yaml::Int(i64::from(job.timeout_minutes.minutes())),
        ),
        ("cache-mode".into(), Yaml::str("write")),
        (
            "if".into(),
            Yaml::str(velnor_actions_contract::workflow::cache_trust::CACHE_TRUSTED_PUSH_EXPR),
        ),
        ("permissions".into(), permissions_yaml(job)),
        ("outputs".into(), Yaml::Map(outputs)),
        ("steps".into(), Yaml::Seq(steps)),
    ]);
    let call_outputs = job
        .outputs
        .iter()
        .map(|output| {
            (
                output.name.clone(),
                Yaml::Map(vec![(
                    "value".into(),
                    Yaml::str(format!("${{{{ jobs.producer.outputs.{} }}}}", output.name)),
                )]),
            )
        })
        .collect();
    Ok(Yaml::Map(vec![
        ("name".into(), Yaml::str("Velnor pure cache producer")),
        ("cache-mode".into(), Yaml::str("read")),
        (
            "on".into(),
            Yaml::Map(vec![(
                "workflow_call".into(),
                Yaml::Map(vec![("outputs".into(), Yaml::Map(call_outputs))]),
            )]),
        ),
        ("permissions".into(), Yaml::Map(Vec::new())),
        (
            "jobs".into(),
            Yaml::Map(vec![("producer".into(), producer)]),
        ),
    ]))
}

fn invalid(reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("cache_producer_workflow_{reason}"))
}
