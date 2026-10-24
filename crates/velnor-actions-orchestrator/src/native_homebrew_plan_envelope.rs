use super::{
    BTreeMap, ExecuteTaskIds, ExecuteTaskRef, IDENTITY_SCHEMA, MatrixEntry,
    NativeCheckIdentityExtension, NativeHomebrewCheck, OPERATION_VERSION, ObligationDecision,
    PlanGenerator, PlanObligation, PlannedPlatform, ProposedTask, Stack, StackExtension,
    TaskConfiguration, TaskGenerator, TaskIdentity, TaskInput, VcsInputs, canonical_json_bytes,
    digest_b3, input_digest,
};

struct PlanDigests {
    closure: String,
    task: String,
    input: String,
    platform: PlannedPlatform,
}

pub(crate) fn derive_plan_envelopes(
    check: &NativeHomebrewCheck,
    task: &ProposedTask,
    run_key: &str,
    generator: &PlanGenerator,
    identity: &NativeCheckIdentityExtension,
) -> Result<(PlanObligation, Vec<MatrixEntry>), crate::OrchestratorError> {
    let digests = plan_digests(check, task, generator, identity)?;
    let job_id = format!("check-{}", check.id);
    let obligation = PlanObligation {
        task_id: task.task_id.clone(),
        decision: ObligationDecision::Execute,
        reason: "native_homebrew_check_requires_execution".to_owned(),
        task_digest: digests.task.clone(),
        input_digest: digests.input.clone(),
        closure_digest: digests.closure,
        job_id: job_id.clone(),
    };
    let metadata = serde_json::json!({
        "check_id": check.id,
        "operation": check.operation,
        "runner": check.runner,
        "identity": identity,
    });
    let entry = MatrixEntry::derive(
        Stack::Native.id(),
        &task.task_id,
        OPERATION_VERSION,
        &digests.task,
        metadata,
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                task.task_kind.clone(),
                ExecuteTaskRef::Single(task.task_id.clone()),
            )]),
        },
        &digests.input,
        run_key,
        &job_id,
        digests.platform,
    )
    .map_err(crate::internal::internal_contract)?;
    Ok((obligation, vec![entry]))
}

fn plan_digests(
    check: &NativeHomebrewCheck,
    task: &ProposedTask,
    generator: &PlanGenerator,
    identity: &NativeCheckIdentityExtension,
) -> Result<PlanDigests, crate::OrchestratorError> {
    use crate::internal::{internal, internal_contract};

    let extension_bytes = canonical_json_bytes(identity).map_err(internal_contract)?;
    let closure = digest_b3(&extension_bytes);
    let stack_extension = StackExtension {
        schema: IDENTITY_SCHEMA.to_owned(),
        data: serde_json::to_value(identity)
            .map_err(|_| internal("native_check_identity_encode"))?,
    };
    let platform = PlannedPlatform::new(&check.runner.label, check.runner.platform.target())
        .map_err(internal_contract)?;
    let task_digest = digest_b3(
        &canonical_json_bytes(&(
            &task.task_id,
            check.operation,
            OPERATION_VERSION,
            &generator.version,
            &generator.sha256,
            &identity.tool_profile,
        ))
        .map_err(internal_contract)?,
    );
    let task_identity = task_identity(
        check,
        task,
        generator,
        identity,
        &closure,
        &platform,
        &stack_extension,
    );
    let input = input_digest(&task_identity).map_err(internal_contract)?;
    Ok(PlanDigests {
        closure,
        task: task_digest,
        input,
        platform,
    })
}

fn task_identity(
    check: &NativeHomebrewCheck,
    task: &ProposedTask,
    generator: &PlanGenerator,
    identity: &NativeCheckIdentityExtension,
    closure: &str,
    platform: &PlannedPlatform,
    stack_extension: &StackExtension,
) -> TaskIdentity {
    TaskIdentity {
        schema_version: 1,
        stack_id: Stack::Native.id().to_owned(),
        project_root: ".".to_owned(),
        component_id: task.component_id.clone(),
        task_kind: task.task_kind.clone(),
        task_id: task.task_id.clone(),
        argv: Vec::new(),
        working_dir: ".".to_owned(),
        configuration: TaskConfiguration {
            target: check.runner.platform.target().to_owned(),
            profile: check.operation.as_str().to_owned(),
            features: Vec::new(),
            flags: Vec::new(),
            task_contract: OPERATION_VERSION.to_owned(),
            compile_driver: "native-homebrew".to_owned(),
            test_runner: "native-homebrew".to_owned(),
        },
        inputs: vec![TaskInput {
            path: "velnor/native-check-identity".to_owned(),
            digest: closure.to_owned(),
        }],
        dependencies: Vec::new(),
        vcs: VcsInputs {
            commit: Some(identity.head.clone()),
            reference: None,
            submodules: BTreeMap::new(),
        },
        toolchain_id: digest_b3(generator.version.as_bytes()),
        platform_id: platform.platform_id.clone(),
        environment: BTreeMap::new(),
        output_contract: "native-homebrew-check-receipt-v1".to_owned(),
        generator: TaskGenerator {
            version: generator.version.clone(),
            target: generator.target.clone(),
        },
        stack_extension: stack_extension.clone(),
    }
}
