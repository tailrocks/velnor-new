use super::plan_envelope::derive_plan_envelopes;
use super::seed::derive_seed_transition;
use super::tree::{
    caller_repository, ensure_tracked_tree_clean, git_value, pull_request_number_from_env,
    tool_profile,
};
use super::{
    BTreeMap, IDENTITY_SCHEMA, MatrixEntry, NativeCheckAuthorityIdentity,
    NativeCheckIdentityExtension, NativeHomebrewCheck, NativeHomebrewOperation, OPERATION_VERSION,
    Path, PlanGenerator, PlanObligation, ProposedTask, ResourceClass, ResourceDemand,
    SeedTransitionAdmission, Stack, WorkflowEvent, digest_b3,
};
use crate::internal::internal;
use std::time::{Duration, Instant};

pub(crate) fn proposal(
    check: &NativeHomebrewCheck,
) -> Result<ProposedTask, velnor_actions_contract::ContractError> {
    let mut flags = vec![format!("operation:{}", check.operation.as_str())];
    let formula_path = "homebrew-native-check";
    flags.sort();
    let task_id = format!("internal/native-check/{}", check.id);
    let resource_class = match check.operation {
        NativeHomebrewOperation::Install => ResourceClass::Network,
        NativeHomebrewOperation::TapTree
        | NativeHomebrewOperation::FormulaContract
        | NativeHomebrewOperation::AdoptionContract => ResourceClass::Lightweight,
    };
    let identity = velnor_actions_contract::propose::IdentityInputs {
        unit_id: check.id.clone(),
        unit_key: check.id.clone(),
        unit_path: formula_path.to_owned(),
        project_root: ".".to_owned(),
        target: check.runner.platform.target().to_owned(),
        features: Vec::new(),
        flags,
        compile_driver: "native-homebrew".to_owned(),
        test_runner: "native-homebrew".to_owned(),
        environment: BTreeMap::new(),
        declared_inputs: Vec::new(),
        undeclared_reads: true,
    };
    let proposal = ProposedTask {
        task_id,
        stack_id: Stack::Native.id().to_owned(),
        component_id: check.id.clone(),
        task_kind: "native-check".to_owned(),
        configuration: check.operation.as_str().to_owned(),
        depends_on: Vec::new(),
        gated_by: Vec::new(),
        reads: Vec::new(),
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: resource_class,
            cpu_milli: None,
            memory_mb: None,
            needs_network: matches!(check.operation, NativeHomebrewOperation::Install),
            service: None,
        },
        cache_policy: velnor_actions_contract::CachePolicy {
            allow_compilation_reuse: false,
            allow_task_reuse: false,
        },
        identity,
        payload: vec![std::ffi::OsString::from(OPERATION_VERSION)],
        display_name: check.id.clone(),
        uses_clock: false,
        uses_random: false,
        no_targets: false,
        runner_profile: check.runner.label.clone(),
    };
    proposal.validate()?;
    Ok(proposal)
}

/// Derive the one mandatory native lane using the ordinary plan envelopes.
pub(crate) fn derive(
    root: &Path,
    check: &NativeHomebrewCheck,
    task: &ProposedTask,
    run_key: &str,
    generator: &PlanGenerator,
    event: WorkflowEvent,
) -> Result<(PlanObligation, Vec<MatrixEntry>), crate::OrchestratorError> {
    validate_task_binding(check, task)?;
    let deadline = velnor_actions_mise::CheckDeadline::from_start(
        Instant::now(),
        Duration::from_secs(u64::from(check.timeout_minutes) * 60),
    )
    .map_err(|error| crate::internal::internal(&error.to_string()))?;
    let context = read_plan_context(root, event, deadline)?;
    let authority = select_authority(root, check, generator, event, &context, deadline)?;
    let identity = NativeCheckIdentityExtension {
        schema: IDENTITY_SCHEMA.to_owned(),
        check_id: check.id.clone(),
        operation: check.operation,
        formula_channel: check.formula_channel,
        operation_version: OPERATION_VERSION.to_owned(),
        sdk_version: generator.version.clone(),
        sdk_sha256: generator.sha256.clone(),
        authority: authority.identity,
        tap_repository: context.repository,
        tap_repository_id: authority.repository_id,
        tap_base_sha: authority.base_sha,
        head: context.head.clone(),
        tree_object: context.tree_object,
        formula_path: authority.formula_path,
        formula_blob_sha: authority.formula_blob_sha,
        runner: check.runner.label.clone(),
        platform: check.runner.platform,
        tool_profile: tool_profile(check.operation, check.runner.platform),
    };
    derive_plan_envelopes(check, task, run_key, generator, &identity)
}

fn validate_task_binding(
    check: &NativeHomebrewCheck,
    task: &ProposedTask,
) -> Result<(), crate::OrchestratorError> {
    if task.stack_id != Stack::Native.id()
        || task.task_id != format!("internal/native-check/{}", check.id)
        || task.configuration != check.operation.as_str()
    {
        return Err(crate::internal::internal("native_check_task_mismatch"));
    }
    Ok(())
}

struct NativePlanContext {
    head: String,
    tree_object: String,
    tree: Vec<velnor_actions_mise::TapTreeEntry>,
    repository: String,
    pull_request: Option<u64>,
}

