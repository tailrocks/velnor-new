// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;

use super::super::{ImagePublicationProfile, PublishImageRequest, authority};
use super::{Run, get_json};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_mise::CheckDeadline;

pub(super) fn authenticate(
    request: &PublishImageRequest,
    token: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let get = |name| env::var(name).map_err(|_| internal("image_publication_invocation_missing"));
    let workflow = match request.profile {
        ImagePublicationProfile::Job => ".github/workflows/publish.yml",
        ImagePublicationProfile::Runner
        | ImagePublicationProfile::Dind
        | ImagePublicationProfile::ResourceProbe => ".github/workflows/product-release-images.yml",
    };
    if get("GITHUB_EVENT_NAME")? != "push"
        || get("GITHUB_REF")? != "refs/heads/main"
        || get("GITHUB_REF_PROTECTED")? != "true"
        || get("GITHUB_SHA")? != request.source_commit
        || get("GITHUB_REPOSITORY")? != authority::IMAGE_REPOSITORY
        || get("GITHUB_WORKFLOW_REF")? != format!("tailrocks/velnor-new/{workflow}@refs/heads/main")
        || get("GITHUB_WORKFLOW_SHA")? != request.source_commit
    {
        return Err(internal("image_publication_invocation_identity"));
    }
    let id: u64 = get("GITHUB_RUN_ID")?
        .parse()
        .map_err(|_| internal("image_publication_invocation_identity"))?;
    let attempt: u32 = get("GITHUB_RUN_ATTEMPT")?
        .parse()
        .map_err(|_| internal("image_publication_invocation_identity"))?;
    if id == 0 || attempt == 0 || id == request.producer_run_id {
        return Err(internal("image_publication_invocation_identity"));
    }
    let route = format!("repos/tailrocks/velnor-new/actions/runs/{id}/attempts/{attempt}");
    let run: Run = get_json(&route, token, deadline)?;
    if run.id != id
        || run.attempt != attempt
        || run.event != "push"
        || run.path != workflow
        || run.head_branch != "main"
        || run.head_sha != request.source_commit
        || run.status != "in_progress"
        || run.conclusion.is_some()
        || run.head_repository.as_ref().is_none_or(|repo| {
            repo.id != authority::IMAGE_REPOSITORY_ID
                || repo.full_name != authority::IMAGE_REPOSITORY
        })
    {
        return Err(internal("image_publication_invocation_identity"));
    }
    Ok(())
}
