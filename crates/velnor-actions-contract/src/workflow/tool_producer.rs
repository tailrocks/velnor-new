//! Closed executable cache domains and isolated publication evidence.
use super::step::StepId;
use crate::ContractError;
use serde::{Deserialize, Serialize};

#[path = "tool_home_environment.rs"]
pub mod homes;

/// Each role owns a fixed executable payload, independent of source downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolCacheDomain {
    /// Small planning tool installation.
    Planning,
    /// Complete isolated Mise, Rustup and Cargo proxy installation.
    Full,
    /// Node bootstrap for anonymous npm source production.
    NpmBootstrap,
    /// Bun bootstrap for anonymous frozen source production.
    BunBootstrap,
    /// `OpenTofu` bootstrap for immutable provider production.
    TofuBootstrap,
    /// Java bootstrap for dependency-only Gradle source production.
    GradleBootstrap,
}

impl ToolCacheDomain {
    /// Fixed Mise data root. User supplied paths never grant authority.
    #[must_use]
    pub const fn root(self) -> &'static str {
        match self {
            Self::Planning => "${{ runner.temp }}/velnor/planning/mise",
            Self::Full => "${{ runner.temp }}/velnor/mise",
            Self::NpmBootstrap => "${{ runner.temp }}/velnor/npm-source/mise",
            Self::BunBootstrap => "${{ runner.temp }}/velnor/bun-source/mise",
            Self::TofuBootstrap => "${{ runner.temp }}/velnor/tofu-provider-producer/mise",
            Self::GradleBootstrap => "${{ runner.temp }}/velnor/gradle-source/mise",
        }
    }

    /// Canonical complete transport roots, in archive identity order.
    #[must_use]
    pub fn payload(self) -> Vec<String> {
        if self == Self::Full {
            vec![
                self.root().to_owned(),
                homes::RUSTUP_HOME.to_owned(),
                format!("{}/bin", homes::CARGO_HOME),
                format!("{}/.crates.toml", homes::CARGO_HOME),
                format!("{}/.crates2.json", homes::CARGO_HOME),
            ]
        } else {
            vec![self.root().to_owned()]
        }
    }

    /// Fixed environment locations for a domain's complete installed payload.
    /// These bindings grant no compiler installation or execution authority.
    #[must_use]
    pub fn home_environment(self) -> std::collections::BTreeMap<String, String> {
        homes::for_domain(self)
    }

    /// Stable namespace component.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::Full => "full",
            Self::NpmBootstrap => "npm-bootstrap",
            Self::BunBootstrap => "bun-bootstrap",
            Self::TofuBootstrap => "tofu-bootstrap",
            Self::GradleBootstrap => "gradle-bootstrap",
        }
    }
}

/// Literal identity inputs; compiled owner qualification grants authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCacheDescriptor {
    /// Closed payload owner.
    pub domain: ToolCacheDomain,
    /// Exact compilation target.
    pub target: String,
    /// Actual literal job runner label.
    pub runs_on: String,
    /// Sorted exact qualified selectors, checked against the compiled catalog.
    pub selectors: Vec<String>,
    /// Complete immutable qualification identity; runtime image is appended separately.
    pub immutable_identity: String,
    /// Exact catalog distribution qualification bound to the compiled helper.
    pub qualification_identity: String,
}

impl ToolCacheDescriptor {
    /// Validate shape. The compiled owner separately recomputes the full identity.
    /// # Errors
    /// Rejects mutable identities, unsupported targets and malformed selectors.
    pub fn validate(&self) -> Result<(), ContractError> {
        let literal = |value: &str| {
            !value.is_empty()
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        };
        if crate::tool_target_for_runner_label(&self.runs_on) != Some(self.target.as_str())
            || !literal(&self.runs_on)
            || self.runs_on.contains("latest")
            || !literal(&self.immutable_identity)
            || !qualified_identity(&self.qualification_identity)
            || self.selectors.is_empty()
            || self.selectors.len() > 2048
            || self.selectors.iter().map(String::len).sum::<usize>() > 524_288
            || self.selectors.windows(2).any(|pair| pair[0] >= pair[1])
            || self.selectors.iter().any(|selector| {
                selector.is_empty()
                    || selector.chars().any(char::is_control)
                    || selector.contains("${{")
            })
        {
            return Err(ContractError::identity(
                "tool_producer",
                "invalid_descriptor",
            ));
        }
        Ok(())
    }
}

fn qualified_identity(value: &str) -> bool {
    value
        .strip_prefix("qualified-tools@")
        .is_some_and(|digest| {
            crate::ids::is_lower_hex_len(digest.strip_prefix("b3-").unwrap_or(digest), 64)
        })
}

/// Pure executable production. This role never executes repository computation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PureToolProducer {
    /// Full payload and compatibility binding.
    pub descriptor: ToolCacheDescriptor,
    /// Exact typed scheduling and selected task authority.
    pub selection: super::tool_producer_selection::ToolProducerSelection,
    /// Canonical restore transport.
    pub restore_step: StepId,
    /// Fixed observation immediately before installation.
    pub before_step: StepId,
    /// Compiled installation and exact verification operation.
    pub installation_step: StepId,
    /// Fixed observation immediately after verification.
    pub after_step: StepId,
    /// Trusted useful-delta transport.
    pub save_step: StepId,
    /// Terminal availability report.
    pub report_step: StepId,
}

impl PureToolProducer {
    /// Validate shape and unique evidence bindings.
    /// # Errors
    /// Rejects malformed identities and colliding evidence identifiers.
    pub fn validate(&self) -> Result<(), ContractError> {
        self.descriptor.validate()?;
        self.selection.validate()?;
        let ids = [
            &self.restore_step,
            &self.before_step,
            &self.installation_step,
            &self.after_step,
            &self.save_step,
            &self.report_step,
        ];
        for (index, id) in ids.iter().enumerate() {
            id.validate()?;
            if ids[..index].contains(id) || id.as_str() == "velnor-tool-publication" {
                return Err(ContractError::identity(
                    "tool_producer",
                    "duplicate_evidence_binding",
                ));
            }
        }
        Ok(())
    }

    /// Publication requires trusted success, exact verification and useful change.
    #[must_use]
    pub fn save_condition(&self) -> String {
        format!(
            "{} && steps.{}.outputs.verified == 'true' && steps.{}.outputs.changed == 'true'",
            super::cache_trust::CACHE_SAVE_CONDITION,
            self.installation_step.as_str(),
            self.after_step.as_str()
        )
    }
}

#[cfg(test)]
#[path = "tool_producer_tests.rs"]
mod tests;
