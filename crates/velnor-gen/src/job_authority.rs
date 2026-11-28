// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod config;
mod error;
mod inventory;
mod jobs;
mod triggers;

use std::collections::BTreeMap;

use velnor_model::{
    HelperRelease, JOB_AUTHORITY_CONFIG_PATH, JOB_AUTHORITY_INVENTORY_PATH, Platform, RepoFacts,
    RepoPath, RepoView,
};
use velnor_rust::detect;

pub use error::JobAuthorityError;
use inventory::{Inventory, Workflow};

use crate::GenerateError;

#[derive(Debug)]
pub struct Identity {
    pub source_sha: String,
    pub artifact_sha256: String,
}

const ARTIFACT_PLATFORM: Platform = Platform::LinuxArm64;

fn identity(helper: &HelperRelease, running: &str) -> Result<Identity, JobAuthorityError> {
    if helper.action().as_str() != running {
        return Err(JobAuthorityError::CommitMismatch {
            running: running.to_owned(),
            pinned: helper.action().to_string(),
        });
    }
    let artifact = helper
        .generator_digests()
        .and_then(|digests| digests.get(&ARTIFACT_PLATFORM))
        .ok_or(JobAuthorityError::NoGeneratorDigest)?;
    Ok(Identity {
        source_sha: helper.action().to_string(),
        artifact_sha256: artifact.to_string(),
    })
}

pub fn identity_of(view: &RepoView, running: &str) -> Result<Option<Identity>, GenerateError> {
    if !view.files.contains_key(&JOB_AUTHORITY_CONFIG_PATH) {
        return Ok(None);
    }
    let facts = detect(view)?;
    let helper = facts
        .helper
        .as_ref()
        .ok_or(GenerateError::HelperNotPinned)?;
    identity(helper, running)
        .map(Some)
        .map_err(GenerateError::JobAuthority)
}

pub(crate) fn emit(
    view: &RepoView,
    facts: &RepoFacts,
    helper: Option<&HelperRelease>,
    running: &str,
    files: &mut BTreeMap<RepoPath, String>,
) -> Result<Vec<String>, JobAuthorityError> {
    let Some(paths) = config::workflows(view)? else {
        return Ok(Vec::new());
    };
    let identity = identity(helper.ok_or(JobAuthorityError::NoGeneratorDigest)?, running)?;
    let mut workflows = Vec::new();
    let mut notes = Vec::new();
    for path in paths {
        let text = files
            .get(&path)
            .ok_or_else(|| JobAuthorityError::NotGenerated(path.clone()))?;
        let (jobs, omitted) = jobs::inventory_jobs(&path, text, &facts.config.default_branch)?;
        notes.extend(omitted);
        workflows.push(Workflow::new(&path, text, jobs));
    }
    let inventory = Inventory::new(identity.source_sha, workflows)?;
    files.insert(JOB_AUTHORITY_INVENTORY_PATH, inventory.render()?);
    Ok(notes)
}
