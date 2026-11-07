// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, error::Error};
use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, JOB_IMAGE_CANDIDATE_JOB_ID, JOB_IMAGE_CANDIDATE_RUNNER,
    JOB_IMAGE_CANDIDATE_TASK_ID, JOB_IMAGE_CANDIDATE_TASK_KIND, JOB_IMAGE_CANDIDATE_TASK_SCHEMA,
    JOB_IMAGE_CANDIDATE_UNIT_ID, MatrixEntry, NativeTaskKind, PlannedPlatform, StackExtension,
    TaskConfiguration, TaskExecutionManifestEntryV1, TaskGenerator, TaskIdentity, TaskInput,
    VcsInputs, canonical_json_bytes, digest_b3,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::verification_jobs::JobImageCandidateExecution;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[test]
fn project_tool_selectors_remain_visible_to_runtime_materialization() -> Result<()> {
    let catalog = ToolCatalog::pinned()
        .with_project_tool(PinnedTool::Rust, "1.90.0")?
        .with_project_tool(PinnedTool::MrBoxington, "0.1.0")?;
    let execution = JobImageCandidateExecution::for_catalog(&catalog)?;
    assert_eq!(execution.argv()[13], "rust@${VELNOR_TOOL_RUST}");
    assert_eq!(
        execution.argv()[14],
        "mr-boxington@${VELNOR_TOOL_MR_BOXINGTON}"
    );
    execution.proposal()?;
    let without_export =
        JobImageCandidateExecution::for_catalog(&catalog.with_export_rustup_toolchain(false))?;
    assert_eq!(without_export.argv()[15], "no-export");
    assert!(
        !without_export
            .environment()
            .contains_key("RUSTUP_TOOLCHAIN")
    );
    let entry = entry(&without_export, &identity(&without_export)?)?;
    record(&without_export, &entry)?.validate()?;
    Ok(())
}

#[test]
fn canonical_candidate_factory_is_admitted_only_by_its_closed_execution_contract() -> Result<()> {
    let execution = JobImageCandidateExecution::for_catalog(&ToolCatalog::pinned())?;
    let proposal = execution.proposal()?;
    velnor_actions_contract::validate_job_image_candidate_proposal(&proposal, execution.inputs())?;
    let mut wrong_inputs = execution.inputs().clone();
    wrong_inputs.mr_boxington = "mr-boxington@0.0.1".to_owned();
    assert!(
        velnor_actions_contract::validate_job_image_candidate_proposal(&proposal, &wrong_inputs)
            .is_err()
    );
    let identity = identity(&execution)?;
    identity.validate()?;
    let entry = entry(&execution, &identity)?;
    entry.validate("r101-a1")?;
    assert_eq!(
        NativeTaskKind::from_entry(&entry)?,
        NativeTaskKind::JobImageCandidate
    );
    let mut record = record(&execution, &entry)?;
    record.validate()?;
    let frame = record.nul_frame()?;
    assert!(frame.starts_with(velnor_actions_contract::TASK_EXECUTION_FRAME_MAGIC.as_bytes()));
    for field in frame.split(|byte| *byte == 0) {
        assert!(!field.contains(&b'\n'));
    }
    for mutate in execution_mutations() {
        let mut changed = record.clone();
        mutate(&mut changed);
        refresh(&mut changed)?;
        assert!(
            changed.validate().is_err(),
            "mutated execution must not enter native dispatch"
        );
    }
    for mutate in entry_mutations() {
        let mut changed = entry.clone();
        mutate(&mut changed)?;
        assert!(
            changed.validate("r101-a1").is_err(),
            "mutated native identity must not be admitted"
        );
    }
    let mut changed = identity.clone();
    changed.argv[7] = "/tmp/python3".to_owned();
    assert!(changed.validate().is_err());
    changed = identity.clone();
    changed.component_id = "foreign".to_owned();
    assert!(changed.validate().is_err());
    let mut changed = identity;
    changed.argv = vec!["inert".to_owned()];
    assert!(changed.validate().is_err());
    record
        .env
        .insert("MISE_NO_CONFIG".to_owned(), "0".to_owned());
    refresh(&mut record)?;
    assert!(record.validate().is_err());
    Ok(())
}

fn identity(execution: &JobImageCandidateExecution) -> Result<TaskIdentity> {
    let proposal = execution.proposal()?;
    let relative = "crates/velnor-actions-workflow-renderer/src/job_image_candidate_execution.rs";
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    Ok(TaskIdentity {
        schema_version: 1,
        stack_id: proposal.stack_id,
        project_root: proposal.identity.project_root,
        component_id: proposal.component_id,
        task_kind: proposal.task_kind,
        task_id: proposal.task_id,
        argv: execution.argv().to_vec(),
        working_dir: ".".to_owned(),
        configuration: TaskConfiguration {
            target: proposal.identity.target,
            profile: proposal.configuration,
            features: proposal.identity.features,
            flags: proposal.identity.flags,
            task_contract: JOB_IMAGE_CANDIDATE_TASK_SCHEMA.to_owned(),
            compile_driver: proposal.identity.compile_driver,
            test_runner: proposal.identity.test_runner,
        },
        inputs: vec![TaskInput {
            path: relative.to_owned(),
            digest: digest_b3(&std::fs::read(source)?),
        }],
        dependencies: proposal.depends_on,
        vcs: VcsInputs {
            commit: None,
            reference: None,
            submodules: BTreeMap::new(),
        },
        toolchain_id: velnor_actions_contract::cachekey::toolchain_id(
            execution.actual_toolchain(),
        )?,
        platform_id: digest_b3(&canonical_json_bytes(&PlannedPlatform::new(
            JOB_IMAGE_CANDIDATE_RUNNER,
            "x86_64-unknown-linux-gnu",
        )?)?),
        environment: execution.environment().clone(),
        output_contract: JOB_IMAGE_CANDIDATE_TASK_SCHEMA.to_owned(),
        generator: TaskGenerator {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            target: std::env::consts::ARCH.to_owned(),
        },
        stack_extension: StackExtension {
            schema: JOB_IMAGE_CANDIDATE_TASK_SCHEMA.to_owned(),
            data: serde_json::to_value(execution.inputs())?,
        },
    })
}

