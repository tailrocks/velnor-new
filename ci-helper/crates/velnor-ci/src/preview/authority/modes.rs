// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use super::files::read_file;
use super::github::verify_tag;
use super::model::{Run, TARGETS, metadata, payload_identity, positive_text};
use super::pair::{Ctx, Pair};
use crate::error::CiError;
use crate::preview::MAX_PAYLOAD;
use crate::preview::attest::{Attestor, Authority};
use crate::preview::json::{Field, Res, digest};

mod draft;
mod prepare;
mod publish;

use self::draft::draft;
use self::prepare::prepare;
use self::publish::publish;

const ADMIT: &str = "Source preview publication / Admit source preview";
const DRAFT: &str = "Source preview publication / Draft source preview";

/// Which step of the publication workflow runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Draft,
    Prepare,
    Publish,
}

impl Mode {
    pub fn parse(text: &str) -> Res<Self> {
        match text {
            "draft" => Ok(Self::Draft),
            "prepare" => Ok(Self::Prepare),
            "publish" => Ok(Self::Publish),
            _ => Err(CiError::new("source preview mode")),
        }
    }
}

/// The step's environment, as the workflow passes it.
#[derive(Debug)]
pub struct Inputs {
    pub vars: BTreeMap<String, String>,
}

impl Inputs {
    pub fn req(&self, key: &str) -> Res<&str> {
        self.vars
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| CiError::new("source preview environment"))
    }

    pub fn run(&self) -> Res<Run<'_>> {
        Ok(Run {
            sha: self.req("GITHUB_SHA")?,
            repository: self
                .vars
                .get("GITHUB_REPOSITORY")
                .map_or("", String::as_str),
            event_name: self
                .vars
                .get("GITHUB_EVENT_NAME")
                .map_or("", String::as_str),
            git_ref: self.vars.get("GITHUB_REF").map_or("", String::as_str),
            workflow_ref: self
                .vars
                .get("GITHUB_WORKFLOW_REF")
                .map_or("", String::as_str),
            workflow_sha: self
                .vars
                .get("GITHUB_WORKFLOW_SHA")
                .map_or("", String::as_str),
            run_id: positive_text(self.req("GITHUB_RUN_ID")?)?,
            run_attempt: positive_text(self.req("GITHUB_RUN_ATTEMPT")?)?,
        })
    }

    pub fn authority(&self) -> Res<Authority> {
        Authority::from_env(
            self.req("AUTHORITY_REPOSITORY")?,
            self.req("AUTHORITY_PATH")?,
            self.req("AUTHORITY_REF")?,
            self.req("AUTHORITY_SHA")?,
        )
    }

    pub fn candidate(&self) -> Res<Value> {
        Ok(json!({
            "source_artifact_id": positive_text(self.req("SOURCE_ARTIFACT_ID")?)?,
            "linux_payload_artifact_id": positive_text(self.req("LINUX_PAYLOAD_ARTIFACT_ID")?)?,
            "darwin_payload_artifact_id": positive_text(self.req("DARWIN_PAYLOAD_ARTIFACT_ID")?)?,
        }))
    }

    fn temp(&self) -> Res<&Path> {
        Ok(Path::new(self.req("RUNNER_TEMP")?))
    }

    fn output(&self) -> Res<&Path> {
        Ok(Path::new(self.req("GITHUB_OUTPUT")?))
    }
}

pub(super) fn pair_outputs(selected: &Pair, release_id: u64) -> String {
    format!(
        "pair_origin={}\npair_id={}\nreceipt_sha256={}\nbundle_sha256={}\n",
        selected.origin,
        selected.artifact_id.unwrap_or(release_id),
        digest(&selected.receipt),
        digest(&selected.bundle)
    )
}

/// Port of the `main` of `schema2_generator_source_preview_publication.py`.
/// `base` is the directory the artifacts were downloaded under (the workspace).
pub fn run(
    mode: Mode,
    client: &dyn super::Client,
    attestor: &dyn Attestor,
    inputs: &Inputs,
    base: &Path,
) -> Res<()> {
    let authority = inputs.authority()?;
    let run = inputs.run()?;
    let raw = read_file(
        &base.join("source-preview-metadata"),
        "source-preview-metadata.json",
        65_536,
    )?;
    let source = metadata(&raw, &run)?;
    let mut payloads = BTreeMap::new();
    for (key, _, name) in TARGETS {
        let data = read_file(
            &base.join(format!("source-preview-payload-{key}")),
            name,
            MAX_PAYLOAD as u64,
        )?;
        payloads.insert(key.to_owned(), data);
    }
    for (key, data) in &payloads {
        payload_identity(key, data)?;
    }
    verify_tag(client, source.at("source_sha").text())?;
    let ctx = Ctx {
        client,
        attestor,
        run: run.clone(),
        authority: &authority,
    };
    match mode {
        Mode::Draft => draft(&ctx, inputs, base, source, payloads),
        Mode::Prepare => prepare(&ctx, inputs, base, &source, &payloads),
        Mode::Publish => publish(&ctx, inputs, base, &source, &payloads),
    }
}
