// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;

use crate::{
    CachePolicy, ContractError, ProposedTask, ResourceClass, ResourceDemand, RoutingWorkflow,
    Stack, VelnorConfig,
};

pub const JOB_IMAGE_CANDIDATE_TASK_SCHEMA: &str = "velnor.job-image-candidate-task.v1";
pub const JOB_IMAGE_CANDIDATE_TASK_ID: &str = "internal/job-image-candidate/oci-linux-amd64";
pub const JOB_IMAGE_CANDIDATE_UNIT_ID: &str = "job-image-candidate";
pub const JOB_IMAGE_CANDIDATE_TASK_KIND: &str = "job_image_candidate";
pub const JOB_IMAGE_CANDIDATE_JOB_ID: &str = "task-job-image-candidate";
pub const JOB_IMAGE_CANDIDATE_ARTIFACT_PREFIX: &str = "velnor-job-image-candidate-";
pub const JOB_IMAGE_CANDIDATE_RUNNER: &str = "ubuntu-26.04";
pub const JOB_IMAGE_CANDIDATE_MAX_SCRIPT_BYTES: usize = 32768;
pub const JOB_IMAGE_CANDIDATE_LOADER: &str = "import os,sys; os.execve('/bin/bash',['/bin/bash','--noprofile','--norc','-p','-e','-o','pipefail','-c',bytes.fromhex(sys.argv[1]).decode('utf-8'),'velnor',*sys.argv[2:]],dict(os.environ))";
pub const JOB_IMAGE_CANDIDATE_DRIVER: &str = "native-job-image-candidate";
pub const JOB_IMAGE_CANDIDATE_OUTPUTS: [&str; 3] = [
    "job-image.oci.tar",
    "job-image-candidate.json",
    "job-image-native-source-closure.json",
];

#[must_use]
pub fn job_image_candidate_enabled(config: &VelnorConfig) -> bool {
    config
        .execution
        .as_ref()
        .is_some_and(|execution| execution.workflows.contains(&RoutingWorkflow::ImageRelease))
}

pub fn job_image_candidate_proposal(
    inputs: &super::JobImageCandidateInputs,
) -> Result<ProposedTask, ContractError> {
    let argv = inputs.argv()?;
    let environment = inputs.environment()?;
    let inputs = super::native_image_candidate_inputs()
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<Vec<_>>();
    let proposal = ProposedTask {
        task_id: JOB_IMAGE_CANDIDATE_TASK_ID.to_owned(),
        stack_id: Stack::Native.id().to_owned(),
        component_id: JOB_IMAGE_CANDIDATE_UNIT_ID.to_owned(),
        task_kind: JOB_IMAGE_CANDIDATE_TASK_KIND.to_owned(),
        configuration: "oci-linux-amd64".to_owned(),
        depends_on: Vec::new(),
        gated_by: Vec::new(),
        reads: inputs.clone(),
        writes: Vec::new(),
        outputs: JOB_IMAGE_CANDIDATE_OUTPUTS
            .iter()
            .map(|path| (*path).to_owned())
            .collect(),
        resource: ResourceDemand {
            class: ResourceClass::Compiler,
            cpu_milli: None,
            memory_mb: None,
            needs_network: true,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: false,
            allow_task_reuse: false,
        },
        identity: crate::propose::IdentityInputs {
            unit_id: JOB_IMAGE_CANDIDATE_UNIT_ID.to_owned(),
            unit_key: JOB_IMAGE_CANDIDATE_UNIT_ID.to_owned(),
            unit_path: "images/job/ubuntu-26.04/Dockerfile".to_owned(),
            project_root: ".".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            features: Vec::new(),
            flags: Vec::new(),
            compile_driver: JOB_IMAGE_CANDIDATE_DRIVER.to_owned(),
            test_runner: JOB_IMAGE_CANDIDATE_DRIVER.to_owned(),
            environment: environment.clone(),
            declared_inputs: inputs,
            undeclared_reads: false,
        },
        payload: argv.iter().map(OsString::from).collect(),
        display_name: "Build Linux job image candidate".to_owned(),
        uses_clock: false,
        uses_random: false,
        no_targets: false,
        runner_profile: JOB_IMAGE_CANDIDATE_RUNNER.to_owned(),
    };
    proposal.validate()?;
    Ok(proposal)
}

pub fn validate_job_image_candidate_proposal(
    task: &ProposedTask,
    inputs: &super::JobImageCandidateInputs,
) -> Result<(), ContractError> {
    if *task != job_image_candidate_proposal(inputs)? {
        return Err(ContractError::identity(
            "job_image_candidate",
            "task_identity",
        ));
    }
    Ok(())
}
