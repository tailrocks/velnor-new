// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::{RenderError, RenderedFile, marker, steps};

#[path = "owned_tool_publication_yaml.rs"]
mod document;
#[path = "owned_tool_qualification_yaml.rs"]
mod qualification;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceQualificationTrigger {
    DefaultBranchDispatch,
    ReviewedInfrastructurePush,
}

pub const REVIEWED_INFRASTRUCTURE_BRANCH: &str = "owned-tool-candidates";

#[derive(Debug, Clone)]
pub struct SourceBuildBinding {
    pub label: String,
    pub source_json: String,
}

impl SourceBuildBinding {
    pub fn validate(&self) -> Result<(), RenderError> {
        if self.label.is_empty()
            || self.label.len() > 64
            || !self
                .label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || self.source_json.len() > 65536
            || self.source_json.contains("${{")
        {
            return Err(RenderError::InvalidWorkflow(
                "source_build_binding".to_owned(),
            ));
        }
        velnor_actions_contract::parse_strict_json(&self.source_json)
            .map_err(RenderError::Contract)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct OwnedBuildBootstrap {
    pub target: String,
    pub runner: String,
    pub assets_json: String,
}

#[derive(Debug, Clone)]
pub struct OwnedPublicationSpec {
    pub trigger: SourceQualificationTrigger,
    pub generator_version: String,
    pub checkout_uses: String,
    pub sources: Vec<SourceBuildBinding>,
    pub bootstraps: Vec<OwnedBuildBootstrap>,
}

impl OwnedPublicationSpec {
    pub fn validate(&self) -> Result<(), RenderError> {
        marker::validate_version(&self.generator_version)?;
        steps::validate_uses(&self.checkout_uses)?;
        let hosts: BTreeSet<_> = self
            .bootstraps
            .iter()
            .map(|item| item.target.as_str())
            .collect();
        let expected = BTreeSet::from([
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "aarch64-apple-darwin",
        ]);
        let tools: BTreeSet<_> = self
            .sources
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        if !self.checkout_uses.starts_with("actions/checkout@")
            || self.sources.is_empty()
            || tools.len() != self.sources.len()
            || self.bootstraps.len() != 3
            || hosts != expected
        {
            return Err(RenderError::InvalidWorkflow(
                "owned_build_host_set".to_owned(),
            ));
        }
        for source in &self.sources {
            source.validate()?;
        }
        for bootstrap in &self.bootstraps {
            let supported = matches!(
                (bootstrap.target.as_str(), bootstrap.runner.as_str()),
                ("x86_64-unknown-linux-gnu", "ubuntu-26.04")
                    | ("aarch64-unknown-linux-gnu", "ubuntu-24.04-arm")
                    | ("aarch64-apple-darwin", "macos-26" | "xcode-27")
            );
            if !supported
                || bootstrap.assets_json.len() > 65536
                || bootstrap.assets_json.contains("${{")
            {
                return Err(RenderError::InvalidWorkflow(
                    "owned_build_bootstrap".to_owned(),
                ));
            }
            velnor_actions_contract::parse_strict_json(&bootstrap.assets_json)
                .map_err(RenderError::Contract)?;
        }
        Ok(())
    }
}

pub fn render_owned_publication_files(
    spec: &OwnedPublicationSpec,
) -> Result<Vec<RenderedFile>, RenderError> {
    spec.validate()?;
    let yaml = document::document(spec);
    Ok(vec![RenderedFile {
        path: ".github/workflows/owned-tools.yml".to_owned(),
        bytes: crate::render::fallback::render_checked_workflow(
            ".github/workflows/owned-tools.yml",
            &yaml,
            &spec.generator_version,
        )?,
    }])
}
