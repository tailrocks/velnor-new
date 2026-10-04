//! Closed artifact channels and output bindings for each release role.
use crate::{
    RenderError,
    release_jobs::ReleaseRole,
    steps::{RUN_KEY_EXPR, UPLOAD_ARTIFACT_USES},
};
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::outputs::{ActionOutput, JobOutput, StepOutputRef};
use velnor_actions_contract::{Step, StepId, StepKind};

pub(crate) struct Channel {
    pub(crate) step_id: &'static str,
    pub(crate) prefix: &'static str,
    pub(crate) path: &'static str,
    pub(crate) output_prefix: &'static str,
    pub(crate) always: bool,
}

pub(crate) fn channel(role: ReleaseRole) -> Channel {
    match role {
        ReleaseRole::SourceSnapshotForge => Channel {
            step_id: "release-source-snapshot-artifact",
            prefix: "source-snapshot",
            path: "release-source-snapshot/snapshot.zip",
            output_prefix: "source-snapshot-artifact",
            always: false,
        },
        ReleaseRole::PackageAnonymous => Channel {
            step_id: "release-package-artifact",
            prefix: "package",
            path: "release-package",
            output_prefix: "package-artifact",
            always: false,
        },
        ReleaseRole::PackagePreparedAnonymous => Channel {
            step_id: "release-package-artifact",
            prefix: "package",
            path: "${{ runner.temp }}/velnor/source-intent-prepared/prepared.zip",
            output_prefix: "package-artifact",
            always: false,
        },
        ReleaseRole::PreflightForge => Channel {
            step_id: "release-preflight-artifact",
            prefix: "preflight",
            path: "release-preflight/evidence.json",
            output_prefix: "preflight-artifact",
            always: false,
        },
        ReleaseRole::RegistryPublishOidc | ReleaseRole::RegistryPublishBootstrap => Channel {
            step_id: "release-registry-receipt-artifact",
            prefix: "registry",
            path: "release-registry/receipt.json",
            output_prefix: "registry-receipt-artifact",
            always: true,
        },
        ReleaseRole::ForgePublish => Channel {
            step_id: "release-forge-receipt-artifact",
            prefix: "forge",
            path: "release-forge/receipt.json",
            output_prefix: "forge-receipt-artifact",
            always: true,
        },
        ReleaseRole::Reconcile => Channel {
            step_id: "release-reconcile-receipt-artifact",
            prefix: "reconcile",
            path: "release-receipt/receipt.json",
            output_prefix: "reconcile-receipt-artifact",
            always: true,
        },
        ReleaseRole::PreparationAnonymous => Channel {
            step_id: "release-proposal-artifact",
            prefix: "proposal",
            path: "release-proposal/evidence.json",
            output_prefix: "artifact",
            always: false,
        },
        ReleaseRole::PreparationForge => Channel {
            step_id: "release-preparation-receipt-artifact",
            prefix: "preparation",
            path: "release-preparation/evidence.json",
            output_prefix: "preparation-receipt-artifact",
            always: true,
        },
    }
}

pub(crate) fn check_upload(role: ReleaseRole, step: &Step) -> Result<(), RenderError> {
    let channel = channel(role);
    let StepKind::Action { uses, with, env } = &step.kind else {
        return Err(invalid());
    };
    let expected = BTreeMap::from([
        (
            "name".to_owned(),
            format!("velnor-release-{}-{RUN_KEY_EXPR}", channel.prefix),
        ),
        ("path".to_owned(), channel.path.to_owned()),
        ("if-no-files-found".to_owned(), "error".to_owned()),
        ("retention-days".to_owned(), "30".to_owned()),
    ]);
    if uses != UPLOAD_ARTIFACT_USES
        || !env.is_empty()
        || with != &expected
        || step
            .id
            .as_ref()
            .map(velnor_actions_contract::StepId::as_str)
            != Some(channel.step_id)
        || step.condition.as_deref() != channel.always.then_some("always()")
    {
        return Err(invalid());
    }
    Ok(())
}

/// Reconstruct exact typed output references owned by this release role.
/// # Errors
/// Rejects malformed fixed step identifiers.
pub fn job_outputs(role: ReleaseRole) -> Result<Vec<JobOutput>, RenderError> {
    let channel = channel(role);
    let mut result = [
        ("id", ActionOutput::ArtifactId),
        ("digest", ActionOutput::ArtifactDigest),
    ]
    .into_iter()
    .map(|(suffix, output)| {
        Ok(JobOutput {
            name: format!("{}-{suffix}", channel.output_prefix),
            value: StepOutputRef {
                step_id: StepId::new(channel.step_id).map_err(RenderError::Contract)?,
                output,
            },
        })
    })
    .collect::<Result<Vec<_>, RenderError>>()?;
    if role == ReleaseRole::SourceSnapshotForge {
        for output in [
            ActionOutput::SourceSnapshotBlobSha256,
            ActionOutput::SourceCommitSha,
            ActionOutput::SourceTreeSha,
        ] {
            result.push(JobOutput {
                name: output.as_str().to_owned(),
                value: StepOutputRef {
                    step_id: StepId::new("release-source-snapshot")
                        .map_err(RenderError::Contract)?,
                    output,
                },
            });
        }
    }
    if role == ReleaseRole::PackagePreparedAnonymous {
        result.push(JobOutput {
            name: ActionOutput::PreparedBlobSha256.as_str().to_owned(),
            value: StepOutputRef {
                step_id: StepId::new("release-package").map_err(RenderError::Contract)?,
                output: ActionOutput::PreparedBlobSha256,
            },
        });
    }
    Ok(result)
}
fn invalid() -> RenderError {
    RenderError::InvalidWorkflow("release_artifact_channel_authority".to_owned())
}

/// Build the sole immutable upload action allowed for this typed release role.
/// # Errors
/// Rejects malformed pinned upload construction.
pub fn upload_step(role: ReleaseRole) -> Result<Step, RenderError> {
    let channel = channel(role);
    let mut step = crate::steps::upload_artifact_step(
        &format!("velnor-release-{}-{RUN_KEY_EXPR}", channel.prefix),
        channel.path,
    )?;
    step.id =
        Some(velnor_actions_contract::StepId::new(channel.step_id).map_err(RenderError::Contract)?);
    step.condition = channel.always.then(|| "always()".to_owned());
    Ok(step)
}
