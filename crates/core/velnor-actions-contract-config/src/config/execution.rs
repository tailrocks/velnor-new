//! Schema 2 execution routing: profiles, mode, and requested workflows.
//!
//! `execution.mode` is optional. An explicit dispatch mode wins over the
//! configured mode. With neither, overrides win over `default_profile`.
//! Migration leaves mode unset and `default_profile` hosted.

use super::runs_on::{
    LINUX_AMD64, SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL, is_hosted_catalog,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract_release::targets::RUNNER_LABEL_CATALOG;

/// Profile id migration writes for GitHub-hosted runs.
pub const HOSTED_PROFILE_ID: &str = "hosted";
/// Profile id migration writes for the scale set.
pub const SCALE_SET_PROFILE_ID: &str = "local";

/// How eligible verification workloads are placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionMode {
    /// Every eligible workload stays on the hosted profile.
    Hosted,
    /// Eligible workloads use the scale-set profile. Control stays hosted.
    ScaleSet,
    /// Eligible workloads run on both profiles.
    Both,
}

impl ExecutionMode {
    /// Parse `hosted`, `scale-set`, or `both`.
    ///
    /// # Errors
    ///
    /// Any other spelling fails.
    pub fn parse(text: &str) -> Result<Self, ContractError> {
        match text {
            "hosted" => Ok(Self::Hosted),
            "scale-set" => Ok(Self::ScaleSet),
            "both" => Ok(Self::Both),
            _ => Err(ContractError::config(
                "config.toml",
                "execution.mode",
                format!("unsupported_mode:{text}"),
            )),
        }
    }
}

/// Provider kind. A runner label is not a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileKind {
    /// GitHub-hosted runner.
    GithubHosted,
    /// Repository-scoped runner scale set.
    GithubScaleSet,
}

/// Trust class of a logical job. Not a user-selectable escape hatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionRole {
    /// Planning, required aggregation, comparison, queue monitoring.
    Control,
    /// Comparable verification workload.
    Verification,
    /// Publish, deploy, release, or baseline promotion.
    Release,
}

/// One execution profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionProfile {
    /// Provider kind.
    pub kind: ProfileKind,
    /// Hosted catalog label. Required for `github-hosted` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Scale-set name. Required for `github-scale-set` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Scale-set labels. Required for `github-scale-set` only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// Execution platform (`linux/amd64`).
    pub platform: String,
    /// Optional capability profile name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_profile: Option<String>,
}

/// Per-logical-job placement override.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionOverride {
    /// Profile id (a key of [`ExecutionConfig::profiles`]).
    pub profile: String,
    /// Declared role. Must match the planner job class.
    pub role: ExecutionRole,
}

/// Paired-qualification flags. Numbers are not invented here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionParity {
    /// Whether parity comparison is required.
    pub required: bool,
    /// Whether both lanes must execute verification.
    pub execute_verification: bool,
}

/// Schema 2 workflow emitted only when listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingWorkflow {
    /// Qualification workflow.
    Qualification,
    /// Image-release workflow.
    ImageRelease,
    /// macOS binary-release workflow.
    MacosBinaryRelease,
    /// `velnor-actions` generator release. Tag is `generator-<sha>`, not `v0.1.0`.
    GeneratorRelease,
    /// Hosted queue-monitoring workflow.
    Monitoring,
}

/// Schema 2 `[execution]` section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionConfig {
    /// Profile used when mode and dispatch are both absent.
    pub default_profile: String,
    /// Optional placement mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ExecutionMode>,
    /// Profile id of the hosted runner.
    pub hosted_profile: String,
    /// Profile id of the scale set.
    pub scale_set_profile: String,
    /// Profiles keyed by id.
    pub profiles: BTreeMap<String, ExecutionProfile>,
    /// Optional parity flags.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parity: Option<ExecutionParity>,
    /// Optional per-job overrides keyed by logical job id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, ExecutionOverride>,
    /// Workflows to emit. Absent means none.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub workflows: BTreeSet<RoutingWorkflow>,
}

impl ExecutionConfig {
    /// Hosted-only execution for migration. Mode stays unset.
    ///
    /// The hosted profile label is the catalog label, never a profile id.
    ///
    /// # Errors
    ///
    /// The label must be in the hosted catalog.
    pub fn hosted_default(hosted_label: &str) -> Result<Self, ContractError> {
        let selector = ScaleSetSelector::try_new(
            SCALE_SET_NAME,
            &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
        )?;
        let mut profiles = BTreeMap::new();
        profiles.insert(
            HOSTED_PROFILE_ID.to_owned(),
            ExecutionProfile::hosted(hosted_label)?,
        );
        profiles.insert(
            SCALE_SET_PROFILE_ID.to_owned(),
            ExecutionProfile::scale_set(&selector),
        );
        let config = Self {
            default_profile: HOSTED_PROFILE_ID.to_owned(),
            mode: None,
            hosted_profile: HOSTED_PROFILE_ID.to_owned(),
            scale_set_profile: SCALE_SET_PROFILE_ID.to_owned(),
            profiles,
            parity: None,
            overrides: BTreeMap::new(),
            workflows: BTreeSet::new(),
        };
        config.validate("config.toml")?;
        Ok(config)
    }

