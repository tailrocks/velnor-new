// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_context::{JobBindingV1, UnitOutcome, UnitResultV1};
use velnor_actions_contract::{
    Digest, PlannedPlatform, PlatformBinding, PlatformRunnerEnvironment, PlatformUnavailableReason,
    digest_b3, task_id_for_internal,
};

use super::HEAD_SHA;

pub(super) fn digest(value: &[u8]) -> Result<Digest, Box<dyn Error>> {
    Ok(Digest::parse(&digest_b3(value))?)
}

pub(super) fn task_id() -> Result<String, Box<dyn Error>> {
    Ok(task_id_for_internal("lint", "dev")?)
}

pub(super) fn planned_platform() -> Result<PlannedPlatform, Box<dyn Error>> {
    Ok(PlannedPlatform::new(
        "ubuntu-22.04",
        "x86_64-unknown-linux-gnu",
    )?)
}

pub(super) fn observed_binding(
    planned: &PlannedPlatform,
) -> Result<PlatformBinding, Box<dyn Error>> {
    Ok(PlatformBinding::unavailable(
        &planned.platform_id,
        PlatformRunnerEnvironment::GithubHosted,
        PlatformUnavailableReason::MissingImageMetadata,
    )?)
}

pub(super) fn digests() -> (String, String, String) {
    (
        digest_b3(b"gc1-t50-task-payload"),
        digest_b3(b"gc1-t50-input"),
        digest_b3(b"gc1-t50-closure"),
    )
}

pub(super) fn unit_result(
    task: &str,
    outcome: UnitOutcome,
    exit_code: Option<i32>,
) -> Result<UnitResultV1, Box<dyn Error>> {
    Ok(UnitResultV1 {
        unit_id: task.to_owned(),
        recipe_id: digest(b"gc1-t50-recipe")?,
        task_identity: task.to_owned(),
        argv_digest: digest(b"gc1-t50-argv")?,
        effective_input_digest: digest(b"gc1-t50-input")?,
        outcome,
        exit_code,
        started_at: "2026-10-26T00:00:00Z".to_owned(),
        finished_at: "2026-10-26T00:00:01Z".to_owned(),
        report_digest: digest(b"gc1-t50-report")?,
    })
}

pub(super) fn job_binding(job_key: &str, event: &str) -> Result<JobBindingV1, Box<dyn Error>> {
    Ok(JobBindingV1 {
        schema: 1,
        repository_id: 1,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: 4250,
        run_attempt: 1,
        job_id: 7,
        job_key: job_key.to_owned(),
        event: event.to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        workflow_sha: HEAD_SHA.to_owned(),
        source_sha: HEAD_SHA.to_owned(),
        plan_digest: digest(b"gc1-t50-plan")?,
    })
}
