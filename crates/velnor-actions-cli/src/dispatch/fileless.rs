//! Admission for private operations that derive their path from runner temp.

use std::env;
use std::path::{Path, PathBuf};

use super::operation::InternalOp;
use super::{InternalRequest, RUNNER_TEMP_ENV};

pub(super) fn uses_runner_temp_gate(op: InternalOp) -> bool {
    matches!(
        op,
        InternalOp::ResolveTaskExecution
            | InternalOp::Fetch
            | InternalOp::Report
            | InternalOp::ExecuteCheck
            | InternalOp::PrivateFetch
            | InternalOp::BunRegistryInstall
            | InternalOp::GradleRun
            | InternalOp::PlaywrightInstall
            | InternalOp::BunAssetsExport
            | InternalOp::BunAssetsPrepare
            | InternalOp::BunAssetsImport
            | InternalOp::GitHubEvidenceFetch
            | InternalOp::GitHubEvidenceImport
            | InternalOp::PrivateRevoke
            | InternalOp::PreseedManifest
            | InternalOp::RepoPolicy
    )
}

pub(super) fn gate_runner_temp_operation(op: InternalOp) -> Option<InternalRequest> {
    match op {
        InternalOp::ResolveTaskExecution
            if env::var("GITHUB_RUN_ID").is_ok_and(|id| !id.is_empty()) =>
        {
            runner_velnor_dir().map(|path| InternalRequest { op, path })
        }
        InternalOp::Fetch | InternalOp::Report | InternalOp::ExecuteCheck
            if env::var("GITHUB_RUN_ID").is_ok_and(|id| !id.is_empty()) =>
        {
            runner_velnor_dir().map(|path| InternalRequest { op, path })
        }
        InternalOp::PrivateFetch
        | InternalOp::PrivateRevoke
        | InternalOp::BunRegistryInstall
        | InternalOp::GradleRun
        | InternalOp::PlaywrightInstall
        | InternalOp::BunAssetsExport
        | InternalOp::BunAssetsPrepare
        | InternalOp::BunAssetsImport
        | InternalOp::PreseedManifest => {
            runner_velnor_dir().map(|path| InternalRequest { op, path })
        }
        InternalOp::GitHubEvidenceFetch | InternalOp::GitHubEvidenceImport
            if env::var("GITHUB_RUN_ID")
                .is_ok_and(|id| id.parse::<u64>().is_ok_and(|id| id > 0))
                && env::var("GITHUB_RUN_ATTEMPT").is_ok_and(|attempt| {
                    attempt.parse::<u32>().is_ok_and(|attempt| attempt > 0)
                }) =>
        {
            runner_velnor_dir().map(|path| InternalRequest { op, path })
        }
        InternalOp::RepoPolicy => {
            crate::dispatch_repo_policy::gate_root().map(|path| InternalRequest { op, path })
        }
        _ => None,
    }
}

/// Runner-temp velnor directory for file-less private operations.
///
/// `None` when `RUNNER_TEMP` is unset or empty; shared by the fetch,
/// report, and preseed-manifest gate branches so the scoping rule has
/// one definition.
fn runner_velnor_dir() -> Option<PathBuf> {
    env::var_os(RUNNER_TEMP_ENV)
        .filter(|value| !value.is_empty())
        .map(|temp| Path::new(&temp).join("velnor"))
}