    /// Scale-set labels when this config will emit that selector.
    #[must_use]
    pub fn actionlint_labels(&self) -> Vec<String> {
        if !self.emits_scale_set_selector() {
            return Vec::new();
        }
        self.scale_selector()
            .map(|selector| selector.labels().to_vec())
            .unwrap_or_default()
    }

    /// True when generated YAML will contain a scale-set `runs-on`.
    #[must_use]
    pub fn emits_scale_set_selector(&self) -> bool {
        let workflows = self.workflows.contains(&RoutingWorkflow::Qualification)
            || self.workflows.contains(&RoutingWorkflow::Monitoring);
        match self.mode {
            Some(ExecutionMode::ScaleSet | ExecutionMode::Both) => true,
            Some(ExecutionMode::Hosted) => workflows,
            None => {
                workflows
                    || self.default_profile == self.scale_set_profile
                    || self.overrides.values().any(|item| {
                        item.role == ExecutionRole::Verification
                            && item.profile == self.scale_set_profile
                    })
            }
        }
    }

    /// Validated scale-set selector for the configured scale-set profile.
    ///
    /// # Errors
    ///
    /// Missing profile or illegal labels.
    pub fn scale_selector(&self) -> Result<ScaleSetSelector, ContractError> {
        let profile = self
            .profiles
            .get(&self.scale_set_profile)
            .ok_or_else(|| missing("execution.scale_set_profile"))?;
        let name = profile
            .name
            .as_deref()
            .ok_or_else(|| missing("execution.profiles.scale_set.name"))?;
        ScaleSetSelector::try_new(name, &profile.labels)
    }

    /// Validate profiles, platforms, and overrides.
    ///
    /// # Errors
    ///
    /// Unknown profiles, hosted labels on a scale set, or platform mismatch.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        self.check_profiles(file)?;
        for (id, over) in &self.overrides {
            if !self.profiles.contains_key(&over.profile) {
                return Err(ContractError::config(
                    file,
                    format!("execution.overrides.{id}.profile"),
                    format!("unknown_profile:{}", over.profile),
                ));
            }
        }
        let _ = self.scale_selector()?;
        Ok(())
    }

    fn check_profiles(&self, file: &str) -> Result<(), ContractError> {
        let hosted = self.profile(file, &self.hosted_profile)?;
        let scale = self.profile(file, &self.scale_set_profile)?;
        if hosted.kind != ProfileKind::GithubHosted || scale.kind != ProfileKind::GithubScaleSet {
            return Err(ContractError::config(
                file,
                "execution.profiles",
                "profile_kind_mismatch",
            ));
        }
        let label = hosted.label.as_deref().unwrap_or("");
        if !RUNNER_LABEL_CATALOG.contains(&label) {
            return Err(ContractError::config(
                file,
                "execution.profiles.hosted.label",
                format!("unsupported_label:{label}"),
            ));
        }
        if hosted.name.is_some() || !hosted.labels.is_empty() || scale.label.is_some() {
            return Err(ContractError::config(
                file,
                "execution.profiles",
                "profile_field_mismatch",
            ));
        }
        if hosted.platform != LINUX_AMD64 || scale.platform != hosted.platform {
            return Err(ContractError::config(
                file,
                "execution.profiles.platform",
                "platform_mismatch",
            ));
        }
        self.check_default(file)?;
        Ok(())
    }

    fn check_default(&self, file: &str) -> Result<(), ContractError> {
        if self.default_profile == self.hosted_profile
            || self.default_profile == self.scale_set_profile
        {
            Ok(())
        } else {
            Err(ContractError::config(
                file,
                "execution.default_profile",
                format!("unknown_profile:{}", self.default_profile),
            ))
        }
    }

    fn profile<'a>(&'a self, file: &str, id: &str) -> Result<&'a ExecutionProfile, ContractError> {
        self.profiles.get(id).ok_or_else(|| {
            ContractError::config(file, "execution.profiles", format!("unknown_profile:{id}"))
        })
    }
}

impl ExecutionProfile {
    /// Hosted catalog profile.
    ///
    /// # Errors
    ///
    /// The label must be in the hosted catalog.
    pub fn hosted(label: &str) -> Result<Self, ContractError> {
        if !RUNNER_LABEL_CATALOG.contains(&label) || !is_hosted_catalog(label) {
            return Err(ContractError::config(
                "config.toml",
                "execution.profiles.hosted.label",
                format!("unsupported_label:{label}"),
            ));
        }
        Ok(Self {
            kind: ProfileKind::GithubHosted,
            label: Some(label.to_owned()),
            name: None,
            labels: Vec::new(),
            platform: LINUX_AMD64.to_owned(),
            capability_profile: None,
        })
    }

    /// Scale-set profile from an already validated selector.
    #[must_use]
    pub fn scale_set(selector: &ScaleSetSelector) -> Self {
        Self {
            kind: ProfileKind::GithubScaleSet,
            label: None,
            name: Some(selector.name().to_owned()),
            labels: selector.labels().to_vec(),
            platform: LINUX_AMD64.to_owned(),
            capability_profile: None,
        }
    }
}

fn missing(key: &str) -> ContractError {
    ContractError::config("config.toml", key, "missing")
}
