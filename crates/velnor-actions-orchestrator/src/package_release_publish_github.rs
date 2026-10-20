//! Fixed, pinned `gh api` transport and strict GitHub response records.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    time::Duration,
};

use sha2::{Digest, Sha256};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

use crate::{OrchestratorError, internal::internal};

#[path = "package_release_publish_fields.rs"]
mod fields;
#[path = "package_release_metadata_download.rs"]
mod metadata_download;
#[path = "package_release_read_transport.rs"]
mod read_transport;
#[path = "package_release_rolling_api.rs"]
mod rolling_api;
#[path = "package_release_publish_github_types.rs"]
mod types;
#[path = "package_release_publish_upload.rs"]
mod upload;
pub(super) use types::*;
#[path = "preview_artifact_transport_download.rs"]
mod preview_download;
#[path = "preview_artifact_transport_api.rs"]
mod preview_reads;
pub(super) use preview_reads::ReleaseArtifactApi;
#[path = "package_release_publish_github_immutable_status.rs"]
mod immutable_status;
#[path = "package_release_publish_preflight.rs"]
mod preflight;
#[cfg(test)]
use immutable_status::immutable_status_token;

#[cfg(test)]
#[path = "package_release_publish_github_fixture.rs"]
mod fixture;

const API_VERSION: &str = "2026-03-10";
const JSON_LIMIT: usize = 1024 * 1024;
const ASSET_LIMIT: usize = 1024 * 1024 * 1024;
const ARTIFACT_LIMIT: usize = 1024 * 1024 * 1024;
const API_TIMEOUT: Duration = Duration::from_secs(90);

pub(super) struct GitHubApi {
    catalog: ToolCatalog,
    repository: String,
    pub(super) run_identity_cache: std::cell::RefCell<super::run_identity::RunIdentityCache>,
    #[cfg(test)]
    fixture: Option<std::rc::Rc<fixture::JsonFixture>>,
}

impl GitHubApi {
    pub(super) fn new(repository: String) -> Self {
        Self {
            catalog: ToolCatalog::pinned(),
            repository,
            run_identity_cache: std::cell::RefCell::default(),
            #[cfg(test)]
            fixture: None,
        }
    }

    pub(super) fn route(&self, suffix: &str) -> String {
        format!("repos/{}/{suffix}", self.repository)
    }

    pub(super) fn post_fields(
        &self,
        route: &str,
        fields: &[(&str, String)],
    ) -> Result<Vec<u8>, OrchestratorError> {
        self.fields_request(route, "POST", fields)
    }

    pub(super) fn post_tag_fields(
        &self,
        fields: &[(&str, String)],
    ) -> Result<TagObjectResponse, OrchestratorError> {
        let bytes = self.fields_request(&self.route("git/tags"), "POST", fields)?;
        serde_json::from_slice(&bytes).map_err(|_| internal("package_publish_tag_response"))
    }

    pub(super) fn create_tag_object(
        &self,
        tag: &str,
        message: &str,
        commit_sha: &str,
    ) -> Result<TagObjectResponse, OrchestratorError> {
        self.post_tag_fields(&[
            ("tag", tag.to_owned()),
            ("message", message.to_owned()),
            ("object", commit_sha.to_owned()),
            ("type", "commit".to_owned()),
            ("tagger[name]", "Velnor Release Publisher".to_owned()),
            (
                "tagger[email]",
                "41898282+github-actions[bot]@users.noreply.github.com".to_owned(),
            ),
        ])
    }

    pub(super) fn patch_fields(
        &self,
        route: &str,
        fields: &[(&str, String)],
    ) -> Result<Vec<u8>, OrchestratorError> {
        self.fields_request(route, "PATCH", fields)
    }

