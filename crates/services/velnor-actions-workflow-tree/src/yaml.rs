//! Deterministic YAML value tree with explicit mapping order and quoting-safe constructors.
//!
//! Deterministic block style with typed anchors for shared job environment maps
//! and repeated run scalars. Flow sequences are empty `[]` plus the typed
//! `runs-on` selector. Key order is caller-controlled.

use std::collections::BTreeMap;

/// Minimal YAML value tree with explicit mapping order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Yaml {
    /// Null renders as a bare key (`key:`).
    Null,
    /// String scalar, quoted only when plain style is unsafe.
    Str(String),
    /// Boolean scalar.
    Bool(bool),
    /// Integer scalar.
    Int(i64),
    /// Block sequence.
    Seq(Vec<Self>),
    /// Block mapping in the given entry order.
    Map(Vec<(String, Self)>),
    /// One-line flow sequence of scalars: `[a, b]`.
    Flow(Vec<String>),
    /// Double-quoted scalar. Plain `6379:6379` is a mapping in a sequence.
    Quoted(String),
    /// Plain or quoted scalar plus a trailing YAML comment.
    Annotated {
        /// Scalar text.
        value: String,
        /// Comment text, without the leading `#`.
        comment: String,
    },
    /// Mapping with a validated YAML anchor name.
    AnchoredMap {
        /// Anchor identifier.
        name: AnchorName,
        /// Mapping entries.
        entries: Vec<(String, Self)>,
    },
    /// Scalar with a validated YAML anchor name.
    AnchoredScalar {
        /// Anchor identifier.
        name: AnchorName,
        /// String value.
        value: String,
    },
    /// Alias to a mapping or scalar emitted earlier in this document.
    Alias(AnchorName),
}

/// Validated identifier for YAML anchors and aliases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorName(String);

impl AnchorName {
    /// Create a conservative ASCII anchor name.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        if !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            Some(Self(value))
        } else {
            None
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl Yaml {
    /// Build a string scalar.
    #[must_use]
    pub fn str(value: impl Into<String>) -> Self {
        Self::Str(value.into())
    }

    /// Build a scalar that is always double-quoted.
    #[must_use]
    pub fn quoted(value: impl Into<String>) -> Self {
        Self::Quoted(value.into())
    }

    /// Build a scalar with a trailing comment on the same line.
    #[must_use]
    pub fn annotated(value: impl Into<String>, comment: impl Into<String>) -> Self {
        Self::Annotated {
            value: value.into(),
            comment: comment.into(),
        }
    }

    /// True when the value fits on one line after `key: ` or `- `.
    pub(crate) fn is_inline(&self) -> bool {
        match self {
            Self::Null
            | Self::Str(_)
            | Self::Bool(_)
            | Self::Int(_)
            | Self::Flow(_)
            | Self::Quoted(_)
            | Self::Annotated { .. }
            | Self::AnchoredScalar { .. }
            | Self::Alias(_) => true,
            Self::Seq(items) => items.is_empty(),
            Self::Map(entries) | Self::AnchoredMap { entries, .. } => entries.is_empty(),
        }
    }
}

pub use crate::yaml_emit::{quote_scalar, render_yaml};

/// Replace repeated `run` scalars with deterministic anchors when the aliases
/// reduce the emitted byte count. This preserves the exact scalar value and
/// leaves every non-`run` field untouched.
#[must_use]
pub fn share_repeated_run_scalars(node: Yaml) -> Yaml {
    crate::yaml_share::share_repeated_run_scalars(node)
}

/// Quote bare env paths in every `run:` scalar.
pub fn quote_run_values_in_yaml(node: Yaml) -> Yaml {
    match node {
        Yaml::Map(entries) => Yaml::Map(
            entries
                .into_iter()
                .map(|(key, value)| {
                    if key == "run" {
                        if let Yaml::Str(line) = value {
                            (
                                key,
                                Yaml::Str(velnor_actions_workflow_steps::commands::quote_run_line_env_paths(&line)),
                            )
                        } else {
                            (key, value)
                        }
                    } else {
                        (key, quote_run_values_in_yaml(value))
                    }
                })
                .collect(),
        ),
        Yaml::Seq(items) => Yaml::Seq(items.into_iter().map(quote_run_values_in_yaml).collect()),
        Yaml::AnchoredMap { name, entries } => Yaml::AnchoredMap {
            name,
            entries: entries
                .into_iter()
                .map(|(key, value)| (key, quote_run_values_in_yaml(value)))
                .collect(),
        },
        other => other,
    }
}

/// Sorted string map as YAML (shared by `with:` and `env:` emission).
#[must_use]
pub fn string_map_yaml(map: &BTreeMap<String, String>) -> Yaml {
    Yaml::Map(
        map.iter()
            .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
            .collect(),
    )
}

#[cfg(test)]
mod tests;
