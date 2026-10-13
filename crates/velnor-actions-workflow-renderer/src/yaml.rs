//! Deterministic YAML emitter: stable order, safe quoting, and 2-space indent.
//!
//! Block style only. Workflow output uses typed anchors for repeated step-run
//! strings and structurally identical workflow mappings. Flow sequences are
//! empty `[]` plus the typed `runs-on` selector. Key order is caller-controlled.

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
    /// One-line flow mapping of strings, used for compact generated inputs.
    FlowMap(Vec<(String, String)>),
    /// Double-quoted scalar. Plain `6379:6379` is a mapping in a sequence.
    Quoted(String),
    /// Plain or quoted scalar plus a trailing YAML comment.
    Annotated {
        /// Scalar text.
        value: String,
        /// Comment text, without the leading `#`.
        comment: String,
    },
    /// Repeated workflow step command emitted with a YAML anchor.
    AnchoredScalar {
        /// YAML anchor name.
        name: AnchorName,
        /// Anchored scalar value.
        value: String,
    },
    /// Repeated workflow mapping emitted with a YAML anchor.
    AnchoredMap {
        /// YAML anchor name.
        name: AnchorName,
        /// Anchored mapping entries.
        entries: Vec<(String, Self)>,
    },
    /// Alias to an earlier YAML anchor.
    Alias(AnchorName),
}

/// Validated, conservative YAML anchor name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AnchorName(String);

impl AnchorName {
    pub(crate) fn new(value: impl Into<String>) -> Option<Self> {
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
    fn is_inline(&self) -> bool {
        match self {
            Self::Null
            | Self::Str(_)
            | Self::Bool(_)
            | Self::Int(_)
            | Self::Flow(_)
            | Self::FlowMap(_)
            | Self::Quoted(_)
            | Self::Annotated { .. }
            | Self::AnchoredScalar { .. }
            | Self::Alias(_) => true,
            Self::Seq(items) => items.is_empty(),
            Self::Map(entries) | Self::AnchoredMap { entries, .. } => entries.is_empty(),
        }
    }
}

#[path = "yaml_share.rs"]
mod share;

/// Share repeated workflow nodes using aliases in an owned workflow tree.
#[must_use]
pub(crate) fn share_repeated_workflow_nodes(node: Yaml) -> Yaml {
    share::share_repeated_workflow_nodes(node)
}

#[path = "yaml_emit.rs"]
mod emit;

/// Render a document with a trailing newline.
#[must_use]
pub fn render_yaml(value: &Yaml) -> String {
    emit::render_yaml(value)
}

/// Quote a scalar only when plain style would be unsafe or ambiguous.
#[must_use]
pub fn quote_scalar(value: &str) -> String {
    emit::quote_scalar(value)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::{Yaml, render_yaml, render_yaml_compact};

    #[test]
    fn compact_render_preserves_multiline_run_scalar_and_structure() {
        let document = Yaml::Map(vec![(
            "jobs".to_owned(),
            Yaml::Map(vec![(
                "build".to_owned(),
                Yaml::Map(vec![(
                    "steps".to_owned(),
                    Yaml::Seq(vec![Yaml::Map(vec![
                        ("name".to_owned(), Yaml::str("compile")),
                        ("run".to_owned(), Yaml::str("echo first\necho second")),
                    ])]),
                )]),
            )]),
        )]);

        let regular = render_yaml(&document);
        let compact = render_yaml_compact(&document);
        assert!(compact.len() < regular.len());
        assert!(regular.contains("run: \"echo first\\necho second\"\n"));
        assert!(compact.contains("run: \"echo first\\necho second\"\n"));
        assert!(compact.contains("jobs:\n build:\n  steps:\n   - name: compile\n     run:"));
    }

    #[test]
    fn compact_render_keeps_scalar_quoting_and_map_order() {
        let document = Yaml::Map(vec![
            ("on".to_owned(), Yaml::str("push")),
            ("timeout-minutes".to_owned(), Yaml::Int(5)),
            (
                "runs-on".to_owned(),
                Yaml::Flow(vec!["ubuntu-latest".to_owned()]),
            ),
        ]);

        assert_eq!(
            render_yaml_compact(&document),
            "\"on\": push\ntimeout-minutes: 5\nruns-on: [ubuntu-latest]\n"
        );
    }

    #[test]
    fn compact_workflow_matches_canonical_yaml_event_stream() -> Result<(), Box<dyn Error>> {
        let document = Yaml::Map(vec![(
            "jobs".to_owned(),
            Yaml::Map(vec![(
                "build".to_owned(),
                Yaml::Map(vec![(
                    "steps".to_owned(),
                    Yaml::Seq(vec![Yaml::Map(vec![
                        ("name".to_owned(), Yaml::str("compile")),
                        ("run".to_owned(), Yaml::str("echo first\necho second")),
                        (
                            "env".to_owned(),
                            Yaml::Map(vec![("MODE".to_owned(), Yaml::str("safe"))]),
                        ),
                        (
                            "with".to_owned(),
                            Yaml::Map(vec![(
                                "args".to_owned(),
                                Yaml::Seq(vec![Yaml::str("one"), Yaml::str("two")]),
                            )]),
                        ),
                    ])]),
                )]),
            )]),
        )]);
        let canonical = render_yaml(&document);
        let compact = render_yaml_compact(&document);
        let canonical_events = yaml_events(&canonical)?;
        let compact_events = yaml_events(&compact)?;

        assert_eq!(compact_events, canonical_events);
        assert!(
            compact_events
                .iter()
                .any(|event| { event.contains("echo first\\necho second") })
        );
        Ok(())
    }

    fn yaml_events(source: &str) -> Result<Vec<String>, granit_parser::ScanError> {
        granit_parser::Parser::new_from_str(source)
            .map(|result| result.map(|(event, _span)| format!("{event:?}")))
            .collect()
    }
}
