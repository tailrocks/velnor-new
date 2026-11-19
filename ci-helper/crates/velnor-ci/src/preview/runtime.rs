// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::attest::{Attestor, Authority};
use super::json::{Field, Res, canonical, digest, is_hex, require, sha40};
use super::net::{Net, repo_path};
use super::record::{approved_preview, read_github_lock};
use super::release::Release;
use super::stage::verify_preview;
use super::{REPOSITORY, REPOSITORY_ID};
use crate::error::CiError;

/// The platform a runtime SDK is installed for. Only these two hosts are supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    Linux,
    Darwin,
}

impl Host {
    /// Port of `host_target`: Linux on x86-64 or macOS on arm64.
    pub fn detect() -> Res<Self> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => Ok(Self::Linux),
            ("macos", "aarch64") => Ok(Self::Darwin),
            _ => Err(CiError::new("source preview unsupported host")),
        }
    }

    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Darwin => "darwin",
        }
    }
}

/// What a verified runtime preview installs.
#[derive(Debug)]
pub struct Verified {
    pub preview: Value,
    pub authority: Authority,
    pub release: Release,
    pub host: Host,
}

impl Verified {
    /// The `identity.json` content: the preview identity of the host's payload.
    #[must_use]
    pub fn identity(&self) -> Vec<u8> {
        let key = self.host.key();
        let preview = &self.preview;
        canonical(&json!({
            "repository": REPOSITORY,
            "version": preview.at("version"),
            "source_commit": preview.at("source_sha"),
            "target": preview.at(&format!("{key}_payload_target")),
            "asset_sha256": preview.at(&format!("{key}_payload_sha256")),
            "authority": {
                "channel": "source-preview",
                "receipt_sha256": preview.at("receipt_sha256"),
                "bundle_sha256": preview.at("bundle_sha256"),
                "authority_sha": self.authority.sha,
                "release_id": preview.at("release_id"),
            },
        }))
    }

    /// The step outputs, in the order the workflow reads them.
    #[must_use]
    pub fn outputs(&self) -> String {
        format!(
            "linux_sha256={}\ndarwin_sha256={}\n",
            self.preview.at("linux_payload_sha256").text(),
            self.preview.at("darwin_payload_sha256").text()
        )
    }
}

/// Port of `public_binding`: the run is the push of `source` to `main` in the image
/// workflow, and `main` still points at `source`. Returns the first parent.
pub fn public_binding(net: &dyn Net, source: &str, run_id: u64, attempt: u64) -> Res<String> {
    require(is_hex(source, 40) && run_id > 0 && attempt > 0)?;
    let repo = net.api(&repo_path(""))?;
    require(repo.at("id").is_u(REPOSITORY_ID) && repo.at("full_name").is_s(REPOSITORY))?;
    require(repo.at("default_branch").is_s("main"))?;
    require(net.api(&repo_path("/commits/main"))?.at("sha").is_s(source))?;
    let commit = net.api(&repo_path(&format!("/commits/{source}")))?;
    let parents = commit.at("parents").as_array();
    require(commit.at("sha").is_s(source) && parents.is_some_and(|p| !p.is_empty()))?;
    let parent = parents
        .and_then(|p| p.first())
        .map_or("", |p| p.at("sha").text());
    require(is_hex(parent, 40))?;
    let run = net.api(&repo_path(&format!(
        "/actions/runs/{run_id}/attempts/{attempt}"
    )))?;
    require(run.at("id").is_u(run_id) && run.at("run_attempt").is_u(attempt))?;
    require(run.at("head_sha").is_s(source))?;
    require(run.at("head_branch").is_s("main") && run.at("event").is_s("push"))?;
    let path = run.at("path").text().split('@').next().unwrap_or("");
    require(path == ".github/workflows/product-release-images.yml")?;
    let head = run.at("head_repository");
    require(head.at("id").is_u(REPOSITORY_ID) && head.at("full_name").is_s(REPOSITORY))?;
    let running = run.at("status").is_s("in_progress") && run.at("conclusion").is_null();
    let done = run.at("status").is_s("completed") && run.at("conclusion").is_s("success");
    require(running || done)?;
    Ok(parent.to_owned())
}

/// Port of `verify_runtime`: the runtime preview in `.velnor/generator.lock` at `source`
/// was approved by the lock at its parent, descends from the parent, and passes every
/// check of `verify_preview`, and the host's payload has the digest the record names.
pub fn verify_runtime(
    net: &dyn Net,
    attestor: &dyn Attestor,
    host: Host,
    source: &str,
    run_id: u64,
    attempt: u64,
) -> Res<Verified> {
    let parent = public_binding(net, source, run_id, attempt)?;
    let current = read_github_lock(net, source)?;
    let base = read_github_lock(net, &parent)?;
    let preview = approved_preview(&current, &base, "runtime", source)?
        .ok_or_else(|| CiError::new("source preview runtime is not approved"))?;
    let preview_source = preview.at("source_sha").text();
    require(sha40(preview.at("source_sha")))?;
    let ancestry = net.api(&repo_path(&format!("/compare/{preview_source}...{parent}")))?;
    require(matches!(
        ancestry.at("status").as_str(),
        Some("ahead" | "identical")
    ))?;
    require(
        ancestry
            .at("merge_base_commit")
            .at("sha")
            .is_s(preview_source),
    )?;
    let authority = Authority::pinned()?;
    let release = verify_preview(net, attestor, &preview, &authority)?;
    let payload = match host {
        Host::Linux => &release.linux,
        Host::Darwin => &release.darwin,
    };
    require(
        preview
            .at(&format!("{}_payload_sha256", host.key()))
            .is_s(&digest(payload)),
    )?;
    require(public_binding(net, source, run_id, attempt)? == parent)?;
    Ok(Verified {
        preview,
        authority,
        release,
        host,
    })
}
