// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDecl {
    pub name: String,
    pub source: ModuleSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleSource {
    Literal(String),
    Dynamic { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceClass {
    Local,
    External,
    Remote(RemoteKind),
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteKind {
    Git,
    Registry,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleRef {
    pub file: String,
    pub name: String,
    pub source: ModuleSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleError {
    Escape { target: String },
    MissingTarget { target: String },
    Cycle { path: Vec<String> },
    Unreadable { target: String },
    TooManyFiles { count: usize },
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Escape { target } => write!(f, "module_escape:{target}"),
            Self::MissingTarget { target } => write!(f, "missing_target:{target}"),
            Self::Cycle { path } => write!(f, "module_cycle:{}", path.join(">")),
            Self::Unreadable { target } if target.is_empty() => {
                write!(f, "unreadable_module_root")
            }
            Self::Unreadable { target } => write!(f, "unreadable_module:{target}"),
            Self::TooManyFiles { count } => write!(f, "module_too_many_files:{count}"),
        }
    }
}

impl std::error::Error for ModuleError {}

pub(crate) fn source_from_native(block: &hcl::Block) -> ModuleSource {
    let mut sources = block
        .body()
        .attributes()
        .filter(|attribute| attribute.key.as_str() == "source");
    let Some(first) = sources.next() else {
        return ModuleSource::Dynamic {
            reason: "missing_source".to_owned(),
        };
    };
    debug_assert!(
        sources.next().is_none(),
        "hcl-rs rejects repeated attributes at parse"
    );
    match &first.expr {
        hcl::Expression::String(literal) => ModuleSource::Literal(literal.clone()),
        hcl::Expression::TemplateExpr(_) => ModuleSource::Dynamic {
            reason: "template_source".to_owned(),
        },
        _ => ModuleSource::Dynamic {
            reason: "dynamic_source".to_owned(),
        },
    }
}

pub(crate) fn source_from_json(body: &serde_json::Value) -> ModuleSource {
    let Some(object) = body.as_object() else {
        return ModuleSource::Dynamic {
            reason: "dynamic_source".to_owned(),
        };
    };
    match object.get("source") {
        None => ModuleSource::Dynamic {
            reason: "missing_source".to_owned(),
        },
        Some(serde_json::Value::String(literal)) => ModuleSource::Literal(literal.clone()),
        Some(_) => ModuleSource::Dynamic {
            reason: "dynamic_source".to_owned(),
        },
    }
}

#[must_use]
pub fn classify_literal(source: &str) -> SourceClass {
    if source.starts_with("./") || source.starts_with("../") {
        SourceClass::Local
    } else if is_absolute(source) {
        SourceClass::External
    } else {
        SourceClass::Remote(remote_kind(source))
    }
}

fn is_absolute(source: &str) -> bool {
    if source.starts_with('/') || source.starts_with('\\') {
        return true;
    }
    let bytes = source.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn remote_kind(source: &str) -> RemoteKind {
    if source.starts_with("git::")
        || source.starts_with("github.com/")
        || source.starts_with("bitbucket.org/")
    {
        RemoteKind::Git
    } else if source.contains("::") || source.contains("://") {
        RemoteKind::Other
    } else {
        RemoteKind::Registry
    }
}
