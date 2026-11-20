// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use velnor_model::{Platform, token_permission_accesses, token_permission_accesses_prose};

use crate::error::ReleaseError;
use crate::plan::BinaryPlan;
use crate::velnor_config::refuse_expressions;

const DEFAULT_CHECK_TIMEOUT_MINUTES: u16 = 30;
const MAX_TIMEOUT_MINUTES: u16 = 360;

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RawStage {
    Tag,
    Publish,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCheck {
    name: String,
    run: String,
    #[serde(default)]
    needs: Vec<String>,
    before: Option<RawStage>,
    #[serde(default)]
    artifacts: Vec<String>,
    runs_on: Option<String>,
    timeout_minutes: Option<u16>,
    #[serde(default)]
    permissions: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStage {
    BeforeTag,
    BeforePublish,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckArtifact {
    pub package: String,
    pub platform: Platform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckPlan {
    pub name: String,
    pub id: String,
    pub run: String,
    pub stage: CheckStage,
    pub needs: Vec<String>,
    pub artifacts: Vec<CheckArtifact>,
    pub permissions: Vec<(String, String)>,
    pub runs_on: Option<String>,
    pub timeout_minutes: u16,
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

fn artifact_of(name: &str, binaries: &[BinaryPlan]) -> Option<CheckArtifact> {
    binaries.iter().find_map(|binary| {
        binary
            .platforms
            .iter()
            .find(|platform| format!("{}-{}", binary.package, platform.file_suffix()) == name)
            .map(|platform| CheckArtifact {
                package: binary.package.clone(),
                platform: *platform,
            })
    })
}

fn permissions_of(
    name: &str,
    declared: BTreeMap<String, String>,
) -> Result<Vec<(String, String)>, ReleaseError> {
    let invalid = |problem| ReleaseError::InvalidCheck(name.to_owned(), problem);
    let mut all = declared;
    for (scope, access) in &all {
        let Some(accesses) = token_permission_accesses(scope) else {
            return Err(invalid(CheckProblem::UnknownPermission(scope.clone())));
        };
        if !accesses.contains(&access.as_str()) {
            return Err(invalid(CheckProblem::InvalidAccess {
                scope: scope.clone(),
                access: access.clone(),
            }));
        }
    }
    if all.get("id-token").is_some_and(|access| access == "write") {
        return Err(invalid(CheckProblem::WritesIdToken));
    }
    if all.get("contents").is_some_and(|access| access == "write") {
        return Err(invalid(CheckProblem::WritesContents));
    }
    all.entry("contents".to_owned())
        .or_insert_with(|| "read".to_owned());
    Ok(all.into_iter().collect())
}

fn artifacts_of(
    check: &RawCheck,
    stage: CheckStage,
    binaries: &[BinaryPlan],
) -> Result<Vec<CheckArtifact>, ReleaseError> {
    let invalid = |problem| ReleaseError::InvalidCheck(check.name.clone(), problem);
    let mut artifacts: Vec<CheckArtifact> = Vec::new();
    for name in &check.artifacts {
        let artifact = artifact_of(name, binaries)
            .ok_or_else(|| invalid(CheckProblem::UnknownArtifact(name.clone())))?;
        if binaries.iter().any(|binary| {
            binary.package == artifact.package && binary.optional_platforms.contains(&artifact.platform)
        }) {
            return Err(invalid(CheckProblem::OptionalArtifact(name.clone())));
        }
        if !artifacts.contains(&artifact) {
            artifacts.push(artifact);
        }
    }
    match (stage, artifacts.is_empty()) {
        (CheckStage::BeforePublish, true) => Err(invalid(CheckProblem::PublishWithoutArtifacts)),
        (CheckStage::BeforeTag, false) => Err(invalid(CheckProblem::ArtifactsBeforeTag)),
        _ => Ok(artifacts),
    }
}

pub(crate) fn plan_checks(
    raw: Vec<RawCheck>,
    binaries: &[BinaryPlan],
) -> Result<Vec<CheckPlan>, ReleaseError> {
    let mut planned: Vec<CheckPlan> = Vec::new();
    for check in raw {
        let invalid = |problem| ReleaseError::InvalidCheck(check.name.clone(), problem);
        refuse_expressions("checks.name", &[&check.name])?;
        let id = slug(&check.name);
        if id.is_empty() {
            return Err(invalid(CheckProblem::Empty("name")));
        }
        if check.run.trim().is_empty() {
            return Err(invalid(CheckProblem::Empty("run")));
        }
        let id = format!("check-{id}");
        if planned.iter().any(|earlier| earlier.id == id) {
            return Err(invalid(CheckProblem::DuplicateName));
        }
        let stage = match check.before {
            Some(RawStage::Publish) => CheckStage::BeforePublish,
            Some(RawStage::Tag) | None => CheckStage::BeforeTag,
        };
        refuse_expressions(
            "checks.needs",
            &check.needs.iter().map(String::as_str).collect::<Vec<_>>(),
        )?;
        refuse_expressions(
            "checks.artifacts",
            &check
                .artifacts
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )?;
        let mut needs = Vec::new();
        for need in &check.needs {
            let earlier = planned
                .iter()
                .find(|earlier| &earlier.name == need)
                .ok_or_else(|| invalid(CheckProblem::UnknownNeed(need.clone())))?;
            if stage == CheckStage::BeforeTag && earlier.stage == CheckStage::BeforePublish {
                return Err(invalid(CheckProblem::NeedsALaterStage(need.clone())));
            }
            needs.push(earlier.id.clone());
        }
        let artifacts = artifacts_of(&check, stage, binaries)?;
        if let Some(runner) = &check.runs_on {
            refuse_expressions("checks.runs_on", &[runner])?;
            if runner.trim().is_empty() {
                return Err(invalid(CheckProblem::Empty("runs_on")));
            }
        }
        let timeout_minutes = check
            .timeout_minutes
            .unwrap_or(DEFAULT_CHECK_TIMEOUT_MINUTES);
        if !(1..=MAX_TIMEOUT_MINUTES).contains(&timeout_minutes) {
            return Err(invalid(CheckProblem::TimeoutOutOfRange));
        }
        let permissions = permissions_of(&check.name, check.permissions)?;
        planned.push(CheckPlan {
            name: check.name,
            id,
            run: check.run,
            stage,
            needs,
            artifacts,
            permissions,
            runs_on: check.runs_on,
            timeout_minutes,
        });
    }
    Ok(planned)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckProblem {
    DuplicateName,
    UnknownNeed(String),
    PublishWithoutArtifacts,
    UnknownArtifact(String),
    OptionalArtifact(String),
    ArtifactsBeforeTag,
    WritesContents,
    WritesIdToken,
    UnknownPermission(String),
    InvalidAccess { scope: String, access: String },
    NeedsALaterStage(String),
    Empty(&'static str),
    TimeoutOutOfRange,
}

impl fmt::Display for CheckProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateName => {
                f.write_str("another check has the same name or the same job id")
            }
            Self::UnknownNeed(name) => {
                write!(f, "needs {name:?}, which is not a check declared before it")
            }
            Self::PublishWithoutArtifacts => {
                f.write_str("runs before publish, so it must name the artifacts it qualifies")
            }
            Self::UnknownArtifact(name) => write!(
                f,
                "artifact {name:?} is not <package>-<platform> of a [[binaries]] entry"
            ),
            Self::OptionalArtifact(name) => write!(
                f,
                "artifact {name:?} is optional, so a check that runs before publish cannot require it"
            ),
            Self::ArtifactsBeforeTag => {
                f.write_str("names artifacts, which exist only when it runs before publish")
            }
            Self::WritesContents => f.write_str(
                "asks for contents: write, which a job that runs repository code must not hold",
            ),
            Self::WritesIdToken => f.write_str(
                "asks for id-token: write, which a job that runs repository code must not hold",
            ),
            Self::InvalidAccess { scope, access } => write!(
                f,
                "permission {scope}: {access:?} is not valid; {}",
                token_permission_accesses_prose(scope)
            ),
            Self::NeedsALaterStage(name) => write!(
                f,
                "runs before the tag but needs {name:?}, which runs before publish"
            ),
            Self::Empty(key) => write!(f, "has an empty {key}"),
            Self::TimeoutOutOfRange => {
                write!(f, "timeout_minutes must be from 1 to {MAX_TIMEOUT_MINUTES}")
            }
            Self::UnknownPermission(scope) => {
                write!(f, "permission {scope:?} is not a GitHub token scope")
            }
        }
    }
}
