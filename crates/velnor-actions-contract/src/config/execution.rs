use super::runs_on::{
    LINUX_AMD64, SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL, is_hosted_catalog,
};
use super::workflow::RUNNER_LABEL_CATALOG;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const HOSTED_PROFILE_ID: &str = "hosted";

pub const SCALE_SET_PROFILE_ID: &str = "local";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionMode {
    Hosted,

    ScaleSet,

    Both,
}

impl ExecutionMode {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileKind {
    GithubHosted,

    GithubScaleSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionRole {
    Control,

    Verification,

    Release,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionProfile {
    pub kind: ProfileKind,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,

    pub platform: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionOverride {
    pub profile: String,

    pub role: ExecutionRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionParity {
    pub required: bool,

    pub execute_verification: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingWorkflow {
    Qualification,

    ImageRelease,

    MacosBinaryRelease,

    GeneratorRelease,

    Monitoring,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionConfig {
    pub default_profile: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ExecutionMode>,

    pub hosted_profile: String,

    pub scale_set_profile: String,

    pub profiles: BTreeMap<String, ExecutionProfile>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parity: Option<ExecutionParity>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, ExecutionOverride>,

    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub workflows: BTreeSet<RoutingWorkflow>,
}

impl ExecutionConfig {
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

    #[must_use]
    pub fn lint_runner_labels(&self) -> Vec<String> {
        if !self.emits_scale_set_selector() {
            return Vec::new();
        }
        self.scale_selector()
            .map(|selector| selector.labels().to_vec())
            .unwrap_or_default()
    }

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
