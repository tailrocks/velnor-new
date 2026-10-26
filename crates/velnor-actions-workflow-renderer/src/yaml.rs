// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Yaml {
    Null,
    Str(String),
    Bool(bool),
    Int(i64),
    Seq(Vec<Self>),
    Map(Vec<(String, Self)>),
    Flow(Vec<String>),
    FlowMap(Vec<(String, String)>),
    Quoted(String),
    Annotated {
        value: String,
        comment: String,
    },
    AnchoredScalar {
        name: AnchorName,
        value: String,
    },
    AnchoredMap {
        name: AnchorName,
        entries: Vec<(String, Self)>,
    },
    Alias(AnchorName),
}

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
    #[must_use]
    pub fn str(value: impl Into<String>) -> Self {
        Self::Str(value.into())
    }

    #[must_use]
    pub fn quoted(value: impl Into<String>) -> Self {
        Self::Quoted(value.into())
    }

    #[must_use]
    pub fn annotated(value: impl Into<String>, comment: impl Into<String>) -> Self {
        Self::Annotated {
            value: value.into(),
            comment: comment.into(),
        }
    }

    #[must_use]
    pub fn cache_mode_field(mode: &velnor_actions_contract::CacheMode) -> (String, Self) {
        ("cache-mode".to_owned(), Self::str(mode.as_str().to_owned()))
    }

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

pub(crate) fn share_workflow_nodes(node: Yaml) -> Result<Yaml, crate::RenderError> {
    share::share_workflow_nodes(node)
}

#[path = "yaml_emit.rs"]
mod emit;

#[must_use]
pub fn render_yaml(value: &Yaml) -> String {
    emit::render_yaml(value)
}

#[must_use]
pub fn render_yaml_compact(value: &Yaml) -> String {
    emit::render_yaml_compact(value)
}

#[must_use]
pub fn quote_scalar(value: &str) -> String {
    emit::quote_scalar(value)
}
