// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    PromoteImageRequest,
    publication::{
        crane,
        github::{ci, read::get_json},
    },
};
use crate::{OrchestratorError, internal::internal};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use std::{
    env,
    ffi::{OsStr, OsString},
};
use velnor_actions_contract::config::{VelnorConfig, VersionSource};
use velnor_actions_mise::{CheckDeadline, PinnedTool, PinnedToolExec, ToolCatalog};

#[derive(Deserialize)]
struct Object {
    sha: String,
    #[serde(rename = "type")]
    kind: String,
}
#[derive(Deserialize)]
struct Reference {
    object: Object,
}
#[derive(Deserialize)]
struct Annotation {
    sha: String,
    #[serde(rename = "tag")]
    name: String,
    object: Object,
}
#[derive(Deserialize)]
struct Content {
    encoding: String,
    content: String,
}
#[derive(Deserialize)]
struct Comparison {
    status: String,
}
#[derive(Deserialize)]
struct Run {
    id: u64,
    #[serde(rename = "run_attempt")]
    attempt: u32,
    event: String,
    path: String,
    head_sha: String,
    status: String,
    conclusion: Option<String>,
    head_repository: Repository,
}
#[derive(Deserialize)]
struct Repository {
    id: u64,
    full_name: String,
}

pub(super) fn promote(request: &PromoteImageRequest) -> Result<(), OrchestratorError> {
    let token = env::var("GH_TOKEN").map_err(|_| internal("image_promotion_token"))?;
    if token.is_empty() {
        return Err(internal("image_promotion_token"));
    }
    let (repository, version) = authorize(request, &token)?;
    crane::promote(
        &repository,
        &format!("sha-{}", request.source_commit),
        &version,
        request.deadline,
        |digest| {
            authorize(request, &token).and_then(|observed| {
                if observed != (repository.clone(), version.clone()) {
                    return Err(internal("image_promotion_authority_changed"));
                }
                verify_attestation(
                    &repository,
                    digest,
                    &request.source_commit,
                    &token,
                    request.deadline,
                )?;
                crane::verify_native_role(
                    &repository,
                    digest,
                    &request.source_commit,
                    request.deadline,
                )
            })
        },
    )?;
    Ok(())
}

fn authorize(
    request: &PromoteImageRequest,
    token: &str,
) -> Result<(String, String), OrchestratorError> {
    let get = |key| env::var(key).map_err(|_| internal("image_promotion_invocation"));
    let sha = &request.source_commit;
    if sha.len() != 40
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(internal("image_promotion_source"));
    }
    let tag = get("GITHUB_REF_NAME")?;
    let parsed = velnor_actions_contract::config::ReleaseTag::parse(&tag)
        .filter(|parsed| parsed.artifact == request.image_id)
        .ok_or_else(|| internal("image_promotion_tag"))?;
    let version = parsed.version.as_str();
    if get("GITHUB_EVENT_NAME")? != "push"
        || get("GITHUB_REF")? != format!("refs/tags/{tag}")
        || get("GITHUB_SHA")? != *sha
        || get("GITHUB_REPOSITORY")? != "tailrocks/velnor-new"
        || get("GITHUB_WORKFLOW_REF")?
            != format!("tailrocks/velnor-new/.github/workflows/publish.yml@refs/tags/{tag}")
        || get("GITHUB_WORKFLOW_SHA")? != *sha
    {
        return Err(internal("image_promotion_invocation"));
    }
    let id: u64 = get("GITHUB_RUN_ID")?
        .parse()
        .map_err(|_| internal("image_promotion_invocation"))?;
    let attempt: u32 = get("GITHUB_RUN_ATTEMPT")?
        .parse()
        .map_err(|_| internal("image_promotion_invocation"))?;
    let run: Run = get_json(
        &format!("repos/tailrocks/velnor-new/actions/runs/{id}/attempts/{attempt}"),
        token,
        request.deadline,
    )?;
    if id == 0
        || attempt == 0
        || run.id != id
        || run.attempt != attempt
        || run.event != "push"
        || run.path != ".github/workflows/publish.yml"
        || run.head_sha != *sha
        || run.status != "in_progress"
        || run.conclusion.is_some()
        || run.head_repository.id != 1_390_620_900
        || run.head_repository.full_name != "tailrocks/velnor-new"
    {
        return Err(internal("image_promotion_run"));
    }
    verify_tag_source(request, token, &tag)?;
    let config = source_file(".velnor/config.toml", sha, token, request.deadline)?;
    let config: VelnorConfig =
        toml::from_str(&config).map_err(|_| internal("image_promotion_config"))?;
    velnor_actions_contract::config::validate_images(&config.images, ".velnor/config.toml")
        .map_err(|_| internal("image_promotion_config"))?;
    let mut images = config
        .images
        .iter()
        .filter(|image| image.id == request.image_id);
    let image = images
        .next()
        .ok_or_else(|| internal("image_promotion_image"))?;
    if images.next().is_some() || !image.repository.starts_with("ghcr.io/tailrocks/") {
        return Err(internal("image_promotion_image"));
    }
    let path = image
        .version_file
        .as_deref()
        .ok_or_else(|| internal("image_promotion_version_file"))?;
    let source =
        VersionSource::for_path(path).ok_or_else(|| internal("image_promotion_version_file"))?;
    let manifest = source_file(path, sha, token, request.deadline)?;
    if source.read(&manifest).as_deref() != Some(version) {
        return Err(internal("image_promotion_version"));
    }
    Ok((image.repository.clone(), version.to_owned()))
}