    pub(super) fn upload_file(
        &self,
        route: &str,
        path: &std::path::Path,
    ) -> Result<(), OrchestratorError> {
        let args = self.upload_arguments(route, path)?;
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            let bytes = super::identity::read_regular(path)?;
            let endpoint = args[0]
                .to_str()
                .ok_or_else(|| internal("fixture_upload_endpoint"))?;
            fixture.mutate(
                "UPLOAD",
                endpoint,
                &[
                    ("sha256", sha256(&bytes)),
                    ("size", bytes.len().to_string()),
                ],
            )?;
            return Ok(());
        }
        self.request(args, JSON_LIMIT)?;
        Ok(())
    }

    pub(super) fn download_asset(&self, id: u64) -> Result<Vec<u8>, OrchestratorError> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            let response = fixture.read(&format!(
                "DOWNLOAD {}",
                self.route(&format!("releases/assets/{id}"))
            ))?;
            return serde_json::from_value(
                response
                    .get("bytes")
                    .cloned()
                    .ok_or_else(|| internal("fixture_download_bytes"))?,
            )
            .map_err(|_| internal("fixture_download_bytes"));
        }
        self.request_accept(
            vec![OsString::from(self.route(&format!("releases/assets/{id}")))],
            ASSET_LIMIT,
            "application/octet-stream",
        )
    }

    pub(super) fn release_list(
        &self,
        tag: &str,
    ) -> Result<Vec<ReleaseResponse>, OrchestratorError> {
        let mut found = Vec::new();
        for page in 1..=20 {
            let route = format!("{}?per_page=100&page={page}", self.route("releases"));
            let page_items: Vec<ReleaseResponse> = self.get_json(&route)?;
            let count = page_items.len();
            found.extend(
                page_items
                    .into_iter()
                    .filter(|release| release.tag_name == tag),
            );
            if count < 100 {
                return Ok(found);
            }
        }
        Err(internal("package_publish_release_pages_exceeded"))
    }

    pub(super) fn workflow_artifacts(
        &self,
        run_id: u64,
    ) -> Result<Vec<WorkflowArtifact>, OrchestratorError> {
        let mut found = Vec::new();
        for page in 1..=10 {
            let response: WorkflowArtifactList = self.get_json(&self.route(&format!(
                "actions/runs/{run_id}/artifacts?per_page=100&page={page}"
            )))?;
            let count = response.artifacts.len();
            if response.total_count > 1000 {
                return Err(internal("package_publish_artifact_count_exceeded"));
            }
            found.extend(response.artifacts);
            if count < 100 {
                return Ok(found);
            }
        }
        Err(internal("package_publish_artifact_pages_exceeded"))
    }

    pub(super) fn download_artifact_archive(
        &self,
        artifact_id: u64,
    ) -> Result<Vec<u8>, OrchestratorError> {
        self.download_artifact_archive_bounded(artifact_id, ARTIFACT_LIMIT)
    }

    pub(super) fn tag_identity(
        &self,
        tag: &str,
    ) -> Result<Option<TagObservation>, OrchestratorError> {
        let route = self.route(&format!("git/matching-refs/tags/{}", encode_segment(tag)));
        let refs: Vec<GitRefResponse> = self.get_json(&route)?;
        let exact = refs
            .into_iter()
            .filter(|entry| entry.ref_name == format!("refs/tags/{tag}"))
            .collect::<Vec<_>>();
        if exact.len() > 1 {
            return Err(internal("package_publish_duplicate_tag_ref"));
        }
        let Some(reference) = exact.into_iter().next() else {
            return Ok(None);
        };
        match reference.object.object_type.as_str() {
            "commit" => Ok(Some(TagObservation {
                source_sha: reference.object.sha,
                message: None,
            })),
            "tag" => {
                let annotated: GitTagResponse =
                    self.get_json(&self.route(&format!("git/tags/{}", reference.object.sha)))?;
                Ok(Some(annotated_tag_observation(tag, annotated)?))
            }
            _ => Err(internal("package_publish_tag_object_invalid")),
        }
    }

    pub(super) fn remote_assets(
        &self,
        release: &ReleaseResponse,
    ) -> Result<BTreeMap<String, String>, OrchestratorError> {
        let mut output = BTreeMap::new();
        for asset in &release.assets {
            if asset.name.is_empty() || output.contains_key(&asset.name) {
                return Err(internal("package_publish_remote_asset_collision"));
            }
            let bytes = self.download_asset(asset.id)?;
            let digest = sha256(&bytes);
            if asset
                .digest
                .as_deref()
                .is_some_and(|remote| remote != format!("sha256:{digest}"))
            {
                return Err(internal("package_publish_remote_asset_digest_mismatch"));
            }
            if asset.size != bytes.len() as u64 {
                return Err(internal("package_publish_remote_asset_size_mismatch"));
            }
            output.insert(asset.name.clone(), digest);
        }
        Ok(output)
    }

    pub(super) fn verify_repository(&self, expected_id: u64) -> Result<(), OrchestratorError> {
        preflight::verify_repository_identity(self, expected_id)?;
        self.verify_immutable_status()
    }

    pub(super) fn verify_release_channel_environment(
        &self,
        channel: velnor_actions_contract::package_release::PackageReleaseChannel,
        branch: &str,
    ) -> Result<Option<String>, OrchestratorError> {
        preflight::verify_release_channel_environment(channel, self, branch)
    }

    fn request(
        &self,
        request_args: Vec<OsString>,
        limit: usize,
    ) -> Result<Vec<u8>, OrchestratorError> {
        self.request_accept(request_args, limit, "application/vnd.github+json")
    }

    fn request_accept(
        &self,
        request_args: Vec<OsString>,
        limit: usize,
        accept: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        #[cfg(test)]
        if self.fixture.is_some() {
            return Err(internal("unexpected_fixture_transport_request"));
        }
        let mut args = vec![OsString::from("api")];
        args.extend([
            OsString::from("-H"),
            OsString::from(format!("Accept: {accept}")),
            OsString::from("-H"),
            OsString::from(format!("X-GitHub-Api-Version: {API_VERSION}")),
        ]);
        args.extend(request_args);
        let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
            .map_err(|_| internal("package_publish_gh_unavailable"))?;
        let result = exec
            .command(&self.catalog)
            .and_then(|command| command.run_bounded(limit, API_TIMEOUT))
            .map_err(|_| internal("package_publish_api_failed"))?;
        if !result.success {
            return Err(internal("package_publish_api_failed"));
        }
        Ok(result.stdout)
    }
}