fn entry(execution: &JobImageCandidateExecution, identity: &TaskIdentity) -> Result<MatrixEntry> {
    let digest = velnor_actions_contract::workflow::crate_job::task_digest_for_execution(
        JOB_IMAGE_CANDIDATE_TASK_ID,
        execution.argv(),
        &velnor_actions_contract::cachekey::toolchain_id(execution.binding_toolchain())?,
    )?;
    Ok(MatrixEntry::derive(
        "native",
        JOB_IMAGE_CANDIDATE_TASK_ID,
        &velnor_actions_workflow_renderer::join_argv_for_run(execution.argv())?,
        &digest,
        serde_json::json!({"unit_id": JOB_IMAGE_CANDIDATE_UNIT_ID}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                JOB_IMAGE_CANDIDATE_TASK_KIND.to_owned(),
                ExecuteTaskRef::Single(JOB_IMAGE_CANDIDATE_TASK_ID.to_owned()),
            )]),
        },
        &velnor_actions_contract::input_digest(identity)?,
        "r101-a1",
        JOB_IMAGE_CANDIDATE_JOB_ID,
        PlannedPlatform::new(JOB_IMAGE_CANDIDATE_RUNNER, "x86_64-unknown-linux-gnu")?,
    )?)
}

fn record(
    execution: &JobImageCandidateExecution,
    entry: &MatrixEntry,
) -> Result<TaskExecutionManifestEntryV1> {
    let mut record = TaskExecutionManifestEntryV1 {
        task_id: entry.task_id.clone(),
        execution_digest: String::new(),
        task_digest: entry.task_digest.clone(),
        toolchain_inputs: execution.binding_toolchain().clone(),
        argv: execution.argv().to_vec(),
        env: execution.environment().clone(),
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        report_helper_version: env!("CARGO_PKG_VERSION").to_owned(),
        matrix_max_parallel: None,
    };
    record.refresh_execution_digest()?;
    Ok(record)
}

fn refresh(record: &mut TaskExecutionManifestEntryV1) -> Result<()> {
    record.task_digest = velnor_actions_contract::workflow::crate_job::task_digest_for_execution(
        &record.task_id,
        &record.argv,
        &velnor_actions_contract::cachekey::toolchain_id(&record.toolchain_inputs)?,
    )?;
    record.matrix_id =
        velnor_actions_contract::matrix_id_for_task_group("native", &record.task_id)?;
    record.matrix_key = velnor_actions_contract::matrix_key_for_id(&record.matrix_id)?;
    record.refresh_execution_digest()?;
    Ok(())
}

type ExecutionMutation = fn(&mut TaskExecutionManifestEntryV1);
fn execution_mutations() -> [ExecutionMutation; 11] {
    [
        |record| "/tmp/python3".clone_into(&mut record.argv[7]),
        |record| record.argv.push("extra".to_owned()),
        |record| "GG".clone_into(&mut record.argv[12]),
        |record| "00".clone_into(&mut record.argv[12]),
        |record| "no-export".clone_into(&mut record.argv[15]),
        |record| {
            record
                .env
                .insert("RUSTDOCFLAGS".to_owned(), "-D warnings".to_owned());
        },
        |record| {
            record
                .toolchain_inputs
                .tools
                .retain(|tool| !tool.starts_with("mr-boxington@"));
            record
                .toolchain_inputs
                .tools
                .push("mr-boxington@0.0.1".to_owned());
            record.toolchain_inputs.tools.sort();
        },
        |record| {
            record.argv[12] =
                "00".repeat(velnor_actions_contract::JOB_IMAGE_CANDIDATE_MAX_SCRIPT_BYTES + 1);
        },
        |record| "internal/job-image-candidate/foreign".clone_into(&mut record.task_id),
        |record| "cargo".clone_into(&mut record.toolchain_inputs.compile_driver),
        |record| {
            record.toolchain_inputs.tools.pop();
        },
    ]
}

type EntryMutation = fn(&mut MatrixEntry) -> Result<()>;
fn entry_mutations() -> [EntryMutation; 7] {
    [
        |entry| {
            entry.adapter_metadata["unit_id"] = "foreign".into();
            Ok(())
        },
        |entry| {
            "task-foreign".clone_into(&mut entry.job_id);
            Ok(())
        },
        |entry| {
            entry.planned_platform = PlannedPlatform::new("macos-26", "aarch64-apple-darwin")?;
            Ok(())
        },
        |entry| {
            entry.execute_task_ids.tasks.insert(
                "foreign".to_owned(),
                ExecuteTaskRef::Single(entry.task_id.clone()),
            );
            Ok(())
        },
        |entry| {
            entry.execute_task_ids.tasks.insert(
                JOB_IMAGE_CANDIDATE_TASK_KIND.to_owned(),
                ExecuteTaskRef::Single("internal/job-image-candidate/foreign".to_owned()),
            );
            Ok(())
        },
        |entry| {
            "foreign".clone_into(&mut entry.stack_id);
            Ok(())
        },
        |entry| {
            entry.execute_task_ids.tasks = BTreeMap::from([(
                "unknown_native".to_owned(),
                ExecuteTaskRef::Single(entry.task_id.clone()),
            )]);
            Ok(())
        },
    ]
}
