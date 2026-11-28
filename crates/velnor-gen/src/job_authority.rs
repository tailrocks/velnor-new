// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod config;
mod error;
mod inventory;
mod jobs;
mod limits;
mod triggers;

use std::collections::BTreeMap;

use velnor_model::{
    HelperRelease, JOB_AUTHORITY_CONFIG_PATH, JOB_AUTHORITY_INVENTORY_PATH, JobAuthorityLimit,
    Platform, RepoFacts, RepoPath, RepoView,
};
use velnor_render::lf_normalized;
use velnor_rust::{LOCK_PATH, RunningGenerator, check_generator_binary, detect};

pub use error::JobAuthorityError;
use inventory::{Inventory, Workflow};

use crate::GenerateError;

#[derive(Debug)]
pub struct Identity {
    pub source_sha: String,
    pub executable_sha256: String,
}

impl Identity {
    #[must_use]
    pub fn lines(&self) -> String {
        format!("{}\n{}\n", self.source_sha, self.executable_sha256)
    }
}

fn pinned(helper: Option<&HelperRelease>) -> Result<String, JobAuthorityError> {
    helper
        .map(|helper| helper.action().to_string())
        .ok_or(JobAuthorityError::NotPinned)
}

pub fn identity_of(
    view: &RepoView,
    running: RunningGenerator<'_>,
) -> Result<Option<Identity>, GenerateError> {
    if !view.files.contains_key(&JOB_AUTHORITY_CONFIG_PATH) {
        return Ok(None);
    }
    let facts = detect(view)?;
    let source_sha = pinned(facts.helper.as_ref()).map_err(GenerateError::JobAuthority)?;
    let identity = written_by(view, running, source_sha).map_err(GenerateError::JobAuthority)?;
    Ok(Some(identity))
}

fn written_by(
    view: &RepoView,
    running: RunningGenerator<'_>,
    source_sha: String,
) -> Result<Identity, JobAuthorityError> {
    if running.commit != source_sha {
        return Err(JobAuthorityError::CommitMismatch {
            running: running.commit.to_owned(),
            pinned: source_sha,
        });
    }
    if !matches!(
        running.platform,
        Some(Platform::LinuxX64 | Platform::LinuxArm64)
    ) {
        return Err(JobAuthorityError::NotLinux(
            running.platform.map(Platform::file_suffix),
        ));
    }
    let executable_sha256 = running.sha256.ok_or(JobAuthorityError::NoDigest)?;
    let lock = view
        .files
        .get(&LOCK_PATH)
        .ok_or(JobAuthorityError::NotPinned)?;
    check_generator_binary(lock, running).map_err(JobAuthorityError::Lock)?;
    Ok(Identity {
        source_sha,
        executable_sha256: executable_sha256.to_owned(),
    })
}

pub fn workflows(view: &RepoView) -> Result<Option<Vec<RepoPath>>, JobAuthorityError> {
    config::workflows(view)
}

pub(crate) fn emit(
    view: &RepoView,
    facts: &RepoFacts,
    helper: Option<&HelperRelease>,
    on_disk: &BTreeMap<RepoPath, Vec<u8>>,
    files: &mut BTreeMap<RepoPath, String>,
) -> Result<Vec<String>, JobAuthorityError> {
    let Some(paths) = config::workflows(view)? else {
        return Ok(Vec::new());
    };
    let source_sha = pinned(helper)?;
    let mut workflows = Vec::new();
    let mut notes = Vec::new();
    for path in paths {
        let text = files
            .get(&path)
            .ok_or_else(|| JobAuthorityError::NotGenerated(path.clone()))?;
        let (jobs, omitted) = jobs::inventory_jobs(&path, text, &facts.config.default_branch)?;
        notes.extend(omitted);
        let committed = on_disk
            .get(&path)
            .filter(|have| lf_normalized(have).as_ref() == text.as_bytes())
            .map_or_else(|| text.as_bytes(), Vec::as_slice);
        workflows.push(Workflow::new(&path, committed, jobs));
    }
    let listed: usize = workflows.iter().map(Workflow::job_count).sum();
    limits::within(
        JobAuthorityLimit::Jobs,
        || format!("the workflows listed in {JOB_AUTHORITY_CONFIG_PATH}"),
        listed,
    )?;
    let inventory = Inventory::new(source_sha, workflows)?;
    files.insert(JOB_AUTHORITY_INVENTORY_PATH, inventory.render()?);
    Ok(notes)
}