fn verify_tag_source(
    request: &PromoteImageRequest,
    token: &str,
    tag: &str,
) -> Result<(), OrchestratorError> {
    let sha = &request.source_commit;
    let reference: Reference = get_json(
        &format!("repos/tailrocks/velnor-new/git/ref/tags/{tag}"),
        token,
        request.deadline,
    )?;
    if reference.object.kind != "tag" {
        return Err(internal("image_promotion_annotated_tag"));
    }
    let annotation: Annotation = get_json(
        &format!(
            "repos/tailrocks/velnor-new/git/tags/{}",
            reference.object.sha
        ),
        token,
        request.deadline,
    )?;
    if annotation.sha != reference.object.sha
        || annotation.name != tag
        || annotation.object.kind != "commit"
        || annotation.object.sha != *sha
    {
        return Err(internal("image_promotion_annotated_tag"));
    }
    let main = ci::main_sha(token, request.deadline)?;
    let comparison: Comparison = get_json(
        &format!("repos/tailrocks/velnor-new/compare/{sha}...{main}"),
        token,
        request.deadline,
    )?;
    if !matches!(comparison.status.as_str(), "ahead" | "identical") {
        return Err(internal("image_promotion_source_not_main"));
    }
    ci::authenticate_source(sha, token, request.deadline)?;
    Ok(())
}

fn source_file(
    path: &str,
    sha: &str,
    token: &str,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    if path.is_empty()
        || path.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-+@".contains(&b))
        })
    {
        return Err(internal("image_promotion_path"));
    }
    let encoded_path = path.replace('+', "%2B").replace('@', "%40");
    let content: Content = get_json(
        &format!("repos/tailrocks/velnor-new/contents/{encoded_path}?ref={sha}"),
        token,
        deadline,
    )?;
    if content.encoding != "base64" {
        return Err(internal("image_promotion_source_encoding"));
    }
    let bytes = content
        .content
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect::<Vec<_>>();
    String::from_utf8(
        STANDARD
            .decode(bytes)
            .map_err(|_| internal("image_promotion_source_encoding"))?,
    )
    .map_err(|_| internal("image_promotion_source_encoding"))
}

fn verify_attestation(
    repository: &str,
    digest: &str,
    source: &str,
    token: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    #[cfg(test)]
    if TEST_PROMOTION_VERIFICATIONS.with(|slot| {
        let Some(count) = slot.get() else {
            return false;
        };
        slot.set(Some(count + 1));
        true
    }) {
        return Ok(());
    }
    let args = [
        "attestation",
        "verify",
        &format!("oci://{repository}@{digest}"),
        "--repo",
        "tailrocks/velnor-new",
        "--signer-workflow",
        "tailrocks/velnor-new/.github/workflows/publish.yml",
        "--signer-digest",
        source,
        "--source-digest",
        source,
        "--source-ref",
        "refs/heads/main",
        "--predicate-type",
        "https://slsa.dev/provenance/v1",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    let command = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
        .map_err(|_| internal("image_promotion_gh"))?
        .command_with_github_token(&ToolCatalog::pinned(), token)
        .map_err(|_| internal("image_promotion_gh"))?;
    let (output, _) = command
        .run_quiescent_until(4 * 1024 * 1024, deadline)
        .map_err(|_| internal("image_promotion_attestation"))?;
    if !output.success || output.code != Some(0) || output.signal.is_some() {
        return Err(internal("image_promotion_attestation"));
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    static TEST_PROMOTION_VERIFICATIONS: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(super) fn enable_test_promotion_verifier() {
    TEST_PROMOTION_VERIFICATIONS.with(|slot| assert!(slot.replace(Some(0)).is_none()));
}

#[cfg(test)]
pub(super) fn assert_test_promotion_verifier_used() {
    TEST_PROMOTION_VERIFICATIONS.with(|slot| assert!(matches!(slot.replace(None), Some(1..=2))));
}
