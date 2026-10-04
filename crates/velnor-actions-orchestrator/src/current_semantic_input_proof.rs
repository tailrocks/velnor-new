//! Private authority derived from the runner's current checkout and adapters.

#[path = "current_execution_inventory.rs"]
mod execution_inventory;

use std::path::{Path, PathBuf};

use velnor_actions_contract::{MatrixEntry, Plan, PlanGenerator, ProposedTask};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;
use crate::current_source_context::CurrentSourceContext;
use crate::current_source_snapshot::FrozenCheckout;
use crate::internal::internal;
use crate::internal::plan_obligation::source_identity::{
    ResolvedSourceIdentity, SourceIdentityInputs, resolve,
};
use crate::internal_plan::snapshot::ExecutionSnapshot;

/// Nonserializable evidence; serialized plan fields cannot construct this grant.
pub(crate) struct CurrentSemanticInputProof {
    identity: ResolvedSourceIdentity,
    task: ProposedTask,
    root: PathBuf,
    context: CurrentSourceContext,
    generator: PlanGenerator,
    runner_label: String,
    _source: FrozenCheckout,
}

impl CurrentSemanticInputProof {
    /// Executing Rust requires inventory issued by the qualified source SDK.
    ///
    /// No issuer exists yet. Refuse before reading source or launching Cargo;
    /// a plan, baseline or downloaded inventory cannot supply this authority.
    pub(crate) fn acquire_for_execution(
        _plan: &Plan,
        _entry: &MatrixEntry,
    ) -> Result<Self, OrchestratorError> {
        match execution_inventory::acquire()? {}
    }

    /// Recapture source facts and reconstruct exactly one guarded proposal.
    pub(crate) fn acquire(plan: &Plan, entry: &MatrixEntry) -> Result<Self, OrchestratorError> {
        plan.validate()?;
        let cwd = std::env::current_dir()
            .map_err(|err| OrchestratorError::io("current_directory", err.to_string()))?;
        let root = crate::root::resolve_root(&cwd)?;
        let context = CurrentSourceContext::capture(&root, plan)?;
        let generator = authenticated_generator(&root, plan)?;
        let source = FrozenCheckout::capture(&root)?;
        crate::current_candidate_source::verify(
            &root,
            source.root(),
            context.repository(),
            context.candidate(),
        )?;
        let config = crate::config::load_config(source.root())?;
        crate::prepare::check_velnor_identity(&root, &config)?;
        let (runner_label, selection) = crate::prepare::runner_label_for(&config);
        if runner_label != plan.runner.label || selection != plan.runner.selection {
            return Err(internal("helper_source_runner_mismatch"));
        }
        let discovery = crate::discover::discover(
            source.root(),
            &config,
            crate::inventory::InventoryProvider::FreshWithoutCargo,
        )?;
        if discovery.skipped_non_utf8 {
            return Err(internal("helper_source_inventory_incomplete"));
        }
        let task = unique_task(&discovery.proposals, entry)?;
        let snapshot = ExecutionSnapshot::build(&discovery).with_checkout(source.root());
        let identity = resolve(
            &SourceIdentityInputs {
                discovery: &discovery,
                task: &task,
                root: source.root(),
                snapshot: &snapshot,
                catalog: &ToolCatalog::pinned(),
                generator: &generator,
                label: &runner_label,
            },
            &mut velnor_actions_tofu::FileCache::default(),
        )?;
        source.verify_frozen()?;
        source.verify_current(&root)?;
        context.verify_current(&root, plan)?;
        Ok(Self {
            identity,
            task,
            root,
            context,
            generator,
            runner_label,
            _source: source,
        })
    }

    pub(crate) fn identity(&self) -> &ResolvedSourceIdentity {
        &self.identity
    }

    pub(crate) fn task(&self) -> &ProposedTask {
        &self.task
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn run_key(&self) -> &str {
        self.context.run_key()
    }

    pub(crate) fn generator(&self) -> &PlanGenerator {
        &self.generator
    }

    pub(crate) fn runner_label(&self) -> &str {
        &self.runner_label
    }
}

/// Same-host binary authority until cross-host acquisition is authenticated.
fn authenticated_generator(root: &Path, plan: &Plan) -> Result<PlanGenerator, OrchestratorError> {
    let actual = crate::internal_plan::default_generator();
    if actual.target != plan.generator.target {
        return crate::pins::runtime_release::authenticate(root, &plan.generator)
            .map_err(|reason| internal(&reason));
    }
    if actual.sha256 == crate::internal_plan::snapshot::UNRESOLVED_GENERATOR_SHA
        || actual.version != plan.generator.version
        || actual.target != plan.generator.target
        || actual.sha256 != plan.generator.sha256
    {
        return Err(internal("helper_source_generator_authority_unverified"));
    }
    Ok(actual)
}

/// An owner ID must name one actual proposal; neither absence nor aliases grant it.
fn unique_task(
    tasks: &[ProposedTask],
    entry: &MatrixEntry,
) -> Result<ProposedTask, OrchestratorError> {
    let mut matching = tasks.iter().filter(|task| task.task_id == entry.task_id);
    let task = matching
        .next()
        .ok_or_else(|| internal("helper_source_proposal_missing"))?;
    if matching.next().is_some() || task.stack_id != entry.stack_id {
        return Err(internal("helper_source_proposal_ambiguous"));
    }
    Ok(task.clone())
}