pub(super) fn verify_tap_update_environment(
    repository: &str,
    branch: &str,
) -> Result<(), OrchestratorError> {
    preflight::tap_update_environment(repository, branch)
}

fn annotated_tag_observation(
    expected_tag: &str,
    annotated: GitTagResponse,
) -> Result<TagObservation, OrchestratorError> {
    if annotated.tag != expected_tag || annotated.object.object_type != "commit" {
        return Err(internal("package_publish_tag_target_invalid"));
    }
    Ok(TagObservation {
        source_sha: annotated.object.sha,
        message: Some(annotated.message),
    })
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        push_hex_byte(&mut out, byte, false);
    }
    out
}

pub(super) fn encode_segment(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            push_hex_byte(&mut out, byte, true);
        }
    }
    out
}

fn push_hex_byte(output: &mut String, byte: u8, uppercase: bool) {
    const LOWER: &[u8; 16] = b"0123456789abcdef";
    const UPPER: &[u8; 16] = b"0123456789ABCDEF";
    let hex = if uppercase { UPPER } else { LOWER };
    output.push(char::from(hex[usize::from(byte >> 4)]));
    output.push(char::from(hex[usize::from(byte & 0x0f)]));
}

pub(super) fn unique_release<'a>(
    releases: &'a [ReleaseResponse],
    tag: &str,
) -> Result<&'a ReleaseResponse, OrchestratorError> {
    let mut found = releases.iter().filter(|release| release.tag_name == tag);
    let first = found
        .next()
        .ok_or_else(|| internal("package_publish_draft_missing"))?;
    if found.next().is_some() {
        return Err(internal("package_publish_duplicate_release_tag"));
    }
    Ok(first)
}

#[cfg(test)]
#[path = "package_release_publish_github_tests.rs"]
mod tests;
