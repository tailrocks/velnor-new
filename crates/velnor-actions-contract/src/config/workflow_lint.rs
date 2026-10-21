//! `[workflow.lint]`: rule strength of the generated workflow linter.
//!
//! jactionlint is Velnor's only workflow linter (hard decision 10). The one
//! knob is its profile; every other jactionlint setting (rules, ignores,
//! `paths`, baselines, online checks) stays generator-owned, so a consumer
//! cannot turn a finding off from configuration.
use serde::{Deserialize, Serialize};

/// `[workflow.lint]` section.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowLintConfig {
    /// jactionlint profile rendered into `.github/jactionlint.yaml`.
    #[serde(default)]
    pub profile: WorkflowLintProfile,
}

impl WorkflowLintConfig {
    /// Whether the section equals its default (omitted on serialization).
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// jactionlint rule profile.
///
/// `strict` and `all` are config-file-only jactionlint aliases of
/// `pedantic` that its command line rejects; they are not accepted here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowLintProfile {
    /// actionlint's checks plus jactionlint's bug detectors (the default,
    /// so the security and policy rule groups are opt-in).
    #[default]
    Correctness,
    /// `correctness` plus the security and policy rules.
    Default,
    /// `default` plus the opinionated rules.
    Pedantic,
}

impl WorkflowLintProfile {
    /// Exact jactionlint `profile:` value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Correctness => "correctness",
            Self::Default => "default",
            Self::Pedantic => "pedantic",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkflowLintConfig, WorkflowLintProfile};

    fn parse(json: &str) -> Result<WorkflowLintConfig, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn absent_profile_is_correctness() {
        let config = parse("{}").expect("empty section");
        assert_eq!(config.profile, WorkflowLintProfile::Correctness);
        assert!(config.is_default());
    }

    #[test]
    fn every_jactionlint_profile_parses_to_its_exact_value() {
        for (text, profile) in [
            ("correctness", WorkflowLintProfile::Correctness),
            ("default", WorkflowLintProfile::Default),
            ("pedantic", WorkflowLintProfile::Pedantic),
        ] {
            let config = parse(&format!("{{\"profile\":\"{text}\"}}")).expect(text);
            assert_eq!(config.profile, profile);
            assert_eq!(profile.as_str(), text);
        }
    }

    #[test]
    fn aliases_unknown_profiles_and_unknown_keys_are_rejected() {
        for text in [
            r#"{"profile":"strict"}"#,
            r#"{"profile":"all"}"#,
            r#"{"profile":"Default"}"#,
            r#"{"profile":""}"#,
            r#"{"ignores":[]}"#,
            r#"{"paths":{}}"#,
            r#"{"rules":{}}"#,
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
    }
}
