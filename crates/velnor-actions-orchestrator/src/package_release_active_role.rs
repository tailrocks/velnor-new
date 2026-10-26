// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{
    PackageReleasePlan,
    package_release::PackageReleaseChannel,
    preview_production::{expected_release_payload_pairs, release_payload_job_id},
};

pub(in crate::package_release_publish) fn release_job_name(
    plan: &PackageReleasePlan,
    job: &str,
) -> Result<String, OrchestratorError> {
    let prefix = match plan.channel {
        PackageReleaseChannel::Preview => "preview",
        PackageReleaseChannel::Stable => "stable",
    };
    if plan.channel == PackageReleaseChannel::Preview && job == "preview-payload" {
        let product_id = std::env::var("VELNOR_RELEASE_PRODUCT_ID")
            .map_err(|_| internal("package_active_role_unknown"))?;
        let target = std::env::var("VELNOR_RELEASE_TARGET")
            .map_err(|_| internal("package_active_role_unknown"))?;
        return preview_payload_job_name(plan, &product_id, &target);
    }
    if plan.channel == PackageReleaseChannel::Preview && job == "preview-eligibility" {
        return Ok("Select authenticated main source for preview".to_owned());
    }
    for (suffix, name) in [
        ("assemble", plan.channel.assemble_job_id()),
        ("signer", plan.channel.signer_job_id()),
        ("publisher", "Publish authenticated channel release"),
        (
            "recovery",
            "Recover the original signed release without rebuilding",
        ),
    ] {
        if job == format!("{prefix}-{suffix}") {
            return Ok(name.to_owned());
        }
    }
    if job.starts_with(&format!("{prefix}-tap-")) && !job.ends_with("-selection") {
        return continuation_job_name(plan.channel, job).map(str::to_owned);
    }
    if plan.channel == PackageReleaseChannel::Stable {
        match job {
            "stable-tag" => return Ok("Authorize the permanent stable tag".to_owned()),
            "stable-eligibility" => {
                return Ok("Authenticate stable source and complete CI".to_owned());
            }
            _ => {}
        }
    }
    for pair in
        expected_release_payload_pairs(plan).map_err(|_| internal("package_active_role_plan"))?
    {
        if release_payload_job_id(plan, &pair).map_err(|_| internal("package_active_role_plan"))?
            == job
        {
            return Ok(format!("Build {} / {}", pair.product_id, pair.target));
        }
    }
    Err(internal("package_active_role_unknown"))
}

pub(super) fn preview_payload_job_name(
    plan: &PackageReleasePlan,
    product_id: &str,
    target: &str,
) -> Result<String, OrchestratorError> {
    let pair = expected_release_payload_pairs(plan)
        .map_err(|_| internal("package_active_role_plan"))?
        .into_iter()
        .find(|pair| pair.product_id == product_id && pair.target == target)
        .ok_or_else(|| internal("package_active_role_unknown"))?;
    Ok(format!("Build {} / {}", pair.product_id, pair.target))
}

pub(in crate::package_release_publish) fn continuation_job_name(
    channel: PackageReleaseChannel,
    job: &str,
) -> Result<&'static str, OrchestratorError> {
    let prefix = match channel {
        PackageReleaseChannel::Preview => "preview",
        PackageReleaseChannel::Stable => "stable",
    };
    for (suffix, name) in [
        (
            "tap-selection",
            match channel {
                PackageReleaseChannel::Preview => "Select the current committed immutable preview",
                PackageReleaseChannel::Stable => {
                    "Select the authenticated permanent stable release"
                }
            },
        ),
        (
            "tap-prepare",
            "Prepare a literal-only tap update without write credentials",
        ),
        (
            "tap-writer",
            "Independently validate and create a signed-off tap PR",
        ),
        (
            "tap-merge",
            "Merge only the validated and fully reviewed PR head",
        ),
    ] {
        if job == format!("{prefix}-{suffix}") {
            return Ok(name);
        }
    }
    Err(internal("package_active_continuation_role_unknown"))
}

impl super::super::github::GitHubApi {
    pub(in crate::package_release_publish) fn verify_direct_release_caller(
        &self,
        plan: &PackageReleasePlan,
        receipt: &velnor_actions_contract::package_release::PackageReleaseReceipt,
    ) -> Result<(), OrchestratorError> {
        let value =
            |name: &str| std::env::var(name).map_err(|_| internal("package_direct_caller_context"));
        for (key, expected) in [
            ("GITHUB_REPOSITORY", plan.repository.clone()),
            ("GITHUB_REPOSITORY_ID", receipt.repository.id.to_string()),
            ("GITHUB_RUN_ID", receipt.workflow.run_id.to_string()),
            (
                "GITHUB_RUN_ATTEMPT",
                receipt.workflow.run_attempt.to_string(),
            ),
            ("GITHUB_SHA", receipt.workflow.authority_sha.clone()),
            ("GITHUB_REF", receipt.source.source_ref.clone()),
            (
                "GITHUB_WORKFLOW_SHA",
                receipt.workflow.authority_sha.clone(),
            ),
            (
                "GITHUB_WORKFLOW_REF",
                format!(
                    "{}/{}@{}",
                    plan.repository, receipt.workflow.path, receipt.workflow.workflow_ref
                ),
            ),
            (
                "GITHUB_EVENT_NAME",
                plan.channel.event().as_str().to_owned(),
            ),
        ] {
            if value(key)? != expected {
                return Err(internal("package_direct_caller_context"));
            }
        }
        let job = release_job_name(plan, &value("GITHUB_JOB")?)?;
        let workflow =
            self.admitted_workflow(plan.channel.workflow_path(), plan.channel.workflow_name())?;
        self.verify_current_role(
            &super::CurrentRun {
                id: receipt.workflow.run_id,
                attempt: receipt.workflow.run_attempt,
                source: super::RunSource {
                    repository_id: receipt.repository.id,
                    sha: &receipt.workflow.authority_sha,
                    branch: &plan.default_branch,
                },
                workflow: &workflow,
                event: plan.channel.event().as_str(),
            },
            &job,
        )
    }
}