fn read_plan_context(
    root: &Path,
    event: WorkflowEvent,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<NativePlanContext, crate::OrchestratorError> {
    let head = git_value(root, vec!["HEAD".to_owned()], deadline)?;
    let tree_object = git_value(root, vec!["HEAD^{tree}".to_owned()], deadline)?;
    ensure_tracked_tree_clean(root, deadline)?;
    let tree = velnor_actions_mise::read_tap_tree(root, &head, deadline)
        .map_err(|error| crate::internal::internal(&error.to_string()))?;
    if tree
        .iter()
        .any(|entry| entry.mode != 0o100_644 && entry.mode != 0o100_755)
    {
        return Err(crate::internal::internal(
            "native_check_tree_nonregular_entry",
        ));
    }
    Ok(NativePlanContext {
        head,
        tree_object,
        tree,
        repository: caller_repository(root)?,
        pull_request: pull_request_number_from_env(event)?,
    })
}

struct AuthoritySelection {
    identity: NativeCheckAuthorityIdentity,
    repository_id: u64,
    base_sha: String,
    formula_path: Option<String>,
    formula_blob_sha: Option<String>,
}

fn select_authority(
    root: &Path,
    check: &NativeHomebrewCheck,
    generator: &PlanGenerator,
    event: WorkflowEvent,
    context: &NativePlanContext,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<AuthoritySelection, crate::OrchestratorError> {
    let seed = if matches!(
        check.operation,
        NativeHomebrewOperation::AdoptionContract | NativeHomebrewOperation::TapTree
    ) {
        derive_seed_transition(
            root,
            &context.repository,
            event,
            context.pull_request,
            &context.head,
            deadline,
            check.operation == NativeHomebrewOperation::AdoptionContract,
        )?
    } else {
        None
    };
    if let Some(seed) = seed {
        return seed_authority(generator, &seed);
    }
    if check.operation == NativeHomebrewOperation::AdoptionContract {
        return Err(crate::internal::internal(
            "homebrew_adoption_transition_not_admitted",
        ));
    }
    source_authority(root, check, event, context, deadline)
}

fn seed_authority(
    generator: &PlanGenerator,
    seed: &SeedTransitionAdmission,
) -> Result<AuthoritySelection, crate::OrchestratorError> {
    use crate::internal::internal_contract;

    let transition = seed.transition.transition();
    if generator.version != seed.sdk.version
        || generator.sha256 != seed.sdk.seed_runner_binary_sha256
        || seed.sdk.seed_runner_target != "x86_64-unknown-linux-gnu"
    {
        return Err(internal("sdk_running_executable_manifest_mismatch"));
    }
    Ok(AuthoritySelection {
        identity: NativeCheckAuthorityIdentity::Seed {
            transition_id: transition.transition_id.clone(),
            transition_digest: transition.digest().map_err(internal_contract)?,
            base_tree: transition.base_tree.clone(),
            sdk_source_sha: seed.sdk.source_sha.clone(),
            sdk_version: seed.sdk.version.clone(),
            sdk_manifest_sha256: seed.sdk.manifest_sha256.clone(),
            sdk_seed_runner_binary_sha256: seed.sdk.seed_runner_binary_sha256.clone(),
        },
        repository_id: transition.tap_repository_id,
        base_sha: transition.base_sha.clone(),
        formula_path: None,
        formula_blob_sha: None,
    })
}

fn source_authority(
    root: &Path,
    check: &NativeHomebrewCheck,
    event: WorkflowEvent,
    context: &NativePlanContext,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<AuthoritySelection, crate::OrchestratorError> {
    use crate::internal::internal_contract;

    let authority = crate::homebrew::authority::load_protected_homebrew_authority(
        root,
        &context.repository,
        event,
        context.pull_request,
        &context.head,
        deadline,
    )?;
    let formula_spec_digest = authority.spec().digest().map_err(internal_contract)?;
    let cutover =
        crate::homebrew::structural_cutover::verify_seed_output_base(root, &authority, deadline)?
            .map(|witness| Box::new(witness.identity().clone()));
    let formula_path = check
        .formula_channel
        .and_then(|channel| {
            authority
                .spec()
                .channels
                .iter()
                .find(|item| item.channel == channel)
        })
        .map(|item| item.destination.clone());
    let formula_blob_sha = formula_blob_sha(formula_path.as_deref(), &context.tree)?;
    require_formula_policy(check, formula_path.as_deref(), formula_blob_sha.as_deref())?;
    Ok(AuthoritySelection {
        identity: NativeCheckAuthorityIdentity::Source {
            source_repository_id: authority.source_repository_id(),
            authority_sha: authority.authority_sha().to_owned(),
            source_plan_digest: digest_b3(
                &authority
                    .plan()
                    .canonical_bytes()
                    .map_err(internal_contract)?,
            ),
            locator_blob_sha: authority.locator_blob_sha().to_owned(),
            formula_spec_digest,
            cutover,
        },
        repository_id: authority.tap_repository_id(),
        base_sha: authority.tap_base_sha().to_owned(),
        formula_path,
        formula_blob_sha,
    })
}

fn formula_blob_sha(
    path: Option<&str>,
    tree: &[velnor_actions_mise::TapTreeEntry],
) -> Result<Option<String>, crate::OrchestratorError> {
    path.map(|path| {
        tree.iter()
            .find(|entry| entry.path == path)
            .map(|entry| entry.blob_sha.clone())
            .ok_or_else(|| crate::internal::internal("native_check_formula_missing"))
    })
    .transpose()
}

fn require_formula_policy(
    check: &NativeHomebrewCheck,
    path: Option<&str>,
    blob: Option<&str>,
) -> Result<(), crate::OrchestratorError> {
    if matches!(
        check.operation,
        NativeHomebrewOperation::FormulaContract | NativeHomebrewOperation::Install
    ) && (path.is_none() || blob.is_none())
    {
        Err(crate::internal::internal(
            "native_check_formula_policy_missing",
        ))
    } else {
        Ok(())
    }
}
